//! What every [`MorphApp`](crate::MorphApp) hook receives.

use std::path::{Path, PathBuf};

use libdevice::DeviceSpec;

use crate::gpu::{BringUp, Gpu, Opened};
use crate::log_once::log_once;
use crate::pipeline_cache::PipelineCacheStore;
use crate::{Audio, OpenLinkError, PermissionRequest, SafeArea, SecureStore};

/// What frames are drawn into. The context's, because an app that owns the
/// frame reaches it through [`MorphContext::acquire_frame`].
pub(crate) enum Target {
    /// A platform surface, presented after each frame.
    Surface(wgpu::Surface<'static>, wgpu::SurfaceConfiguration),
    /// A texture for scripted runs: rendered, never presented.
    #[cfg(script_runner)]
    Offscreen(wgpu::Texture),
}

impl std::fmt::Debug for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Surface(_, config) => f
                .debug_struct("Surface")
                .field("size", &(config.width, config.height))
                .field("format", &config.format)
                .finish_non_exhaustive(),
            #[cfg(script_runner)]
            Self::Offscreen(_) => f.write_str("Offscreen"),
        }
    }
}

/// Where a frame an app acquired came from, and what presenting it means.
enum FrameSource {
    Surface(wgpu::SurfaceTexture),
    /// A scripted run: there is nothing to present to.
    #[cfg(script_runner)]
    Offscreen,
}

/// A frame an app acquired for itself with
/// [`MorphContext::acquire_frame`], under
/// [`Owned::FRAME`](crate::Owned::FRAME). It owns its texture, because the
/// app presents **or drops** it; [`Frame`](crate::Frame), which `render`
/// is lent, borrows a view libmorph presents itself.
pub struct AcquiredFrame {
    pub view: wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    /// Physical pixels.
    pub size: (u32, u32),
    /// The surface wants reconfiguring;
    /// [`present_frame`](MorphContext::present_frame) does it after this
    /// frame, as the managed path does.
    pub suboptimal: bool,
    source: FrameSource,
}

impl std::fmt::Debug for AcquiredFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcquiredFrame")
            .field("format", &self.format)
            .field("size", &self.size)
            .field("suboptimal", &self.suboptimal)
            .finish_non_exhaustive()
    }
}

/// Why there is no frame to draw into this iteration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AcquireError {
    /// [`OWNS`](crate::MorphApp::OWNS) does not contain
    /// [`Owned::FRAME`](crate::Owned::FRAME), so the frame is libmorph's
    /// and it is drawn in [`render`](crate::MorphApp::render). Logged once.
    NotOwned,
    /// No surface: suspended, or the window is not up yet.
    NoSurface,
    /// `Timeout` or `Occluded`: nothing to draw into this iteration.
    NotReady,
    /// The surface was outdated. libmorph has reconfigured it; acquire
    /// again next iteration.
    Outdated,
    /// The surface is lost. libmorph has dropped it and the runner
    /// recreates it before the next iteration. The app hears nothing else,
    /// as a managed app does not today.
    Lost,
    /// This iteration's frame is still held: one per
    /// [`construct_frame`](crate::MorphApp::construct_frame). Logged once.
    Held,
    /// wgpu reported a validation error acquiring. Logged.
    Validation,
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotOwned => "the frame is libmorph's, not the app's",
            Self::NoSurface => "there is no surface",
            Self::NotReady => "the surface has no texture ready",
            Self::Outdated => "the surface was outdated and has been reconfigured",
            Self::Lost => "the surface is lost",
            Self::Held => "this iteration's frame is still held",
            Self::Validation => "wgpu reported a validation error",
        })
    }
}

impl std::error::Error for AcquireError {}

/// Shared state for the whole run. The surface size, format, safe area and
/// scale factor follow the host. The device and queue stay for the whole
/// run unless the app's
/// [`GPU_IN_BACKGROUND`](crate::MorphApp::GPU_IN_BACKGROUND) is `Release`,
/// in which case they may be released while it has no surface: the device
/// accessors return `Option` for that reason (plan
/// `20261111_03_THE_DEVICE_MAY_BE_ABSENT_PLAN.md`).
#[derive(Debug)]
pub struct MorphContext {
    /// Kept for the whole run: a restore creates its surface from it.
    pub(crate) instance: wgpu::Instance,
    /// The limits first granted; a restore asks for the same.
    pub(crate) limits: wgpu::Limits,
    /// The device, queue and adapter: `None` while released.
    pub(crate) device: Option<Opened>,
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
    pub(crate) media: crate::media::MediaState,
    /// What frames are drawn into, while there is one.
    pub(crate) target: Option<Target>,
    /// `A::OWNS.contains(Owned::FRAME)`, which the context cannot read
    /// itself: it is not generic over the app. Set once, before `init`, so
    /// `acquire_frame` can answer `NotOwned` rather than hand a managed
    /// app's frame to the app twice.
    pub(crate) owns_frame: bool,
    /// A frame acquired this iteration has not been presented. Cleared
    /// before every `construct_frame`, so dropping a frame to skip an
    /// iteration costs the app nothing.
    pub(crate) frame_held: bool,
    /// `present_frame` was called this iteration.
    pub(crate) frame_presented: bool,
    /// `acquire_frame` found the surface lost and dropped it; the runner
    /// recreates it before the next iteration.
    pub(crate) surface_lost: bool,
}

impl MorphContext {
    pub(crate) fn new(gpu: BringUp, spec: DeviceSpec, args: Vec<String>, audio: Audio) -> Self {
        Self {
            instance: gpu.instance,
            limits: gpu.limits,
            device: Some(gpu.opened),
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
            media: crate::media::MediaState::default(),
            target: None,
            owns_frame: false,
            frame_held: false,
            frame_presented: false,
            surface_lost: false,
        }
    }

    /// The device and the queue, cloned (an `Arc` each, so the value holds
    /// no borrow of the context): `None` while the device is released,
    /// which only an app whose
    /// [`GPU_IN_BACKGROUND`](crate::MorphApp::GPU_IN_BACKGROUND) is
    /// `Release` can see.
    ///
    /// Take it once at the top of a callback that does GPU work, and decide
    /// there what that callback does without a device: return, defer the
    /// GPU part, or carry on with the part that needs none.
    ///
    /// ```ignore
    /// fn on_intent(&mut self, ctx: &mut MorphContext, intent: &Intent) {
    ///     self.settings.apply(intent); // needs no GPU: always runs
    ///     let Some(gpu) = ctx.gpu() else {
    ///         self.pending_scene = scene_of(intent); // the GPU part waits
    ///         return;
    ///     };
    ///     self.scene.load(&gpu.device, &gpu.queue, intent);
    /// }
    /// ```
    ///
    /// In [`init`](crate::MorphApp::init), [`update`](crate::MorphApp::update),
    /// [`render`](crate::MorphApp::render) and the other callbacks
    /// libmorph only calls with a device (see
    /// [`MorphApp`](crate::MorphApp)), it is always `Some`; there,
    /// `let Some(gpu) = ctx.gpu() else { return };` (with
    /// `RenderStatus::Skipped` from `render`) never returns early.
    #[must_use]
    pub fn gpu(&self) -> Option<Gpu> {
        self.device.as_ref().map(|opened| opened.gpu.clone())
    }

    /// The device: `None` while it is released (see [`gpu`](Self::gpu)).
    #[must_use]
    pub fn device(&self) -> Option<&wgpu::Device> {
        self.device.as_ref().map(|opened| &opened.gpu.device)
    }

    /// The queue: `None` while the device is released (see
    /// [`gpu`](Self::gpu)).
    #[must_use]
    pub fn queue(&self) -> Option<&wgpu::Queue> {
        self.device.as_ref().map(|opened| &opened.gpu.queue)
    }

    /// The adapter's description: `None` while the device is released
    /// (see [`gpu`](Self::gpu)).
    #[must_use]
    pub fn adapter_info(&self) -> Option<wgpu::AdapterInfo> {
        self.device.as_ref().map(|opened| opened.adapter.get_info())
    }

    /// The limits the device was granted; feature-detect against these.
    /// Always the limits first granted: a restore asks for the same, so
    /// this is there with or without a device.
    #[must_use]
    pub fn limits(&self) -> &wgpu::Limits {
        &self.limits
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
    /// `None` unless the host persists one (Android on Vulkan), and while
    /// the device is released: libmorph loads it from
    /// `<data_dir>/libmorph/` before [`init`](crate::MorphApp::init) and
    /// saves it after `init`, on suspend, on exit and before a release,
    /// and reopens it with a restored device. It is a cheap clone for
    /// pipelines built on another thread.
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

    /// The clipboard's text, for the app's own Paste button: the plain text
    /// of the clipboard's first item. `None` when it holds no text, on hosts
    /// without a clipboard call, and when the platform will not give it.
    ///
    /// Android: libmobile's `clipboard::read_text` (feature
    /// `mobile-clipboard`; without it `None`); macOS: libdevice's
    /// `clipboard::read_text`; other desktops, the web and scripted runs:
    /// `None`. An empty text is `None`.
    ///
    /// Call it when the user asks to paste. On Android 12 and later the
    /// system shows a "pasted from your clipboard" toast for text another
    /// app copied; on Android 10 and later only the app in front (with
    /// window focus) may read it, and a refusal is logged at debug with its
    /// reason. libmorph never logs the text.
    #[must_use]
    pub fn clipboard_text(&self) -> Option<String> {
        crate::clipboard::text(self)
    }

    /// Whether the clipboard holds text, for enabling a Paste button. On
    /// Android it reads only the clip's description (libmobile's
    /// `clipboard::has_text`), so no toast is shown; on macOS it reads the
    /// text. `false` wherever [`clipboard_text`](Self::clipboard_text) is
    /// always `None`.
    #[must_use]
    pub fn clipboard_has_text(&self) -> bool {
        crate::clipboard::has_text(self)
    }

    /// Put `text` on the clipboard as plain text, replacing what was there,
    /// for the app's own Copy button. Call it on the user's tap.
    ///
    /// `Ok(ClipboardWrite::SystemNotice)` when the system confirmed the
    /// copy itself (Android 13 and later); `Ok(ClipboardWrite::Silent)`
    /// when it did not, and the app may show its own notice. An empty
    /// `text` is written as given. libmorph never logs the text or its
    /// length.
    ///
    /// Android: libmobile's `clipboard::write_text` (feature
    /// `mobile-clipboard`; without it [`ClipboardError::Unsupported`]);
    /// macOS: libdevice's `clipboard::write_text`; other desktops and the
    /// web: [`ClipboardError::Unsupported`]. Scripted runs log
    /// `libmorph: script: clipboard written` and answer `Silent` without
    /// touching the host's clipboard.
    ///
    /// [`ClipboardWrite::SystemNotice`]: crate::ClipboardWrite::SystemNotice
    /// [`ClipboardError::Unsupported`]: crate::ClipboardError::Unsupported
    pub fn set_clipboard_text(
        &self,
        text: &str,
    ) -> Result<crate::ClipboardWrite, crate::ClipboardError> {
        crate::clipboard::write(self, text)
    }

    /// Whether [`set_clipboard_text`](Self::set_clipboard_text) can work on
    /// this host, for showing or hiding a Copy button. Decided at build
    /// time: `true` on macOS, Android with `mobile-clipboard`, and scripted
    /// runs; `false` elsewhere.
    #[must_use]
    pub fn clipboard_can_write(&self) -> bool {
        crate::clipboard::can_write(self)
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

    /// Ask the user for a photo, a video or a file
    /// ([`media`](crate::media)). Returns at once; the answer comes to
    /// [`MorphApp::on_media`](crate::MorphApp::on_media) with this id.
    /// Android (feature `mobile-media-request`): the system chooser with an
    /// optional camera entry, or the Photo Picker. The web: the browser's
    /// file dialog; call it while handling a touch or key, or the browser
    /// ignores it. The desktop and Android without the feature:
    /// [`MediaError::Unsupported`](crate::media::MediaError::Unsupported) on
    /// the next frame. Scripted runs: the `media` commands answer.
    pub fn media_request(
        &mut self,
        request: &crate::media::MediaRequest,
    ) -> crate::media::MediaRequestId {
        let host = crate::media::Host {
            cache_dir: self.cache_dir.as_deref(),
            #[cfg(all(target_os = "android", feature = "mobile"))]
            android: self.android.as_ref(),
        };
        crate::media::request(&mut self.media, host, request)
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
            libmobile::android::set_text_input_with(
                app,
                Some(value),
                &crate::text_input::to_mobile(options),
            );
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

    /// The surface's next texture, for an app whose
    /// [`OWNS`](crate::MorphApp::OWNS) contains
    /// [`Owned::FRAME`](crate::Owned::FRAME). At most one per
    /// [`construct_frame`](crate::MorphApp::construct_frame).
    ///
    /// Present it with [`present_frame`](Self::present_frame), or drop it
    /// to skip this iteration. **A frame acquired in `construct_frame` is
    /// presented or dropped before `construct_frame` returns**: libmorph
    /// reconfigures the surface between iterations, and on Android drops
    /// it inside the window-destroyed handler before the activity takes
    /// the window back, so an image held across either is
    /// [`Outdated`](AcquireError::Outdated) at best and a texture of a
    /// window that no longer exists at worst.
    ///
    /// On the offscreen target of a scripted run this hands back that
    /// texture, so an app that constructs its own frames is captured the
    /// same way a managed one is.
    pub fn acquire_frame(&mut self) -> Result<AcquiredFrame, AcquireError> {
        if !self.owns_frame {
            log_once!(
                "libmorph: MorphContext::acquire_frame needs \
                 `const OWNS: Owned = Owned::FRAME;`. Without it the frame \
                 is libmorph's and the app draws in render."
            );
            return Err(AcquireError::NotOwned);
        }
        if self.frame_held {
            log_once!(
                "libmorph: MorphContext::acquire_frame was called twice in \
                 one construct_frame. Present or drop a frame before \
                 acquiring the next, and do not hold one past the end of \
                 construct_frame: libmorph reconfigures and drops the \
                 surface between iterations."
            );
            return Err(AcquireError::Held);
        }
        let surface = match &self.target {
            None => return Err(AcquireError::NoSurface),
            Some(Target::Surface(surface, _)) => surface,
            #[cfg(script_runner)]
            Some(Target::Offscreen(texture)) => {
                let frame = AcquiredFrame {
                    view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
                    format: self.surface_format,
                    size: self.surface_size,
                    suboptimal: false,
                    source: FrameSource::Offscreen,
                };
                self.frame_held = true;
                return Ok(frame);
            }
        };
        let texture = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                let frame = self.frame_from(texture, true);
                self.frame_held = true;
                return Ok(frame);
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Err(AcquireError::NotReady);
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.reconfigure();
                return Err(AcquireError::Outdated);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.target = None;
                self.surface_lost = true;
                return Err(AcquireError::Lost);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("libmorph: validation error acquiring the surface texture");
                return Err(AcquireError::Validation);
            }
        };
        let frame = self.frame_from(texture, false);
        self.frame_held = true;
        Ok(frame)
    }

    /// Present `frame`, then reconfigure the surface if it was
    /// suboptimal, as the managed path does. On the offscreen target of a
    /// scripted run the present itself is a no-op.
    ///
    /// The device is not polled: pacing is the app's, which is the point.
    pub fn present_frame(&mut self, frame: AcquiredFrame) {
        let AcquiredFrame {
            view,
            suboptimal,
            source,
            ..
        } = frame;
        // The view borrows the texture, so it goes first.
        drop(view);
        match source {
            FrameSource::Surface(texture) => {
                // `construct_frame` is only called with a device.
                if let Some(opened) = &self.device {
                    opened.gpu.queue.present(texture);
                }
            }
            #[cfg(script_runner)]
            FrameSource::Offscreen => {}
        }
        if suboptimal {
            self.reconfigure();
        }
        self.frame_held = false;
        self.frame_presented = true;
    }

    /// A surface texture, as the app sees it.
    fn frame_from(&self, texture: wgpu::SurfaceTexture, suboptimal: bool) -> AcquiredFrame {
        AcquiredFrame {
            view: texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
            format: self.surface_format,
            size: self.surface_size,
            suboptimal,
            source: FrameSource::Surface(texture),
        }
    }

    /// Configure the surface again with the configuration it has.
    pub(crate) fn reconfigure(&mut self) {
        if let (Some(Target::Surface(surface, config)), Some(opened)) = (&self.target, &self.device)
        {
            surface.configure(&opened.gpu.device, config);
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
