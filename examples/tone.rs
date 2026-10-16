//! A sine tone from a streaming source (`Audio::add_source`). It sounds while
//! a finger (or the mouse button) is down, and its pitch follows the touch's
//! height: 220 Hz at the bottom, 880 Hz at the top. The screen brightens with
//! the pitch.

use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use libmorph::{Frame, MorphApp, MorphContext, Touch, TouchPhase, wgpu};

/// An `f32` shared with the audio thread without locking.
#[derive(Clone, Default)]
struct Shared(Arc<AtomicU32>);

impl Shared {
    fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

struct Tone {
    pitch: Shared,
    gain: Shared,
    /// 0 at the bottom, 1 at the top.
    height: f32,
}

impl MorphApp for Tone {
    fn init(ctx: &mut MorphContext) -> Self {
        let pitch = Shared::default();
        let gain = Shared::default();
        pitch.set(440.0);
        match ctx.audio().format() {
            Some(format) => {
                log::info!("output: {format:?}");
                let (source_pitch, source_gain) = (pitch.clone(), gain.clone());
                let rate = format.sample_rate as f32;
                let channels = usize::from(format.channels);
                let (mut phase, mut level) = (0.0f32, 0.0f32);
                ctx.audio().add_source(Box::new(move |out: &mut [f32]| {
                    let step = source_pitch.get() / rate;
                    let target = source_gain.get();
                    for frame in out.chunks_exact_mut(channels) {
                        // Glide to the target level so starts and stops do
                        // not click.
                        level += (target - level) * 0.002;
                        let sample = (phase * TAU).sin() * level;
                        frame.fill(sample);
                        phase = (phase + step).fract();
                    }
                }));
            }
            None => log::warn!("no audio output"),
        }
        Self {
            pitch,
            gain,
            height: 0.5,
        }
    }

    fn update(&mut self, _ctx: &mut MorphContext, _dt: Duration) {}

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        let level = f64::from(self.gain.get()) * (0.2 + 0.6 * f64::from(self.height)) / 0.25;
        let color = wgpu::Color {
            r: 0.05 + 0.3 * level,
            g: 0.05 + 0.5 * level,
            b: 0.15 + 0.6 * level,
            a: 1.0,
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

    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        match touch.phase {
            TouchPhase::Started | TouchPhase::Moved => {
                let height = ctx.surface_size().1.max(1) as f32;
                self.height = (1.0 - touch.y / height).clamp(0.0, 1.0);
                // Two octaves, evenly spaced in pitch.
                self.pitch.set(220.0 * 4.0f32.powf(self.height));
                self.gain.set(0.25);
            }
            TouchPhase::Ended | TouchPhase::Cancelled => self.gain.set(0.0),
        }
    }
}

fn main() -> Result<(), libmorph::MorphError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    libmorph::run::<Tone>()
}
