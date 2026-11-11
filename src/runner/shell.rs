//! The host-independent half of a runner: the app, its context and the
//! current surface, and the per-frame sequence.

use std::time::Duration;

use web_time::Instant;

use crate::context::Target;
use crate::gpu::WgpuConfig;
use crate::log_once::log_once;
use crate::{Frame, MorphApp, MorphContext, MorphError, Owned, RenderStatus, Touch};
use crate::{GpuEvent, GpuInBackground};

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
use super::overlay::Overlay;

/// What a frame came to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FrameStatus {
    Presented,
    /// No surface, or the platform asked to skip this frame.
    Skipped,
    /// The surface is lost: the runner must create a new one and
    /// [`attach`](Shell::attach) it.
    Lost,
}

/// Whether the app has been told it is suspended. Keeps `on_suspend` and
/// `on_resume` to one call per transition, and exit to a single `on_suspend`
/// however many teardown paths fire.
///
/// Intents and links are not held here: they are delivered when they
/// arrive, also while suspended (plan 20261111_03, decision 6).
#[derive(Debug, Default)]
struct Lifecycle {
    suspended: bool,
}

impl Lifecycle {
    /// Becoming suspended: whether the app must hear `on_suspend`.
    fn suspend(&mut self) -> bool {
        !std::mem::replace(&mut self.suspended, true)
    }

    /// Becoming resumed: whether the app must hear `on_resume`.
    fn resume(&mut self) -> bool {
        std::mem::replace(&mut self.suspended, false)
    }
}

/// How long a failed restore waits before the next try, while the window
/// is up (plan 20261111_03, decision 10).
const RETRY: Duration = Duration::from_secs(2);

/// How long a release waits for the device to finish its work (decision 9).
const RELEASE_WAIT: Duration = Duration::from_secs(2);

/// A surface that came while the device was released and could not be
/// restored for: kept, and the restore tried again every [`RETRY`].
enum Pending {
    Surface(wgpu::Surface<'static>, (u32, u32)),
    /// A scripted run's `resume`.
    #[cfg(script_runner)]
    Offscreen,
}

/// When a failed restore is tried again: every [`RETRY`] after the last
/// failure, while the window is up.
#[derive(Debug, Default)]
struct Retry {
    at: Option<Instant>,
}

impl Retry {
    fn failed(&mut self, now: Instant) {
        self.at = Some(now + RETRY);
    }

    fn due(&self, now: Instant) -> bool {
        self.at.is_some_and(|at| now >= at)
    }

    fn clear(&mut self) {
        self.at = None;
    }
}

pub(crate) struct Shell<A> {
    pub ctx: MorphContext,
    pub app: A,
    lifecycle: Lifecycle,
    last_frame: Instant,
    /// Scripted runs: the `dt` every frame gets, instead of the clock.
    fixed_dt: Option<Duration>,
    /// Scripted runs: the time, advanced by `dt` each frame (and each frame
    /// that passes while suspended), so a release's `after` is counted in
    /// frames. `None` reads the wall clock.
    script_now: Option<Instant>,
    /// When the app last lost its surface: the start of a releasing app's
    /// `after`. `None` while it has one.
    off_since: Option<Instant>,
    /// A surface waiting for a restore that failed.
    pending: Option<Pending>,
    retry: Retry,
    /// Input was dropped for want of a device since it was last present:
    /// logged once per stretch.
    dropping_input: bool,
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    pub overlay: Option<Overlay>,
    /// The runner asked for the overlay: rebuilt after a restore.
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    overlay_wanted: bool,
}

impl<A: MorphApp> Shell<A> {
    /// `A::wgpu_config`, but only for an app that owns the frame: a
    /// managed app's surface knob is `SRGB_SURFACE`, and its configuration
    /// stays the one it had before the hook existed. `A` is
    /// monomorphized, so this folds to one arm at compile time.
    fn wgpu_config() -> Option<WgpuConfig> {
        if A::OWNS.contains(Owned::FRAME) {
            Some(A::wgpu_config)
        } else {
            None
        }
    }

    /// Configure the first surface, then build the app against it.
    pub fn start(
        mut ctx: MorphContext,
        surface: wgpu::Surface<'static>,
        size: (u32, u32),
    ) -> Result<Self, MorphError> {
        let config = ctx
            .device
            .as_ref()
            .and_then(|opened| {
                opened.configure(&surface, size, None, A::SRGB_SURFACE, Self::wgpu_config())
            })
            .ok_or(MorphError::NoSurfaceFormat)?;
        ctx.surface_format = config.format;
        ctx.surface_size = (config.width, config.height);
        Ok(Self::with_app(ctx, Target::Surface(surface, config), None))
    }

    fn with_app(mut ctx: MorphContext, target: Target, fixed_dt: Option<Duration>) -> Self {
        ctx.target = Some(target);
        // The context is not generic over the app, so the one thing it
        // needs from `OWNS` is read once, here, before `init`.
        ctx.owns_frame = A::OWNS.contains(Owned::FRAME);
        let mut app = A::init(&mut ctx);
        if ctx.audio.capture_format().is_some() {
            app.on_capture_ready(&mut ctx);
        }
        // Pipelines built in `init` are kept even if the process is killed
        // without a clean suspend.
        ctx.save_pipeline_cache();
        Self {
            ctx,
            app,
            lifecycle: Lifecycle::default(),
            last_frame: Instant::now(),
            fixed_dt,
            script_now: fixed_dt.map(|_| Instant::now()),
            off_since: None,
            pending: None,
            retry: Retry::default(),
            dropping_input: false,
            #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
            overlay: None,
            #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
            overlay_wanted: false,
        }
    }

    /// Draw the desktop overlay over every frame, from now on and after
    /// each restore.
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    pub fn enable_overlay(&mut self) {
        self.overlay_wanted = true;
        self.overlay = Overlay::new(&self.ctx);
    }

    /// The time the release clock and the restore retry read.
    fn now(&self) -> Instant {
        self.script_now.unwrap_or_else(Instant::now)
    }

    /// `after` for an app that releases its device off screen; `None` for
    /// one that keeps it, and on the web, where nothing is released.
    fn release_after() -> Option<Duration> {
        match A::GPU_IN_BACKGROUND {
            GpuInBackground::Release { after } if !cfg!(target_arch = "wasm32") => Some(after),
            _ => None,
        }
    }

    /// Whether the app gives its device back off screen.
    pub fn releases() -> bool {
        Self::release_after().is_some()
    }

    /// Whether the app has been told it is suspended.
    #[cfg(script_runner)]
    pub fn suspended(&self) -> bool {
        self.lifecycle.suspended
    }

    /// Whether there is a device: `false` only while released.
    pub fn has_device(&self) -> bool {
        self.ctx.device.is_some()
    }

    /// The instance, kept for the whole run: surfaces are created from it.
    pub fn instance(&self) -> &wgpu::Instance {
        &self.ctx.instance
    }

    pub fn has_surface(&self) -> bool {
        self.ctx.target.is_some()
    }

    /// Install a new surface: after a resume, or to replace a lost one. The
    /// app hears [`MorphApp::on_resume`] only if it was suspended. If the
    /// device was released, it is restored first, and the app hears
    /// [`MorphApp::on_gpu`] with [`GpuEvent::Restored`] before `on_resume`;
    /// if that fails the surface is kept and the restore tried again every
    /// [`RETRY`] from [`poll_gpu`](Self::poll_gpu).
    pub fn attach(&mut self, surface: wgpu::Surface<'static>, size: (u32, u32)) {
        let restored = !self.has_device();
        if restored && !self.restore(Some(&surface)) {
            self.pending = Some(Pending::Surface(surface, size));
            return;
        }
        let previous = Some(self.ctx.surface_format);
        let Some(config) = self.ctx.device.as_ref().and_then(|opened| {
            opened.configure(
                &surface,
                size,
                previous,
                A::SRGB_SURFACE,
                Self::wgpu_config(),
            )
        }) else {
            log::error!("libmorph: the new surface has no presentable format");
            return;
        };
        self.ctx.surface_format = config.format;
        self.ctx.surface_size = (config.width, config.height);
        self.install(Target::Surface(surface, config), restored);
    }

    /// The new target is in place: `on_gpu(Restored)` after a restore,
    /// then `on_resume` if the app was suspended.
    fn install(&mut self, target: Target, restored: bool) {
        self.ctx.target = Some(target);
        self.off_since = None;
        // Frame time does not accumulate across a suspension.
        self.last_frame = Instant::now();
        if restored {
            self.app.on_gpu(&mut self.ctx, &GpuEvent::Restored);
            self.gpu_report("restore");
        }
        if self.lifecycle.resume() {
            self.ctx.camera.resume();
            self.app.on_resume(&mut self.ctx);
        }
    }

    /// A device again, after a release (decision 10): an adapter for
    /// `surface` and a device with the limits first granted, then the
    /// pipeline cache reopened from its saved data and the overlay rebuilt.
    /// The caller configures the surface and tells the app. On failure it
    /// logs, schedules the next try and returns `false`.
    #[cfg(not(target_arch = "wasm32"))]
    fn restore(&mut self, surface: Option<&wgpu::Surface<'static>>) -> bool {
        let reopened = pollster::block_on(crate::gpu::reopen(
            &self.ctx.instance,
            surface,
            self.ctx.request,
            &self.ctx.limits,
            A::MEMORY_HINTS,
        ));
        let opened = match reopened {
            Ok(opened) => opened,
            Err(error) => {
                log::error!(
                    "libmorph: the GPU could not be restored ({error}); trying again in {} s",
                    RETRY.as_secs()
                );
                let now = self.now();
                self.retry.failed(now);
                return false;
            }
        };
        log::info!("libmorph: GPU restored");
        self.retry.clear();
        self.dropping_input = false;
        if self.ctx.request.pipeline_cache
            && let Some(dir) = self.ctx.data_dir.as_deref()
        {
            self.ctx.pipeline_cache = crate::pipeline_cache::PipelineCacheStore::open(
                &opened.gpu.device,
                &opened.adapter.get_info(),
                dir,
            );
        }
        self.ctx.device = Some(opened);
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        if self.overlay_wanted {
            self.overlay = Overlay::new(&self.ctx);
        }
        true
    }

    /// The web never releases, so it never restores.
    #[cfg(target_arch = "wasm32")]
    fn restore(&mut self, _surface: Option<&wgpu::Surface<'static>>) -> bool {
        false
    }

    /// Give the device back (decision 9): `on_gpu(Release)`, then the
    /// pipeline cache saved and dropped, the overlay dropped, the device
    /// waited on so queued work and dropped objects finish, and the device,
    /// queue and adapter dropped. Only for a releasing app, suspended, with
    /// a device.
    fn release(&mut self, why: &str) {
        if !Self::releases() || !self.lifecycle.suspended || !self.has_device() {
            return;
        }
        self.app.on_gpu(&mut self.ctx, &GpuEvent::Release);
        self.ctx.save_pipeline_cache();
        self.ctx.pipeline_cache = None;
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        {
            self.overlay = None;
        }
        if let Some(device) = self.ctx.device() {
            let wait = wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(RELEASE_WAIT),
            };
            if let Err(error) = device.poll(wait) {
                log::warn!("libmorph: waiting on the GPU before the release: {error}");
            }
        }
        self.gpu_report("release");
        self.ctx.device = None;
        log::info!("libmorph: GPU released ({why})");
    }

    /// The release clock and the restore retry, called from the runner's
    /// loop: releases a releasing app's device once it has been off screen
    /// for `after`, and tries a failed restore again when it is due.
    /// Returns when it next needs calling, for the runner's wait.
    pub fn poll_gpu(&mut self) -> Option<Instant> {
        let after = Self::release_after()?;
        let now = self.now();
        if self.has_device() {
            let due = self.off_since? + after;
            if now < due {
                return Some(due);
            }
            self.release("off screen");
            return None;
        }
        if self.pending.is_some() && self.retry.due(now) {
            match self.pending.take() {
                Some(Pending::Surface(surface, size)) => self.attach(surface, size),
                #[cfg(script_runner)]
                Some(Pending::Offscreen) => self.attach_offscreen(),
                None => {}
            }
        }
        self.retry.at
    }

    /// Memory pressure: a releasing app off screen gives its device back at
    /// once (decision 8).
    #[cfg(any(script_runner, target_os = "android"))]
    fn release_under_pressure(&mut self, why: &str) {
        if self.off_since.is_some() {
            self.release(why);
        }
    }

    /// A link the app was opened with, delivered now, also while suspended.
    /// The `open` verb's, scripted runs only - Android's links come in
    /// through [`intent`](Self::intent), which delivers the whole event.
    #[cfg(script_runner)]
    pub fn open_url(&mut self, url: &str) {
        self.app.on_open_url(&mut self.ctx, url);
    }

    /// An intent that reached the app, delivered now, also while
    /// suspended.
    #[cfg(any(script_runner, target_os = "android"))]
    pub fn intent(&mut self, intent: &crate::Intent) {
        self.deliver_intent(intent);
    }

    /// `on_intent`, and then `on_open_url` for an intent that is a link.
    ///
    /// One place, so the two hooks cannot come to disagree about which
    /// intents are links or about which order they arrive in.
    #[cfg(any(script_runner, target_os = "android"))]
    fn deliver_intent(&mut self, intent: &crate::Intent) {
        self.app.on_intent(&mut self.ctx, intent);
        if let Some(url) = intent.link() {
            self.app.on_open_url(&mut self.ctx, url);
        }
    }

    /// The system is low on memory: tell the app, suspended or not, then
    /// release a releasing app's device if it is off screen.
    #[cfg(any(script_runner, target_os = "android"))]
    pub fn low_memory(&mut self) {
        self.app.on_low_memory(&mut self.ctx);
        self.release_under_pressure("low memory");
    }

    /// Android asked the app to trim memory: tell it, suspended or not,
    /// then, from `Background` up, release a releasing app's device if it
    /// is off screen.
    #[cfg(any(script_runner, target_os = "android"))]
    pub fn trim_memory(&mut self, level: crate::TrimLevel) {
        self.app.on_trim_memory(&mut self.ctx, level);
        if level >= crate::TrimLevel::Background {
            self.release_under_pressure("trim memory");
        }
    }

    /// Drop the surface (it must not outlive the platform window) and tell
    /// the app, unless it already knows it is suspended.
    pub fn detach(&mut self) {
        self.ctx.target = None;
        // A surface waiting for a restore goes with the window too.
        self.pending = None;
        self.retry.clear();
        if Self::releases() && self.off_since.is_none() {
            self.off_since = Some(self.now());
        }
        if self.lifecycle.suspend() {
            self.ctx.camera.suspend();
            self.camera_events();
            self.app.on_suspend(&mut self.ctx);
            self.gpu_report("suspend");
        }
        self.ctx.save_pipeline_cache();
    }

    /// Log the allocator report once, `at` a point of the lifecycle.
    pub fn gpu_report(&self, at: &str) {
        log::info!("{}", crate::gpu::report_line(at, self.ctx.device()));
    }

    /// Whether the app called [`MorphContext::exit`].
    pub fn exit_requested(&self) -> bool {
        self.ctx.exit_requested
    }

    /// The end of the run, however it came: `on_suspend` exactly once (unless
    /// the app is already suspended), then the surface, then the app.
    pub fn shutdown(mut self) {
        if self.lifecycle.suspend() {
            self.app.on_suspend(&mut self.ctx);
            self.gpu_report("suspend");
            self.ctx.save_pipeline_cache();
        }
        self.ctx.target = None;
        drop(self.app);
    }

    pub fn resize(&mut self, (width, height): (u32, u32)) {
        let size = (width.max(1), height.max(1));
        // A scripted run has no surface to configure: its texture is
        // replaced at the new size instead, which is what a window being
        // resized does to an offscreen target. Without this the whole
        // method was a no-op there and a script could not resize at all.
        #[cfg(script_runner)]
        if matches!(self.ctx.target, Some(Target::Offscreen(_))) {
            self.ctx.surface_size = size;
            self.ctx.target = offscreen_texture(&self.ctx).map(Target::Offscreen);
            return;
        }
        let (Some(Target::Surface(surface, config)), Some(opened)) =
            (&mut self.ctx.target, &self.ctx.device)
        else {
            return;
        };
        (config.width, config.height) = size;
        surface.configure(&opened.gpu.device, config);
        self.ctx.surface_size = (config.width, config.height);
    }

    /// Deliver the camera's waiting answers to `on_camera`, oldest first.
    /// Answers the app's own `on_camera` causes wait for the next call.
    pub fn camera_events(&mut self) {
        for event in self.ctx.camera.take_events() {
            self.app.on_camera(&mut self.ctx, &event);
        }
    }

    /// Check the audio streams (stalls, losses, attempts due) and deliver
    /// what changed to `on_audio`, oldest first.
    pub fn audio_events(&mut self) {
        self.ctx.audio.poll(Instant::now());
        for event in self.ctx.audio.take_events() {
            self.app.on_audio(&mut self.ctx, &event);
        }
    }

    /// Deliver the media answers waiting to `on_media`, oldest first.
    pub fn media_events(&mut self) {
        for response in self.ctx.media.take_responses() {
            self.app.on_media(&mut self.ctx, &response);
        }
    }

    /// One media answer from the platform (Android), now.
    #[cfg(all(target_os = "android", feature = "mobile-media-request"))]
    pub fn media_response(&mut self, response: libmobile::media::MediaResponse) {
        let response = self.ctx.media.android_response(response);
        self.app.on_media(&mut self.ctx, &response);
    }

    /// One camera event from the platform (Android), now.
    #[cfg(all(target_os = "android", feature = "mobile-camera"))]
    pub fn camera_event(&mut self, event: &crate::camera::CameraEvent) {
        self.app.on_camera(&mut self.ctx, event);
    }

    /// Input reaches the app only with a device (decision 4): what arrives
    /// with a window but none, after a failed restore, is dropped, with one
    /// log line per stretch.
    fn input_allowed(&mut self) -> bool {
        if self.has_device() {
            return true;
        }
        if !std::mem::replace(&mut self.dropping_input, true) {
            log::warn!("libmorph: input dropped: the GPU is released and not yet restored");
        }
        false
    }

    pub fn touch(&mut self, touch: Touch) {
        if self.input_allowed() {
            self.app.on_touch(&mut self.ctx, touch);
        }
    }

    pub fn key(&mut self, key: crate::Key) -> crate::KeyStatus {
        if self.input_allowed() {
            self.app.on_key(&mut self.ctx, key)
        } else {
            crate::KeyStatus::Unhandled
        }
    }

    pub fn text_input(&mut self, event: crate::TextInputEvent) {
        if self.input_allowed() {
            self.app.on_text_input(&mut self.ctx, &event);
        }
    }

    #[cfg(not(target_os = "android"))]
    pub fn hover(&mut self, x: f32, y: f32) {
        if self.input_allowed() {
            self.app.on_hover(&mut self.ctx, x, y);
        }
    }

    /// A scripted run's clock moves on by one frame's `dt`.
    fn tick_script_clock(&mut self) {
        if let (Some(now), Some(dt)) = (&mut self.script_now, self.fixed_dt) {
            *now += dt;
        }
    }

    /// One iteration: the subsystem pumps and `update`, then either
    /// libmorph's frame or the app's.
    ///
    /// `A` is monomorphized, so the branch folds at compile time and a
    /// managed app is compiled to the managed path alone.
    pub fn frame(&mut self) -> FrameStatus {
        self.tick_script_clock();
        // No frames without a device (decision 4).
        if !self.has_device() {
            return FrameStatus::Skipped;
        }
        if A::OWNS.contains(Owned::FRAME) {
            self.app_frame()
        } else {
            self.managed_frame()
        }
    }

    /// `Owned::FRAME`: the pumps and `update` as always, then
    /// `construct_frame` in place of libmorph's acquire, `render`, overlay
    /// and present.
    fn app_frame(&mut self) -> FrameStatus {
        if self.ctx.target.is_none() {
            return FrameStatus::Skipped;
        }
        self.pump_and_update();
        // Cleared each iteration: dropping a frame rather than presenting
        // it is how the app skips, and must cost it nothing next time.
        self.ctx.frame_held = false;
        self.ctx.frame_presented = false;
        self.app.construct_frame(&mut self.ctx);
        if self.ctx.frame_held {
            log_once!(
                "libmorph: a frame acquired in construct_frame was not \
                 presented before it returned. libmorph reconfigures the \
                 surface between iterations, and Android drops it inside \
                 the window-destroyed handler, so an image held across \
                 either is outdated at best. To skip an iteration, do not \
                 acquire."
            );
        }
        // The surface, not the picture: the runner only needs to know
        // whether it must build a new one.
        if std::mem::take(&mut self.ctx.surface_lost) {
            return FrameStatus::Lost;
        }
        if self.ctx.frame_presented {
            FrameStatus::Presented
        } else {
            FrameStatus::Skipped
        }
    }

    /// Update, then render and present if the surface hands out a texture
    /// and the app says the frame needs presenting.
    fn managed_frame(&mut self) -> FrameStatus {
        let surface = match &self.ctx.target {
            None => return FrameStatus::Skipped,
            Some(Target::Surface(surface, _)) => surface,
            #[cfg(script_runner)]
            Some(Target::Offscreen(texture)) => {
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                // The app's own answer, so both targets report alike. A
                // skipped frame keeps the texture's prior contents, which
                // is what a device's surface does; a texture nothing has
                // written reads black, since wgpu zero-initializes it on
                // first read.
                return match self.draw(&view) {
                    RenderStatus::Drawn => FrameStatus::Presented,
                    RenderStatus::Skipped => FrameStatus::Skipped,
                };
            }
        };
        let texture = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => Ok(texture),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Err(texture),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return FrameStatus::Skipped;
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.ctx.reconfigure();
                return FrameStatus::Skipped;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.ctx.target = None;
                return FrameStatus::Lost;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("libmorph: validation error acquiring the surface texture");
                return FrameStatus::Skipped;
            }
        };
        let suboptimal = texture.is_err();
        let texture = texture.unwrap_or_else(|texture| texture);
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let status = self.draw(&view);
        // Nothing was drawn, so there is nothing to present: the texture
        // goes back unpresented and the previous frame stays on screen.
        // The surface's own business is still finished - a suboptimal
        // swapchain is reconfigured either way, once the texture is gone.
        if status == RenderStatus::Skipped {
            drop(view);
            drop(texture);
            if suboptimal {
                self.ctx.reconfigure();
            }
            return FrameStatus::Skipped;
        }
        // A frame is only drawn with a device.
        if let Some(opened) = &self.ctx.device {
            opened.gpu.queue.present(texture);
        }
        if suboptimal {
            self.ctx.reconfigure();
        }
        FrameStatus::Presented
    }

    /// The per-iteration work that is libmorph's under either setting: the
    /// subsystem pumps, then `update`. Taking the frame cannot silence a
    /// subsystem libmorph still drives.
    fn pump_and_update(&mut self) {
        self.audio_events();
        self.camera_events();
        self.media_events();
        let now = Instant::now();
        let dt = self.fixed_dt.unwrap_or(now - self.last_frame);
        self.last_frame = now;
        self.app.update(&mut self.ctx, dt);
    }

    /// `update`, `render` and, if the app drew, the overlay into `view`.
    fn draw(&mut self, view: &wgpu::TextureView) -> RenderStatus {
        self.pump_and_update();
        let frame = Frame {
            view,
            format: self.ctx.surface_format,
            size: self.ctx.surface_size,
        };
        let status = self.app.render(&mut self.ctx, &frame);
        // The overlay goes over the app's frame, so a skipped one has no
        // base under it and it is not drawn: an emulated cutout or a
        // pressed dpad button does not appear while frames are skipped.
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        if status == RenderStatus::Drawn
            && let Some(overlay) = &mut self.overlay
        {
            overlay.draw(&self.ctx, view);
        }
        status
    }
}

/// Scripted runs: an offscreen texture in place of a surface.
#[cfg(script_runner)]
impl<A: MorphApp> Shell<A> {
    /// While suspended, `n` frames' time passes with no frame run, and the
    /// release clock is read after each (decision 14).
    pub fn idle_frames(&mut self, n: u32) {
        for _ in 0..n {
            self.tick_script_clock();
            self.poll_gpu();
        }
    }

    /// The offscreen format: RGBA so readback needs no channel swap, sRGB
    /// or not as the app asks.
    pub fn offscreen_format() -> wgpu::TextureFormat {
        if A::SRGB_SURFACE {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        }
    }

    /// Create the first offscreen target at `size`, then build the app.
    /// Every frame gets `dt`.
    pub fn start_offscreen(mut ctx: MorphContext, size: (u32, u32), dt: Duration) -> Self {
        ctx.surface_format = Self::offscreen_format();
        ctx.surface_size = (size.0.max(1), size.1.max(1));
        let texture = offscreen_texture(&ctx).expect("a device at bring-up");
        Self::with_app(ctx, Target::Offscreen(texture), Some(dt))
    }

    /// A new offscreen target in the kept format and size, after
    /// [`detach`](Self::detach).
    /// Restores the device first if it was released.
    pub fn attach_offscreen(&mut self) {
        let restored = !self.has_device();
        if restored && !self.restore(None) {
            self.pending = Some(Pending::Offscreen);
            return;
        }
        if let Some(texture) = offscreen_texture(&self.ctx) {
            self.install(Target::Offscreen(texture), restored);
        }
    }

    /// The offscreen texture, while attached.
    pub fn offscreen(&self) -> Option<&wgpu::Texture> {
        match &self.ctx.target {
            Some(Target::Offscreen(texture)) => Some(texture),
            _ => None,
        }
    }
}

/// A texture at the context's size and format: `None` without a device.
#[cfg(script_runner)]
fn offscreen_texture(ctx: &MorphContext) -> Option<wgpu::Texture> {
    let (width, height) = ctx.surface_size;
    Some(ctx.device()?.create_texture(&wgpu::TextureDescriptor {
        label: Some("libmorph offscreen"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: ctx.surface_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    }))
}

#[cfg(test)]
mod tests {
    use super::{Lifecycle, Retry};
    use std::time::Duration;

    #[test]
    fn one_suspend_and_resume_per_transition() {
        let mut life = Lifecycle::default();
        assert!(!life.resume(), "running: nothing to resume");
        assert!(life.suspend());
        assert!(!life.suspend(), "already suspended");
        assert!(life.resume());
        assert!(!life.resume());
    }

    /// A failed restore is tried again every two seconds after the last
    /// failure, and not before; a success, or the window going, stops it.
    #[test]
    fn a_failed_restore_is_retried_every_two_seconds() {
        let start = web_time::Instant::now();
        let mut retry = Retry::default();
        assert!(!retry.due(start), "nothing failed");
        retry.failed(start);
        assert!(!retry.due(start + Duration::from_millis(1999)));
        assert!(retry.due(start + Duration::from_secs(2)));
        retry.failed(start + Duration::from_secs(2));
        assert!(!retry.due(start + Duration::from_secs(3)));
        assert!(retry.due(start + Duration::from_secs(4)));
        retry.clear();
        assert!(!retry.due(start + Duration::from_secs(60)));
    }

    #[test]
    fn exit_suspends_once() {
        // Exit while running: the shutdown suspend is the one call.
        let mut life = Lifecycle::default();
        assert!(life.suspend());
        // Exit after the window went away (Android `WindowDestroyed`, then
        // `Destroy`): already told, so the shutdown is silent.
        let mut life = Lifecycle::default();
        assert!(life.suspend(), "WindowDestroyed");
        assert!(!life.suspend(), "Destroy");
        // Exit after a suspend and a resume suspends again, once.
        let mut life = Lifecycle::default();
        life.suspend();
        life.resume();
        assert!(life.suspend());
    }
}
