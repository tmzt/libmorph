//! GPU bring-up. libmorph is the only crate in the stack that touches wgpu,
//! so backends, limits and the async strategy are chosen here per host.

use std::fmt;

/// Why the GPU could not be brought up.
#[derive(Debug)]
#[non_exhaustive]
pub enum GpuError {
    CreateSurface(wgpu::CreateSurfaceError),
    Adapter(wgpu::RequestAdapterError),
    Device(wgpu::RequestDeviceError),
    /// The adapter can't meet the app's required limits; one entry per
    /// field.
    Limits(Vec<LimitFailure>),
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateSurface(error) => write!(f, "failed to create surface: {error}"),
            Self::Adapter(error) => write!(f, "no compatible GPU adapter: {error}"),
            Self::Device(error) => write!(f, "failed to request GPU device: {error}"),
            Self::Limits(failures) => {
                f.write_str("the adapter can't meet the app's required limits: ")?;
                for (i, failure) in failures.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(
                        f,
                        "{} {} (adapter {})",
                        failure.name, failure.required, failure.available
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for GpuError {}

/// One limit the adapter can't meet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitFailure {
    /// The `wgpu::Limits` field name, e.g. `max_storage_buffers_per_shader_stage`.
    pub name: &'static str,
    pub required: u64,
    pub available: u64,
}

/// What an app asks of the device, as whole `wgpu::Limits` values.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitsRequest {
    /// Bring-up fails if the adapter can't meet these.
    pub required: wgpu::Limits,
    /// Asked for where the adapter allows; never below `required`.
    pub preferred: wgpu::Limits,
}

impl LimitsRequest {
    /// `required` and `preferred` both `base`: what libmorph asks for when
    /// the app doesn't say.
    #[must_use]
    pub fn base(base: &wgpu::Limits) -> Self {
        Self {
            required: base.clone(),
            preferred: base.clone(),
        }
    }
}

/// The limits to request: `required` must fit the adapter (every field that
/// doesn't is reported), then `preferred` is clamped to the adapter and
/// raised back to `required`. wgpu-types' own helpers know which fields are
/// maxima and which (the alignments) are minima.
pub(crate) fn resolve_limits(
    request: &LimitsRequest,
    adapter: &wgpu::Limits,
) -> Result<wgpu::Limits, GpuError> {
    let mut failures = Vec::new();
    request
        .required
        .check_limits_with_fail_fn(adapter, false, |name, required, available| {
            failures.push(LimitFailure {
                name,
                required,
                available,
            });
        });
    if !failures.is_empty() {
        return Err(GpuError::Limits(failures));
    }
    Ok(request
        .preferred
        .clone()
        .or_worse_values_from(adapter)
        .or_better_values_from(&request.required))
}

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
    /// Pick an adapter that can present to `surface` (any adapter for
    /// scripted runs, which have none) and open a device on it with the
    /// limits `limits` asks for (the app's `MorphApp::limits`). A plain `fn`
    /// pointer, so it moves into the web's `spawn_local` future.
    /// `want_pipeline_cache` asks for `PIPELINE_CACHE` where the adapter
    /// offers it on Vulkan: `true` only from a runner whose host keeps a data
    /// directory (Android), which is where the cache's data is kept.
    pub async fn new(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        force_fallback_adapter: bool,
        limits: fn(&wgpu::Limits) -> LimitsRequest,
        want_pipeline_cache: bool,
    ) -> Result<Self, GpuError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
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
        let base = base.using_resolution(adapter.limits());
        let limits = resolve_limits(&limits(&base), &adapter.limits())?;
        let required_features = if want_pipeline_cache
            && wgpu::util::pipeline_cache_key(&info).is_some()
            && adapter.features().contains(wgpu::Features::PIPELINE_CACHE)
        {
            wgpu::Features::PIPELINE_CACHE
        } else {
            wgpu::Features::empty()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("libmorph"),
                required_features,
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

    mod limits {
        use crate::gpu::{GpuError, LimitFailure, LimitsRequest, resolve_limits};

        /// The baseline libmorph asks for, and an adapter with more.
        fn base() -> wgpu::Limits {
            wgpu::Limits::downlevel_defaults()
        }

        fn adapter() -> wgpu::Limits {
            wgpu::Limits {
                max_storage_buffers_per_shader_stage: 16,
                max_texture_dimension_2d: 8192,
                min_uniform_buffer_offset_alignment: 256,
                ..wgpu::Limits::default()
            }
        }

        fn with_storage(limits: &wgpu::Limits, n: u32) -> wgpu::Limits {
            wgpu::Limits {
                max_storage_buffers_per_shader_stage: n,
                ..limits.clone()
            }
        }

        fn failures(request: &LimitsRequest, adapter: &wgpu::Limits) -> Vec<LimitFailure> {
            match resolve_limits(request, adapter) {
                Err(GpuError::Limits(failures)) => failures,
                other => panic!("expected a limits error, got {other:?}"),
            }
        }

        #[test]
        fn the_base_request_resolves_to_the_base() {
            let base = base().using_resolution(adapter());
            let granted = resolve_limits(&LimitsRequest::base(&base), &adapter()).unwrap();
            assert_eq!(granted, base);
        }

        #[test]
        fn required_within_the_adapter_is_granted() {
            let required = with_storage(&base(), 8);
            let request = LimitsRequest {
                preferred: required.clone(),
                required: required.clone(),
            };
            let granted = resolve_limits(&request, &adapter()).unwrap();
            assert_eq!(granted.max_storage_buffers_per_shader_stage, 8);
            assert_eq!(granted, required);
        }

        #[test]
        fn required_beyond_the_adapter_fails_naming_every_field() {
            let small = with_storage(&base(), 4);
            let request = LimitsRequest::base(&with_storage(&base(), 8));
            assert_eq!(
                failures(&request, &small),
                [LimitFailure {
                    name: "max_storage_buffers_per_shader_stage",
                    required: 8,
                    available: 4,
                }]
            );
            let error = resolve_limits(&request, &small).unwrap_err().to_string();
            assert_eq!(
                error,
                "the adapter can't meet the app's required limits: \
                 max_storage_buffers_per_shader_stage 8 (adapter 4)"
            );

            let greedy = wgpu::Limits {
                max_texture_dimension_2d: 16_384,
                ..with_storage(&base(), 32)
            };
            let names: Vec<&str> = failures(&LimitsRequest::base(&greedy), &adapter())
                .iter()
                .map(|failure| failure.name)
                .collect();
            assert_eq!(
                names,
                [
                    "max_texture_dimension_2d",
                    "max_storage_buffers_per_shader_stage"
                ]
            );
        }

        #[test]
        fn preferred_is_clamped_to_the_adapter() {
            let request = LimitsRequest {
                required: base(),
                preferred: with_storage(&base(), 64),
            };
            let granted = resolve_limits(&request, &adapter()).unwrap();
            assert_eq!(granted.max_storage_buffers_per_shader_stage, 16);
        }

        #[test]
        fn preferred_below_required_is_raised() {
            let request = LimitsRequest {
                required: with_storage(&base(), 8),
                preferred: with_storage(&base(), 2),
            };
            let granted = resolve_limits(&request, &adapter()).unwrap();
            assert_eq!(granted.max_storage_buffers_per_shader_stage, 8);
        }

        #[test]
        fn alignments_resolve_the_smaller_is_better_way() {
            let aligned = |n| wgpu::Limits {
                min_uniform_buffer_offset_alignment: n,
                ..base()
            };
            // Preferring 64 on an adapter that only offers 256 gets 256.
            let request = LimitsRequest {
                required: base(),
                preferred: aligned(64),
            };
            let granted = resolve_limits(&request, &adapter()).unwrap();
            assert_eq!(granted.min_uniform_buffer_offset_alignment, 256);
            // Requiring 64 there fails.
            assert_eq!(
                failures(&LimitsRequest::base(&aligned(64)), &adapter()),
                [LimitFailure {
                    name: "min_uniform_buffer_offset_alignment",
                    required: 64,
                    available: 256,
                }]
            );
        }
    }
}
