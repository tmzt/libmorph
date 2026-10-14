//! The trait an application implements.

use std::time::Duration;

use crate::{Key, KeyStatus, MorphContext, Touch};

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
