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
