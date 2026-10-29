# VERSIONS

Each change of a crate version in this workspace and the public API changes
it carries (AGENTS.md, Rule 5). Internal changes are not listed, and there
is no entry without a version change. Entries are appended, oldest first,
and never edited.

## libmorph 0.1.0 — 2026-10-14 (`9bf131f`, tag `v0.1.0`)

Initial release, and every change up to `8f19048`. Recorded retroactively
on 2026-10-16; the tag was added on 2026-10-16.

## libmorph-build 0.1.0 — 2026-10-14 (`933c5be`, tag `libmorph-build-v0.1.0`)

Initial release. Recorded retroactively on 2026-10-16; the tag was added on
2026-10-16.

## libmorph 0.2.0 — 2026-10-16 (tag `v0.2.0`)

Audio output device and streaming sources
(`docs/design/20261016_01_AUDIO_DEVICE_AND_SOURCES_PLAN.md`). The commit is
the one tagged `v0.2.0`: the entry is written in that commit, which cannot
name its own hash.

- **Breaking:** on desktop and the web, the audio stream (voices and
  sources) pauses in winit's `suspended` and plays again in `resumed`, as
  it already did on Android.
- Added `Audio::add_source(Source) -> SourceId`, `remove_source`,
  `format() -> Option<OutputFormat>` and `set_master_volume`, with the
  types `Source` (`Box<dyn FnMut(&mut [f32]) + Send>`), `SourceId` and
  `OutputFormat { sample_rate, channels }`. Without `audio`, they exist and
  do nothing.
- Added the `audio-cpal` feature: `libmorph::cpal`, and
  `Audio::device() -> Option<&impl DeviceTrait>`, `stream_config()` and
  `sample_format()`.
- `stop_all` stops voices only; sources keep running.
- On Android, cpal now comes from libmobile 0.1.1 (`libmobile/cpal`); the
  libmobile version is compatible with 0.1.0.

## libmorph 0.2.1 — 2026-10-20 (tag v0.2.1)

Strictly additive (Rule 5.4), plan
`docs/design/20261016_02_PIPELINE_CACHE_PLAN.md`:

- Added `MorphContext::pipeline_cache() -> Option<&wgpu::PipelineCache>`:
  on Android with Vulkan, a cache kept in `<data_dir>/libmorph/`, loaded
  before `init` and saved after `init`, on suspend and on exit; `None`
  elsewhere. On Android the device is now requested with
  `Features::PIPELINE_CACHE` where the adapter offers it.

## libmorph 0.3.0 — 2026-10-22 (tag v0.3.0)

Audio capture (`docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md`) and runtime
permissions for apps (`docs/design/20261022_02_APP_PERMISSIONS_PLAN.md`).
libmorph's own changes are additive; the minor bump comes from libmobile
(Rule 5.5).

- **Breaking:** on Android, libmobile 0.1.1 → 0.2.0 (libmorph re-exports
  its `AndroidApp` and cpal). Its `cpal` feature, which libmorph's `audio`
  turns on, now needs Android 9 (API 28).
- Added `MorphApp::audio_setup(&mut AudioSetup)` (no `self`, before audio
  opens), with `AudioSetup { output, capture, capture_mode }`
  (`#[non_exhaustive]`; defaults: output on, capture off, `Default` mode).
  With `output: false` no output stream opens.
- Added capture: `Audio::add_sink(Sink) -> SinkId`, `remove_sink`,
  `capture_format() -> Option<CaptureFormat>`, `capture_mode()` and
  `set_capture_mode(CaptureMode)`, with the types `Sink`
  (`Box<dyn FnMut(&Captured) + Send>`), `SinkId`, `Captured { samples,
  frame, time }`, `CaptureFormat { sample_rate, channels }` and
  `CaptureMode` (`#[non_exhaustive]`: `Default`, `Generic`, `Camcorder`,
  `VoiceRecognition`, `VoiceCommunication`, `Unprocessed`,
  `VoicePerformance`; Android input presets, ignored elsewhere). Without
  `audio`, they exist and do nothing. Capture pauses with the app.
- Added `MorphApp::on_capture_ready`, called once after `init` when capture
  has opened. On Android, with `capture` set, libmorph asks for
  `RECORD_AUDIO` itself if it is not granted, and opens capture once the
  user allows it.
- Added under `audio-cpal`: `Audio::capture_device()`,
  `capture_stream_config()` and `capture_sample_format()`; `None` on
  Android, where capture goes through libmobile's `CaptureStream`.
- Added runtime permissions: `MorphContext::request_permissions(&[&str]) ->
  Option<PermissionRequest>`, `permission_granted` and
  `should_show_rationale`, the hook `MorphApp::on_permissions(&Permissions)`,
  and the types `PermissionRequest` and `Permissions { request, permissions }`
  (`#[non_exhaustive]`, `granted()`). Off Android there are none: `None`,
  `true` and `false`.

## libmorph-build 0.2.0 — 2026-10-22 (tag libmorph-build-v0.2.0)

No changes of its own; the minor bump comes from its dependency (Rule 5.5).

- **Breaking:** libmobile 0.1.1 → 0.2.0, whose `gradle` module
  libmorph-build re-exports. `gradle_project` and `LINKS` are unchanged.
  Generated projects now carry libmobile's `bridge.PermissionPlugin`.

## libmorph 0.3.1 — 2026-10-22 (tag v0.3.1)

The secure store (`docs/design/20261022_03_SECURE_STORE_PLAN.md`) and the
key store (`docs/design/20261022_06_KEY_STORE_PLAN.md`). All additions.

- **Feature `secure-store`** (off by default):
  - `MorphContext::secure_store() -> Option<&SecureStore>`: `get`,
    `put`, `put_with`, `delete` and `protection()`, with `PutOptions`,
    `Protection`, `SecureStoreError`, `MAX_NAME_LEN` and
    `MAX_VALUE_LEN`.
  - `MorphContext::key_store() -> Option<&Vault>`, and `libmorph::key_store`,
    which is libmobile's engine on Android and libdevice's elsewhere. At the
    root: `SecretKey`, `SealToken`, `Vault`, `KeyStoreError`,
    `FLAG_DEVICE_UNLOCK_REQUIRED` and `FLAG_APP_UNLOCK_REQUIRED`.
  - It turns on libdevice's `secure-store`.
- **Feature `mobile-secure-store-no-strongbox`:** the passthrough to
  libmobile's `secure-store-no-strongbox`.
- `MorphApp::SECURE_STORE_ID`, a defaulted associated const: the
  namespace `libmorph.<id>`.
- The `script` feature also turns on libdevice's `test-vault`, so
  scripted runs get memory stores.
- **Dependencies:** libmobile 0.2.0 → 0.2.1 and libdevice 0.1.0 → 0.1.1,
  both compatible.

## libmorph-build 0.2.1 — 2026-10-22 (tag libmorph-build-v0.2.1)

No changes of its own.

- libmobile 0.2.0 → 0.2.1 (compatible), whose `gradle` module it
  re-exports, unchanged. Generated projects now carry libmobile's core
  `store.SecureStorePlugin` and `GeneratedPlugins.SECURE_STORE_STRONGBOX`.

## libmorph 0.3.2 — 2026-10-23 (tag v0.3.2)

URL schemes (`docs/design/20261022_07_URL_SCHEMES_PLAN.md`). All
additions.

- `MorphApp::on_open_url(&mut self, ctx, url: &str)`, with a default
  body: a link with a scheme the app's build script declared
  (`gradle_project(id).url_scheme("myapp")`). On Android, VIEW intents
  with data, after `init` (a link that started the app waits for it), in
  order; while suspended, after `on_resume`.
- `script::Command::Open(String)` and the script command `open <url>`
  (`Command` is `#[non_exhaustive]`). On an `open` line, `#` belongs to
  the URL.
- **Dependencies:** libmobile 0.2.1 → 0.2.2 (compatible).

## libmorph-build 0.2.2 — 2026-10-23 (tag libmorph-build-v0.2.2)

No changes of its own.

- libmobile 0.2.1 → 0.2.2 (compatible), whose `gradle` module it
  re-exports: `Project::url_scheme`, so
  `gradle_project(id).url_scheme("myapp")` works in app build scripts,
  and `codegen::url_scheme_filters`.

## libmorph 0.4.0 — 2026-10-24 (tag v0.4.0)

The foreground service (`docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md`),
opening links (`docs/design/20261023_02_OPEN_LINK_PLAN.md`), app storage
(`docs/design/20261023_03_APP_STORAGE_PLAN.md`) and memory pressure, stage 1
(`docs/design/20261024_01_MEMORY_PRESSURE_PLAN.md`).

- **Breaking:** `MorphContext::data_dir()` is now `Some` on every host with
  a filesystem: the per-app directory from libdevice on the desktop
  (`~/Library/Application Support/<APP_ID>` on macOS), the run's
  `storage/` in scripted runs, and Android's files directory as before.
  It is created before `init`, and is `None` only on the web or if it
  could not be created. The signature is unchanged.
- `MorphContext::cache_dir()`, created before `init` the same way.
- `MorphApp::APP_ID`, a defaulted const: the directory name on the
  desktop (default: the executable's stem).
- `MorphContext::open_link(url)` and `OpenLinkError`: http and https
  links, through libmobile on Android, libdevice's `NSWorkspace` on macOS,
  a new tab on the web; logged and not opened in scripted runs.
- `libmorph::service`: `start`, `update`, `update_text`, `stop`,
  `is_running`, `set_wake_lock`, `wake_lock_held`, with `ServiceOptions`,
  `NotificationContent`, `NotificationAction` (each with `new`),
  `LockScreenVisibility`, `Controls`, `ControlLabels`, `ServiceType`,
  `ServiceEvent`, `ServiceError` and `MAX_ACTIONS`. `Unsupported` off
  Android.
- `MorphApp::on_service(&mut self, ctx, &ServiceEvent)`, defaulted.
- `libmorph::battery`: `state`, `is_ignoring_optimizations`,
  `request_ignore_optimizations`, `open_settings`, `open_app_details`,
  `open_vendor_battery`, with `BatteryState`, `Page` and `BatteryError`.
- `libmorph::system_events`: `subscribe`, `current`, `Events`,
  `SystemEvent`, `SystemState`, `NetworkInfo`, `Transport`; and
  `subscribe_tokio` under the feature `tokio`. Off Android, only
  `Visible` from the window's lifecycle.
- `MorphApp::on_low_memory(&mut self, ctx)`, defaulted, and the script
  command `low-memory`.
- **Features:** `mobile-foreground-service`, `mobile-system-events`,
  `mobile-battery` and `tokio` (adds `tokio` 1 with `sync` only).
- **Dependencies:** libmobile 0.2.2 → 0.2.3 and libdevice 0.1.1 → 0.1.2,
  both compatible; on macOS libdevice's `open-link` is always on.

## libmorph-build 0.2.3 — 2026-10-24 (tag libmorph-build-v0.2.3)

No changes of its own.

- libmobile 0.2.2 → 0.2.3 (compatible), whose `gradle` module it
  re-exports: `Project::foreground_service`,
  `foreground_service_keeps_running` and
  `request_ignore_battery_optimizations`, and `gradle::ServiceType`.

## libmorph 0.4.1 — 2026-10-24 (tag v0.4.1)

The device's build info (`docs/design/20261024_02_DEVICE_INFO_PLAN.md`)
and memory pressure, stage 2
(`docs/design/20261024_01_MEMORY_PRESSURE_PLAN.md`). All additions.

- `MorphContext::build_info() -> Option<&BuildInfo>` and
  `BuildInfo { vendor, make, model }` (`#[non_exhaustive]`): Android's
  `Build.MANUFACTURER`, `BRAND` and `MODEL`; `None` on the desktop, the
  web and in scripted runs.
- `MorphApp::on_trim_memory(&mut self, ctx, TrimLevel)`, defaulted, and
  `TrimLevel` (`RunningModerate`, `RunningLow`, `RunningCritical`,
  `UiHidden`, `Background`, `Moderate`, `Complete`; ordered,
  `#[non_exhaustive]`).
- `script::Command::TrimMemory(TrimLevel)` and the script command
  `trim-memory <level>`.
- **Dependencies:** libmobile 0.2.3 → 0.2.4 (compatible).

## libmorph-build 0.2.4 — 2026-10-24 (tag libmorph-build-v0.2.4)

No changes of its own.

- libmobile 0.2.3 → 0.2.4 (compatible), whose `gradle` module it
  re-exports, unchanged. Generated projects now carry libmobile's core
  `bridge.BuildInfoPlugin` and `bridge.TrimMemoryPlugin`.

## libmorph 0.4.2 — 2026-10-24 (tag v0.4.2)

No public API changes of its own; released with libmorph-build 0.2.5's app
icons (`docs/design/20261024_03_APP_ICON_PLAN.md`).

- **Dependencies:** libmobile 0.2.4 → 0.2.5 (compatible).

## libmorph-build 0.2.5 — 2026-10-24 (tag libmorph-build-v0.2.5)

App icons (`docs/design/20261024_03_APP_ICON_PLAN.md`). All additions.

- libmobile 0.2.4 → 0.2.5 (compatible), now with its `icons` feature:
  `gradle::Icon` (`png`, `pngs`, `background_color`, `background_png`,
  `monochrome_png`, `full_bleed`) and `Project::icon`, through the
  existing `gradle` re-export, so `gradle_project(id).icon(..)` generates
  the Android launcher, round, adaptive and monochrome icons. It adds
  `image` 0.25 (PNG only) to app build scripts.

## libmorph 0.5.0 — 2026-10-28 (tag v0.5.0)

The camera (docs/design/20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md and its
amendment of 2026-10-28 14:46 EDT).

- **Breaking: libmobile 0.2.6 → 0.3.1.**
- **Breaking: feature `mobile-camera` is now libmobile's in-process camera**
  (`libmobile/camera`); the system camera app hand-off it used to enable is
  `mobile-camera-intent` (`libmobile/camera-intent`).
- Features `mobile-camera-capture` (`libmobile/camera-capture`) and
  `mobile-camera-qr` (`libmobile/camera-qr`).
- `libmorph::camera`: `Camera` (`capture_frame`, `cancel_capture`, `start_qr`,
  `start_preview`, `stop`, `preview`, `zoom_range`, `zoom`, `set_zoom`,
  `zoom_by`), `Facing`, `ScanId`, `CaptureId`, `CameraError`, `CameraEvent`,
  `ZoomSource`, `PreviewOptions`, `PreviewFrame`, `CaptureOptions`,
  `FrameFormat`, `Resolution`, `Frame`, `QrOptions`, `QrMode`,
  `ScreenExposure`, `QrCode`, `CAPTURE_TIMEOUT`; the wgpu `Viewfinder`,
  `ViewfinderOptions`, `Fit`. On every host; `Unsupported` off Android.
- `MorphContext::camera`, `MorphContext::open_app_settings`.
- `MorphApp::on_camera` (default body).
- `script::Command::{CameraQr, CameraFrame, CameraDeny}` (`camera-qr`,
  `camera-frame`, `camera-deny`, `camera-deny-forever`).

## libmorph-build 0.3.0 — 2026-10-28 (tag libmorph-build-v0.3.0)

- **Breaking: libmobile 0.2.6 → 0.3.1** (Rule 5.5): `libmorph_build::gradle`
  re-exports `libmobile::gradle`, and libmobile's 0.3 renamed its `camera`
  feature (`camera-intent` is the system camera app hand-off; `camera` is the
  in-process camera). No other change.

## libmorph 0.5.1 — 2026-10-28 (tag v0.5.1)

The clipboard's text (`docs/design/20261028_03_CLIPBOARD_PLAN.md`). Additive.

- libmobile 0.3.1 → 0.3.2 (compatible).
- `MorphContext::clipboard_text() -> Option<String>` and
  `MorphContext::clipboard_has_text() -> bool`, on every host: libmobile's
  `clipboard` on Android, libdevice's `clipboard::read_text` on macOS,
  `None` and `false` on other desktops, the web and in scripted runs.
- Feature `mobile-clipboard` (`libmobile/clipboard`).
- On macOS, libdevice's `clipboard` feature is turned on (no new crates).

## libmorph 0.5.2 — 2026-10-29 (tag v0.5.2)

Asking the user for a photo, a video or a file
(`docs/design/20261028_02_MEDIA_REQUEST_PLAN.md`). Additive.

- libmobile 0.3.2 → 0.3.4 (compatible).
- `MorphContext::media_request(&MediaRequest) -> MediaRequestId` and
  `MorphApp::on_media(&mut self, &mut MorphContext, &MediaResponse)` (a
  default body), on every host: libmobile's `media-request` on Android,
  a file input on the web, `MediaError::Unsupported` on the desktop and
  Android without the feature; script commands `media`, `media-camera`,
  `media-cancel`, `media-deny` and `media-deny-forever`.
- Module `libmorph::media`: `MediaRequestId` (`id()`); `MediaKind`,
  `CameraEntry`, `CameraPermission`, `Picker`, `Delivery`, `MediaData`
  (`#[non_exhaustive]` enums, as libmobile's); `MediaSource` (`Picked`,
  `Camera`); `MediaRequest` (`#[non_exhaustive]`, `Default`), `MediaItem`,
  `MediaReply`, `MediaResponse` (`#[non_exhaustive]`); `MediaError`
  (`#[non_exhaustive]`: libmobile's variants and `Unsupported`), with
  `Display` and `Error`; `MEDIA_MAX_BYTES`, `MEDIA_MAX_ITEMS`.
- Feature `mobile-media-request` (`libmobile/media-request`).
- The web build turns on the web-sys features `Blob`, `Event`, `File`,
  `FileList`, `HtmlInputElement` and `Navigator` (no new crates).
