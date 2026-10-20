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
