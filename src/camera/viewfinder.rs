//! The camera viewfinder (plan `20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md`,
//! amendment point 3): preview frames drawn through wgpu, with no Android
//! view. Each new frame's I420 planes are uploaded into three `R8Unorm`
//! textures and converted to RGB in `viewfinder.wgsl`, which also turns the
//! image upright by the frame's `quarter_turns` and fits it into a
//! rectangle.
//!
//! ```ignore
//! // init
//! let viewfinder = Viewfinder::new(ctx);
//! // update
//! viewfinder.update(ctx, scan); // uploads only when the frame is new
//! // render, inside the app's own render pass
//! viewfinder.draw(ctx, &mut pass, [0.0, 0.0, w as f32, h as f32], &ViewfinderOptions::default());
//! ```

use super::convert::{Fit, layout};
use super::{PreviewFrame, ScanId};
use crate::MorphContext;

/// Bytes of the uniform block: `crop` and `flags`, a `vec4<f32>` each.
const UNIFORM_BYTES: u64 = 32;

/// How [`Viewfinder::draw`] lays the image out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ViewfinderOptions {
    /// Default [`Fit::Cover`].
    pub fit: Fit,
    /// Flip left and right after turning upright (a selfie view). Default
    /// `false`.
    pub mirror: bool,
}

/// The three plane textures of the frame size last uploaded.
struct Planes {
    size: (u32, u32),
    y: wgpu::Texture,
    u: wgpu::Texture,
    v: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

/// Draws the camera's preview frames into a rectangle of the app's render
/// pass. Built once (in `init`) for the format it draws into; the frame's
/// textures are made on the first upload and remade only when the frame
/// size changes.
pub struct Viewfinder {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    format: wgpu::TextureFormat,
    planes: Option<Planes>,
    /// `(sequence, timestamp_ns)` of the frame uploaded: a new session
    /// counts from 1 again, so the sequence alone is not enough.
    uploaded: Option<(u64, i64)>,
    quarter_turns: u8,
}

impl Viewfinder {
    /// A viewfinder drawing into the surface's format.
    #[must_use]
    pub fn new(ctx: &MorphContext) -> Self {
        Self::with_format(ctx, ctx.surface_format())
    }

    /// A viewfinder drawing into `format` (an offscreen target). An sRGB
    /// format gets linear values, so the GPU's encoding gives back the
    /// camera's colours.
    #[must_use]
    pub fn with_format(ctx: &MorphContext, format: wgpu::TextureFormat) -> Self {
        let device = ctx.device();
        let shader = device.create_shader_module(wgpu::include_wgsl!("viewfinder.wgsl"));
        let plane = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("libmorph viewfinder"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                plane(2),
                plane(3),
                plane(4),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("libmorph viewfinder"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("libmorph viewfinder"),
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
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: ctx.pipeline_cache(),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("libmorph viewfinder"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("libmorph viewfinder"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            sampler,
            uniforms,
            format,
            planes: None,
            uploaded: None,
            quarter_turns: 0,
        }
    }

    /// Poll the scan's newest preview frame and upload it if it is new.
    /// Returns whether it uploaded. Call it once a frame, before `draw`.
    pub fn update(&mut self, ctx: &mut MorphContext, scan: ScanId) -> bool {
        match ctx.camera().preview(scan) {
            Some(frame) => self.upload(ctx, &frame),
            None => false,
        }
    }

    /// Upload `frame`'s planes, unless it is the frame already uploaded
    /// (same `sequence` and timestamp). Returns whether it uploaded.
    pub fn upload(&mut self, ctx: &MorphContext, frame: &PreviewFrame) -> bool {
        let key = (frame.sequence(), frame.timestamp_ns());
        let size = (frame.width(), frame.height());
        if self.uploaded == Some(key) && self.planes.as_ref().is_some_and(|p| p.size == size) {
            return false;
        }
        if self.planes.as_ref().is_none_or(|p| p.size != size) {
            self.planes = Some(self.make_planes(ctx.device(), size));
        }
        let planes = self.planes.as_ref().expect("made above");
        let queue = ctx.queue();
        let (cw, ch) = frame.chroma_size();
        write_plane(queue, &planes.y, frame.y(), size);
        write_plane(queue, &planes.u, frame.u(), (cw, ch));
        write_plane(queue, &planes.v, frame.v(), (cw, ch));
        self.uploaded = Some(key);
        self.quarter_turns = frame.quarter_turns();
        true
    }

    fn make_planes(&self, device: &wgpu::Device, (width, height): (u32, u32)) -> Planes {
        let texture = |label, (width, height): (u32, u32)| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let chroma = (width.div_ceil(2), height.div_ceil(2));
        let y = texture("libmorph viewfinder y", (width, height));
        let u = texture("libmorph viewfinder u", chroma);
        let v = texture("libmorph viewfinder v", chroma);
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let (vy, vu, vv) = (view(&y), view(&u), view(&v));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("libmorph viewfinder"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&vy),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&vu),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&vv),
                },
            ],
        });
        Planes {
            size: (width, height),
            y,
            u,
            v,
            bind_group,
        }
    }

    /// Whether a frame has been uploaded (`draw` draws nothing before).
    #[must_use]
    pub fn has_frame(&self) -> bool {
        self.uploaded.is_some()
    }

    /// The uploaded frame's size once turned upright.
    #[must_use]
    pub fn image_size(&self) -> Option<(u32, u32)> {
        let (w, h) = self.planes.as_ref()?.size;
        Some(if self.quarter_turns % 2 == 1 {
            (h, w)
        } else {
            (w, h)
        })
    }

    /// The format it draws into.
    #[must_use]
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Forget the uploaded frame (a session ended): `draw` draws nothing
    /// until the next upload. The textures are kept for reuse.
    pub fn clear(&mut self) {
        self.uploaded = None;
    }

    /// Draw the uploaded frame into `rect` (`[x, y, width, height]`, pixels
    /// of the render target, inside it) of `pass`, whose target has this
    /// viewfinder's format. Sets the pass's viewport; the app sets its own
    /// again for what it draws after. Does nothing before the first upload.
    /// One `draw` per viewfinder per queue submission: the layout is a
    /// uniform written through the queue.
    pub fn draw(
        &self,
        ctx: &MorphContext,
        pass: &mut wgpu::RenderPass<'_>,
        rect: [f32; 4],
        options: &ViewfinderOptions,
    ) {
        let (Some(planes), Some(image)) = (&self.planes, self.image_size()) else {
            return;
        };
        if self.uploaded.is_none() || rect[2] <= 0.0 || rect[3] <= 0.0 {
            return;
        }
        let (viewport, crop) = layout(rect, image, options.fit);
        let flags = [
            f32::from(self.quarter_turns),
            if options.mirror { 1.0 } else { 0.0 },
            if self.format.is_srgb() { 1.0 } else { 0.0 },
            0.0,
        ];
        let mut bytes = [0u8; UNIFORM_BYTES as usize];
        for (chunk, value) in bytes.chunks_exact_mut(4).zip(crop.iter().chain(&flags)) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
        ctx.queue().write_buffer(&self.uniforms, 0, &bytes);
        let [x, y, w, h] = viewport;
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &planes.bind_group, &[]);
        pass.draw(0..4, 0..1);
    }
}

fn write_plane(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    data: &[u8],
    (width, height): (u32, u32),
) {
    queue.write_texture(
        texture.as_image_copy(),
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests {
    /// The shader parses and validates (naga, as wgpu does), with the entry
    /// points the pipeline names. naga is a desktop dev-dependency.
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    #[test]
    fn shader_validates() {
        let source = include_str!("viewfinder.wgsl");
        let module = naga::front::wgsl::parse_str(source).expect("viewfinder.wgsl parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("viewfinder.wgsl validates");
        let names: Vec<_> = module
            .entry_points
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["vs_main", "fs_main"]);
    }
}
