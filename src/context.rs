//! What every [`MorphApp`](crate::MorphApp) hook receives.

use std::path::{Path, PathBuf};

use libdevice::DeviceSpec;

use crate::gpu::Gpu;
use crate::pipeline_cache::PipelineCacheStore;
use crate::{Audio, OpenLinkError, PermissionRequest, SafeArea, SecureStore};

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
    pub(crate) cache_dir: Option<PathBuf>,
    /// The real device's vendor, make and model (Android).
    pub(crate) build_info: Option<crate::BuildInfo>,
    pub(crate) pipeline_cache: Option<PipelineCacheStore>,
    pub(crate) exit_requested: bool,
    /// A scripted run: links are logged, not opened.
    pub(crate) scripted: bool,
    /// Set by the runner where the host has one (feature `secure-store`).
    pub(crate) secure_store: Option<SecureStore>,
    /// Set by the runner where the host has one (feature `secure-store`).
    #[cfg(feature = "secure-store")]
    pub(crate) key_store: Option<crate::Vault>,
    /// For libmobile's calls (permissions); set before `init`.
    #[cfg(all(target_os = "android", feature = "mobile"))]
    pub(crate) android: Option<libmobile::android::AndroidApp>,
    pub(crate) text_input_active: bool,
    pub(crate) text_input_value: String,
    pub(crate) text_input_options: crate::TextInputOptions,
    pub(crate) keyboard_inset: f32,
    pub(crate) camera: crate::camera::CameraState,
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
            cache_dir: None,
            build_info: None,
            pipeline_cache: None,
            exit_requested: false,
            scripted: false,
            secure_store: None,
            #[cfg(feature = "secure-store")]
            key_store: None,
            #[cfg(all(target_os = "android", feature = "mobile"))]
            android: None,
            text_input_active: false,
            text_input_value: String::new(),
            text_input_options: crate::TextInputOptions::default(),
            keyboard_inset: 0.0,
            camera: crate::camera::CameraState::default(),
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

    /// A private directory for files the app keeps, created before
    /// [`init`](crate::MorphApp::init). Android: the app's files directory
    /// (libmobile); macOS: `~/Library/Application Support/<APP_ID>`, and
    /// libdevice's per-user directory on other desktops
    /// ([`MorphApp::APP_ID`](crate::MorphApp::APP_ID)); scripted runs:
    /// `storage/` in the run's output, empty at the start of each run.
    /// `None` on the web, and where it could not be created (logged at
    /// error).
    #[must_use]
    pub fn data_dir(&self) -> Option<&Path> {
        self.data_dir.as_deref()
    }

    /// The real device's vendor, make and model (Android's
    /// `Build.MANUFACTURER`, `BRAND` and `MODEL`), and nothing that
    /// identifies the unit or its owner. `None` on the desktop, the web and
    /// in scripted runs.
    #[must_use]
    pub fn build_info(&self) -> Option<&crate::BuildInfo> {
        self.build_info.as_ref()
    }

    /// The same as [`data_dir`](Self::data_dir), for files the app can make
    /// again: the system may delete them. Android: the app's cache
    /// directory; macOS: `~/Library/Caches/<APP_ID>`; scripted runs:
    /// `cache/` in the run's output, emptied each run; `None` on the web.
    #[must_use]
    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache_dir.as_deref()
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

    /// Named secrets, encrypted at rest (feature `secure-store`): Android
    /// Keystore through libmobile, the login Keychain on macOS, memory in
    /// scripted runs. `None` without the feature, on the web and on other
    /// desktops. Clone it to keep it in the app's own types or threads.
    #[must_use]
    pub fn secure_store(&self) -> Option<&SecureStore> {
        self.secure_store.as_ref()
    }

    /// The app's key store vault (feature `secure-store`, plan
    /// `20261022_06_KEY_STORE_PLAN.md`), for the functions in
    /// [`key_store`](crate::key_store): namespace `libmorph.<SECURE_STORE_ID>`
    /// on Android (Keystore) and macOS (login Keychain), a fresh memory vault
    /// in scripted runs, `None` elsewhere. Clone it to keep it in the app's
    /// own types or threads; make others with `Vault::named`.
    #[cfg(feature = "secure-store")]
    #[must_use]
    pub fn key_store(&self) -> Option<&crate::Vault> {
        self.key_store.as_ref()
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

    /// Open `url` (http or https) in the app that handles it, normally the
    /// browser. Android: libmobile, while the app is visible
    /// ([`OpenLinkError::NotVisible`] otherwise); macOS: libdevice; the web:
    /// a new tab; other desktops: [`OpenLinkError::Unsupported`]. Scripted
    /// runs check the link and log it, cut after its path, without opening
    /// it. Any other scheme, or whitespace or control characters, is
    /// [`OpenLinkError::BadUrl`] on every host. Returns once the system has
    /// the link; it does not wait for the browser. libmorph never logs the
    /// link past its path.
    pub fn open_link(&self, url: &str) -> Result<(), OpenLinkError> {
        if self.scripted {
            crate::link::check_link(url)?;
            log::info!("libmorph: script: open link {}", crate::link::redacted(url));
            return Ok(());
        }
        crate::link::open(url)
    }

    /// The camera ([`camera`](crate::camera)): preview, one still frame and
    /// QR scanning. Calls never block; answers come to
    /// [`MorphApp::on_camera`](crate::MorphApp::on_camera). On Android with
    /// the `mobile-camera` features it is libmobile's camera; elsewhere, and
    /// without them, every call answers
    /// [`CameraError::Unsupported`](crate::camera::CameraError::Unsupported)
    /// on the next frame. Scripted runs have a stand-in camera.
    pub fn camera(&mut self) -> crate::camera::Camera<'_> {
        crate::camera::Camera::new(self)
    }

    /// Open this app's page in the system Settings, where the user can grant
    /// a permission Android no longer asks for (after
    /// [`CameraError::PermissionDenied`](crate::camera::CameraError::PermissionDenied)
    /// with `settings_only`). libmorph never opens it by itself. Android:
    /// libmobile's `open_app_settings`, while the app is visible; elsewhere
    /// [`OpenLinkError::Unsupported`].
    pub fn open_app_settings(&self) -> Result<(), OpenLinkError> {
        #[cfg(all(target_os = "android", feature = "mobile"))]
        return match &self.android {
            Some(app) => {
                libmobile::android::open_app_settings(app).map_err(crate::link::from_mobile)
            }
            None => Err(OpenLinkError::NotVisible),
        };
        #[cfg(not(all(target_os = "android", feature = "mobile")))]
        Err(OpenLinkError::Unsupported)
    }

    /// Start editing `value`: on Android the soft keyboard opens; on the
    /// desktop typed text and the IME are routed to the app. Calling it
    /// again replaces the value and options.
    pub fn start_text_input(&mut self, value: &str, options: &crate::TextInputOptions) {
        self.text_input_active = true;
        self.text_input_value = value.to_string();
        self.text_input_options = options.clone();
        #[cfg(all(target_os = "android", feature = "mobile"))]
        if let Some(app) = &self.android {
            libmobile::android::set_text_input(app, Some(value));
        }
    }

    /// Stop editing; the keyboard closes. No event follows.
    pub fn stop_text_input(&mut self) {
        self.text_input_active = false;
        self.text_input_value.clear();
        #[cfg(all(target_os = "android", feature = "mobile"))]
        if let Some(app) = &self.android {
            libmobile::android::set_text_input(app, None);
        }
    }

    /// Whether text editing is currently active.
    #[must_use]
    pub fn text_input_active(&self) -> bool {
        self.text_input_active
    }

    /// The keyboard's height over the bottom of the surface, in physical
    /// pixels; 0 when it is closed or there is none. Keep the field above it.
    #[must_use]
    pub fn keyboard_inset(&self) -> f32 {
        self.keyboard_inset
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
