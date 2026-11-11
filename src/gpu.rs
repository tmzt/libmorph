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

/// The allocator report's log line (plan 20261111_03, decision 13): what
/// the device has allocated and reserved `at` a point of the lifecycle, or
/// that the backend keeps no report (Metal, GL). `None` for `device` is a
/// released device.
pub(crate) fn report_line(at: &str, device: Option<&wgpu::Device>) -> String {
    match device {
        None => format!("libmorph: gpu memory at {at}: no device (released)"),
        Some(device) => format_report(at, device.generate_allocator_report().as_ref()),
    }
}

fn format_report(at: &str, report: Option<&wgpu::AllocatorReport>) -> String {
    match report {
        Some(report) => format!(
            "libmorph: gpu memory at {at}: {} bytes allocated, {} bytes reserved",
            report.total_allocated_bytes, report.total_reserved_bytes
        ),
        None => format!("libmorph: gpu memory at {at}: the backend gives no allocator report"),
    }
}

/// The device and queue an app draws with, as
/// [`MorphContext::gpu`](crate::MorphContext::gpu) hands them out: cheap to
/// clone (an `Arc` each), and holding no borrow of the context.
///
/// A clone keeps the device alive. An app whose
/// [`GPU_IN_BACKGROUND`](crate::MorphApp::GPU_IN_BACKGROUND) is `Release`
/// drops every clone, with every other wgpu object, in
/// [`on_gpu(Release)`](crate::MorphApp::on_gpu): memory goes back only when
/// the last handle goes.
#[derive(Clone, Debug)]
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// What an app does with its GPU device while it has no surface, as
/// [`MorphApp::GPU_IN_BACKGROUND`](crate::MorphApp::GPU_IN_BACKGROUND).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GpuInBackground {
    /// Keep the device off screen: libmorph's behaviour before this
    /// existed, and the default.
    Keep,
    /// Give the device back after `after` without a surface, or at once,
    /// without a surface, under memory pressure (a `Background` trim or
    /// above, or low memory), and request a new one at the next surface.
    Release { after: std::time::Duration },
}

/// What happened to the device, for an app whose
/// [`GPU_IN_BACKGROUND`](crate::MorphApp::GPU_IN_BACKGROUND) is `Release`.
/// Passed to [`MorphApp::on_gpu`](crate::MorphApp::on_gpu), always with a
/// device.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GpuEvent {
    /// The device goes when this returns: drop every wgpu object, cloned
    /// [`Gpu`] values included.
    Release,
    /// A new device, with the same limits: rebuild before the next frame.
    /// [`on_resume`](crate::MorphApp::on_resume) follows.
    Restored,
}

/// The device libmorph opened, with its adapter: what a release drops.
#[derive(Debug)]
pub(crate) struct Opened {
    pub adapter: wgpu::Adapter,
    pub gpu: Gpu,
}

/// What bring-up gives: what stays for the whole run (the instance, the
/// limits first granted, how the device was asked for), and the first
/// device.
#[derive(Debug)]
pub(crate) struct BringUp {
    pub instance: wgpu::Instance,
    pub limits: wgpu::Limits,
    pub request: DeviceRequest,
    pub opened: Opened,
}

/// How a runner asks for a device, kept so a restore asks the same way.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DeviceRequest {
    pub force_fallback_adapter: bool,
    /// `PIPELINE_CACHE` where the adapter offers it on Vulkan: `true` only
    /// from a runner whose host keeps a data directory (Android), which is
    /// where the cache's data is kept.
    pub pipeline_cache: bool,
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

/// An adapter that can present to `surface` (any adapter for scripted
/// runs, which have none).
async fn adapter(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
    request: DeviceRequest,
) -> Result<wgpu::Adapter, GpuError> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: surface,
            force_fallback_adapter: request.force_fallback_adapter,
            apply_limit_buckets: false,
        })
        .await
        .map_err(GpuError::Adapter)?;
    let info = adapter.get_info();
    log::info!("libmorph: {} ({:?})", info.name, info.backend);
    Ok(adapter)
}

/// A device on `adapter` with exactly `limits`.
async fn open(
    adapter: wgpu::Adapter,
    limits: &wgpu::Limits,
    request: DeviceRequest,
    memory_hints: wgpu::MemoryHints,
) -> Result<Opened, GpuError> {
    let info = adapter.get_info();
    let required_features = if request.pipeline_cache
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
            memory_hints,
            ..Default::default()
        })
        .await
        .map_err(GpuError::Device)?;
    Ok(Opened {
        adapter,
        gpu: Gpu { device, queue },
    })
}

/// Pick an adapter for `surface` and open the run's first device on it with
/// the limits `limits` asks for (the app's `MorphApp::limits`) and the
/// app's `memory_hints`. A plain `fn` pointer, so it moves into the web's
/// `spawn_local` future.
pub(crate) async fn bring_up(
    instance: wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
    request: DeviceRequest,
    limits: fn(&wgpu::Limits) -> LimitsRequest,
    memory_hints: wgpu::MemoryHints,
) -> Result<BringUp, GpuError> {
    let adapter = adapter(&instance, surface, request).await?;
    // GLES (WebGL2, older Android) gets the WebGL2 floor; everything
    // else the downlevel one. Texture sizes rise to what the adapter
    // allows, so full-resolution device frames fit.
    let base = if adapter.get_info().backend == wgpu::Backend::Gl {
        wgpu::Limits::downlevel_webgl2_defaults()
    } else {
        wgpu::Limits::downlevel_defaults()
    };
    let base = base.using_resolution(adapter.limits());
    let limits = resolve_limits(&limits(&base), &adapter.limits())?;
    let opened = open(adapter, &limits, request, memory_hints).await?;
    Ok(BringUp {
        instance,
        limits,
        request,
        opened,
    })
}

/// A device again after a release: an adapter for `surface`, then a device
/// with the `limits` first granted, so everything the app built against
/// them still fits.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) async fn reopen(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
    request: DeviceRequest,
    limits: &wgpu::Limits,
    memory_hints: wgpu::MemoryHints,
) -> Result<Opened, GpuError> {
    let adapter = adapter(instance, surface, request).await?;
    open(adapter, limits, request, memory_hints).await
}

impl Opened {
    /// Configure `surface` at `size`, keeping `previous` (the format the
    /// app's pipelines were built for) whenever the surface still offers it,
    /// else preferring an sRGB or non-sRGB format as `srgb` asks.
    ///
    /// `wgpu_config` is the app's
    /// [`MorphApp::wgpu_config`](crate::MorphApp::wgpu_config), and is
    /// `Some` only when the app owns the frame; a managed app's surface
    /// knob is `SRGB_SURFACE` and the configuration it gets is the one it
    /// got before the hook existed.
    pub fn configure(
        &self,
        surface: &wgpu::Surface<'_>,
        (width, height): (u32, u32),
        previous: Option<wgpu::TextureFormat>,
        srgb: bool,
        wgpu_config: Option<WgpuConfig>,
    ) -> Option<wgpu::SurfaceConfiguration> {
        let mut config = surface.get_default_config(&self.adapter, width.max(1), height.max(1))?;
        let caps = surface.get_capabilities(&self.adapter);
        config.format = choose_format(&caps.formats, config.format, previous, srgb);
        if previous.is_some_and(|format| format != config.format) {
            log::warn!(
                "libmorph: surface format changed from {previous:?} to {:?}; \
                 pipelines built for the old format will not match",
                config.format
            );
        }
        let config = apply_wgpu_config(config, &caps, wgpu_config, (width, height));
        surface.configure(&self.gpu.device, &config);
        Some(config)
    }
}

/// An app's `wgpu_config` hook, as a `fn` pointer: an associated function
/// with no `self`, because the first surface is configured before `init`
/// builds the app.
pub(crate) type WgpuConfig =
    fn(wgpu::SurfaceConfiguration, &wgpu::SurfaceCapabilities) -> wgpu::SurfaceConfiguration;

/// Hand `config` to the app's hook, if it has one, and then take `width`
/// and `height` back: libmorph knows the window's size and a wrong one
/// breaks the surface. With no hook the configuration is returned
/// untouched, so a managed app's surface is configured exactly as it was
/// before the hook existed.
fn apply_wgpu_config(
    config: wgpu::SurfaceConfiguration,
    caps: &wgpu::SurfaceCapabilities,
    hook: Option<WgpuConfig>,
    (width, height): (u32, u32),
) -> wgpu::SurfaceConfiguration {
    match hook {
        None => config,
        Some(hook) => {
            let mut config = hook(config, caps);
            (config.width, config.height) = (width.max(1), height.max(1));
            config
        }
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
    use super::{apply_wgpu_config, choose_format, format_report};

    #[test]
    fn the_report_line_says_what_the_backend_gave() {
        let report = wgpu::AllocatorReport {
            allocations: Vec::new(),
            blocks: Vec::new(),
            total_allocated_bytes: 1_048_576,
            total_reserved_bytes: 4_194_304,
        };
        assert_eq!(
            format_report("suspend", Some(&report)),
            "libmorph: gpu memory at suspend: 1048576 bytes allocated, 4194304 bytes reserved"
        );
        assert_eq!(
            format_report("gpu-report", None),
            "libmorph: gpu memory at gpu-report: the backend gives no allocator report"
        );
        assert_eq!(
            super::report_line("release", None),
            "libmorph: gpu memory at release: no device (released)"
        );
    }
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

    /// A baseline as `get_default_config` would hand it over.
    fn baseline() -> wgpu::SurfaceConfiguration {
        wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: Bgra8UnormSrgb,
            width: 1080,
            height: 2340,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: Vec::new(),
        }
    }

    fn caps() -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            formats: OFFERED.to_vec(),
            present_modes: vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox],
            alpha_modes: vec![wgpu::CompositeAlphaMode::Auto],
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format_capabilities: Vec::new(),
        }
    }

    /// A managed app has no hook, so the configuration it gets is the one
    /// it got before the hook existed, field for field.
    #[test]
    fn no_hook_leaves_the_configuration_alone() {
        let config = apply_wgpu_config(baseline(), &caps(), None, (1080, 2340));
        assert_eq!(config, baseline());
    }

    /// An owning app that asks for another present mode gets it, and can
    /// see from `caps` whether the surface offers it.
    #[test]
    fn a_hook_chooses_a_present_mode_from_caps() {
        fn hook(
            default: wgpu::SurfaceConfiguration,
            caps: &wgpu::SurfaceCapabilities,
        ) -> wgpu::SurfaceConfiguration {
            let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
                wgpu::PresentMode::Mailbox
            } else {
                default.present_mode
            };
            wgpu::SurfaceConfiguration {
                present_mode,
                desired_maximum_frame_latency: 1,
                ..default
            }
        }
        let config = apply_wgpu_config(baseline(), &caps(), Some(hook), (1080, 2340));
        assert_eq!(config.present_mode, wgpu::PresentMode::Mailbox);
        assert_eq!(config.desired_maximum_frame_latency, 1);
        assert_eq!(config.format, baseline().format, "the rest is the baseline");

        // A surface that does not offer Mailbox: the hook reads caps and
        // keeps what libmorph chose.
        let mut only_fifo = caps();
        only_fifo.present_modes = vec![wgpu::PresentMode::Fifo];
        let config = apply_wgpu_config(baseline(), &only_fifo, Some(hook), (1080, 2340));
        assert_eq!(config.present_mode, wgpu::PresentMode::Fifo);
    }

    /// The window's size is libmorph's, whatever the hook says: a wrong
    /// one breaks the surface. Zero is raised to one, as elsewhere.
    #[test]
    fn libmorph_takes_the_size_back_after_the_hook() {
        fn wrong_size(
            default: wgpu::SurfaceConfiguration,
            _caps: &wgpu::SurfaceCapabilities,
        ) -> wgpu::SurfaceConfiguration {
            wgpu::SurfaceConfiguration {
                width: 1,
                height: 1,
                ..default
            }
        }
        let config = apply_wgpu_config(baseline(), &caps(), Some(wrong_size), (720, 1600));
        assert_eq!((config.width, config.height), (720, 1600));
        let config = apply_wgpu_config(baseline(), &caps(), Some(wrong_size), (0, 0));
        assert_eq!((config.width, config.height), (1, 1));
    }
}
