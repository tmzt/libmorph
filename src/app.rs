//! The trait an application implements.

use std::time::Duration;

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
