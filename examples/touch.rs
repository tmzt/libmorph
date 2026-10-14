//! Clears to a colour that follows the last touch (or mouse drag), with the
//! safe area outlined by a darker band. Try `--device iphone_15`.

use std::time::Duration;

use libmorph::{Frame, MorphApp, MorphContext, Touch, TouchPhase, wgpu};

struct TouchDemo {
    touch: Option<(f32, f32)>,
    elapsed: f32,
}

impl MorphApp for TouchDemo {
    fn init(ctx: &mut MorphContext) -> Self {
        let spec = ctx.device_spec();
        log::info!(
            "{} at {:?}, safe area {:?}, format {:?}, args {:?}",
            spec.name,
            ctx.surface_size(),
            ctx.safe_area(),
            ctx.surface_format(),
            ctx.args(),
        );
        Self {
            touch: None,
            elapsed: 0.0,
        }
    }

    fn update(&mut self, _ctx: &mut MorphContext, dt: Duration) {
        self.elapsed += dt.as_secs_f32();
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        let (width, height) = (frame.size.0 as f64, frame.size.1 as f64);
        let pulse = 0.5 + 0.5 * f64::from(self.elapsed.sin());
        let color = match self.touch {
            Some((x, y)) => wgpu::Color {
                r: f64::from(x) / width,
                g: f64::from(y) / height,
                b: pulse,
                a: 1.0,
            },
            None => wgpu::Color {
                r: 0.1,
                g: 0.1,
                b: 0.2 + 0.3 * pulse,
                a: 1.0,
            },
        };
        let mut encoder = ctx.device().create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        ctx.queue().submit([encoder.finish()]);
    }

    fn on_touch(&mut self, _ctx: &mut MorphContext, touch: Touch) {
        log::info!("{touch:?}");
        self.touch = match touch.phase {
            TouchPhase::Started | TouchPhase::Moved => Some((touch.x, touch.y)),
            TouchPhase::Ended | TouchPhase::Cancelled => None,
        };
    }
}

fn main() -> Result<(), libmorph::MorphError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    libmorph::run::<TouchDemo>()
}
