---
title: Camera capture and QR scanning for apps
date: 2026-10-28
status: Draft
covers:
  - "Passthrough features `mobile-camera-capture` and `mobile-camera-qr`
    for libmobile's `camera-capture` and `camera-qr`"
  - "`MorphContext::camera()`: `capture_frame`, `cancel_capture`,
    `start_qr`, `start_preview`, `stop`, `preview`, `set_zoom`,
    `zoom_by`, `zoom`, `zoom_range`; the same API on every host"
  - "`MorphApp::on_camera(&mut self, ctx, &CameraEvent)`"
  - "Pinch zoom in the app, from `Touch` events, driving `zoom_by`"
  - "Desktop and the web: `Unsupported` for now; scripted runs:
    `camera-qr` and `camera-frame` commands for headless tests"
sources:
  - "Owner (2026-10-28, through the flankr lead, verbatim): \"reaserch
    adding single frame camera capture to l8bmobile/libmorph in subagent
    as plan in that repo, use worktree in those repos, add qr capture mode
    with auto zoom, uee features for each subpart (such as qr capture) and
    related perm8ssions, only request camera access if caller uses
    camera\"; and \"it should also adjust exposure when reading screens
    and allow pinch zoom for gross adjustment (remote tv etc.)\"."
  - "libmobile docs/design/20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md (Draft):
    features `camera-device`, `camera-capture`, `camera-qr`; the lazy
    CAMERA gate; `android::camera_device`; `MobileEvent::Camera`."
  - "libmorph today: `Cargo.toml:28` `mobile-camera = [\"mobile\",
    \"libmobile/camera\"]`, a passthrough with no libmorph API (nothing in
    `src/` names a camera; `src/runner/overlay.wgsl` draws the emulated
    display's cutouts, not a preview). No desktop or web camera.
    `src/context.rs:223-266` (`request_permissions`, `permission_granted`,
    `should_show_rationale`), `src/runner/android.rs:176-201` (libmorph's
    own `RECORD_AUDIO` answer kept from `on_permissions` by request id),
    `src/input.rs:16` (`Touch` with a stable `id`), `src/script.rs:524-601`
    (script commands)."
  - "docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md (Hold): the
    earlier camera design, with platform-neutral `camera-view` and
    `camera-capture` features, nokhwa on the desktop and `getUserMedia` on
    the web."
related:
  - libmobile docs/design/20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md (the
    Android implementation; read it first)
  - docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md (Hold;
    overlaps, see section 7)
  - docs/design/20261022_02_APP_PERMISSIONS_PLAN.md
---

# Camera capture and QR scanning for apps

## 1. Goal

A libmorph app takes one still frame, or scans QR codes with auto zoom,
with the same code on every host. On Android it is libmobile's NDK camera
(libmobile plan of the same date); elsewhere, for now, every call answers
`Unsupported`, and scripted runs can inject frames and codes. The CAMERA
permission is asked for by libmobile on the first camera call, never at
launch, and only in apps built with a camera feature.

## 2. Features

```toml
# Passthrough: one still frame (`ctx.camera().capture_frame`).
mobile-camera-capture = ["mobile", "libmobile/camera-capture"]
# Passthrough: QR scanning with auto zoom and screen exposure
# (`ctx.camera().start_qr`); adds rqrr through libmobile.
mobile-camera-qr = ["mobile", "libmobile/camera-qr"]
```

Both turn on libmobile's `camera-device`, which adds CAMERA and optional
`uses-feature`s to the manifest; libmorph-build forwards that through the
existing Gradle handoff with no change. `mobile-camera` (the system camera
app) is unchanged. No `mobile-camera-device` passthrough: preview without
capture or QR has no user yet (question 3).

`ctx.camera()` and `on_camera` exist in every build. Without a camera
feature, or off Android, calls answer `CameraEvent::...` with
`CameraError::Unsupported` on the next frame, as the battery module does
with `BatteryError::Unsupported`.

## 3. API

`src/camera.rs`, always compiled. Types are libmorph's own, converted
from libmobile's with `From` under `cfg(all(target_os = "android",
feature = "mobile"))` (as `src/permission.rs` does), so desktop builds do
not need libmobile.

```rust
pub use types::{Facing, FrameFormat, Resolution, CaptureOptions, Frame,
                QrMode, ScreenExposure, QrOptions, QrCode, PreviewOptions,
                ZoomSource, CameraError, CameraEvent, CaptureId, ScanId};
```

Each mirrors libmobile's section 4 type field for field (libmobile's
`SessionId` is `ScanId` here), all `#[non_exhaustive]` where libmobile's
is, plus `CameraError::Unsupported` meaning "no camera on this host or
build".

```rust
impl MorphContext {
    /// The camera. Calls never block; answers come to `on_camera`.
    pub fn camera(&mut self) -> Camera<'_>;
}

impl Camera<'_> {
    pub fn capture_frame(&mut self, options: &CaptureOptions) -> CaptureId; // mobile-camera-capture
    pub fn cancel_capture(&mut self, capture: CaptureId);
    pub fn start_qr(&mut self, options: &QrOptions) -> ScanId;             // mobile-camera-qr
    pub fn start_preview(&mut self, facing: Facing, preview: PreviewOptions) -> ScanId;
    pub fn stop(&mut self, scan: ScanId);
    /// The newest preview frame (Gray8 or Rgba8, CPU memory).
    pub fn preview(&self, scan: ScanId) -> Option<Frame>;
    pub fn zoom_range(&self, scan: ScanId) -> Option<(f32, f32)>;
    pub fn zoom(&self, scan: ScanId) -> Option<f32>;
    pub fn set_zoom(&mut self, scan: ScanId, ratio: f32);
    pub fn zoom_by(&mut self, scan: ScanId, factor: f32);
}

// MorphApp, with a default body so existing apps compile unchanged:
fn on_camera(&mut self, ctx: &mut MorphContext, event: &CameraEvent) {
    let _ = (ctx, event);
}
```

- **Permission.** libmobile's gate asks for CAMERA and consumes its own
  answer; it never reaches `on_permissions`. A refusal arrives as
  `CameraError::PermissionDenied { settings_only }`; the app can then call
  `ctx.open_app_settings()`, a new `MorphContext` call wrapping libmobile's
  `open_app_settings` (`Err(OpenLinkError::Unsupported)` off Android).
- **Preview drawing.** `preview` gives CPU frames (at most 640 px on the
  long side by default); the app uploads them to its own texture with
  `queue.write_texture`. The GPU frame path of the Hold plan is not part
  of this plan.
- **Pinch zoom stays in the app.** libmorph delivers `Touch` events with
  stable ids; it does not recognise gestures. The app keeps the two
  touches of a pinch, and on each `Moved` calls
  `ctx.camera().zoom_by(scan, d_now / d_prev)`. The example (step 4)
  shows the twenty lines this takes. Auto zoom then works upward from the
  app's zoom, pauses 1.5 s after each call, and returns to it when the
  code is lost (libmobile 6.2).
- **Lifecycle.** libmobile closes the camera on pause and reopens a scan
  on resume; libmorph passes `Suspended` and `Opened` through. A capture
  in flight at suspend ends `Interrupted`.
- **Ordering.** Camera events are drained with libmobile's other bridge
  events, before input and `update`, so `on_camera` runs before the
  frame that sees its result.

## 4. Scripted runs

Under `script` (desktop, headless), for tests of apps such as flankr's
pairing scan:

- `camera-qr <text>`: the running scan, if any, reports
  `Qr { code }` with `text` as its payload (`version` 1, corners of the
  centred square); a `Once` scan then closes.
- `camera-frame <png path>`: the next `capture_frame` (or the one
  pending) answers with that PNG decoded as `Rgba8` (or its luma as
  `Gray8`; `Jpeg` answers `Unsupported`), path relative to the script.
- `camera-deny` / `camera-deny-forever`: the next camera call answers
  `PermissionDenied { settings_only: false | true }`.

`start_qr` in a scripted run answers `Opened` at once with a zoom range
of `(1.0, 8.0)`; zoom calls are recorded and reported as `Zoom` events.
The script log prints payload lengths, not payloads.

## 5. Steps

Each step one commit to libmorph, passing `cargo fmt -p libmorph --
--check`, `cargo clippy -p libmorph --all-targets` (desktop, wasm32
`web`, and `aarch64-linux-android` with `mobile`,
`mobile-camera-capture`, `mobile-camera-qr`) and `cargo test -p libmorph`.
libmobile's steps 2-5 are released before step 2 here.

1. **Types, `Camera`, `on_camera`, stubs** (3): every call answers
   `Unsupported`; `ctx.open_app_settings`. Unit test: each call queues
   one `Unsupported` answer with its id, delivered on the next frame.
   **Done when:** the checks pass on all three targets with no camera
   feature.
2. **Android** (2, 3): the passthroughs, the `From` conversions, the
   calls forwarded to `libmobile::android::camera_device`,
   `MobileEvent::Camera` routed to `on_camera`.
   **Done when:** the checks pass with both features, and a generated
   manifest has CAMERA only with one of them.
3. **Scripted runs** (4): the three commands. Tests: a script that
   starts a scan, injects a code and checks the app saw it; a capture
   answered from a PNG; a denial.
   **Done when:** those tests pass.
4. **Example `examples/android/camerascan`** (its own commit): the
   preview drawn from `preview()`, pinch zoom through `zoom_by`, tap to
   capture, `Continuous` QR with payload lengths on screen, a Settings
   button after `settings_only`.
   **Done when:** on the owner's S23 (asked first) the checks of
   libmobile's step 6 pass through libmorph, and the APK built with
   neither camera feature has no CAMERA permission.
5. **AGENTS.md and README** (separate commits): the two passthroughs in
   "Cargo Features & Passthrough" and the camera in "Core API"; this
   plan's row updated.
   **Done when:** both list the features and API.

## 6. Release

All additive: two features, a new module, a new `MorphContext` call
pair, and a `MorphApp` hook with a default body. libmobile 0.2.6 to 0.2.7
is compatible. Released as the next libmorph patch after the text input
release, with libmorph-build's next patch for the libmobile bump. If
libmobile takes the rename of its question 1 (0.3.0), libmorph's release
is a minor one under Rule 5.5, `mobile-camera` follows the rename, and
the entry is marked **Breaking**.

## 7. Relation to the Hold plan

`20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md` stays on Hold and is not
changed. It proposes platform-neutral `camera-view` and `camera-capture`
features (removing `mobile-camera`), nokhwa and `getUserMedia` backends,
a GPU texture path, and Android capture through the system camera app.
This plan instead adds `mobile-` passthroughs, as AGENTS.md asks for
features that exist only in libmobile, and an Android in-process capture.
The API here is host-neutral, so desktop and web backends from the Hold
plan can be added under it later, together with platform-neutral feature
names.

## 8. Open questions

1. **Feature names.** `mobile-camera-capture` and `mobile-camera-qr` now,
   with platform-neutral `camera-capture` / `camera-qr` once desktop and
   web backends exist; or platform-neutral names now?
2. **The Hold plan.** Once this is approved, an amendment to the Hold
   plan returning it to Draft with its capture parts replaced by this
   plan, or leave it on Hold for the live GPU view only?
3. **A preview-only feature.** Add `mobile-camera-device` for apps that
   want only a viewfinder?
4. **Desktop QR.** rqrr builds on the desktop; is a nokhwa webcam scan
   (for testing pairing on a Mac) wanted soon, or left with the Hold
   plan?
5. **Script command names.** `camera-qr`, `camera-frame`, `camera-deny`,
   `camera-deny-forever`?

## Amendment — 2026-10-28 14:46 EDT: `camera` is the in-process base, the hand-off is `camera-intent`; a wgpu viewfinder; fine zoom; failing with the reason; approved

The owner (2026-10-28), in order: "what's CameraX and camera-app? we
shouldnt be bringing in jetpack or similar if it can be avoided using a
wgpu viewfinder. the autozoom should also adjust fine zoom on supported
devices to read qr code. what is open_app_settings and why would it be
rejecting camera permission? the camera init should fail if user rejects
with the reason given if we hit an Android limit that might be why we'd
open the permissions screen in settings"; on the hand-off's name, "that's
more like camera-intent"; on libmorph's Hold plan, "just change it to
camera-intent in the plan and record the change of the camera feature";
then "approve this and the camera plans". Where this disagrees with the
body, it wins.

1. **Features.** libmobile's `camera` becomes the base of the in-process
   NDK camera (what the body calls `camera-device`: `ACamera*`,
   `AImageReader`, CAMERA and `uses-feature` with `required=false`,
   `libcamera2ndk`); `camera-capture` and `camera-qr` build on it. Today's
   system-camera-app hand-off (`ACTION_IMAGE_CAPTURE`) is renamed
   **`camera-intent`**. This is breaking: libmobile **0.3.0**. libmorph's
   passthroughs are `mobile-camera` (the base), `mobile-camera-capture`,
   `mobile-camera-qr` and `mobile-camera-intent` (the hand-off), recorded
   in libmorph's camera view plan (Hold) by its 2026-10-28 amendment.
   Every `camera-device` in the body reads `camera`, and the body's
   `camera` for the hand-off reads `camera-intent`.
2. **No Jetpack.** No AndroidX, CameraX or other Jetpack library: the NDK
   camera only, and the small Kotlin plugin for display rotation.
3. **A wgpu viewfinder.** Preview frames reach the screen through wgpu:
   each frame's YUV planes are uploaded into textures and converted to RGB
   in a shader; libmorph provides this as its camera viewfinder (no
   Android view). CPU frames stay available for capture and QR decoding.
4. **Fine zoom.** On API 30+ (`CONTROL_ZOOM_RATIO`, a continuous value)
   auto zoom ends with fine adjustments to the ratio that gives the code a
   readable module size, rather than stopping at the coarse steps; on API
   29 (crop region) it keeps the steps.
5. **Failing with the reason.** Starting the camera fails with
   `PermissionDenied { settings_only }` when the user refuses;
   `settings_only` is true when Android will no longer show the dialog.
   The library never opens Settings itself; `open_app_settings` is a
   helper the app may call (for example from a "Grant in Settings" button)
   when `settings_only` is true.
6. **Open questions** not answered keep the body's proposals (rqrr; the
   camera's permission answer consumed by its gate; no 2× probe when
   nothing is detected; torch and app-set exposure later; scan once with
   the preview on by default; the `mobile-` names).
7. **Approved.** The plan is approved with this amendment; the status
   becomes `In implementation` with its first code commit.
