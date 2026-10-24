//! Android: libmobile's `NativeActivity` loop drives the app. winit is not
//! involved; libmobile is the only thing that drains its Kotlin bridges
//! (insets, intents, IME text).

use libmobile::MobileEvent;
use libmobile::android::android_activity::input::{
    KeyAction, KeyEvent, Keycode, MotionAction, MotionEvent,
};
use libmobile::android::{
    AndroidApp, DEFAULT_POLL_TIMEOUT, InputEvent, InputStatus, MobileApp, SurfaceTarget,
};
use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle};
use std::time::Duration;

use super::shell::{FrameStatus, Shell};
use crate::gpu::{self, Gpu};
use crate::input::KeyRouting;
use crate::pipeline_cache::PipelineCacheStore;
use crate::{
    GpuError, Key, KeyStatus, MorphApp, MorphContext, PermissionRequest, Permissions, SafeArea,
    Touch, TouchPhase,
};

/// Run `A` until the activity is destroyed. Call it from the app's
/// `android_main`:
///
/// ```ignore
/// #[unsafe(no_mangle)]
/// fn android_main(app: libmorph::AndroidApp) {
///     libmorph::run_android::<MyApp>(app);
/// }
/// ```
pub fn run_android<A: MorphApp>(app: AndroidApp) {
    libmobile::android::run(
        app,
        Adapter::<A> {
            shell: None,
            failed: false,
            resumed: false,
            safe_area: SafeArea::NONE,
            keys: KeyRouting::default(),
            finishing: false,
            wants_capture: false,
            capture_request: None,
            urls: Vec::new(),
        },
    );
}

/// Android's display connection carries no data, so GLES can be given one
/// before any window exists.
#[derive(Debug)]
struct AndroidDisplay;

impl HasDisplayHandle for AndroidDisplay {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::android())
    }
}

struct Adapter<A> {
    shell: Option<Shell<A>>,
    /// Bring-up failed; don't retry on every window.
    failed: bool,
    resumed: bool,
    /// The latest inset report, kept for a shell that does not exist yet.
    safe_area: SafeArea,
    keys: KeyRouting,
    /// `exit()` was answered with `finish`; the teardown is under way.
    finishing: bool,
    /// `audio_setup` asked for capture.
    wants_capture: bool,
    /// libmorph's own `RECORD_AUDIO` request for capture, whose answer is
    /// not passed to the app.
    capture_request: Option<PermissionRequest>,
    /// Links that arrived before the app existed (a link that cold-started
    /// it), delivered right after `init`.
    urls: Vec<String>,
}

/// The action of an intent that opens a link.
const ACTION_VIEW: &str = "android.intent.action.VIEW";

/// What capture needs granted at runtime.
const RECORD_AUDIO: &str = "android.permission.RECORD_AUDIO";

fn scale_factor(app: &AndroidApp) -> f32 {
    app.config().density().map_or(1.0, |dpi| dpi as f32 / 160.0)
}

impl<A: MorphApp> Adapter<A> {
    fn window_created(&mut self, app: &AndroidApp) {
        let Some(target) = SurfaceTarget::current(app) else {
            return;
        };
        let size = target.size();
        match &mut self.shell {
            Some(shell) => {
                if shell.has_surface() {
                    return shell.resize(size);
                }
                match shell.gpu().instance.create_surface(target) {
                    Ok(surface) => shell.attach(surface, size),
                    Err(error) => log::error!("libmorph: {}", GpuError::CreateSurface(error)),
                }
            }
            None if !self.failed => self.boot(app, target),
            None => {}
        }
    }

    /// First window: instance, surface, device, then the app.
    fn boot(&mut self, app: &AndroidApp, target: SurfaceTarget) {
        let size = target.size();
        let boot = async {
            let instance = gpu::instance(Some(Box::new(AndroidDisplay))).await;
            let surface = instance
                .create_surface(target)
                .map_err(GpuError::CreateSurface)?;
            let gpu = Gpu::new(instance, Some(&surface), false, A::limits, true).await?;
            Ok::<_, GpuError>((gpu, surface))
        };
        let (gpu, surface) = match pollster::block_on(boot) {
            Ok(boot) => boot,
            Err(error) => {
                log::error!("libmorph: {error}");
                self.failed = true;
                return;
            }
        };
        let mut spec = libdevice::DEFAULT_DEVICE;
        (spec.geometry.width, spec.geometry.height) = size;
        // Capture without the permission would fail to open: open the rest
        // of audio now, and capture once the user allows it.
        let mut setup = super::audio_setup::<A>();
        self.wants_capture = setup.capture;
        let ask = setup.capture && !libmobile::android::permission_granted(app, RECORD_AUDIO);
        setup.capture &= !ask;
        let mut ctx = MorphContext::new(gpu, spec, Vec::new(), crate::Audio::open(setup));
        ctx.safe_area = self.safe_area;
        ctx.scale_factor = scale_factor(app);
        ctx.data_dir = app.internal_data_path();
        ctx.android = Some(app.clone());
        #[cfg(feature = "secure-store")]
        {
            ctx.secure_store = Some(crate::SecureStore::android());
            ctx.key_store =
                crate::Vault::named(&crate::secure_store::namespace(A::SECURE_STORE_ID)).ok();
        }
        ctx.pipeline_cache = ctx.data_dir.as_deref().and_then(|dir| {
            PipelineCacheStore::open(&ctx.gpu.device, &ctx.gpu.adapter.get_info(), dir)
        });
        match Shell::start(ctx, surface, size) {
            Ok(mut shell) => {
                for url in std::mem::take(&mut self.urls) {
                    shell.open_url(url);
                }
                self.shell = Some(shell);
                if ask {
                    self.capture_request =
                        Some(libmobile::android::request_permissions(app, &[RECORD_AUDIO]).into());
                }
            }
            Err(error) => {
                log::error!("libmorph: {error}");
                self.failed = true;
            }
        }
    }

    /// An answer to a permission request: libmorph's own for capture, or
    /// the app's. Either way, a grant of `RECORD_AUDIO` opens capture if the
    /// app asked for it and it is still closed.
    fn permissions(&mut self, answer: Permissions) {
        let Some(shell) = &mut self.shell else {
            log::warn!("libmorph: a permission answer before the app exists");
            return;
        };
        if self.capture_request == Some(answer.request) {
            self.capture_request = None;
            if !answer.granted(RECORD_AUDIO) {
                log::warn!("libmorph: audio capture: RECORD_AUDIO denied");
            }
        } else {
            shell.app.on_permissions(&mut shell.ctx, &answer);
        }
        if !(self.wants_capture
            && answer.granted(RECORD_AUDIO)
            && shell.ctx.audio.capture_format().is_none())
        {
            return;
        }
        if shell.ctx.audio.open_capture() {
            if !self.resumed {
                shell.ctx.audio.pause();
            }
            shell.app.on_capture_ready(&mut shell.ctx);
        }
    }

    fn resized(&mut self, app: &AndroidApp) {
        if let (Some(shell), Some(target)) = (&mut self.shell, SurfaceTarget::current(app)) {
            let size = target.size();
            shell.resize(size);
            (
                shell.ctx.spec.geometry.width,
                shell.ctx.spec.geometry.height,
            ) = size;
        }
    }

    /// Back and Escape reach the app on their first press; the app's answer
    /// decides whether Android acts on the key (see [`KeyRouting`]). Other
    /// keys produce no [`Key`] and stay unhandled.
    fn key(&mut self, event: &KeyEvent<'_>) -> InputStatus {
        let key = match event.key_code() {
            Keycode::Back => Key::Back,
            Keycode::Escape => Key::Escape,
            _ => return InputStatus::Unhandled,
        };
        let status = match event.action() {
            KeyAction::Down if event.repeat_count() == 0 => {
                let status = self
                    .shell
                    .as_mut()
                    .map_or(KeyStatus::Unhandled, |shell| shell.key(key));
                self.keys.pressed(key, status)
            }
            _ => self.keys.followup(key),
        };
        match status {
            KeyStatus::Handled => InputStatus::Handled,
            KeyStatus::Unhandled => InputStatus::Unhandled,
        }
    }

    fn touches(&mut self, motion: &MotionEvent<'_>) {
        let Some(shell) = &mut self.shell else {
            return;
        };
        let changed = motion.pointer_index();
        let phase = match motion.action() {
            MotionAction::Down | MotionAction::PointerDown => TouchPhase::Started,
            MotionAction::Up | MotionAction::PointerUp => TouchPhase::Ended,
            MotionAction::Move => TouchPhase::Moved,
            MotionAction::Cancel => TouchPhase::Cancelled,
            _ => return,
        };
        // Down/up concern one pointer; move and cancel every pointer.
        let only_changed = matches!(phase, TouchPhase::Started | TouchPhase::Ended);
        for (index, pointer) in motion.pointers().enumerate() {
            if !only_changed || index == changed {
                shell.touch(Touch {
                    id: pointer.pointer_id() as u64,
                    phase,
                    x: pointer.x(),
                    y: pointer.y(),
                    pressure: Some(pointer.pressure()),
                });
            }
        }
    }

    /// The app asked to exit: finish the activity, once. The normal teardown
    /// follows (`WindowDestroyed` suspends the app, `Destroy` shuts down).
    fn check_exit(&mut self, app: &AndroidApp) {
        if !self.finishing && self.shell.as_ref().is_some_and(Shell::exit_requested) {
            self.finishing = true;
            libmobile::android::finish(app);
        }
    }

    /// Render a frame while resumed, replacing a lost surface.
    fn frame(&mut self, app: &AndroidApp) {
        let Some(shell) = &mut self.shell else {
            return;
        };
        if !self.resumed || shell.frame() != FrameStatus::Lost {
            return;
        }
        if let Some(target) = SurfaceTarget::current(app) {
            let size = target.size();
            match shell.gpu().instance.create_surface(target) {
                Ok(surface) => shell.attach(surface, size),
                Err(error) => log::error!("libmorph: {}", GpuError::CreateSurface(error)),
            }
        }
    }
}

impl<A: MorphApp> MobileApp for Adapter<A> {
    fn event(&mut self, app: &AndroidApp, event: MobileEvent) {
        match event {
            MobileEvent::WindowCreated => self.window_created(app),
            MobileEvent::WindowResized => self.resized(app),
            // Synchronous inside the poll callback: the surface must be gone
            // before Android takes the window back.
            MobileEvent::WindowDestroyed => {
                if let Some(shell) = &mut self.shell {
                    shell.detach();
                }
            }
            MobileEvent::Resume => {
                self.resumed = true;
                if let Some(shell) = &self.shell {
                    shell.ctx.audio.resume();
                }
            }
            MobileEvent::Pause => {
                self.resumed = false;
                if let Some(shell) = &self.shell {
                    shell.ctx.audio.pause();
                }
            }
            MobileEvent::Insets(insets) => {
                self.safe_area = insets.safe_area.into();
                if let Some(shell) = &mut self.shell {
                    shell.ctx.safe_area = self.safe_area;
                }
            }
            MobileEvent::Permissions(outcome) => self.permissions(outcome.into()),
            // A link with one of the app's schemes. Intents without data,
            // such as the launcher's, are not links.
            MobileEvent::Intent(intent) if intent.action.as_deref() == Some(ACTION_VIEW) => {
                if let Some(url) = intent.data {
                    match &mut self.shell {
                        Some(shell) => shell.open_url(url),
                        None => self.urls.push(url),
                    }
                }
            }
            // The foreground service, on the main loop: also while suspended.
            #[cfg(feature = "mobile-foreground-service")]
            MobileEvent::Service(event) => {
                match (
                    &mut self.shell,
                    crate::service::from_mobile_event(event.clone()),
                ) {
                    (Some(shell), Some(event)) => shell.app.on_service(&mut shell.ctx, &event),
                    (None, _) => log::warn!("libmorph: a service event before the app exists"),
                    (_, None) => log::debug!("libmorph: an unknown service event: {event:?}"),
                }
            }
            MobileEvent::ConfigChanged => {
                if let Some(shell) = &mut self.shell {
                    shell.ctx.scale_factor = scale_factor(app);
                }
            }
            // The loop returns after this event: the end of the run.
            MobileEvent::Destroy => {
                if let Some(shell) = self.shell.take() {
                    shell.shutdown();
                }
            }
            _ => {}
        }
        self.check_exit(app);
    }

    fn input(&mut self, app: &AndroidApp, event: &InputEvent<'_>) -> InputStatus {
        let status = match event {
            InputEvent::MotionEvent(motion) => {
                self.touches(motion);
                InputStatus::Handled
            }
            InputEvent::KeyEvent(key) => self.key(key),
            _ => InputStatus::Unhandled,
        };
        self.check_exit(app);
        status
    }

    fn tick(&mut self, app: &AndroidApp) {
        self.frame(app);
        self.check_exit(app);
    }

    fn poll_timeout(&self) -> Option<Duration> {
        // Render continuously while visible; Fifo presentation paces it.
        if self.resumed && self.shell.as_ref().is_some_and(Shell::has_surface) {
            Some(Duration::ZERO)
        } else {
            Some(DEFAULT_POLL_TIMEOUT)
        }
    }
}
