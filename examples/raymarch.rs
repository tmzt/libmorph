//! A raymarched scene drawn by one fragment shader (`raymarch.wgsl`). The
//! scene is read from eight storage buffers in the fragment stage, more than
//! the downlevel baseline of four, so the app requires
//! `max_storage_buffers_per_shader_stage: 8` in `MorphApp::limits`. An
//! adapter without that fails at bring-up with `GpuError::Limits` (WebGL2
//! has none) instead of drawing a blank screen.
//!
//! Drag with any number of fingers to orbit (their centroid moves the
//! camera), and pinch to zoom; left alone, the camera circles slowly. On a
//! Mac, two trackpad fingers are touches. Scripted, without a
//! window: `cargo run --example raymarch --features script --
//! --morph-script examples/scripts/raymarch.morph`.

use std::time::Duration;

use libmorph::wgpu::util::DeviceExt;
use libmorph::{Frame, LimitsRequest, MorphApp, MorphContext, Touch, TouchPhase, wgpu};

/// Storage buffers the shader binds in the fragment stage.
const STORAGE_BUFFERS: u32 = 8;

/// The scene, one array per storage buffer, in binding order (1..=8). See
/// `raymarch.wgsl` for what each column means.
const SCENE: [&[[f32; 4]]; STORAGE_BUFFERS as usize] = [
    // spheres: centre, radius
    &[[-0.55, 0.0, 0.0, 0.6], [0.9, 0.9, -0.6, 0.3]],
    // sphere motion: bob height, speed, phase
    &[[0.25, 1.3, 0.0, 0.0], [0.35, 2.1, 1.0, 0.0]],
    // box centres, corner radius
    &[[0.6, -0.35, 0.1, 0.08]],
    // box half extents, spin speed
    &[[0.35, 0.35, 0.35, 0.7]],
    // rings: centre, major radius
    &[[0.0, 0.1, 0.0, 1.25]],
    // ring shapes: minor radius, tilt, tilt swing, swing speed
    &[[0.07, 0.6, 0.3, 0.5]],
    // materials: floor, spheres, box, ring (albedo, specular)
    &[
        [0.5, 0.5, 0.5, 0.0],
        [0.9, 0.35, 0.2, 0.5],
        [0.2, 0.6, 0.9, 0.8],
        [0.3, 0.8, 0.4, 0.3],
        [0.95, 0.8, 0.3, 1.0],
    ],
    // lights: direction towards the light, intensity
    &[[0.6, 0.8, 0.4, 1.4], [-0.7, 0.3, -0.5, 0.35]],
];

struct Raymarch {
    pipeline: wgpu::RenderPipeline,
    frame: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    time: f32,
    yaw: f32,
    pitch: f32,
    /// The camera's distance from the scene.
    distance: f32,
    /// The touches down: id and last position.
    touches: Vec<(u64, f32, f32)>,
}

fn bytes(values: &[[f32; 4]]) -> Vec<u8> {
    values
        .iter()
        .flatten()
        .flat_map(|v| v.to_ne_bytes())
        .collect()
}

impl MorphApp for Raymarch {
    fn limits(base: &wgpu::Limits) -> LimitsRequest {
        let required = wgpu::Limits {
            max_storage_buffers_per_shader_stage: STORAGE_BUFFERS,
            ..base.clone()
        };
        LimitsRequest {
            preferred: required.clone(),
            required,
        }
    }

    fn init(ctx: &mut MorphContext) -> Self {
        log::info!(
            "{} ({:?}), storage buffers per stage: {}",
            ctx.adapter_info().name,
            ctx.surface_format(),
            ctx.limits().max_storage_buffers_per_shader_stage,
        );
        let device = ctx.device();
        let shader = device.create_shader_module(wgpu::include_wgsl!("raymarch.wgsl"));

        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }];
        entries.extend(
            (1..=STORAGE_BUFFERS).map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }),
        );
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("raymarch"),
            entries: &entries,
        });

        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("raymarch frame"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scene: Vec<wgpu::Buffer> = SCENE
            .iter()
            .map(|values| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("raymarch scene"),
                    contents: &bytes(values),
                    usage: wgpu::BufferUsages::STORAGE,
                })
            })
            .collect();
        let mut bindings = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: frame.as_entire_binding(),
        }];
        bindings.extend(
            (1..)
                .zip(&scene)
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding,
                    resource: buffer.as_entire_binding(),
                }),
        );
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("raymarch"),
            layout: &layout,
            entries: &bindings,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("raymarch"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("raymarch"),
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
            cache: None,
        });
        Self {
            pipeline,
            frame,
            bind_group,
            time: 0.0,
            yaw: 0.6,
            pitch: 0.35,
            distance: 6.0,
            touches: Vec::new(),
        }
    }

    fn update(&mut self, _ctx: &mut MorphContext, dt: Duration) {
        self.time += dt.as_secs_f32();
        if self.touches.is_empty() {
            self.yaw += 0.25 * dt.as_secs_f32();
        }
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        let encode = if frame.format.is_srgb() { 0.0 } else { 1.0 };
        let (width, height) = frame.size;
        let uniforms = [
            [width as f32, height as f32, self.time, encode],
            [self.yaw, self.pitch, self.distance, 0.0],
        ];
        ctx.queue().write_buffer(&self.frame, 0, &bytes(&uniforms));

        let mut encoder = ctx.device().create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame.view,
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
    }

    fn on_touch(&mut self, _ctx: &mut MorphContext, touch: Touch) {
        match touch.phase {
            TouchPhase::Started => self.touches.push((touch.id, touch.x, touch.y)),
            TouchPhase::Moved => {
                let Some(index) = self.touches.iter().position(|t| t.0 == touch.id) else {
                    return;
                };
                // The centroid moves by this finger's share of its movement.
                let n = self.touches.len() as f32;
                let (_, x, y) = self.touches[index];
                self.yaw -= 0.005 * (touch.x - x) / n;
                self.pitch = (self.pitch + 0.005 * (touch.y - y) / n).clamp(0.05, 1.4);
                let before = spread(&self.touches);
                self.touches[index] = (touch.id, touch.x, touch.y);
                // Fingers apart move the camera in, by the ratio of spreads.
                if let (Some(before), Some(after)) = (before, spread(&self.touches)) {
                    self.distance = (self.distance * before / after).clamp(3.0, 15.0);
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => self.touches.retain(|t| t.0 != touch.id),
        }
    }
}

/// The mean distance of the touches from their centroid, with two or more.
fn spread(touches: &[(u64, f32, f32)]) -> Option<f32> {
    if touches.len() < 2 {
        return None;
    }
    let n = touches.len() as f32;
    let (cx, cy) = touches
        .iter()
        .fold((0.0, 0.0), |(x, y), t| (x + t.1 / n, y + t.2 / n));
    let spread = touches
        .iter()
        .map(|t| (t.1 - cx).hypot(t.2 - cy))
        .sum::<f32>()
        / n;
    (spread > 1.0).then_some(spread)
}

fn main() -> Result<(), libmorph::MorphError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    libmorph::run::<Raymarch>()
}
