//! A high-frame-rate game loop that owns its frames.
//!
//! `ownframe` shows the mechanics of `const OWNS: Owned = Owned::FRAME;`.
//! This one shows what a game does with them once it has them, which is
//! four things libmorph deliberately no longer decides:
//!
//! 1. **The present mode.** `Fifo` paces the loop to the display, which is
//!    what most apps want and the opposite of what this example wants. It
//!    asks for `Immediate`, then `Mailbox`, so the loop runs as fast as the
//!    surface allows.
//! 2. **The queue's depth.** `desired_maximum_frame_latency: 1`, so a frame
//!    is presented before the next is built rather than several being in
//!    flight. A request, clamped to what the surface reports.
//! 3. **Maintaining the device.** libmorph does not poll under
//!    `Owned::FRAME` - the comment in `construct_frame` says why it matters
//!    here and nowhere else.
//! 4. **What to do when a frame is not available.** Return, count it, and
//!    come back next iteration. Never block.
//!
//! The motion is driven by `update`'s `dt`, not by a frame counter, so the
//! bar crosses the screen in two seconds whether that takes 120 frames or
//! 2000 - and identically in a scripted run, where `dt` is fixed.
//!
//! ```sh
//! cargo run --release --example hifps
//! cargo run --example hifps --features script -- \
//!     --morph-script examples/scripts/hifps.morph
//! ```

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use libmorph::{MorphApp, MorphContext, Owned, wgpu};

/// Frame times kept for the log line.
const RECENT: usize = 5;
/// How often the rate is reported.
const REPORT_EVERY: Duration = Duration::from_secs(1);

struct HiFps {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,

    /// Simulated seconds, accumulated from `update`. Drives the picture, so
    /// it is independent of how fast the loop actually runs.
    elapsed: f32,

    /// Wall-clock measurement, which is a different question from `dt` and
    /// deliberately kept apart from it.
    last: Option<Instant>,
    recent: VecDeque<Duration>,
    since_report: Duration,
    presented: u64,
    unavailable: u64,
}

impl MorphApp for HiFps {
    /// The frame is the app's: libmorph stops acquiring, calling `render`,
    /// drawing the overlay and presenting. Everything else stays its own.
    const OWNS: Owned = Owned::FRAME;

    /// The surface is still libmorph's to configure, and this is where an
    /// owning app says how. Not called in a scripted run, which has no
    /// surface.
    fn wgpu_config(
        default: wgpu::SurfaceConfiguration,
        caps: &wgpu::SurfaceCapabilities,
    ) -> wgpu::SurfaceConfiguration {
        // In order of preference: Immediate never waits for the display,
        // Mailbox waits but keeps only the newest frame, and Fifo - the
        // default everywhere - paces the loop to the refresh rate, which
        // would cap this example at the display and hide the point.
        let wanted = [wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox];
        let present_mode = wanted
            .into_iter()
            .find(|mode| caps.present_modes.contains(mode))
            .unwrap_or(default.present_mode);
        if present_mode == default.present_mode {
            log::warn!(
                "hifps: neither Immediate nor Mailbox is offered ({:?}); \
                 the loop will be paced by the display",
                caps.present_modes
            );
        } else {
            log::info!("hifps: presenting with {present_mode:?}");
        }
        wgpu::SurfaceConfiguration {
            present_mode,
            // One frame in flight. A request, clamped to the surface's
            // own range, so read nothing back as a guarantee.
            desired_maximum_frame_latency: 1,
            ..default
        }
    }

    fn init(ctx: &mut MorphContext) -> Self {
        let device = ctx.device();
        let shader = device.create_shader_module(wgpu::include_wgsl!("hifps.wgsl"));

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hifps"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hifps frame"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hifps"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hifps"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hifps"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ctx.surface_format().into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: ctx.pipeline_cache(),
        });

        Self {
            pipeline,
            uniform,
            bind_group,
            elapsed: 0.0,
            last: None,
            recent: VecDeque::with_capacity(RECENT),
            since_report: Duration::ZERO,
            presented: 0,
            unavailable: 0,
        }
    }

    /// Still called under `Owned::FRAME`, which is why the simulation can
    /// stay here and only the frame moves.
    fn update(&mut self, _ctx: &mut MorphContext, dt: Duration) {
        self.elapsed += dt.as_secs_f32();
    }

    fn construct_frame(&mut self, ctx: &mut MorphContext) {
        self.measure();

        let frame = match ctx.acquire_frame() {
            Ok(frame) => frame,
            Err(error) => {
                // Not ready, suspended, or just reconfigured. Returning is
                // the whole of the recovery: nothing is presented, the
                // previous frame stays on screen, and the next iteration
                // tries again. A game must not block here.
                self.unavailable += 1;
                log::debug!("hifps: no frame this iteration ({error})");
                return;
            }
        };

        let (width, height) = frame.size;
        let uniforms = [self.elapsed, width as f32, height as f32, 0.0];
        ctx.queue()
            .write_buffer(&self.uniform, 0, &bytes(&uniforms));

        let mut encoder = ctx.device().create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hifps"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        ctx.queue().submit([encoder.finish()]);

        // Presented before this returns, which is the one rule an owning
        // app keeps: libmorph may reconfigure the surface between
        // iterations, and Android takes the window back synchronously.
        ctx.present_frame(frame);
        self.presented += 1;

        // libmorph does not poll the device for an app that owns the frame,
        // so this is the app's to do. It costs almost nothing - it retires
        // submissions that have already finished and waits for nothing -
        // and it matters most exactly here: a loop running flat out with a
        // shallow queue submits often, and on a backend that needs it,
        // nothing is reclaimed until something maintains the device.
        if let Err(error) = ctx.device().poll(wgpu::PollType::Poll) {
            log::warn!("hifps: polling the device: {error}");
        }
    }
}

impl HiFps {
    /// One wall-clock reading per iteration, reported once a second.
    ///
    /// Deliberately not `update`'s `dt`: that is the simulation's step, and
    /// in a scripted run it is a fixed number that says nothing about how
    /// long a frame took.
    fn measure(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last.replace(now) {
            let frame = now - last;
            if self.recent.len() == RECENT {
                self.recent.pop_front();
            }
            self.recent.push_back(frame);
            self.since_report += frame;
        }
        if self.since_report < REPORT_EVERY {
            return;
        }
        let frames = self.presented + self.unavailable;
        let fps = frames as f64 / self.since_report.as_secs_f64();
        let last5: Vec<String> = self
            .recent
            .iter()
            .map(|d| format!("{:.2}", d.as_secs_f64() * 1000.0))
            .collect();
        log::info!(
            "hifps: {fps:.0} fps, last{} [{}] ms, {} presented, {} unavailable",
            self.recent.len(),
            last5.join(","),
            self.presented,
            self.unavailable,
        );
        self.since_report = Duration::ZERO;
        self.presented = 0;
        self.unavailable = 0;
    }
}

fn bytes(values: &[f32; 4]) -> [u8; 16] {
    let mut out = [0; 16];
    for (slot, value) in out.chunks_exact_mut(4).zip(values) {
        slot.copy_from_slice(&value.to_le_bytes());
    }
    out
}

fn main() -> Result<(), libmorph::MorphError> {
    env_logger::init();
    libmorph::run::<HiFps>()
}
