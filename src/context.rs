//! What every [`MorphApp`](crate::MorphApp) hook receives.

use std::path::{Path, PathBuf};

use libdevice::DeviceSpec;

use crate::gpu::Gpu;
use crate::pipeline_cache::PipelineCacheStore;
use crate::{Audio, PermissionRequest, SafeArea};

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
    pub(crate) pipeline_cache: Option<PipelineCacheStore>,
    pub(crate) exit_requested: bool,
    /// For libmobile's calls (permissions); set before `init`.
    #[cfg(all(target_os = "android", feature = "mobile"))]
    pub(crate) android: Option<libmobile::android::AndroidApp>,
}

impl MorphContext {
    pub(crate) fn new(gpu: Gpu, spec: DeviceSpec, args: Vec<String>, audio: Audio) -> Self {
        Self {
            gpu,
            spec,
            safe_area: SafeArea::NONE,
            scale_factor: spec.pixel_ratio,
            surface_size: (spec.geometry.width, spec.geometry.height),
            // Replaced when the first surface is configured, before `init`.
            surface_format: wgpu::TextureFormat::Rgba8UnormSrgb,
            audio,
            args,
            data_dir: None,
            pipeline_cache: None,
            exit_requested: false,
            #[cfg(all(target_os = "android", feature = "mobile"))]
            android: None,
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

    /// Surface pixels per millimetre of the device's display. On desktop,
    /// the emulated device's density, scaled when the window is smaller
    /// than the device; on Android and the web, the spec's own figure.
    #[must_use]
    pub fn px_per_mm(&self) -> f32 {
        surface_px_per_mm(&self.spec, self.surface_size)
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

    /// The pipeline cache to pass as `cache` in pipeline descriptors.
    /// `None` unless the host persists one (Android on Vulkan): libmorph
    /// loads it from `<data_dir>/libmorph/` before
    /// [`init`](crate::MorphApp::init) and saves it after `init`, on suspend
    /// and on exit. It is a cheap clone for pipelines built on another
    /// thread.
    #[must_use]
    pub fn pipeline_cache(&self) -> Option<&wgpu::PipelineCache> {
        self.pipeline_cache.as_ref().map(PipelineCacheStore::cache)
    }

    /// Ask the user for `permissions` (full names such as
    /// `"android.permission.CAMERA"`, each declared in the app's manifest).
    /// The answer arrives in [`MorphApp::on_permissions`](crate::MorphApp::on_permissions);
    /// requests are made one at a time, in order. `None` where the host has no
    /// runtime permissions (desktop, web): carry on as if granted.
    pub fn request_permissions(&mut self, permissions: &[&str]) -> Option<PermissionRequest> {
        #[cfg(all(target_os = "android", feature = "mobile"))]
        return self
            .android
            .as_ref()
            .map(|app| libmobile::android::request_permissions(app, permissions).into());
        #[cfg(not(all(target_os = "android", feature = "mobile")))]
        {
            let _ = permissions;
            None
        }
    }

    /// Whether `permission` is granted now. `true` off Android.
    #[must_use]
    pub fn permission_granted(&self, permission: &str) -> bool {
        #[cfg(all(target_os = "android", feature = "mobile"))]
        return self
            .android
            .as_ref()
            .is_some_and(|app| libmobile::android::permission_granted(app, permission));
        #[cfg(not(all(target_os = "android", feature = "mobile")))]
        {
            let _ = permission;
            true
        }
    }

    /// After a denial: `true` if asking again shows a dialog, `false` if
    /// Android would deny it without one (the user chose "Don't allow"
    /// twice: point them to Settings). `false` off Android.
    #[must_use]
    pub fn should_show_rationale(&self, permission: &str) -> bool {
        #[cfg(all(target_os = "android", feature = "mobile"))]
        return self
            .android
            .as_ref()
            .is_some_and(|app| libmobile::android::should_show_rationale(app, permission));
        #[cfg(not(all(target_os = "android", feature = "mobile")))]
        {
            let _ = permission;
            false
        }
    }

    /// Save the pipeline cache's data, if there is a cache and it changed.
    pub(crate) fn save_pipeline_cache(&mut self) {
        if let Some(store) = &mut self.pipeline_cache {
            store.save();
        }
    }
}

/// `spec.px_per_mm()` in pixels of a surface of `width` (device pixels
/// scaled by surface width / device width).
fn surface_px_per_mm(spec: &DeviceSpec, (width, _): (u32, u32)) -> f32 {
    spec.px_per_mm() * width as f32 / spec.geometry.width as f32
}

#[cfg(test)]
mod tests {
    use super::surface_px_per_mm;
    use libdevice::GALAXY_S24;

    #[test]
    fn px_per_mm_follows_the_surface() {
        let full = surface_px_per_mm(&GALAXY_S24, (1080, 2340));
        assert!((full - 416.0 / 25.4).abs() < 1e-4);
        let half = surface_px_per_mm(&GALAXY_S24, (540, 1170));
        assert!((half - full / 2.0).abs() < 1e-4);
    }
}
