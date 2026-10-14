//! The host-independent half of a runner: the app, its context and the
//! current surface, and the per-frame sequence.

use web_time::Instant;

use crate::gpu::Gpu;
use crate::{Frame, MorphApp, MorphContext, MorphError, Touch};

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
use super::overlay::Overlay;

/// What a frame came to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FrameStatus {
    Presented,
    /// No surface, or the platform asked to skip this frame.
    Skipped,
    /// The surface is lost: the runner must create a new one and
    /// [`attach`](Shell::attach) it.
    Lost,
}

pub(crate) struct Shell<A> {
    pub ctx: MorphContext,
    pub app: A,
    surface: Option<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration)>,
    last_frame: Instant,
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    pub overlay: Option<Overlay>,
}

impl<A: MorphApp> Shell<A> {
    /// Configure the first surface, then build the app against it.
    pub fn start(
        mut ctx: MorphContext,
        surface: wgpu::Surface<'static>,
        size: (u32, u32),
    ) -> Result<Self, MorphError> {
        let config = ctx
            .gpu
            .configure(&surface, size, None)
            .ok_or(MorphError::NoSurfaceFormat)?;
        ctx.surface_format = config.format;
        ctx.surface_size = (config.width, config.height);
        let app = A::init(&mut ctx);
        Ok(Self {
            ctx,
            app,
            surface: Some((surface, config)),
            last_frame: Instant::now(),
            #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
            overlay: None,
        })
    }

    pub fn gpu(&self) -> &Gpu {
        &self.ctx.gpu
    }

    pub fn has_surface(&self) -> bool {
        self.surface.is_some()
    }

    /// Install a new surface: after a resume, or to replace a lost one.
    /// `resumed` decides whether the app hears [`MorphApp::on_resume`].
    pub fn attach(&mut self, surface: wgpu::Surface<'static>, size: (u32, u32), resumed: bool) {
        let previous = Some(self.ctx.surface_format);
        let Some(config) = self.ctx.gpu.configure(&surface, size, previous) else {
            log::error!("libmorph: the new surface has no presentable format");
            return;
        };
        self.ctx.surface_format = config.format;
        self.ctx.surface_size = (config.width, config.height);
        self.surface = Some((surface, config));
        // Frame time does not accumulate across a suspension.
        self.last_frame = Instant::now();
        if resumed {
            self.app.on_resume(&mut self.ctx);
        }
    }

    /// Drop the surface (it must not outlive the platform window) and tell
    /// the app. Does nothing without a surface.
    pub fn detach(&mut self) {
        if self.surface.take().is_some() {
            self.app.on_suspend(&mut self.ctx);
        }
    }

    pub fn resize(&mut self, (width, height): (u32, u32)) {
        let Some((surface, config)) = &mut self.surface else {
            return;
        };
        (config.width, config.height) = (width.max(1), height.max(1));
        surface.configure(&self.ctx.gpu.device, config);
        self.ctx.surface_size = (config.width, config.height);
    }

    fn reconfigure(&mut self) {
        if let Some((surface, config)) = &self.surface {
            surface.configure(&self.ctx.gpu.device, config);
        }
    }

    pub fn touch(&mut self, touch: Touch) {
        self.app.on_touch(&mut self.ctx, touch);
    }

    pub fn key(&mut self, key: crate::Key) -> crate::KeyStatus {
        self.app.on_key(&mut self.ctx, key)
    }

    #[cfg(not(target_os = "android"))]
    pub fn hover(&mut self, x: f32, y: f32) {
        self.app.on_hover(&mut self.ctx, x, y);
    }

    /// Update, then render and present if the surface hands out a texture.
    pub fn frame(&mut self) -> FrameStatus {
        let Some((surface, _)) = &self.surface else {
            return FrameStatus::Skipped;
        };
        let texture = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => Ok(texture),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => Err(texture),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return FrameStatus::Skipped;
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.reconfigure();
                return FrameStatus::Skipped;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = None;
                return FrameStatus::Lost;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("libmorph: validation error acquiring the surface texture");
                return FrameStatus::Skipped;
            }
        };
        let suboptimal = texture.is_err();
        let texture = texture.unwrap_or_else(|texture| texture);

        let now = Instant::now();
        let dt = now - self.last_frame;
        self.last_frame = now;
        self.app.update(&mut self.ctx, dt);

        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let frame = Frame {
            view: &view,
            format: self.ctx.surface_format,
            size: self.ctx.surface_size,
        };
        self.app.render(&mut self.ctx, &frame);
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        if let Some(overlay) = &mut self.overlay {
            overlay.draw(&self.ctx, &view);
        }
        self.ctx.gpu.queue.present(texture);
        if suboptimal {
            self.reconfigure();
        }
        FrameStatus::Presented
    }
}
