//! GPU bring-up. libmorph is the only crate in the stack that touches wgpu,
//! so backends, limits and the async strategy are chosen here per host.

use std::fmt;

/// Why the GPU could not be brought up.
#[derive(Debug)]
pub enum GpuError {
    CreateSurface(wgpu::CreateSurfaceError),
    Adapter(wgpu::RequestAdapterError),
    Device(wgpu::RequestDeviceError),
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateSurface(error) => write!(f, "failed to create surface: {error}"),
            Self::Adapter(error) => write!(f, "no compatible GPU adapter: {error}"),
            Self::Device(error) => write!(f, "failed to request GPU device: {error}"),
        }
    }
}

impl std::error::Error for GpuError {}

/// The handles that outlive every surface.
#[derive(Debug)]
pub(crate) struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub limits: wgpu::Limits,
}

fn backends() -> wgpu::Backends {
    if cfg!(target_os = "android") {
        wgpu::Backends::VULKAN | wgpu::Backends::GL
    } else if cfg!(target_arch = "wasm32") {
        wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL
    } else {
        wgpu::Backends::PRIMARY
    }
}

/// Create the instance. `display` is the platform display connection, which
/// GLES needs to present (Wayland, Android EGL). On the web this checks for
/// working WebGPU and falls back to WebGL2, hence `async`.
pub(crate) async fn instance(
    display: Option<Box<dyn wgpu::wgt::WgpuHasDisplayHandle>>,
) -> wgpu::Instance {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = backends();
    descriptor.display = display;
    if cfg!(target_arch = "wasm32") {
        wgpu::util::new_instance_with_webgpu_detection(descriptor).await
    } else {
        wgpu::Instance::new(descriptor)
    }
}

impl Gpu {
    /// Pick an adapter that can present to `surface` and open a device on it.
    pub async fn new(
        instance: wgpu::Instance,
        surface: &wgpu::Surface<'_>,
        force_fallback_adapter: bool,
    ) -> Result<Self, GpuError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(surface),
                force_fallback_adapter,
                apply_limit_buckets: false,
            })
            .await
            .map_err(GpuError::Adapter)?;
        let info = adapter.get_info();
        log::info!("libmorph: {} ({:?})", info.name, info.backend);

        // GLES (WebGL2, older Android) gets the WebGL2 floor; everything
        // else the downlevel one. Texture sizes rise to what the adapter
        // allows, so full-resolution device frames fit.
        let base = if info.backend == wgpu::Backend::Gl {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            wgpu::Limits::downlevel_defaults()
        };
        let limits = base.using_resolution(adapter.limits());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("libmorph"),
                required_limits: limits.clone(),
                memory_hints: wgpu::MemoryHints::Performance,
                ..Default::default()
            })
            .await
            .map_err(GpuError::Device)?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            limits,
        })
    }

    /// Configure `surface` at `size`, keeping `previous` (the format the
    /// app's pipelines were built for) whenever the surface still offers it,
    /// else preferring an sRGB or non-sRGB format as `srgb` asks.
    pub fn configure(
        &self,
        surface: &wgpu::Surface<'_>,
        (width, height): (u32, u32),
        previous: Option<wgpu::TextureFormat>,
        srgb: bool,
    ) -> Option<wgpu::SurfaceConfiguration> {
        let mut config = surface.get_default_config(&self.adapter, width.max(1), height.max(1))?;
        let formats = surface.get_capabilities(&self.adapter).formats;
        config.format = choose_format(&formats, config.format, previous, srgb);
        if previous.is_some_and(|format| format != config.format) {
            log::warn!(
                "libmorph: surface format changed from {previous:?} to {:?}; \
                 pipelines built for the old format will not match",
                config.format
            );
        }
        surface.configure(&self.device, &config);
        Some(config)
    }
}

/// The surface format: `previous` while the surface still offers it (so a
/// resume keeps the app's pipelines valid), else the first offered format
/// whose sRGB-ness matches `srgb`, else the adapter's `default`.
fn choose_format(
    formats: &[wgpu::TextureFormat],
    default: wgpu::TextureFormat,
    previous: Option<wgpu::TextureFormat>,
    srgb: bool,
) -> wgpu::TextureFormat {
    previous
        .filter(|format| formats.contains(format))
        .or_else(|| {
            formats
                .iter()
                .copied()
                .find(|format| format.is_srgb() == srgb)
        })
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::choose_format;
    use wgpu::TextureFormat::{Bgra8Unorm, Bgra8UnormSrgb, Rgba16Float};

    const OFFERED: [wgpu::TextureFormat; 3] = [Bgra8UnormSrgb, Bgra8Unorm, Rgba16Float];

    #[test]
    fn preference_picks_by_srgb() {
        assert_eq!(
            choose_format(&OFFERED, Rgba16Float, None, true),
            Bgra8UnormSrgb
        );
        assert_eq!(
            choose_format(&OFFERED, Rgba16Float, None, false),
            Bgra8Unorm
        );
    }

    #[test]
    fn previous_wins_while_offered() {
        assert_eq!(
            choose_format(&OFFERED, Rgba16Float, Some(Bgra8Unorm), true),
            Bgra8Unorm
        );
        assert_eq!(
            choose_format(&OFFERED, Rgba16Float, Some(Bgra8UnormSrgb), false),
            Bgra8UnormSrgb
        );
    }

    #[test]
    fn falls_back_when_nothing_matches() {
        let srgb_only = [Bgra8UnormSrgb];
        assert_eq!(
            choose_format(&srgb_only, Bgra8UnormSrgb, None, false),
            Bgra8UnormSrgb
        );
        assert_eq!(
            choose_format(&srgb_only, Bgra8UnormSrgb, Some(Bgra8Unorm), false),
            Bgra8UnormSrgb
        );
        assert_eq!(choose_format(&[], Bgra8Unorm, None, true), Bgra8Unorm);
    }
}
