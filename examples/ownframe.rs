//! An app that takes the frame: `const OWNS: Owned = Owned::FRAME;`.
//!
//! libmorph brings up the GPU, opens the window, configures the surface and
//! pumps every subsystem as it does for any app. It does not acquire, call
//! `render`, draw the desktop overlay or present: `construct_frame` does
//! that, with `ctx.acquire_frame` and `ctx.present_frame`.
//!
//! The clear colour steps with the frame counter, and every fourth frame is
//! skipped by not acquiring at all, so whatever was presented last stays on
//! screen. There is no `render`: an owning app writes none, which is why
//! the hook has a default.
//!
//! ```sh
//! cargo run --example ownframe
//! cargo run --example ownframe --features script -- \
//!     --morph-script examples/scripts/ownframe.morph
//! ```

use libmorph::{MorphApp, MorphContext, Owned, wgpu};

/// Every fourth frame is skipped, so three in four are presented.
const SKIP_EVERY: u64 = 4;

struct OwnFrame {
    frames: u64,
}

impl MorphApp for OwnFrame {
    /// The frame is the app's. Everything else - the device, the queue,
    /// the window, the surface's lifecycle, every event - stays libmorph's.
    const OWNS: Owned = Owned::FRAME;

    /// Ask for the lowest latency the surface offers, and say what was
    /// available either way. Not called in a scripted run, where there is
    /// no surface.
    fn wgpu_config(
        default: wgpu::SurfaceConfiguration,
        caps: &wgpu::SurfaceCapabilities,
    ) -> wgpu::SurfaceConfiguration {
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            default.present_mode
        };
        log::info!(
            "ownframe: present modes {:?}, asking for {present_mode:?}",
            caps.present_modes
        );
        wgpu::SurfaceConfiguration {
            present_mode,
            // A request, clamped to the range the surface reports.
            desired_maximum_frame_latency: 1,
            ..default
        }
    }

    fn init(_ctx: &mut MorphContext) -> Self {
        Self { frames: 0 }
    }

    fn construct_frame(&mut self, ctx: &mut MorphContext) {
        let n = self.frames;
        self.frames += 1;

        // Skipping is not acquiring. The previous frame stays on screen,
        // and the iteration returns at once.
        if n % SKIP_EVERY == SKIP_EVERY - 1 {
            return;
        }

        let frame = match ctx.acquire_frame() {
            Ok(frame) => frame,
            Err(error) => {
                // Nothing to draw into this iteration: suspended, not
                // ready, or the surface has just been reconfigured.
                log::debug!("ownframe: no frame this iteration ({error})");
                return;
            }
        };

        let step = (n / SKIP_EVERY % 4) as f64 / 3.0;
        let color = wgpu::Color {
            r: 0.08 + 0.5 * step,
            g: 0.10,
            b: 0.55 - 0.4 * step,
            a: 1.0,
        };
        let Some(gpu) = ctx.gpu() else {
            return;
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        gpu.queue.submit([encoder.finish()]);

        // Presented before `construct_frame` returns, which is the one
        // rule an owning app keeps. Pacing is the app's: `Fifo` paces the
        // loop here, exactly as it does for a managed app.
        ctx.present_frame(frame);
    }
}

fn main() -> Result<(), libmorph::MorphError> {
    env_logger::init();
    libmorph::run::<OwnFrame>()
}
