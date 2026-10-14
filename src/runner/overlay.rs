//! Desktop emulation overlay: blacks out the device's rounded corners and
//! cutouts over the app's frame, so layouts are judged against the real
//! obstructions. Continuous (iOS) corners are drawn as circular arcs, and
//! SVG-path cutouts as their bounding box.

use libdevice::{CutoutShape, DeviceSpec};

use crate::MorphContext;

const MAX_CUTOUTS: usize = 4;
/// `params`, `corners`, `info`, then two `vec4`s per cutout.
const UNIFORM_FLOATS: usize = 4 * 3 + MAX_CUTOUTS * 8;

pub(crate) struct Overlay {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    format: wgpu::TextureFormat,
}

impl Overlay {
    pub fn new(ctx: &MorphContext) -> Self {
        let device = ctx.device();
        let format = ctx.surface_format();
        let shader = device.create_shader_module(wgpu::include_wgsl!("overlay.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("libmorph overlay"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("libmorph overlay"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("libmorph overlay"),
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
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("libmorph overlay"),
            size: (UNIFORM_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("libmorph overlay"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        if ctx.device_spec().cutouts.len() > MAX_CUTOUTS {
            log::warn!("libmorph: the overlay draws only the first {MAX_CUTOUTS} cutouts");
        }
        Self {
            pipeline,
            uniforms,
            bind_group,
            format,
        }
    }

    pub fn draw(&mut self, ctx: &MorphContext, view: &wgpu::TextureView) {
        if ctx.surface_format() != self.format {
            *self = Self::new(ctx);
        }
        let bytes: Vec<u8> = uniforms(ctx.device_spec(), ctx.surface_size())
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect();
        ctx.queue().write_buffer(&self.uniforms, 0, &bytes);

        let mut encoder = ctx
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("libmorph overlay"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("libmorph overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
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
}

/// The shader's uniform block for `spec` drawn into a `size` surface.
fn uniforms(spec: &DeviceSpec, (width, height): (u32, u32)) -> [f32; UNIFORM_FLOATS] {
    let mut u = [0.0; UNIFORM_FLOATS];
    let (device_width, device_height) = (spec.geometry.width as f32, spec.geometry.height as f32);
    u[..4].copy_from_slice(&[
        width as f32 / device_width,
        height as f32 / device_height,
        device_width,
        device_height,
    ]);
    let corners = spec.corners;
    u[4..8].copy_from_slice(&[
        corners.top_left as f32,
        corners.top_right as f32,
        corners.bottom_right as f32,
        corners.bottom_left as f32,
    ]);
    let cutouts = &spec.cutouts[..spec.cutouts.len().min(MAX_CUTOUTS)];
    u[8] = cutouts.len() as f32;
    for (cutout, slot) in cutouts.iter().zip(u[12..].chunks_exact_mut(8)) {
        let (kind, radius) = match cutout.shape {
            CutoutShape::Circle => (1.0, 0.0),
            CutoutShape::Capsule => (2.0, 0.0),
            CutoutShape::Notch { corner_radius } => (3.0, corner_radius as f32),
            CutoutShape::Rect | CutoutShape::Path(_) => (4.0, 0.0),
        };
        let bounds = cutout.bounds;
        slot.copy_from_slice(&[
            bounds.x as f32,
            bounds.y as f32,
            bounds.width as f32,
            bounds.height as f32,
            kind,
            radius,
            0.0,
            0.0,
        ]);
    }
    u
}

#[cfg(test)]
mod tests {
    use super::{UNIFORM_FLOATS, uniforms};
    use libdevice::{GALAXY_S24, IPHONE_15};

    #[test]
    fn layout_matches_the_shader_block() {
        // 3 vec4 + 4 cutouts of 2 vec4, 16-byte aligned.
        assert_eq!(UNIFORM_FLOATS * 4, 3 * 16 + 4 * 32);
    }

    #[test]
    fn s24_hole_punch_at_native_size() {
        let size = (GALAXY_S24.geometry.width, GALAXY_S24.geometry.height);
        let u = uniforms(&GALAXY_S24, size);
        assert_eq!(&u[..4], &[1.0, 1.0, 1080.0, 2340.0]);
        assert_eq!(&u[4..8], &[110.0; 4]);
        assert_eq!(u[8], 1.0);
        assert_eq!(&u[12..18], &[517.0, 31.0, 46.0, 46.0, 1.0, 0.0]);
    }

    #[test]
    fn half_size_window_scales_down() {
        let size = (IPHONE_15.geometry.width / 2, IPHONE_15.geometry.height / 2);
        let u = uniforms(&IPHONE_15, size);
        assert!(u[..2].iter().all(|scale| (scale - 0.5).abs() < 0.001));
        assert_eq!(u[16], 2.0, "the Dynamic Island is a capsule");
    }
}
