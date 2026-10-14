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
