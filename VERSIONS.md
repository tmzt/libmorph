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

## libmorph 0.5.3 — 2026-10-29 (tag v0.5.3)

Writing text to the clipboard
(`docs/design/20261029_01_CLIPBOARD_WRITE_PLAN.md`). Additive.

- libmobile 0.3.4 → 0.3.5 and libdevice 0.1.2 → 0.1.3 (compatible).
- `MorphContext::set_clipboard_text(&self, &str) -> Result<ClipboardWrite,
  ClipboardError>` and `MorphContext::clipboard_can_write(&self) -> bool`,
  on every host: libmobile's `clipboard::write_text` on Android under
  `mobile-clipboard`, libdevice's `clipboard::write_text` on macOS, a log
  line only in scripted runs, `ClipboardError::Unsupported` on other
  desktops, the web and Android without the feature.
- `libmorph::ClipboardWrite` (`#[non_exhaustive]`: `SystemNotice`,
  `Silent`) and `libmorph::ClipboardError` (`#[non_exhaustive]`:
  `Unsupported`, `Platform(String)`), with `Display` and `Error`.
- Feature `mobile-clipboard` now also gives the write (no new feature).

## libmorph 0.5.4 — 2026-10-29 (tag v0.5.4)

Audio streams that recover after a resume or a lost device
(`docs/design/20261029_02_AUDIO_STREAM_RECOVERY_PLAN.md`). Additive; no
dependency moves (libmobile 0.3.5, cpal 0.18.2).

- Fixed: a capture or output stream whose start fails on resume (Android's
  `InvalidState`), that cpal reports lost (`DeviceNotAvailable`,
  `StreamInvalidated`), or that delivers nothing for 1000 ms while it
  should run (not the web's output) is reopened with the same format,
  sinks, voices and sources: four attempts at 0, 250, 1000 and 3000 ms,
  then `Failed`. `on_capture_ready` is still called once per run, and
  `capture_format()` and `format()` never change.
- `MorphApp::on_audio(&mut self, &mut MorphContext, &AudioEvent)` (a
  default body), on the main thread before `update`, oldest first.
- `libmorph::AudioEvent` (`#[non_exhaustive]`: `Interrupted { stream,
  reason }`, `Restored { stream, attempts }`, `Failed { stream, reason }`),
  `libmorph::AudioStream` (`#[non_exhaustive]`: `Output`, `Capture`) and
  `libmorph::AudioState` (`#[non_exhaustive]`: `Closed`, `Running`,
  `Paused`, `Recovering`, `Failed`).
- `Audio::output_state(&self) -> AudioState`,
  `Audio::capture_state(&self) -> AudioState` and
  `Audio::restart(&self, AudioStream) -> bool`, on every host and in the
  stub without `audio` (`Closed`, `false`).
- `Audio::device()` and `capture_device()` (`audio-cpal`) are documented
  as the device first opened, which a reopen does not change.

## libmorph 0.6.0 — 2026-11-03 (tag v0.6.0)

The soft keyboard's options, action key and dismissal on Android
(`docs/design/20261024_06_TEXT_INPUT_PLAN.md`, step 1's Android path).

- **Breaking: libmobile 0.3.5 → 0.4.0** (Rule 5.5).
- **Breaking** (behaviour): on Android, `MorphContext::start_text_input`
  now applies its `TextInputOptions` (lines, keyboard type, action key,
  autocorrect) through libmobile's `set_text_input_with`, and the
  keyboard's action key, or Enter on one line, is
  `TextInputEvent::Action` with the keyboard left up; Done no longer
  closes it. The keyboard closed by the user or the system is
  `TextInputEvent::Dismissed`; editing stays active, so
  `start_text_input` shows it again.
- **Breaking:** `TextInputOptions::default()` has `autocorrect: true`, as
  the plan's defaults (libmobile's) say; it was `false`, and unused.

## libmorph-build 0.4.0 — 2026-11-03 (tag libmorph-build-v0.4.0)

- **Breaking: libmobile 0.3.1 → 0.4.0** (Rule 5.5): `libmorph_build::gradle`
  re-exports `libmobile::gradle`. Its Gradle and manifest output is
  unchanged; the Kotlin libmobile stages has the keyboard's new options.
  No other change.


## libmorph 0.6.1 — 2026-11-04 (tag v0.6.1)

An intent's own extras, delivered whole
(`docs/design/20261104_01_INTENT_EXTRAS_PLAN.md`, steps 1 and 2). Strictly
additive: no signature, type or observable behaviour of an existing item
changes, and an app that uses only `on_open_url` sees exactly what it saw.

- Added `libmorph::Intent` (`intent::Intent`): `action: Option<String>`,
  `data: Option<String>`, `extras: Vec<(String, String)>` and `is_new: bool`,
  with `extra(&self, key: &str) -> Option<&str>`. `Clone`, `Debug`,
  `Default`, `PartialEq`, `Eq`. libmorph's own type, field for field
  libmobile's `IntentData`, because a scripted run builds one on a desktop
  where libmobile is not compiled in.
- Added `libmorph::intent`, the module it lives in.
- Added `MorphApp::on_intent(&mut self, ctx, intent: &Intent)`, with a
  default empty body. Every Android intent, whole, after `init` and in
  order; one that arrives while suspended comes after `on_resume`. A `VIEW`
  intent carrying a URI calls `on_intent` and then `on_open_url`.
- Added the script command `intent <key>=<value> [<key>=<value> ...]`
  (`script::Command::Intent`), which delivers extras with no action, no data
  and `is_new: true`.
- Android: the runner forwarded only a `VIEW` intent's `data` and dropped
  every other intent and every extra. It now forwards each intent whole.
  `on_open_url` is called for the same intents as before.

## libmorph 0.7.0 — 2026-11-05 (tag v0.7.0)

An app declares the subsystems it takes, and `render` says whether to
present (`docs/design/20261105_04_OWNS_AND_THE_RENDER_STATUS_PLAN.md`).
Minor under Rule 5.4: `render`'s signature changes. Everything else is
additive and would have been a patch on its own; both parts ship in one
break deliberately, because adding `OWNS` while `render` stayed required
would leave every owning app writing a dead method.

- **Breaking:** `MorphApp::render` returns `RenderStatus` instead of `()`.
  Every impl changes: `-> RenderStatus`, with `RenderStatus::Drawn` at the
  end of a body that draws. It is a hard error at every impl, not a
  warning.
- **Breaking** (a required method becomes defaulted): `MorphApp::render`
  has a default body. It draws nothing, returns `RenderStatus::Skipped`
  and logs once, naming both ways out. An app that owns the frame writes
  no `render` at all; a managed app that has none now compiles, and the
  log is the only signal, because Rust cannot ask whether a method was
  overridden.
- **Breaking** (observable behaviour): a frame whose `render` returns
  `RenderStatus::Skipped` is not presented. The surface texture is dropped
  unpresented, the previous frame stays on screen, and the desktop overlay
  is not drawn either, since it goes over the app's own frame. `update`
  still runs. On the offscreen target of a scripted run a skipped frame
  leaves the texture's prior contents, so both targets report alike.
- Added `libmorph::RenderStatus`: `Drawn` and `Skipped`. `Clone`, `Copy`,
  `Debug`, `Eq`, `Hash`, `PartialEq`; not `#[must_use]`, as `KeyStatus` is
  not. Exhaustive.
- Added `libmorph::Owned`, a hand-rolled flags type over a `u32`, so no
  foreign type enters this crate's public API: `Owned::FRAME`,
  `Owned::empty()`, `Owned::of(&[..])` and `contains`, all `const fn`.
  There is deliberately no `BitOr`: `MorphApp::OWNS` is a `const` and is
  the only place an `Owned` is built, and a trait impl cannot be `const`
  on stable Rust, so `|` would fail with E0015 at every site that
  matters. `Clone`, `Copy`, `Debug`, `Default`, `Eq`, `Hash`,
  `PartialEq`.
- Added `MorphApp::OWNS: Owned`, defaulting to `Owned::empty()`. A set bit
  means libmorph does not drive that subsystem. `Owned::FRAME` is the only
  flag: the device, queue and surface cannot be the app's, because the
  surface comes from the window the platform's loop owns. Adding a flag
  later is strictly additive.
- Added `MorphApp::construct_frame(&mut self, ctx)`, with a default body
  that logs once. Called once per iteration when `OWNS` contains
  `Owned::FRAME`, after `update`, in place of libmorph's acquire,
  `render`, desktop overlay and present. A frame acquired in it is
  presented or dropped before it returns.
- Added `MorphContext::acquire_frame(&mut self) -> Result<AcquiredFrame,
  AcquireError>` and `MorphContext::present_frame(&mut self,
  AcquiredFrame)`. At most one frame per `construct_frame`; not acquiring
  is how an app skips an iteration. `present_frame` reconfigures a
  suboptimal surface afterwards, as the managed path does, and does not
  poll the device: pacing is the app's.
- Added `libmorph::AcquiredFrame`: `view: wgpu::TextureView`, `format`,
  `size: (u32, u32)` and `suboptimal: bool`, plus a private source. It
  owns its texture, because the app presents or drops it, where `Frame`
  lends a view libmorph presents itself.
- Added `libmorph::AcquireError`, `#[non_exhaustive]`: `NotOwned`,
  `NoSurface`, `NotReady`, `Outdated`, `Lost`, `Held`, `Validation`. It
  implements `Display` and `std::error::Error`.
- Added `MorphApp::wgpu_config(default: wgpu::SurfaceConfiguration, caps:
  &wgpu::SurfaceCapabilities) -> wgpu::SurfaceConfiguration`, an
  associated function with a default that returns `default`. Called at
  every attach when `OWNS` contains `Owned::FRAME` - the first window, and
  again after a rotation, a resume or a lost surface - and libmorph
  overrides `width` and `height` after it returns. By value in and out,
  as `limits` is. Not called for a managed frame, whose surface knob stays
  `SRGB_SURFACE`, and not called offscreen, where there is no surface.
- Added the `ownframe` example and `examples/scripts/ownframe.morph`: an
  app with `const OWNS: Owned = Owned::FRAME;` that presents a cleared
  frame, skips every fourth by not acquiring, and asks for the lowest
  latency the surface offers.
- libdevice and libmobile are now pinned by tag, `v0.1.3` and `v0.4.0`.
  Both are the versions already in use and every commit between each tag
  and the revision the lock held is documentation only, so no code
  changes and Rule 5.5 does not apply.

## libmorph 0.7.1 — 2026-11-05 (tag v0.7.1)

Strictly additive under Rule 5.4.

- Added the script verb `geometry <width> <height>`: the window reports a
  new size, in physical pixels, as it does when the device turns. It
  reaches the target and `ctx.device_spec().geometry`, which is what
  Android's `WindowResized` does. Accepted while suspended, where it sets
  the size the next `resume` attaches at. A turned window is not the old
  one reversed - the navigation bar and a display cutout sit on different
  edges - and the insets arrive on their own event, so the verb supplies
  the size rather than asking for a rotation.
- Added `libmorph::script::Command::Geometry { width, height }`. `Command`
  is `#[non_exhaustive]`, so a new variant is additive.
- `Shell::resize` now resizes a scripted run's offscreen target by
  replacing its texture, where it previously returned early unless the
  target was a surface and so did nothing. No public signature changes;
  a script can now resize, which it could not before.

Not covered, and unchanged: a scripted run has no swapchain, so the
surface's own reconfiguration, `MorphApp::wgpu_config` at the following
attach, the format kept across a change, and a frame held over one are
not reachable from a script. That wants a target that simulates a
swapchain.
