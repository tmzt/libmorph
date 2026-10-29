//! The trait an application implements.

use std::time::Duration;

use crate::service::ServiceEvent;
use crate::{AudioSetup, Key, KeyStatus, LimitsRequest, MorphContext, Permissions, Touch};

/// The surface texture for one frame. libmorph presents it after
/// [`MorphApp::render`] returns.
pub struct Frame<'a> {
    pub view: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    /// Physical pixels.
    pub size: (u32, u32),
}

/// A libmorph application. Every hook runs on the main thread with the
/// shared [`MorphContext`].
pub trait MorphApp: Sized + 'static {
    /// Prefer an sRGB surface format (the GPU encodes gamma on write). Set
    /// `false` for apps whose shaders gamma-encode themselves. Either way, a
    /// format already in use is kept across resume when the new surface
    /// offers it.
    const SRGB_SURFACE: bool = true;

    /// On macOS, the Keychain service of
    /// [`MorphContext::secure_store`](crate::MorphContext::secure_store) is
    /// `libmorph.<SECURE_STORE_ID>`; empty (the default) uses the
    /// executable's file stem. Unused elsewhere.
    const SECURE_STORE_ID: &'static str = "";

    /// The app's directory name on the desktop, for
    /// [`MorphContext::data_dir`] and [`cache_dir`](MorphContext::cache_dir):
    /// `~/Library/Application Support/<APP_ID>` and `~/Library/Caches/<APP_ID>`
    /// on macOS. 1 to 64 of `[A-Za-z0-9._-]`, not `.` or `..`; empty (the
    /// default) uses the executable's file stem, with any other character
    /// replaced by `_`. Android uses the package's own directories instead.
    const APP_ID: &'static str = "";

    /// The device limits to ask for, before [`init`](Self::init). `base` is
    /// what libmorph asks for by default: the downlevel defaults (WebGL2
    /// defaults on a GL adapter) with texture sizes raised to the adapter's.
    /// Limits in `required` that the adapter lacks fail bring-up with
    /// [`GpuError::Limits`](crate::GpuError::Limits); `preferred` is granted
    /// as far as the adapter allows. Read what was granted from
    /// `ctx.limits()`.
    ///
    /// ```
    /// # use libmorph::{wgpu, LimitsRequest};
    /// fn limits(base: &wgpu::Limits) -> LimitsRequest {
    ///     let required = wgpu::Limits { max_storage_buffers_per_shader_stage: 8, ..base.clone() };
    ///     LimitsRequest { preferred: required.clone(), required }
    /// }
    /// ```
    fn limits(base: &wgpu::Limits) -> LimitsRequest {
        LimitsRequest::base(base)
    }

    /// What audio to open, before [`init`](Self::init). `setup` starts with
    /// output on and capture off; set `setup.capture` to record from the
    /// default input device, and read it through `ctx.audio()`'s
    /// [`add_sink`](crate::Audio::add_sink). On Android, libmorph asks for
    /// `RECORD_AUDIO` after `init` if it is not granted yet (the app's
    /// manifest must declare it); see
    /// [`on_capture_ready`](Self::on_capture_ready).
    ///
    /// ```
    /// # use libmorph::AudioSetup;
    /// fn audio_setup(setup: &mut AudioSetup) {
    ///     setup.capture = true;
    /// }
    /// ```
    fn audio_setup(setup: &mut AudioSetup) {
        let _ = setup;
    }

    /// Build the app once the GPU is up and the first surface is configured,
    /// so `ctx.surface_format()` is already known.
    fn init(ctx: &mut MorphContext) -> Self;

    /// Advance state by `dt`, once per frame before [`render`](Self::render).
    fn update(&mut self, ctx: &mut MorphContext, dt: Duration) {
        let _ = (ctx, dt);
    }

    /// Record and submit this frame's work to `frame.view`.
    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>);

    /// A touch, or on desktop the mouse translated into one.
    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        let _ = (ctx, touch);
    }

    /// The cursor moved with no button held (desktop and web). Physical
    /// pixels, like touches.
    fn on_hover(&mut self, ctx: &mut MorphContext, x: f32, y: f32) {
        let _ = (ctx, x, y);
    }

    /// A key was pressed. On Android, `Handled` keeps Back inside the app and
    /// `Unhandled` lets the system act; desktop ignores the result.
    fn on_key(&mut self, ctx: &mut MorphContext, key: Key) -> KeyStatus {
        let _ = (ctx, key);
        KeyStatus::Unhandled
    }

    /// Text input event: a change in text, an action key press, or keyboard dismissal.
    fn on_text_input(&mut self, ctx: &mut MorphContext, event: &crate::TextInputEvent) {
        let _ = (ctx, event);
    }

    /// The capture stream asked for in [`audio_setup`](Self::audio_setup)
    /// has opened, and `ctx.audio().capture_format()` is `Some` from now on.
    /// Called once per run, after [`init`](Self::init): right after it when
    /// capture opened with the rest of audio, or on Android once the user
    /// allows `RECORD_AUDIO`. Never called if capture did not open.
    fn on_capture_ready(&mut self, ctx: &mut MorphContext) {
        let _ = ctx;
    }

    /// The answer to a
    /// [`ctx.request_permissions`](MorphContext::request_permissions) call,
    /// once per request the app made. Answers to libmorph's own requests
    /// (`RECORD_AUDIO` for capture) are not passed here.
    fn on_permissions(&mut self, ctx: &mut MorphContext, answer: &Permissions) {
        let _ = (ctx, answer);
    }

    /// The app was opened with `url`, a link with one of the URL schemes its
    /// build script declared (`gradle_project(id).url_scheme("myapp")`), at
    /// launch or while running. Called after [`init`](Self::init), in the
    /// order the links arrived; links that arrive while the app is suspended
    /// come after [`on_resume`](Self::on_resume). Android only, and scripted
    /// runs (`open <url>`). The URL is untrusted input: validate it, and do
    /// not log it past its path, since a query or fragment may be a secret.
    fn on_open_url(&mut self, ctx: &mut MorphContext, url: &str) {
        let _ = (ctx, url);
    }

    /// The foreground service ([`service`](crate::service)) did something,
    /// or its notification was tapped: an app button
    /// ([`ServiceEvent::Action`]), Exit ([`ServiceEvent::Exited`], after
    /// which the activity closes), a wake-lock change, `Started`, `Stopped`,
    /// `Failed` and the rest. On the main thread, whether or not the app is
    /// suspended, after [`init`](Self::init). Android with
    /// `mobile-foreground-service` only; the app's other threads hear the
    /// same through [`system_events`](crate::system_events).
    fn on_service(&mut self, ctx: &mut MorphContext, event: &ServiceEvent) {
        let _ = (ctx, event);
    }

    /// The camera answered ([`camera`](crate::camera)): a session opened,
    /// closed (with the reason when it failed, such as
    /// [`CameraError::PermissionDenied`](crate::camera::CameraError::PermissionDenied)
    /// or [`Unsupported`](crate::camera::CameraError::Unsupported)), was
    /// suspended or zoomed, a capture finished, or a QR code was read. On
    /// the main thread, before input and [`update`](Self::update), so the
    /// frame after it sees the result; after [`init`](Self::init).
    fn on_camera(&mut self, ctx: &mut MorphContext, event: &crate::camera::CameraEvent) {
        let _ = (ctx, event);
    }

    /// A media request answered ([`media`](crate::media)): the chosen items,
    /// or why there are none (`Cancelled`,
    /// [`PermissionDenied`](crate::media::MediaError::PermissionDenied),
    /// [`Unsupported`](crate::media::MediaError::Unsupported)...). On the
    /// main thread, after [`init`](Self::init): on Android as soon as it
    /// arrives, also while suspended (one that arrived before the app
    /// existed comes right after `init`, with `request: None`); elsewhere
    /// before the next frame's [`update`](Self::update).
    fn on_media(&mut self, ctx: &mut MorphContext, response: &crate::media::MediaResponse) {
        let _ = (ctx, response);
    }

    /// The system is low on memory (Android's `onLowMemory`). Free what can
    /// be rebuilt: caches, offscreen resources, decoded data. Called once
    /// per report after [`init`](Self::init), whether or not the app is
    /// suspended. Never called on the desktop or the web, or in scripted
    /// runs unless the script says `low-memory`.
    fn on_low_memory(&mut self, ctx: &mut MorphContext) {
        let _ = ctx;
    }

    /// Android asked the app to trim memory (`onTrimMemory`). Levels are
    /// ordered, so `level >= TrimLevel::Background` means "free everything
    /// you can". Called once per report after [`init`](Self::init),
    /// whether or not the app is suspended. Never called on the desktop or
    /// the web, or in scripted runs unless the script says
    /// `trim-memory <level>`.
    fn on_trim_memory(&mut self, ctx: &mut MorphContext, level: crate::TrimLevel) {
        let _ = (ctx, level);
    }

    /// The surface is gone (app backgrounded). The device, queue and every
    /// resource built on them stay valid.
    fn on_suspend(&mut self, ctx: &mut MorphContext) {
        let _ = ctx;
    }

    /// A new surface is configured after [`on_suspend`](Self::on_suspend).
    fn on_resume(&mut self, ctx: &mut MorphContext) {
        let _ = ctx;
    }
}
