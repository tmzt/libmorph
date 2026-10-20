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
use crate::{GpuError, Key, KeyStatus, MorphApp, MorphContext, SafeArea, Touch, TouchPhase};

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
}

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
        let mut ctx = MorphContext::new(gpu, spec, Vec::new(), crate::Audio::open());
        ctx.safe_area = self.safe_area;
        ctx.scale_factor = scale_factor(app);
        ctx.data_dir = app.internal_data_path();
        ctx.pipeline_cache = ctx.data_dir.as_deref().and_then(|dir| {
            PipelineCacheStore::open(&ctx.gpu.device, &ctx.gpu.adapter.get_info(), dir)
        });
        match Shell::start(ctx, surface, size) {
            Ok(shell) => self.shell = Some(shell),
            Err(error) => {
                log::error!("libmorph: {error}");
                self.failed = true;
            }
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
