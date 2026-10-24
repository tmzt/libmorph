//! Opening the app from a link (`MorphApp::on_open_url`). On Android the
//! app declares the `libmorph-urlopen` scheme in its build script (see
//! `examples/android/urlopen`), so a `libmorph-urlopen://…` link, tapped,
//! scanned with the camera or sent with
//! `adb shell am start -a android.intent.action.VIEW -d 'libmorph-urlopen://a/b#secret'`,
//! opens it. The last link's scheme and host pick the screen's hue (grey
//! before any link), and each link is logged without its query or
//! fragment, which may hold a secret.
//!
//! The other way round, a tap opens `https://example.com/` in the browser
//! (`MorphContext::open_link`); a scripted run only logs it. On the
//! desktop, `--open-link` also opens it once at start, to check the opener
//! without a tap (`cargo run --example urlopen -- --open-link`).
//!
//! On the desktop, links come only from scripted runs:
//!
//! ```text
//! cargo run --example urlopen --features script -- --morph-script examples/scripts/urlopen.morph
//! ```

use std::time::Duration;

use libmorph::{Frame, MorphApp, MorphContext, Touch, TouchPhase, wgpu};

/// What a tap opens.
const LINK: &str = "https://example.com/";

pub struct UrlOpen {
    /// 0..1, from the last link's scheme and host.
    hue: Option<f64>,
}

/// `scheme://host`, if the URL has that shape.
fn scheme_and_host(url: &str) -> Option<(&str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    Some((scheme, host))
}

/// The URL cut after its path.
fn redacted(url: &str) -> String {
    match url.find(['?', '#']) {
        Some(end) => format!("{}<redacted>", &url[..end]),
        None => url.to_owned(),
    }
}

/// FNV-1a, so the hue is the same on every run.
fn hue(text: &str) -> f64 {
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    });
    (hash % 360) as f64 / 360.0
}

/// A saturated colour of hue `h` (0..1).
fn color(h: f64) -> wgpu::Color {
    let channel = |offset: f64| {
        let x = ((h + offset).fract() * 6.0 - 3.0).abs() - 1.0;
        0.1 + 0.7 * x.clamp(0.0, 1.0)
    };
    wgpu::Color {
        r: channel(0.0),
        g: channel(2.0 / 3.0),
        b: channel(1.0 / 3.0),
        a: 1.0,
    }
}

impl UrlOpen {
    fn open(&self, ctx: &MorphContext) {
        match ctx.open_link(LINK) {
            Ok(()) => log::info!("urlopen: open_link {LINK}: Ok"),
            Err(error) => log::warn!("urlopen: open_link {LINK}: {error}"),
        }
    }
}

impl MorphApp for UrlOpen {
    fn init(ctx: &mut MorphContext) -> Self {
        log::info!("urlopen: waiting for a link");
        let app = Self { hue: None };
        if ctx.args().iter().any(|arg| arg == "--open-link") {
            app.open(ctx);
        }
        app
    }

    fn update(&mut self, _ctx: &mut MorphContext, _dt: Duration) {}

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        let clear = self.hue.map_or(
            wgpu::Color {
                r: 0.2,
                g: 0.2,
                b: 0.2,
                a: 1.0,
            },
            color,
        );
        let mut encoder = ctx.device().create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        ctx.queue().submit([encoder.finish()]);
    }

    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        if touch.phase == TouchPhase::Ended {
            self.open(ctx);
        }
    }

    fn on_open_url(&mut self, _ctx: &mut MorphContext, url: &str) {
        match scheme_and_host(url) {
            Some((scheme, host)) => {
                log::info!(
                    "urlopen: opened with {} (scheme {scheme}, host {host})",
                    redacted(url)
                );
                self.hue = Some(hue(&format!("{scheme}://{host}")));
            }
            None => log::warn!("urlopen: not a scheme://host link: {}", redacted(url)),
        }
    }
}

#[cfg(not(target_os = "android"))]
fn main() -> Result<(), libmorph::MorphError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    libmorph::run::<UrlOpen>()
}

#[cfg(test)]
mod tests {
    use super::{redacted, scheme_and_host};

    #[test]
    fn links_are_cut_and_split() {
        let url = "libmorph-urlopen://a.b:7/c#secret";
        assert_eq!(redacted(url), "libmorph-urlopen://a.b:7/c<redacted>");
        assert_eq!(scheme_and_host(url), Some(("libmorph-urlopen", "a.b:7")));
        assert_eq!(scheme_and_host("x://h?q"), Some(("x", "h")));
        assert_eq!(scheme_and_host("mailto:a@b"), None);
    }
}
