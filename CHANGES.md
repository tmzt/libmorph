# CHANGES

Addendum log (AGENTS.md Rule 0). Entries are appended, never edited.

## 2026-10-14 12:09 EDT — Camera view and capture plan

New planned work: camera-view (a live preview as a wgpu texture) and
camera-capture (one still photo) on libmorph's public surface. They replace
the `mobile-camera` passthrough. `camera-stream` (recording) is reserved
for later. libmobile's `camera` feature is renamed to `camera-capture`,
and it gains `camera-view`: the NDK camera plus a CAMERA permission
plugin. Desktop uses nokhwa; the web uses `getUserMedia`.
Plan: `docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md` (Draft),
built in the nine steps of section 9, one crate per commit.

## 2026-10-14 12:31 EDT — libmorph-build crate plan

New planned work: a `crates/libmorph-build` crate re-exporting
`libmobile::gradle` with libmorph's `links` key preset, so Android apps take
libmorph-build rather than libmobile as their build-dependency. The repository
root becomes a Cargo workspace. Plan:
`docs/design/20261014_02_LIBMORPH_BUILD_CRATE_PLAN.md` (Draft, approved),
built in the three steps of section 5.

## 2026-10-14 12:58 EDT — Shell input and lifecycle plan

New planned work: touch pressure, hover, keys (`Key`, `KeyStatus`, Android
Back routed by the app's status), a second mouse finger on desktop,
`MorphContext::exit` with exactly one `on_suspend` per exit, `data_dir`, and
`MorphApp::SRGB_SURFACE`. Needs `libmobile::android::finish`. Plan:
`docs/design/20261014_03_SHELL_INPUT_AND_LIFECYCLE_PLAN.md` (Draft), built in
the nine steps of section 9.

## 2026-10-14 13:06 EDT — Shell input and lifecycle plan, step 1: departure

Plan: `docs/design/20261014_03_SHELL_INPUT_AND_LIFECYCLE_PLAN.md`.

- Section 7: `AndroidApp::activity_as_ptr()` returns a JNI global reference
  to the Java `Activity`, not an `ANativeActivity*`. libmobile's
  `jni_util` already relies on that. Passing it to
  `ndk::native_activity::NativeActivity::from_ptr` would have been
  unsound. `libmobile::android::finish(app)` keeps the planned signature
  and behaviour, but calls `Activity.finish()` through JNI. That goes
  through a new `jni_util::with_activity`, which `with_app_class` now builds
  on. libmobile `e09eba7`.

## 2026-10-14 13:12 EDT — Shell input and lifecycle plan, steps 4–6: departures

Plan: `docs/design/20261014_03_SHELL_INPUT_AND_LIFECYCLE_PLAN.md`.

- Step 4, section 3 keys: winit's synthetic key presses (`is_synthetic`,
  sent for keys already held when the window gains focus) are skipped as
  well as repeats. Otherwise a key held while switching windows would reach
  `on_key` as a new press.
- Step 5, section 4: `KeyRouting::followup` takes `&mut self` (it shares the
  slot lookup with `pressed`). It stores one status each for Back and
  Escape, not `[(Key, KeyStatus); 2]`. The behaviour is as planned.
- Step 6, Done when: the once-per-exit rule is tested on the host without a
  GPU, in place of the planned `#[ignore]` GPU test of a whole `Shell`.
  `Shell` keeps its suspended state in a `Lifecycle` with
  `suspend()`/`resume()` returning whether the hook must run. Unit tests
  cover running → suspend → resume, exit while running, exit after
  `WindowDestroyed`, and exit after a resume. A `Shell` needs a real surface,
  which macOS test threads cannot create (winit requires the main thread).

## 2026-10-14 14:59 EDT — Scripted runs and PNG capture: new plan (Draft)

Plan: `docs/design/20261014_04_SCRIPTED_RUNS_AND_PNG_CAPTURE_PLAN.md`.

- Proposes a line-based `.morph` script format, a headless desktop runner
  that renders offscreen with a fixed frame time, PNG captures through the
  `png` 0.18.1 crate with fixed encoder settings, and a `run.json`
  manifest.
- Output goes to `--morph-out <dir>` exactly, or by default to
  `out/morph/YYYYMMDD_NN_<stem>/`, following the plan naming convention.
- Behind a new `script` feature, off by default. No existing signature
  changes; `MorphError` gains a `Script` variant.
- Section 10 lists five decisions for the owner. No implementation starts
  until the plan is approved.

Also: `.gitignore` now lists `/out`. AGENTS.md (Rule 3.5) already
described `out/` as gitignored, but the file didn't list it.

## 2026-10-14 15:04 EDT — App-requested device limits: new plan (Draft)

Plan: `docs/design/20261014_05_APP_LIMITS_REQUEST_PLAN.md`. Requested by
the owner (relayed by another session) after a Galaxy S23 compute shader hit
the downlevel limit of 4 storage buffers.

- Proposes `LimitsRequest { required, preferred }` of whole
  `wgpu::Limits`, and `MorphApp::limits(base)` with a default that keeps
  today's behaviour.
- `required` is checked against the adapter and never clamped; a failure
  is a new `GpuError::Limits` naming each field. `preferred` is clamped
  to the adapter and raised to `required` with wgpu-types' own helpers.
- `GpuError` is not `#[non_exhaustive]`, so the new variant is a public
  enum change. Section 7 asks the owner how to handle it. No implementation
  starts until the plan is approved.

## 2026-10-14 15:23 EDT — Scripted runs plan, steps 1–8: departures

Plan: `docs/design/20261014_04_SCRIPTED_RUNS_AND_PNG_CAPTURE_PLAN.md`. All
small, and none changes an existing signature.

- Section 7, additions to the public API: `Script::path()` (the file a
  script was loaded from, for `run.json`), `Script::fps()`, `DEFAULT_FPS`,
  `DEFAULT_OUT_ROOT`, `OutDir::for_script(path)`, and `Clone` on
  `ScriptRun`.
- Step 6: a missing value (`--morph-script` with nothing after it, or
  `--morph-script=`) and `--morph-out` without `--morph-script` are reported
  as a new `MorphError::InvalidOption { option, reason }`. `MorphError` is
  already `#[non_exhaustive]`. A value given to a `--morph-*` flag that takes
  none (`--morph-help=yes`) is `UnknownOption`.
- `build.rs` sets `cfg(script_runner)` for the `script` feature on native
  desktop targets, in place of repeating that condition.
- Section 3, `drag`: the intermediate points are computed in `f64`, and the
  last move is exactly `<x1> <y1>`, so round operands give round points.
- Section 3, `key`: `#` can't be written directly because it starts a
  comment; `U+0023` gives it.
- Step 4 carried `#[allow(dead_code)]` on the offscreen methods until step 5
  used them.
- Section 4.2: a failed readback (device poll, map or mapped range) is
  `ScriptError::Io` rather than a panic.
- `MorphContext::new` (crate-private) takes the `Audio` handle, so scripted
  runs pass a silent one without opening the device.
- Step 5's GPU test ran on this Mac (Apple M4, Metal), not skipped. Step 7's
  evidence is in `out/scripted-runs.md` in the worktree.

## 2026-10-15 23:29 EDT — Mac trackpad fingers as touches: plan drafted

Plan: `docs/design/20261015_01_MAC_TRACKPAD_TOUCH_PLAN.md` (Draft). The
owner asked for this through the Noble's Keep session.

- On macOS, AppKit `NSTouch` fingers, read through a local event monitor
  on winit's view, become `Touch` events with ids from `1 << 32`. Step 1
  is a trial of that route.
- A session starts at two fingers and lasts until the last finger lifts.
  One finger stays the mouse, and the mouse is held off during a session.
- `--morph-trackpad <physical|fit|off>`. `physical` places the fingers
  at phone scale around the cursor, using libdevice's planned `ppi` /
  `px_per_mm`. libdevice keeps only device facts; host input stays here.
- No public type changes. Section 8 asks the owner four questions. No
  implementation starts until the plan is approved.

## 2026-10-15 23:43 EDT — Mac trackpad touch plan, steps 1–7: departures

Plan: `docs/design/20261015_01_MAC_TRACKPAD_TOUCH_PLAN.md`. All small.

- Steps 1–4 went in one commit (be32d07). The trial's logging (step 1) is
  the `log::debug!("trackpad: …")` line in the runner. The owner's check on
  hardware is still to come.
- Section 2, monitor mask: `ScrollWheel` is left out and `SmartMagnify`
  added. `touchesMatchingPhase:inView:` is documented for gesture events
  only, and an Objective-C exception from a scroll event would abort the
  process.
- Section 2, dependencies: no `raw-window-handle` entry, since winit
  re-exports it as `winit::raw_window_handle`. objc2-foundation also
  needs `NSEnumerator` (for `NSSet::to_vec`), and objc2-app-kit needs
  `block2` (for the monitor). The lockfile resolves objc2 0.5.3, the copy
  winit already uses.
- Section 10: `MorphContext::px_per_mm()` returns `f32`, not
  `Option<f32>`. libdevice b27ba72 was pinned in the same step, and its
  `ppi` is a required field.
- `MorphOptions` no longer derives `Eq`, because `WindowScale::Factor`
  holds an `f32`.

## 2026-10-15 23:52 EDT — Mac trackpad touch plan: trial result and fix

Plan: `docs/design/20261015_01_MAC_TRACKPAD_TOUCH_PLAN.md`, steps 1, 3, 5
and 7, checked by the owner on a MacBook Pro (Apple M1, macOS 14.5).

- Step 1: the local event monitor on winit's view does receive per-finger
  `NSTouch` data, so the `class_addMethod` fallback is not needed.
- Some gesture events (magnify, rotate) carry no touches. They were being
  read as every finger lifting, so a pinch ended and restarted every few
  events. Such events are now ignored, because a lift arrives as a touch
  in the `Ended` phase (a1555a6).
- The raymarch example orbits by the centroid of all touches (0a1c025),
  and zooms with a pinch (4abb56a), for step 5's check.
- The owner confirmed: a two-finger pinch keeps two ids from `Started` to
  `Ended`; one finger still clicks and hovers; raymarch's pinch zoom
  follows the fingers; the window fits on the display, and
  `--morph-window-scale 1` gives the device size.

## 2026-10-16 08:42 EDT — Audio output device and streaming sources

Plan: `docs/design/20261016_01_AUDIO_DEVICE_AND_SOURCES_PLAN.md` (Draft).

Requested by the owner for the tower defense game, whose audio is a node
graph rendered by the app. libmorph keeps owning the cpal stream and adds
streaming sources (`Audio::add_source`, `remove_source`, `format`), mixed
with the `Sound` voices. A new `audio-cpal` feature re-exports cpal and
exposes the output device, with passthroughs for cpal's host backends. The
stream is also paused in winit's `suspended`. Waiting for the owner's
approval before implementation.

## 2026-10-16 09:16 EDT — Rule 5: versions and tags

- Owner's instruction (2026-10-16): apply libcontrols' versioning rules,
  recorded here rather than in a plan. AGENTS.md gains Rule 5: the version
  commit is tagged, `VERSIONS.md` lists the public API changes of each
  version, `0.x` bumps the minor version for anything not strictly
  additive and the patch version otherwise, and incompatible bumps of a
  dependency propagate up. libmobile adopted the same rules (its
  `9b36728`, released as 0.1.1).
- libmorph and libmorph-build are versioned separately, tagged
  `v<version>` and `libmorph-build-v<version>`.
- `VERSIONS.md` starts with both crates' 0.1.0, recorded retroactively,
  and the tags `v0.1.0` (`9bf131f`) and `libmorph-build-v0.1.0`
  (`933c5be`) are added.
- The audio plan (`docs/design/20261016_01_AUDIO_DEVICE_AND_SOURCES_PLAN.md`)
  is released as libmorph 0.2.0: besides its additions, the desktop and web
  stream now pauses in winit's `suspended`, which a caller can observe
  (Rule 5.4). libmorph-build is unchanged at 0.1.0; libmobile 0.1.1 is
  compatible with 0.1.0.

## 2026-10-16 13:20 EDT — Persistent pipeline cache

Plan: `docs/design/20261016_02_PIPELINE_CACHE_PLAN.md` (Draft).

Requested by the owner for the tower defense game, which spends about
0.9 s compiling one compute pipeline on every launch on a Galaxy S23.
libmorph will request wgpu's `PIPELINE_CACHE` on Android with Vulkan,
keep the cache's data under `<data_dir>/libmorph/`, offer it as
`ctx.pipeline_cache()`, and save it after `init`, on suspend and on
exit. Waiting for the owner's approval before implementation.

## 2026-10-20 00:06 EDT — Persistent pipeline cache approved and built

Plan: `docs/design/20261016_02_PIPELINE_CACHE_PLAN.md` (In implementation).

The owner approved the plan with section 8's recommendations (no
opt-out switch, no desktop Vulkan cache, cache growth accepted). Steps 1
to 3 are built: `Gpu::new` requests `PIPELINE_CACHE` from the Android
runner, `src/pipeline_cache.rs` loads and saves the data, and
`MorphContext::pipeline_cache()` offers it. Step 4, the trial on the S23,
follows in the game. To be released as libmorph 0.2.1 (strictly additive).

## 2026-10-20 00:14 EDT — Persistent pipeline cache: the trial on the S23

Plan: `docs/design/20261016_02_PIPELINE_CACHE_PLAN.md`, step 4.

The tower defense game, on libmorph 0.2.1, builds every pipeline through
`ctx.pipeline_cache()`. On the owner's Galaxy S23 (Adreno 740, Vulkan),
unlocked: the second launch logged `pipeline cache: loaded 627724 bytes`,
and its slowest pipeline (a raymarched render pipeline, on a worker
thread) was built in 34 ms, from 1247–1309 ms without the cache. The file
is `files/libmorph/wgpu_pipeline_cache_vulkan_20803_1124403713.bin`.

## 2026-10-22 06:50 EDT — Audio capture and the audio_setup hook

Plan: `docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md` (Draft).

Requested by the owner, with the instruction to implement it in a
worktree. A new `MorphApp::audio_setup` hook, called before audio is
opened, asks for a capture stream on the default input device and can
turn the output off. Captured blocks are handed to sinks
(`Audio::add_sink`) on the audio input thread, with a frame count and a
capture time, and the input stream pauses with the app. An
`audiocapture` example writes one `.au` file per second, named by
Unix milliseconds, and logs each name. Built on the `audio-capture`
branch; to be released as libmorph 0.2.2 (strictly additive).

## 2026-10-22 06:58 EDT — Audio capture: the example on Android

Plan: `docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md` (In implementation),
amended for step 4.

The owner asked for the `audiocapture` example on the Galaxy S23. libmorph
had no Android example, so a cdylib crate under `examples/android/` wraps
it, with a build script in the manner of libmobile's. libmobile only puts
permissions in the manifest and cannot request one at runtime, so the
script grants `RECORD_AUDIO` with `adb`; a runtime request belongs in a
libmobile plan. Small departure in section 7: each file's Unix time is
anchored when the sink receives its first block (wall clock minus that
block's `time`), not when the sink is added, because `time` counts from
the stream's first block, which comes before the sink is added.

## 2026-10-22 07:03 EDT — Audio capture: the trial on the S23

Plan: `docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md`, step 4.

`examples/android/build.sh audiocapture --install` on the owner's Galaxy
S23 Ultra (SM-S918U). The input stream opened at 48000 Hz, 2 channels
(`dumpsys audio`: source `VOICE_RECOGNITION`, client format float, not
silenced). It wrote one 1 s file a second to
`files/audiocapture/<unix ms>.au`, and each name was logged. Of 59
files pulled to `out/audiocapture/s23/`, those written while the app
was in front hold sound (peaks 0.008–0.10). Those written while it was
behind Recents are all zeros: the audio policy moved the recording to
the background (`setAppState(portId:6828, state:2)`), and Android
silences the input instead of stopping it. Not yet checked: the owner
listening to the files.

No permission prompt appeared: `build.sh` grants `RECORD_AUDIO` with
`adb shell pm grant`. The owner noted that a real request needs a Kotlin
plugin in libmobile (2026-10-22).

## 2026-10-22 07:05 EDT — Audio capture: asking for RECORD_AUDIO

Plan: `docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md` (In implementation),
amended for step 5.

Agreed with the owner: on Android, libmorph asks for `RECORD_AUDIO`
when the app sets `AudioSetup::capture` and the permission is not
granted, through libmobile's new runtime permission request (libmobile
`docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md`, Draft). The
capture stream then opens after `init`, so a new
`MorphApp::on_capture_ready` hook says when it is ready, and sinks added
earlier are kept until then. Waits for libmobile 0.1.2.

## 2026-10-22 07:18 EDT — Audio capture: asking for RECORD_AUDIO and the spectrogram on the S23

Plan: `docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md`, steps 5 and 6.

Built against libmobile's `runtime-permissions` branch through a local
patch; libmorph's `Cargo.lock` still names libmobile 0.1.1 from GitHub, so
the branch builds only with the patch until libmobile is pushed.

On the owner's Galaxy S23 Ultra, with `RECORD_AUDIO` revoked, launching
the example showed "Allow libmorph audiocapture to record audio?". A
reinstall while the dialog was up interrupted the request, which arrived
as denied: libmorph logged `audio capture: RECORD_AUDIO denied` and the
screen stayed red. On the next launch the owner allowed it (07:15:48), the
capture stream opened 266 ms later, `on_capture_ready` added the sinks,
and one file a second followed. The owner saw the spectrogram and was
satisfied ("this looks good"). The owner also decided against the
plasma view ("we don't need it").

Not yet checked: an explicit "Don't allow" in this app, the spectrogram
on the desktop, and the owner listening to the files (steps 4 and 6).

A hazard found on the way, for libmobile: when the libmobile copy changes
(here the patched one replaced the git one), its `OUT_DIR` and so the
staged Kotlin source root move, and Gradle's incremental Kotlin compile
then fails with "Unresolved reference 'WindowInsetsPlugin'" until
`gradle clean`.

## 2026-10-22 07:24 EDT — Audio capture: modes on Android

Plan: `docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md` (In implementation),
amended for step 7.

Agreed with the owner: `AudioSetup::capture_mode` and
`Audio::set_capture_mode` choose Android's AAudio input preset (generic,
camcorder, voice recognition, voice communication, unprocessed, voice
performance). cpal cannot set one, so on Android libmorph opens the
capture stream itself through the NDK's AAudio, which libmobile already
links; output stays on cpal. A first step towards the owner's eventual
full-duplex speech-to-text and text-to-speech, which is not in this plan.

## 2026-10-22 07:32 EDT — Audio capture: modes on the S23

Plan: `docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md`, step 7.

Built against libmobile's `runtime-permissions` branch, whose `cpal`
feature also enables `ndk/api-level-28`: ndk 0.9 gates input presets
behind it (libmobile `CHANGES.md`).

On the owner's Galaxy S23 Ultra, a tap cycled the example through every
mode. Each was granted as asked, and `Default` was granted as
`VoiceRecognition`. `dumpsys audio` showed the matching source each
time: `MIC` (Generic), `CAMCORDER`, `VOICE_RECOGNITION`,
`VOICE_COMMUNICATION`, `UNPROCESSED` and `VOICE_PERFORMANCE`. Files
kept arriving across every switch, named `<unix ms>_<mode>.au`. In a
quiet room, one file per mode measured from -66.6 dBFS RMS (Unprocessed)
to -45.1 dBFS (Camcorder). The files were taken seconds apart, so this
only suggests the processing differs.

Departure from the amendment's wording: the example cycles through the
modes it asked for, not the granted ones; otherwise `Default`, granted as
`VoiceRecognition`, skipped `Generic` and `Camcorder`. The first
reinstall in this trial showed the permission dialog again, as an earlier
grant had not survived it. After "While using the app", it survived
`adb install -r`.

## 2026-10-22 07:53 EDT — Runtime permissions for apps

Plan: `docs/design/20261022_02_APP_PERMISSIONS_PLAN.md` (Draft).

On the owner's instruction, to be implemented now. Apps can ask for any
runtime permission through `MorphContext` and get the answer in a new
`on_permissions` hook. The Android runner matches answers to requests by
the ids libmobile now returns, so libmorph's own `RECORD_AUDIO` request
for capture can no longer take an app's answer. Depends on the amended
libmobile permissions plan. To be released with audio capture as 0.2.2.

## 2026-10-22 08:15 EDT — Audio capture: Android through libmobile

Plan: `docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md` (In implementation),
amended for step 8.

libmorph's Android capture had opened AAudio streams itself. On the owner's
direction that code moved to libmobile as `android::CaptureStream`
(libmobile `docs/design/20261022_02_AUDIO_CAPTURE_PLAN.md`), and libmorph
now wraps it. Nothing changes for apps. On the owner's S23, libmobile's
own `audiocapture` example asked for the microphone, recorded at 48 kHz
stereo once allowed, and switched to `Generic` and `Camcorder` on taps.

## 2026-10-22 08:16 EDT — Audio capture through libmobile on the S23

Plan: `docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md`, step 8.

With capture opened through libmobile's `CaptureStream`, the example on the
owner's Galaxy S23 Ultra cycled through every mode on taps. Each was
granted as asked, and `dumpsys audio` showed the matching source: `MIC`,
`CAMCORDER`, `VOICE_RECOGNITION`, `VOICE_COMMUNICATION`, `UNPROCESSED`,
`VOICE_PERFORMANCE`. 24 files were written across the switches.
`src/audio.rs` has no `ndk::audio` and no `unsafe`.

## 2026-10-22 08:28 EDT — Released: libmorph 0.3.0 and libmorph-build 0.2.0

Plans: `docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md` and
`docs/design/20261022_02_APP_PERMISSIONS_PLAN.md` (both In implementation).

The owner asked for the work to be merged with proper versioning. The plans
and earlier entries said 0.2.2, but libmobile was released as 0.2.0, not
0.1.2: its `cpal` feature now needs Android 9. By Rule 5.5 that makes
libmorph 0.3.0 (tag `v0.3.0`) and libmorph-build 0.2.0 (tag
`libmorph-build-v0.2.0`), both resolving libmobile `d7ab0c1` (tag
`v0.2.0`) from GitHub. libmorph's own changes are additive.

Left open in the plans, so neither is Implemented yet: on the S23, an
explicit "Don't allow" twice (the Settings message), and the owner
listening to the files; on the desktop, the spectrogram with a live
microphone.

## 2026-10-22 08:30 EDT — Platform layering: libmobile first

Owner's rule (2026-10-22), recorded here rather than in a plan: Android
platform features are built in libmobile first, with an example there, and
libmorph wraps them. It follows the move of libmorph's AAudio capture code
into libmobile's `CaptureStream`. AGENTS.md gains "Platform Layering
(libmobile first)", and the README's Android section explains it.

## 2026-10-22 18:19 EDT — Secure store (proposed)

Plan: `docs/design/20261022_03_SECURE_STORE_PLAN.md` (Draft), with
libmobile's `docs/design/20261022_03_SECURE_STORE_PLAN.md`, which comes
first. Requested by the handlr session with the owner's approval, for the
companion app's Noise key and server record. Proposed only. Nothing is
implemented until the owner approves, and the desktop backend is an
open question there.

## 2026-10-22 19:04 EDT — Opaque keys in the secure store (proposed)

Plan: `docs/design/20261022_04_OPAQUE_KEYS_PLAN.md` (Draft), which builds
on `20261022_03_SECURE_STORE_PLAN.md`. The owner asked for keys that can
be generated and validated without being read: the key logic stays in
an adapter (a black box such as handlr's Noise adapter) that is passed to
the store. The store is generic over the adapter's private type `K` and
public type `P`, and seals `K` under names the blob API refuses. A
token only libmorph can make keeps sealing in the store's hands.
libmobile is unchanged.

## 2026-10-22 19:08 EDT — Secure store: the desktop backends move to libdevice

Plans: `docs/design/20261022_03_SECURE_STORE_PLAN.md` (In implementation)
and `docs/design/20261022_04_OPAQUE_KEYS_PLAN.md` (Draft, amendment "a key
store of its own").

This departs from the secure-store plan, section 3. Its macOS Keychain
and memory backends, built on libmorph's unpushed `secure-store` branch,
move to libdevice behind a `secure-store` feature, with the same API as
libmobile's store. libmorph's `SecureStore` then wraps libdevice on the
desktop as it wraps libmobile on Android. The owner agreed (2026-10-22)
that both crates need wrapped implementations with the same API.
libmorph's public API for the store does not change. The work is a step
of the opaque keys plan.

## 2026-10-22 19:55 EDT — Key store for keying material (proposed); opaque keys plan superseded

Plan: `docs/design/20261022_05_KEY_STORE_PLAN.md` (Draft), superseding
`20261022_04_OPAQUE_KEYS_PLAN.md`, libmobile's key store plan and the key
half of libdevice's plan. Based on the owner's engine sketch and the review
of it:
- one engine file, identical in libmobile and libdevice, over a per-platform
  `Vault`;
- `with_key` lends the key for one call, without holding the lock;
- the flags are enforced by the platform, and the app-unlock flag is
  refused until it exists;
- libmorph re-exports the platform's engine instead of bridging.

handlr's Noise adapter does its static DH inside `with_key`, so snow never
holds the private key.

## 2026-10-22 20:31 EDT — Key store plan consolidated; implementation next

Plan: `docs/design/20261022_06_KEY_STORE_PLAN.md` (Draft), superseding
`20261022_05_KEY_STORE_PLAN.md`. It is one document drafted from plan 05
and its amendments, reviewed, and adjusted:
- `Public` keeps its bounds;
- the engine and memory vault build on every host;
- the concurrent-migration test is runnable.

The owner said it is ready to build. The Android blob store moves to the
Application context with it, because the two stores share
`SecureStorePlugin`.

## 2026-10-22 20:36 EDT — Secure store: Android without an app clone

Plans: `docs/design/20261022_03_SECURE_STORE_PLAN.md` (In implementation),
and `docs/design/20261022_06_KEY_STORE_PLAN.md`, step 2.

A departure from the secure-store plan, which had the Android backend hold
an `AndroidApp` clone. libmobile's blob store now uses the Application
from `ndk-context` and takes no app, so libmorph's backend holds nothing.
On the S23 Ultra, the Android `securestore` example reports
`AndroidStrongBox` and counts launch 3 after the earlier two.

## 2026-10-22 20:52 EDT — Secure store: desktop backends from libdevice

Plans: `docs/design/20261022_03_SECURE_STORE_PLAN.md` (In implementation),
and `docs/design/20261022_06_KEY_STORE_PLAN.md`, step 7.

As recorded earlier (the desktop backends move to libdevice), libmorph's
`SecureStore` now wraps libdevice's `secure_store` for the macOS
Keychain and the scripted runs' memory store. libmorph no longer depends
on `security-framework`.

The key store is in: `libmorph::key_store` and `ctx.key_store()`. On the
owner's S23 Ultra, the Android `securestore` example generated key
`device` in namespace `libmorph.securestore`, and loaded the same
fingerprint after a force-stop.

## 2026-10-22 21:09 EDT — The key store checked on macOS

Plan: `docs/design/20261022_06_KEY_STORE_PLAN.md`, step 7.

The owner ran the `securestore` example twice from a Terminal on the Mac
mini (Apple M4):
- run 1: `key device: f26e6d8ecc648479 (generated)`, `kept in
  MacKeychain`, `launch 1`, and the `unlocked-only` blob written and
  read;
- run 2: the same fingerprint `(loaded)` and `launch 2`.

So with this, libmorph's key store and secure store keep their values
across relaunches on both the S23 Ultra and macOS. The first run also
logged a cpal "buffer underrun or overrun" from the audio output at
startup, which is unrelated to the store.

## 2026-10-22 21:13 EDT — Released libmorph 0.3.1 and libmorph-build 0.2.1

Plans: `docs/design/20261022_03_SECURE_STORE_PLAN.md` and
`docs/design/20261022_06_KEY_STORE_PLAN.md` (both Implemented).

On the owner's word, the `secure-store` branches were merged and released
in order:
1. libmobile 0.2.1 (`b3bf43e`, tag `v0.2.1`);
2. libdevice 0.1.1 (`b46963b`, tag `v0.1.1`), its first release under the
   critical rules;
3. libmorph 0.3.1 (tag `v0.3.1`) and libmorph-build 0.2.1 (tag
   `libmorph-build-v0.2.1`).

`Cargo.lock` was resolved against the pushed crates with the local patch
moved aside. Next, the handlr session gets the revision to pin.

## 2026-10-22 23:01 EDT — Opening the app from a URL scheme (proposed)

Plan: `docs/design/20261022_07_URL_SCHEMES_PLAN.md` (Draft). Requested by
the handlr session with the owner's approval, for `handlr://…/pair#…`
pairing links scanned or tapped on the phone.
- libmobile gains `Project::url_scheme` for the manifest's intent
  filters, and stops logging intents in full: it currently logs the whole
  URL, which would leak the pairing secret.
- libmorph gains `MorphApp::on_open_url` over libmobile's existing intent
  bridge.
- macOS needs an app bundle and is deferred.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-22 23:30 EDT — Opening the app from a URL scheme (approved)

Plan: `docs/design/20261022_07_URL_SCHEMES_PLAN.md` (Draft, approved).
The owner approved it with its libmobile part, which is now libmobile's
`docs/design/20261022_05_URL_SCHEMES_PLAN.md`. No Cargo feature. The
launch mode question waits for the S23 check; macOS stays open.

## 2026-10-22 23:30 EDT — URL schemes: implemented in libmobile, wrapped here

Plan: `docs/design/20261022_07_URL_SCHEMES_PLAN.md`. On the owner's
instruction, an amendment records that the schemes and the log redaction
are implemented in libmobile (its `docs/design/20261022_05_URL_SCHEMES_PLAN.md`),
and libmorph only wraps them with `on_open_url`, the `open` script
command and the `urlopen` example.

## 2026-10-23 01:24 EDT — Foreground service and system events (proposed)

Plan: `docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md` (Draft).
Requested by the handlr session on the owner's instruction: the companion
app's link to the daemon closes about 7 seconds after the app is
backgrounded, because of the cached-app freezer.
- libmobile would generate a foreground service from the app's build
  script, with Rust control from any thread, and add a runtime-agnostic
  stream of system events that keeps working while the UI is paused.
- libmorph wraps both.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-23 18:04 EDT — URL schemes: `#` on an `open` line

Plan: `docs/design/20261022_07_URL_SCHEMES_PLAN.md`, step 4 (`319e16c`).
A small departure needed to make the step work. In scripts, `#` to the
end of a line is a comment, which would cut a URL's fragment, and
pairing links carry their secret there. On a line whose first word is
`open`, `#` is part of the URL, and the line has no comment. Other
lines are unchanged.

## 2026-10-23 18:10 EDT — URL schemes on the S23 (libmorph)

Plan: `docs/design/20261022_07_URL_SCHEMES_PLAN.md`, step 5
(`228d2df`, `801ae00`), on the owner's S23 Ultra with the `urlopen`
example:
- `am start -a android.intent.action.VIEW -d 'libmorph-urlopen://a/b#secret'`
  cold-started it (`LaunchState: COLD`). The app logged `waiting for a
  link` from `init` and then, once, `opened with
  libmorph-urlopen://a/b<redacted>`;
- a link sent while the app was in front, and one sent after Home, were
  each logged once (`is_new=true`), with one activity in one task
  (`t5017`);
- `secret` appeared in logcat only in adbd's and the system's records of
  the command, never from the app.

Waiting on the owner: scanning a QR code of a `libmorph-urlopen://` URL
with the Samsung camera.

## 2026-10-23 18:13 EDT — URL schemes: the camera check passed on the owner's word

Plan: `docs/design/20261022_07_URL_SCHEMES_PLAN.md`, step 5. Scanning a
QR code of `libmorph-urlopen://camera-scan/qr#secret` with the camera
app (Google Lens) on the S23 started no intent. The owner: "it should work
if the scheme is registered. let's just pass as is, I'll test it with
handlr". Step 5 is passed as is; the camera check moves to handlr.

## 2026-10-23 18:14 EDT — Released libmorph 0.3.2 and libmorph-build 0.2.2

Plan: `docs/design/20261022_07_URL_SCHEMES_PLAN.md` (Implemented). On
the owner's word ("build next release"): libmorph 0.3.2 (`1b3d7d8`, tag
`v0.3.2`) with `on_open_url` and `open <url>`, and libmorph-build
0.2.2 (tag `libmorph-build-v0.2.2`), both taking libmobile 0.2.2.
`Cargo.lock` was resolved with the local patch moved aside.

## 2026-10-23 18:18 EDT — Foreground service approved, built in libmobile first

Plan: `docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md` (Draft,
approved). The owner approved it with libmobile's
`docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md`, which now carries
the service, its Termux-style notification controls and the system
events. libmorph only wraps them, after libmobile 0.2.3. No background
microphone: Android stops input when the app is hidden.

## 2026-10-23 21:59 EDT — Opening a link in another app (proposed)

Plan: `docs/design/20261023_02_OPEN_LINK_PLAN.md` (Draft). Requested by the handlr
session for repowiki links in its agent view: an outbound call that opens
an http or https URL in the browser. libmobile builds it for Android,
libdevice for the Mac, and libmorph wraps both as
`MorphContext::open_link`.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-23 22:02 EDT — Plans approved

The owner approved, on 2026-10-23 ("approve plans"), with the
recommended answers:
- `docs/design/20261023_02_OPEN_LINK_PLAN.md`: `MorphContext::open_link`,
  after libmobile's and libdevice's releases.

## 2026-10-23 23:30 EDT — App storage directories (proposed)

Plan: `docs/design/20261023_03_APP_STORAGE_PLAN.md` (Draft). On the owner's request, relayed by the
handlr session: "libmorph should provide a persistent storage location on
all platforms, with mkdir_all". libmobile gives Android's files and cache
directories, libdevice the desktop paths by app id, and libmorph creates
them before `init` as `MorphContext::storage_dir()` and `cache_dir()`.

Proposed only; nothing is implemented until the owner approves.

## 2026-10-23 23:30 EDT — App storage: `data_dir()` itself becomes cross-platform

Plan: `docs/design/20261023_03_APP_STORAGE_PLAN.md` (Draft). The owner's
correction, relayed by the handlr session: `data_dir()` was meant to be
cross-platform. An amendment drops `storage_dir()`; `data_dir()` keeps
its signature and is `Some` on every host with a filesystem, created
before `init`. The release becomes libmorph 0.4.0.
