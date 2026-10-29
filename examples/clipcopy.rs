//! Writing the clipboard (`MorphContext::set_clipboard_text`). A tap copies
//! a fixed check text and logs the answer, never the text. The screen is
//! grey before the first tap, then green after a copy and red after a
//! refusal.
//!
//! On the Mac, check the pasteboard after a click:
//!
//! ```text
//! cargo run --example clipcopy
//! test "$(pbpaste)" = libmorph-clipboard-check-29Q
//! ```
//!
//! `--copy` copies once at start and closes the window, to check the
//! write without a click (`cargo run --example clipcopy -- --copy`).
//!
//! Scripted, the copy is only logged (`libmorph: script: clipboard
//! written`) and the host's clipboard is left alone:
//!
//! ```text
//! cargo run --example clipcopy --features script -- --morph-script examples/scripts/clipcopy.morph
//! ```

use std::time::Duration;

use libmorph::{Frame, MorphApp, MorphContext, Touch, TouchPhase, wgpu};

/// What a tap copies.
const TEXT: &str = "libmorph-clipboard-check-29Q";

pub struct ClipCopy {
    /// `None` before the first tap; then whether the last copy worked.
    copied: Option<bool>,
}

impl MorphApp for ClipCopy {
    fn init(ctx: &mut MorphContext) -> Self {
        log::info!(
            "clipcopy: tap to copy (clipboard_can_write: {})",
            ctx.clipboard_can_write()
        );
        let mut app = Self { copied: None };
        if ctx.args().iter().any(|arg| arg == "--copy") {
            app.copy(ctx);
            ctx.exit();
        }
        app
    }

    fn update(&mut self, _ctx: &mut MorphContext, _dt: Duration) {}

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        let (r, g, b) = match self.copied {
            None => (0.2, 0.2, 0.2),
            Some(true) => (0.1, 0.6, 0.2),
            Some(false) => (0.7, 0.1, 0.1),
        };
        let mut encoder = ctx.device().create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        ctx.queue().submit([encoder.finish()]);
    }

    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        if touch.phase == TouchPhase::Ended {
            self.copy(ctx);
        }
    }
}

impl ClipCopy {
    fn copy(&mut self, ctx: &MorphContext) {
        let result = ctx.set_clipboard_text(TEXT);
        // The result only: never the text.
        log::info!("clipcopy: set_clipboard_text: {result:?}");
        self.copied = Some(result.is_ok());
    }
}

#[cfg(not(target_os = "android"))]
fn main() -> Result<(), libmorph::MorphError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    libmorph::run::<ClipCopy>()
}
