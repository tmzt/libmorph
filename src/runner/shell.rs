//! The host-independent half of a runner: the app, its context and the
//! current surface, and the per-frame sequence.

use std::time::Duration;

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

/// Whether the app has been told it is suspended. Keeps `on_suspend` and
/// `on_resume` to one call per transition, and exit to a single `on_suspend`
/// however many teardown paths fire.
#[derive(Debug, Default)]
struct Lifecycle {
    suspended: bool,
    /// Links that arrived while suspended, delivered after `on_resume`.
    urls: Vec<String>,
}

impl Lifecycle {
    /// Becoming suspended: whether the app must hear `on_suspend`.
    fn suspend(&mut self) -> bool {
        !std::mem::replace(&mut self.suspended, true)
    }

    /// Becoming resumed: whether the app must hear `on_resume`.
    fn resume(&mut self) -> bool {
        std::mem::replace(&mut self.suspended, false)
    }

    /// A link: `Some` to deliver it now, `None` if it waits for a resume.
    #[cfg(any(test, script_runner, target_os = "android"))]
    fn open_url(&mut self, url: String) -> Option<String> {
        if self.suspended {
            self.urls.push(url);
            None
        } else {
            Some(url)
        }
    }

    /// The links that waited, in order, once resumed.
    fn take_urls(&mut self) -> Vec<String> {
        if self.suspended {
            Vec::new()
        } else {
            std::mem::take(&mut self.urls)
        }
    }
}

/// What frames are drawn into.
enum Target {
    /// A platform surface, presented after each frame.
    Surface(wgpu::Surface<'static>, wgpu::SurfaceConfiguration),
    /// A texture for scripted runs: rendered, never presented.
    #[cfg(script_runner)]
    Offscreen(wgpu::Texture),
}

pub(crate) struct Shell<A> {
    pub ctx: MorphContext,
    pub app: A,
    target: Option<Target>,
    lifecycle: Lifecycle,
    last_frame: Instant,
    /// Scripted runs: the `dt` every frame gets, instead of the clock.
    fixed_dt: Option<Duration>,
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
            .configure(&surface, size, None, A::SRGB_SURFACE)
            .ok_or(MorphError::NoSurfaceFormat)?;
        ctx.surface_format = config.format;
        ctx.surface_size = (config.width, config.height);
        Ok(Self::with_app(ctx, Target::Surface(surface, config), None))
    }

    fn with_app(mut ctx: MorphContext, target: Target, fixed_dt: Option<Duration>) -> Self {
        let mut app = A::init(&mut ctx);
        if ctx.audio.capture_format().is_some() {
            app.on_capture_ready(&mut ctx);
        }
        // Pipelines built in `init` are kept even if the process is killed
        // without a clean suspend.
        ctx.save_pipeline_cache();
        Self {
            ctx,
            app,
            target: Some(target),
            lifecycle: Lifecycle::default(),
            last_frame: Instant::now(),
            fixed_dt,
            #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
            overlay: None,
        }
    }

    pub fn gpu(&self) -> &Gpu {
        &self.ctx.gpu
    }

    pub fn has_surface(&self) -> bool {
        self.target.is_some()
    }

    /// Install a new surface: after a resume, or to replace a lost one. The
    /// app hears [`MorphApp::on_resume`] only if it was suspended.
    pub fn attach(&mut self, surface: wgpu::Surface<'static>, size: (u32, u32)) {
        let previous = Some(self.ctx.surface_format);
        let Some(config) = self
            .ctx
            .gpu
            .configure(&surface, size, previous, A::SRGB_SURFACE)
        else {
            log::error!("libmorph: the new surface has no presentable format");
            return;
        };
        self.ctx.surface_format = config.format;
        self.ctx.surface_size = (config.width, config.height);
        self.install(Target::Surface(surface, config));
    }

    fn install(&mut self, target: Target) {
        self.target = Some(target);
        // Frame time does not accumulate across a suspension.
        self.last_frame = Instant::now();
        if self.lifecycle.resume() {
            self.app.on_resume(&mut self.ctx);
        }
        for url in self.lifecycle.take_urls() {
            self.app.on_open_url(&mut self.ctx, &url);
        }
    }

    /// A link the app was opened with: now, or after `on_resume` if the app
    /// is suspended.
    #[cfg(any(script_runner, target_os = "android"))]
    pub fn open_url(&mut self, url: String) {
        if let Some(url) = self.lifecycle.open_url(url) {
            self.app.on_open_url(&mut self.ctx, &url);
        }
    }

    /// The system is low on memory: tell the app, suspended or not.
    #[cfg(any(script_runner, target_os = "android"))]
    pub fn low_memory(&mut self) {
        self.app.on_low_memory(&mut self.ctx);
    }

    /// Android asked the app to trim memory: tell it, suspended or not.
    #[cfg(any(script_runner, target_os = "android"))]
    pub fn trim_memory(&mut self, level: crate::TrimLevel) {
        self.app.on_trim_memory(&mut self.ctx, level);
    }

    /// Drop the surface (it must not outlive the platform window) and tell
    /// the app, unless it already knows it is suspended.
    pub fn detach(&mut self) {
        self.target = None;
        if self.lifecycle.suspend() {
            self.app.on_suspend(&mut self.ctx);
        }
        self.ctx.save_pipeline_cache();
    }

    /// Whether the app called [`MorphContext::exit`].
    pub fn exit_requested(&self) -> bool {
        self.ctx.exit_requested
    }

    /// The end of the run, however it came: `on_suspend` exactly once (unless
    /// the app is already suspended), then the surface, then the app.
    pub fn shutdown(mut self) {
        if self.lifecycle.suspend() {
            self.app.on_suspend(&mut self.ctx);
            self.ctx.save_pipeline_cache();
        }
        self.target = None;
        drop(self.app);
    }

    pub fn resize(&mut self, (width, height): (u32, u32)) {
        let Some(Target::Surface(surface, config)) = &mut self.target else {
            return;
        };
        (config.width, config.height) = (width.max(1), height.max(1));
        surface.configure(&self.ctx.gpu.device, config);
        self.ctx.surface_size = (config.width, config.height);
    }

    fn reconfigure(&mut self) {
        if let Some(Target::Surface(surface, config)) = &self.target {
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
        let surface = match &self.target {
            None => return FrameStatus::Skipped,
            Some(Target::Surface(surface, _)) => surface,
            #[cfg(script_runner)]
            Some(Target::Offscreen(texture)) => {
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                self.draw(&view);
                return FrameStatus::Presented;
            }
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
                self.target = None;
                return FrameStatus::Lost;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("libmorph: validation error acquiring the surface texture");
                return FrameStatus::Skipped;
            }
        };
        let suboptimal = texture.is_err();
        let texture = texture.unwrap_or_else(|texture| texture);
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.draw(&view);
        self.ctx.gpu.queue.present(texture);
        if suboptimal {
            self.reconfigure();
        }
        FrameStatus::Presented
    }

    /// `update`, `render` and the overlay into `view`.
    fn draw(&mut self, view: &wgpu::TextureView) {
        let now = Instant::now();
        let dt = self.fixed_dt.unwrap_or(now - self.last_frame);
        self.last_frame = now;
        self.app.update(&mut self.ctx, dt);

        let frame = Frame {
            view,
            format: self.ctx.surface_format,
            size: self.ctx.surface_size,
        };
        self.app.render(&mut self.ctx, &frame);
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        if let Some(overlay) = &mut self.overlay {
            overlay.draw(&self.ctx, view);
        }
    }
}

/// Scripted runs: an offscreen texture in place of a surface.
#[cfg(script_runner)]
impl<A: MorphApp> Shell<A> {
    /// The offscreen format: RGBA so readback needs no channel swap, sRGB
    /// or not as the app asks.
    pub fn offscreen_format() -> wgpu::TextureFormat {
        if A::SRGB_SURFACE {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        }
    }

    /// Create the first offscreen target at `size`, then build the app.
    /// Every frame gets `dt`.
    pub fn start_offscreen(mut ctx: MorphContext, size: (u32, u32), dt: Duration) -> Self {
        ctx.surface_format = Self::offscreen_format();
        ctx.surface_size = (size.0.max(1), size.1.max(1));
        let texture = offscreen_texture(&ctx);
        Self::with_app(ctx, Target::Offscreen(texture), Some(dt))
    }

    /// A new offscreen target in the kept format and size, after
    /// [`detach`](Self::detach).
    pub fn attach_offscreen(&mut self) {
        let texture = offscreen_texture(&self.ctx);
        self.install(Target::Offscreen(texture));
    }

    /// The offscreen texture, while attached.
    pub fn offscreen(&self) -> Option<&wgpu::Texture> {
        match &self.target {
            Some(Target::Offscreen(texture)) => Some(texture),
            _ => None,
        }
    }
}

#[cfg(script_runner)]
fn offscreen_texture(ctx: &MorphContext) -> wgpu::Texture {
    let (width, height) = ctx.surface_size;
    ctx.gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("libmorph offscreen"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: ctx.surface_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

#[cfg(test)]
mod tests {
    use super::Lifecycle;

    #[test]
    fn one_suspend_and_resume_per_transition() {
        let mut life = Lifecycle::default();
        assert!(!life.resume(), "running: nothing to resume");
        assert!(life.suspend());
        assert!(!life.suspend(), "already suspended");
        assert!(life.resume());
        assert!(!life.resume());
    }

    #[test]
    fn links_wait_while_suspended() {
        let mut life = Lifecycle::default();
        assert_eq!(life.open_url("a".into()), Some("a".into()));
        life.suspend();
        assert_eq!(life.open_url("b".into()), None);
        assert_eq!(life.open_url("c".into()), None);
        assert!(life.take_urls().is_empty(), "still suspended");
        assert!(life.resume());
        assert_eq!(life.take_urls(), ["b", "c"]);
        assert!(life.take_urls().is_empty(), "delivered once");
        assert_eq!(life.open_url("d".into()), Some("d".into()));
    }

    #[test]
    fn exit_suspends_once() {
        // Exit while running: the shutdown suspend is the one call.
        let mut life = Lifecycle::default();
        assert!(life.suspend());
        // Exit after the window went away (Android `WindowDestroyed`, then
        // `Destroy`): already told, so the shutdown is silent.
        let mut life = Lifecycle::default();
        assert!(life.suspend(), "WindowDestroyed");
        assert!(!life.suspend(), "Destroy");
        // Exit after a suspend and a resume suspends again, once.
        let mut life = Lifecycle::default();
        life.suspend();
        life.resume();
        assert!(life.suspend());
    }
}
