//! What every [`MorphApp`](crate::MorphApp) hook receives.

use std::path::{Path, PathBuf};

use libdevice::DeviceSpec;

use crate::gpu::Gpu;
use crate::{Audio, SafeArea};

/// Shared state for the whole run. The device and queue never change; the
/// surface size, format, safe area and scale factor follow the host.
#[derive(Debug)]
pub struct MorphContext {
    pub(crate) gpu: Gpu,
    pub(crate) spec: DeviceSpec,
    pub(crate) safe_area: SafeArea,
    pub(crate) scale_factor: f32,
    pub(crate) surface_size: (u32, u32),
    pub(crate) surface_format: wgpu::TextureFormat,
    pub(crate) audio: Audio,
    pub(crate) args: Vec<String>,
    pub(crate) data_dir: Option<PathBuf>,
    pub(crate) exit_requested: bool,
}

impl MorphContext {
    pub(crate) fn new(gpu: Gpu, spec: DeviceSpec, args: Vec<String>) -> Self {
        Self {
            gpu,
            spec,
            safe_area: SafeArea::NONE,
            scale_factor: spec.pixel_ratio,
            surface_size: (spec.geometry.width, spec.geometry.height),
            // Replaced when the first surface is configured, before `init`.
            surface_format: wgpu::TextureFormat::Rgba8UnormSrgb,
            audio: Audio::open(),
            args,
            data_dir: None,
            exit_requested: false,
        }
    }

    #[must_use]
    pub fn device(&self) -> &wgpu::Device {
        &self.gpu.device
    }

    #[must_use]
    pub fn queue(&self) -> &wgpu::Queue {
        &self.gpu.queue
    }

    #[must_use]
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.gpu.adapter.get_info()
    }

    /// The limits the device was granted; feature-detect against these.
    #[must_use]
    pub fn limits(&self) -> &wgpu::Limits {
        &self.gpu.limits
    }

    /// The device being emulated on desktop. On Android and the web, the
    /// default spec with its geometry following the real surface.
    #[must_use]
    pub fn device_spec(&self) -> &DeviceSpec {
        &self.spec
    }

    /// Where content can draw unobscured, in physical pixels of the surface.
    #[must_use]
    pub fn safe_area(&self) -> SafeArea {
        self.safe_area
    }

    /// Physical pixels per logical unit (dp, point or CSS pixel).
    #[must_use]
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Physical pixels.
    #[must_use]
    pub fn surface_size(&self) -> (u32, u32) {
        self.surface_size
    }

    /// The format to build render pipelines for. Stable across
    /// suspend/resume whenever the new surface offers it.
    #[must_use]
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_format
    }

    #[must_use]
    pub fn audio(&self) -> &Audio {
        &self.audio
    }

    /// The command line with libmorph's and the device options removed,
    /// program name first. Empty on Android and the web.
    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Ask the runner to stop. It finishes the current hook, calls
    /// [`on_suspend`](crate::MorphApp::on_suspend) if the app is not already
    /// suspended, then drops the surface and the app.
    pub fn exit(&mut self) {
        self.exit_requested = true;
    }

    /// A private, persistent directory for app data: Android's internal data
    /// path. `None` on desktop and the web.
    #[must_use]
    pub fn data_dir(&self) -> Option<&Path> {
        self.data_dir.as_deref()
    }
}
