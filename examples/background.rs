//! A foreground service keeps a plain thread's TCP connection alive while
//! the app is in the background and the screen is off
//! ([`libmorph::service`], [`libmorph::system_events`]).
//!
//! * In `init`, which runs while the app is visible, it asks for
//!   `POST_NOTIFICATIONS`; once answered (at once off Android) it starts the
//!   service (`specialUse`) with the three built-in controls (ongoing, Exit,
//!   wake-lock button) and one button of its own, "Ping", and logs the
//!   battery state.
//! * A plain thread connects to the host it is given and writes a line
//!   every 30 s, logging any reply and putting the count in the
//!   notification's text. On `Exited` it stops.
//! * Another thread logs every system event, and `on_service` logs every
//!   service event on the main thread.
//! * `on_low_memory` is logged (`adb shell am send-trim-memory <package> COMPLETE`).
//! * Each launch counts itself in `launches` in `ctx.data_dir()`, which
//!   survives restarts: `~/Library/Application Support/<APP_ID>` on the Mac,
//!   the app's files directory on Android.
//!
//! On Android the host comes from a link with the example's scheme (see
//! `examples/android/background`):
//!
//! ```sh
//! nc -lk 7999      # on a computer on the same network
//! adb shell am start -a android.intent.action.VIEW -d libmorph-background://192.168.1.10:7999 \
//!     -n dev.highbay.libmorph.example.background/dev.highbay.libmobile.MobileNativeActivity
//! adb logcat -s background libmobile libmorph
//! ```
//!
//! On the desktop there is no service (`Unsupported`), and the only system
//! events are `Visible` from the window's lifecycle; the host is the first
//! argument:
//!
//! ```sh
//! cargo run --example background -- 127.0.0.1:7999
//! ```

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use libmorph::service::{
    self, Controls, NotificationAction, NotificationContent, ServiceEvent, ServiceOptions,
    ServiceType,
};
use libmorph::system_events::{self, SystemEvent};
use libmorph::{Frame, MorphApp, MorphContext, PermissionRequest, Permissions, battery, wgpu};

const POST_NOTIFICATIONS: &str = "android.permission.POST_NOTIFICATIONS";
/// As declared in the Android build script; only the kind matters at start.
const SPECIAL_USE: ServiceType =
    ServiceType::SpecialUse("a persistent connection to the user's own server");
/// The link scheme that gives the host on Android.
const SCHEME: &str = "libmorph-background";
const PERIOD: Duration = Duration::from_secs(30);

/// `ip:port` to write to.
static HOST: Mutex<Option<String>> = Mutex::new(None);
/// Set on `Exited`: the network thread ends.
static STOP: AtomicBool = AtomicBool::new(false);

/// Line `n`, with the clock in seconds since 1970.
fn line(n: u64, unix_secs: u64) -> String {
    format!("libmorph-background line {n} at {unix_secs}\n")
}

/// The notification's text after `n` lines.
fn status(n: u64, host: &str) -> String {
    match n {
        1 => format!("1 line sent to {host}"),
        n => format!("{n} lines sent to {host}"),
    }
}

/// The host of a `libmorph-background://<ip:port>` link.
fn host_from_link(url: &str) -> Option<&str> {
    let rest = url.strip_prefix(SCHEME)?.strip_prefix("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then_some(host)
}

fn set_host(host: &str) {
    log::info!("background: host {host}");
    *HOST.lock().unwrap_or_else(|e| e.into_inner()) = Some(host.to_owned());
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn connect(host: &str) -> Option<TcpStream> {
    let addr = match host.to_socket_addrs().map(|mut a| a.next()) {
        Ok(Some(addr)) => addr,
        other => {
            log::error!("background: cannot resolve {host}: {other:?}");
            return None;
        }
    };
    match TcpStream::connect_timeout(&addr, Duration::from_secs(5)) {
        Ok(stream) => {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
            log::info!("background: connected to {host}");
            Some(stream)
        }
        Err(err) => {
            log::error!("background: connect {host}: {err}");
            None
        }
    }
}

/// Writes a line every 30 s while there is a host, until `STOP`.
fn network_loop() {
    log::info!("background: network thread start");
    let mut stream: Option<(String, TcpStream)> = None;
    let mut next = Instant::now();
    let mut sent = 0u64;
    while !STOP.load(Ordering::Relaxed) {
        let host = HOST.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(host) = host
            && Instant::now() >= next
        {
            next = Instant::now() + PERIOD;
            if stream.as_ref().is_none_or(|(to, _)| *to != host) {
                stream = connect(&host).map(|s| (host.clone(), s));
            }
            if let Some((_, s)) = stream.as_mut() {
                match s.write_all(line(sent + 1, unix_now()).as_bytes()) {
                    Ok(()) => {
                        sent += 1;
                        log::info!("background: sent line {sent}");
                        let mut reply = [0u8; 512];
                        match s.read(&mut reply) {
                            Ok(0) => {
                                log::info!("background: connection closed");
                                stream = None;
                            }
                            Ok(n) => log::info!(
                                "background: reply {:?}",
                                String::from_utf8_lossy(&reply[..n]).trim_end()
                            ),
                            Err(err)
                                if matches!(
                                    err.kind(),
                                    ErrorKind::WouldBlock | ErrorKind::TimedOut
                                ) => {}
                            Err(err) => log::error!("background: read: {err}"),
                        }
                        if let Err(err) = service::update_text(&status(sent, &host)) {
                            log::warn!("background: update_text: {err}");
                        }
                    }
                    Err(err) => {
                        log::error!("background: write: {err}");
                        stream = None;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    log::info!("background: network thread exit after {sent} lines");
}

/// Log every system event; `Exited` stops the network thread.
fn events_loop(events: system_events::Events) {
    log::info!(
        "background: system events now {:?}",
        system_events::current()
    );
    loop {
        let event = events.next_blocking();
        log::info!("background: system event {event:?}");
        if event == SystemEvent::Service(ServiceEvent::Exited) {
            STOP.store(true, Ordering::Relaxed);
        }
    }
}

/// Add one to the `launches` file in `dir`, returning the new count.
fn count_launch(dir: &std::path::Path) -> std::io::Result<u64> {
    let path = dir.join("launches");
    let count = match std::fs::read_to_string(&path) {
        Ok(text) => text.trim().parse().unwrap_or(0),
        Err(error) if error.kind() == ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    } + 1;
    std::fs::write(&path, format!("{count}\n"))?;
    Ok(count)
}

fn start_service() {
    let mut content = NotificationContent::new("libmorph background", "starting");
    content
        .actions
        .push(NotificationAction::new("ping", "Ping"));
    let mut options = ServiceOptions::new(vec![SPECIAL_USE], content);
    let mut controls = Controls::default();
    controls.ongoing = true;
    controls.exit = true;
    controls.wake_lock_toggle = true;
    options.controls = controls;
    log::info!("background: start: {:?}", service::start(&options));
    log::info!("background: battery {:?}", battery::state());
}

pub struct Background {
    /// This app's `POST_NOTIFICATIONS` request.
    request: Option<PermissionRequest>,
}

impl MorphApp for Background {
    const APP_ID: &'static str = "dev.highbay.libmorph.example.background";

    fn init(ctx: &mut MorphContext) -> Self {
        log::info!("background: init");
        match ctx.data_dir() {
            Some(dir) => match count_launch(dir) {
                Ok(count) => log::info!("background: launch {count} ({})", dir.display()),
                Err(error) => log::error!("background: launches in {}: {error}", dir.display()),
            },
            None => log::warn!("background: no data_dir on this host"),
        }
        log::info!("background: cache_dir {:?}", ctx.cache_dir());
        // Subscribed here, so the thread misses nothing from now on.
        let events = system_events::subscribe();
        std::thread::spawn(move || events_loop(events));
        std::thread::spawn(network_loop);
        if let Some(host) = ctx.args().get(1) {
            set_host(host);
        }
        // `init` runs while the app is visible, so the service may start.
        let request = ctx.request_permissions(&[POST_NOTIFICATIONS]);
        if request.is_none() {
            start_service();
        }
        Self { request }
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        let mut encoder = ctx.device().create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.05,
                        g: 0.08,
                        b: 0.12,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        ctx.queue().submit([encoder.finish()]);
    }

    fn on_permissions(&mut self, _ctx: &mut MorphContext, answer: &Permissions) {
        if self.request == Some(answer.request) {
            self.request = None;
            log::info!("background: permissions {:?}", answer.permissions);
            // Started whatever the answer: without the permission the
            // notification is hidden, and the service still runs.
            start_service();
        }
    }

    fn on_service(&mut self, _ctx: &mut MorphContext, event: &ServiceEvent) {
        log::info!("background: on_service {event:?}");
        match event {
            ServiceEvent::Action(id) if id == "ping" => log::info!("background: ping"),
            ServiceEvent::Exited => STOP.store(true, Ordering::Relaxed),
            _ => {}
        }
    }

    fn on_open_url(&mut self, _ctx: &mut MorphContext, url: &str) {
        match host_from_link(url) {
            Some(host) => set_host(host),
            None => log::warn!("background: not a {SCHEME}://<ip:port> link"),
        }
    }

    fn on_low_memory(&mut self, _ctx: &mut MorphContext) {
        log::info!("background: on_low_memory");
    }

    fn on_suspend(&mut self, _ctx: &mut MorphContext) {
        log::info!("background: on_suspend");
    }

    fn on_resume(&mut self, _ctx: &mut MorphContext) {
        log::info!("background: on_resume");
    }
}

#[cfg(not(target_os = "android"))]
fn main() -> Result<(), libmorph::MorphError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    libmorph::run::<Background>()
}

#[cfg(test)]
mod tests {
    use super::{count_launch, host_from_link, line, status};

    #[test]
    fn launches_count_up() {
        let dir = std::env::temp_dir().join(format!("libmorph-launches-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(count_launch(&dir).unwrap(), 1);
        assert_eq!(count_launch(&dir).unwrap(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lines_and_status() {
        assert_eq!(line(3, 10), "libmorph-background line 3 at 10\n");
        assert_eq!(status(1, "h:1"), "1 line sent to h:1");
        assert_eq!(status(2, "h:1"), "2 lines sent to h:1");
    }

    #[test]
    fn hosts_from_links() {
        assert_eq!(
            host_from_link("libmorph-background://192.168.1.10:7999"),
            Some("192.168.1.10:7999")
        );
        assert_eq!(host_from_link("libmorph-background://h:1/x?y"), Some("h:1"));
        assert_eq!(host_from_link("libmorph-background://"), None);
        assert_eq!(host_from_link("other://h:1"), None);
    }
}
