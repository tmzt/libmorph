---
title: Camera view and camera capture across desktop, web and Android
date: 2026-10-14
status: Hold
covers:
  - Splitting the camera into two features on libmorph's public surface,
    `camera-view` (a live preview the app samples as a wgpu texture) and
    `camera-capture` (one still photo), with `camera-stream` (encoded
    recording) reserved for later
  - Replacing the `mobile-camera` passthrough with those platform-neutral
    features
  - libmobile renaming `camera` to `camera-capture` and adding `camera-view`
    (Android NDK camera, CAMERA permission plugin)
  - Desktop webcam through nokhwa, web webcam through `getUserMedia`
  - One GPU frame path, the `MorphApp::on_camera` hook and the `Camera` API
  - A camera example app crate that runs on desktop and Android
sources:
  - Owner's question (2026-10-14): "can we add a basic view-only camera app to
    test this or does that need some support in libdevice or a mac-specific
    camera input? ... (webcam emulation only, not photo)"
  - Owner's question (2026-10-14): "can we integrate nokhwa on mobile? it
    claims to be cross-platform?" Answer: no. nokhwa 0.10.11 has backends for
    AVFoundation (macOS), V4L2 (Linux) and Media Foundation (Windows) only.
    Its README advertises a browser backend, but the published crate has no
    such feature and the code is commented out (`src/lib.rs`).
  - Owner's direction (2026-10-14): "split camera feature into two
    camera-capture and camera-view (with camera-stream added later for actual
    webcam-like recording). this should change the exposed wrapper surface."
  - libmobile `src/camera.rs`, `src/android/camera.rs`,
    `android/java/dev/highbay/libmobile/camera/CameraPlugin.kt`: today's
    `camera` feature hands off to the system camera app
    (`ACTION_IMAGE_CAPTURE`) and returns a JPEG thumbnail; there is no
    frame stream and no permission flow
  - ndk-sys 0.6: declares `ACameraManager_*`, `ACameraDevice_*`,
    `ACameraCaptureSession_*`, but does not link `libcamera2ndk`; ndk 0.9
    `media::image_reader` wraps `AImageReader` (feature `media`)
related:
  - AGENTS.md (Cargo Features & Passthrough, Gradle Handoff, Core API,
    Lifecycle): this plan changes the feature list and adds to the Core API
  - libmobile README (Cargo features table)
living sections:
  - "10. Tunable constants"
---

# Camera view and camera capture

## 1. Aim

An app built on libmorph can show a live camera preview and take a still
photo with the same code on desktop, the web and Android. The preview is a
wgpu texture the app samples in its own render pass; libmorph owns the
device, the permission flow, the frame upload and the lifecycle.

The camera is split by what the app needs, not by platform:

| Feature | Gives the app | Android | Desktop | Web |
|---|---|---|---|---|
| `camera-view` | a live preview texture | NDK camera (libmobile `camera-view`) | nokhwa webcam | `getUserMedia` |
| `camera-capture` | one still photo, RGBA | system camera app (libmobile `camera-capture`) | one frame from the webcam | one frame from the webcam |
| `camera-stream` (later, section 11) | encoded recording | — | — | — |

Desktop and web read the real webcam: that is the emulation. libdevice is
not involved.

## 2. Feature surface

### 2.1 libmorph `Cargo.toml`

`mobile-camera` is removed. New features:

```toml
[features]
# Internal: the shared camera machinery. Not for apps to name.
_camera = [
    "dep:nokhwa",
    "dep:zune-jpeg",
    "web-sys?/MediaDevices",
    "web-sys?/MediaStream",
    "web-sys?/MediaStreamConstraints",
    "web-sys?/MediaStreamTrack",
    "web-sys?/HtmlVideoElement",
    "web-sys?/Navigator",
]
camera-view = ["_camera", "libmobile?/camera-view"]
camera-capture = ["_camera", "libmobile?/camera-capture"]
```

`libmobile?/…` enables libmobile's feature only when `mobile` is on. Android
already requires `mobile` (runner guard), so on Android the camera features
always reach libmobile.

Dependencies:

```toml
[target.'cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))'.dependencies]
nokhwa = { version = "0.10.11", optional = true, default-features = false, features = ["input-native"] }

[dependencies]
zune-jpeg = { version = "0.5", optional = true }   # MJPEG webcams; Android photos
```

nokhwa's `default` (`decoding`) pulls in mozjpeg, a C library, and its
`output-wgpu` is pinned to wgpu 27. Neither is used: frames are converted in
libmorph (section 5).

`mobile-google-auth` is unchanged.

### 2.2 libmobile `Cargo.toml`

```toml
[features]
default = []
google-auth = []
# Renamed from `camera`. Still the system-camera-app hand-off.
camera-capture = []
# New: live frames from the NDK camera, plus the CAMERA permission plugin.
camera-view = ["ndk/media"]
```

There is no `camera` alias. libmobile is 0.1 and unpublished, and libmorph
is its only consumer outside its own examples.

## 3. libmorph public API

### 3.1 Types (`src/camera/mod.rs`, always compiled)

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Facing { Front, Back }

/// The newest preview frame, valid until the next `update`.
pub struct CameraFrame<'a> {
    pub view: &'a wgpu::TextureView,   // Rgba8Unorm, sampled, not sRGB
    pub size: (u32, u32),              // as delivered, before rotation
    pub quarter_turns: u8,             // clockwise turns to make it upright, 0..=3
    pub mirrored: bool,                // true for front cameras
}

#[derive(Clone, Debug)]
pub struct Photo { pub width: u32, pub height: u32, pub rgba: Vec<u8> }

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum CameraEvent {
    Started { facing: Facing, size: (u32, u32) },
    Stopped,
    PermissionDenied,
    /// The feature for the call is off, or the host has no camera.
    Unsupported,
    Failed(String),
    Photo(Photo),
}
```

### 3.2 `Camera` (reached as `ctx.camera()` / `ctx.camera_mut()`)

```rust
impl Camera {
    pub fn start(&mut self, facing: Facing);     // camera-view
    pub fn stop(&mut self);                      // camera-view
    pub fn is_running(&self) -> bool;            // camera-view
    pub fn frame(&self) -> Option<CameraFrame<'_>>; // camera-view
    pub fn capture_photo(&mut self);             // camera-capture
}
```

Every method exists whatever the features. With its feature off, `start` and
`capture_photo` queue `CameraEvent::Unsupported`, `frame` returns `None` and
`is_running` returns false. Without `_camera`, `Camera` is zero-sized.

Results arrive through a new `MorphApp` hook with a default body, so existing
apps compile unchanged:

```rust
fn on_camera(&mut self, ctx: &mut MorphContext, event: CameraEvent) {}
```

`start` is asynchronous everywhere (permission prompts, device open).
`Started` reports the delivered size; `frame()` is `Some` from the first frame
after it.

### 3.3 Ordering within a frame

The runners' per-frame sequence (`Shell::frame`) becomes:

1. drain camera events → `on_camera` for each;
2. upload the newest camera frame, if one arrived (section 5);
3. `update`;
4. `render`.

### 3.4 Lifecycle

On suspend (`Shell::detach`) the camera device is closed and the requested
facing is remembered; `Stopped` is sent. On resume (`Shell::attach` with
`resumed`) it is reopened and `Started` is sent again. Android requires this:
a backgrounded app must release the camera. A pending `capture_photo` on
Android survives backgrounding, because the system camera app backgrounds the
activity by design.

## 4. Backends

All backends implement one internal trait (`src/camera/backend.rs`):

```rust
pub(crate) trait Backend {
    fn start(&mut self, facing: Facing);
    fn stop(&mut self);
    fn capture(&mut self);
    /// Events since the last call, and the newest frame if one arrived.
    fn poll(&mut self) -> (Vec<CameraEvent>, Option<RawFrame>);
}

pub(crate) enum RawFrame {
    Rgba { width: u32, height: u32, data: Vec<u8> },
    /// Android YUV_420_888, copied out of the AImage before it is released.
    Yuv420 { width: u32, height: u32, y: Vec<u8>, y_stride: u32,
             u: Vec<u8>, v: Vec<u8>, uv_row_stride: u32, uv_pixel_stride: u32 },
    /// Web: drawn straight from the <video> element on the GPU.
    #[cfg(target_arch = "wasm32")]
    Video,
}
```

### 4.1 Desktop (`src/camera/desktop.rs`, nokhwa)

- A capture thread owns the `nokhwa::Camera` (created on that thread; it is
  not `Send` on every platform). It sends frames over a bounded channel of
  capacity 1 using `try_send`, so a slow app drops frames rather than queues
  them.
- Device choice: the first device from `nokhwa::query(ApiBackend::Auto)`.
  `Facing::Front` and `Facing::Back` both open it; `mirrored` is true for
  `Front`. Desktops rarely say which way a webcam faces.
- Format: request the closest to `CAMERA_SIZE` at `CAMERA_FPS` (section 10),
  preferring NV12, then YUYV, then MJPEG. Convert to RGBA on the capture
  thread: NV12 and YUYV with the BT.601 full-range converter in
  `src/camera/convert.rs`, MJPEG with zune-jpeg.
- macOS: call `nokhwa::nokhwa_initialize` before the first open. A refusal
  becomes `PermissionDenied`. The TCC prompt is attributed to the terminal or
  IDE that launched the binary.
- `capture_photo`: the next converted frame, as `Photo`. If the view is not
  running, the camera is opened for one frame and closed again.

### 4.2 Web (`src/camera/web.rs`)

- `navigator.mediaDevices.getUserMedia({ video: { facingMode, width, height } })`
  with `facingMode` `"user"` (Front) or `"environment"` (Back). A
  `NotAllowedError` becomes `PermissionDenied`.
- The stream plays in a hidden, muted, `playsinline` `<video>` element that is
  not attached to the page. Each frame where `readyState >= 2`, the frame
  is copied with `Queue::copy_external_image_to_texture` into the camera
  texture (WebGPU and WebGL2 both support it).
- `capture_photo`: the next copied frame, read back with a buffer mapping and
  sent as `Photo`.
- `stop`: stop every track on the stream and drop the element.

### 4.3 Android (`src/camera/android.rs`, libmobile)

- `start` → `libmobile::android::camera_view::start(app, facing)`.
  libmobile asks for the permission if needed (section 6), opens the camera
  and feeds an `AImageReader`.
- Each tick, `camera_view::latest_frame()` returns the newest `AImage` planes.
  libmorph copies them into `RawFrame::Yuv420` (about 1.4 MB at 1280×720),
  and the image goes straight back to the reader.
- `quarter_turns` comes from the sensor orientation that libmobile reports
  (`ACAMERA_SENSOR_ORIENTATION`), corrected for display rotation
  (`AConfiguration` orientation). `mirrored` is true for front cameras.
- libmobile events become `CameraEvent`s: `CameraPermission(false)` →
  `PermissionDenied`, `CameraOpened { .. }` → `Started`,
  `CameraError(msg)` → `Failed`.
- `capture_photo` → `libmobile::android::capture_photo` (the system camera
  app). `MobileEvent::Capture(Jpeg)` is decoded with zune-jpeg into `Photo`;
  `Failed(msg)` becomes `Failed`.

## 5. GPU frame path (`src/camera/texture.rs`, `src/camera/yuv.wgsl`)

- One `Rgba8Unorm` texture with `TEXTURE_BINDING | COPY_DST |
  RENDER_ATTACHMENT`, recreated when the frame size changes.
- `RawFrame::Rgba` is uploaded with `Queue::write_texture`.
- `RawFrame::Yuv420` is uploaded as an `R8Unorm` Y texture and an `Rg8Unorm`
  chroma texture at half size. When `uv_pixel_stride` is 2 and U and V
  interleave, the chroma rows are copied as they are; otherwise they are
  interleaved on the CPU. A full-screen pass in `yuv.wgsl` writes RGBA with
  BT.601 full-range coefficients:

  ```
  r = y + 1.402 (v − 0.5)
  g = y − 0.344136 (u − 0.5) − 0.714136 (v − 0.5)
  b = y + 1.772 (u − 0.5)
  ```

- `RawFrame::Video`: `copy_external_image_to_texture` writes into the texture
  directly.
- `src/camera/convert.rs` holds the CPU versions of the same conversion
  (NV12, YUYV). They are used by the desktop backend and serve as the
  reference that the shader is tested against.

## 6. libmobile changes

### 6.1 Rename `camera` → `camera-capture`

The Cargo feature, the `codegen.rs` plugin registry entry and its tests
(`known_features()` becomes `["google-auth", "camera-capture", "camera-view"]`),
the README table and the module `cfg`s. The Kotlin directory `camera/` and
`MobileEvent::Capture` keep their names.

### 6.2 Permission results reach plugins

`MobileActivityPlugin` gains:

```kotlin
fun onRequestPermissionsResult(
    activity: Activity, requestCode: Int,
    permissions: Array<String>, grantResults: IntArray,
): Boolean = false
```

`MobileNativeActivity.onRequestPermissionsResult` dispatches to each plugin
until one returns true.

### 6.3 `camera-view`

- Kotlin `cameraview/CameraViewPlugin.kt` exposes
  `requestPermission(activity)`, with request code `0x4c56` ("LV"). The
  result goes to Rust through `nativePermissionResult(granted: Boolean)`.
  Its codegen contributions are `<uses-permission android:name="android.permission.CAMERA"/>`
  and `<uses-feature android:name="android.hardware.camera.any" android:required="false"/>`.
- Rust `src/android/camera_view.rs`, using the ndk-sys `ACamera*`
  declarations. `build.rs` adds `cargo:rustc-link-lib=camera2ndk` when the
  target is Android and `camera-view` is on.
  - `start(app, facing)`: check the permission with JNI `checkSelfPermission`,
    request it if missing, then open the first camera whose
    `ACAMERA_LENS_FACING` matches. Pick the largest
    `ACAMERA_SCALER_AVAILABLE_STREAM_CONFIGURATIONS` YUV_420_888 size that
    fits `CAMERA_SIZE` (section 10). Create an `AImageReader` with
    `max_images = 3` and a repeating `TEMPLATE_PREVIEW` request targeting it.
  - `stop()`: close the session, the device and the reader, in that order.
  - `latest_frame() -> Option<YuvFrame<'_>>`: `acquire_latest_image`. The
    planes are borrowed until the `YuvFrame` is dropped, which releases the
    image.
  - Camera callbacks (device errors, session state) push into a `Mailbox`
    and arrive in the loop as `MobileEvent::CameraPermission(bool)`,
    `MobileEvent::CameraOpened { width, height, sensor_orientation, front }`
    and `MobileEvent::CameraError(String)`.
  - `run` stops the camera on `Pause` and on `Destroy`. Restarting is the
    caller's job, which libmorph does in section 3.4.

## 7. Example app (`examples/camera-app/`)

A standalone crate with its own `[workspace]` table, so it never joins a
parent workspace:

- `src/lib.rs`: `CameraApp`.
  - `init` calls `ctx.camera_mut().start(Facing::Back)`.
  - `render` draws the frame with aspect fill inside the safe area, applying
    `quarter_turns` and `mirrored`.
  - A tap in the top half switches facing.
  - With `camera-capture`, a tap in the bottom half takes a photo and shows
    it as an inset for 2 s.
  - `on_camera` logs every event and shows `PermissionDenied`/`Failed` as a
    tinted screen.
  - `android_main` calls `run_android::<CameraApp>`.
- `src/main.rs` (not Android, not wasm): `libmorph::run::<CameraApp>()`.
- `build.rs`: `libmobile::gradle::Project::new("dev.highbay.libmorph.camera")
  .env_prefix("libmorph").generate()`.
- `Cargo.toml`: `crate-type = ["lib", "cdylib"]`, libmorph with
  `features = ["camera-view", "camera-capture"]`, plus `mobile` on Android, and
  libmobile as a build-dependency.

## 8. Testing

- Host unit tests:
  - `convert.rs`: NV12 and YUYV against known pixels (black, white, the
    primary colours).
  - The Yuv420 chroma packing for both pixel strides.
  - `quarter_turns` for every sensor orientation and display rotation.
  - The `Unsupported` events of the stubs.
  - libmobile size selection and the feature registry.
- GPU test (`#[ignore]`, run on hosts with an adapter): `yuv.wgsl` against
  `convert.rs` on a 16×16 pattern, within 1/255.
- Build matrix, clippy-clean: desktop {default, `camera-view`,
  `camera-capture`, both, `--no-default-features`}; wasm32 `web` with and
  without the camera features; aarch64-linux-android `mobile` with and without
  them.
- Device checks (the owner runs these; agent sandboxes cannot open windows or
  cameras), recorded under `out/camera/` (AGENTS.md Rule 3.5):
  - macOS: preview, flip, photo, and the permission refusal path;
  - Chrome over WebGPU and over WebGL2 (`--disable-features=WebGPU`);
  - an Android phone: background and resume while previewing, rotating the
    device, front and back, and photo.

## 9. Steps

Each step is one crate per commit (Rule 3). libmobile steps are pushed before
the libmorph steps that use them, and libmorph's `Cargo.lock` is re-resolved
with no local patch active.

1. **libmobile: rename `camera` → `camera-capture`** (6.1).
   **Done when:** `cargo test` passes, with or without
   `--features camera-capture`, and `GeneratedPlugins.kt` lists
   `CameraPlugin` only under that feature.
2. **libmobile: permission results reach plugins** (6.2).
   **Done when:** the Kotlin compiles in a generated Gradle project
   (`gradle assembleDebug` of `examples/bitmap`) and nothing changes for
   plugins that don't override the hook.
3. **libmobile: `camera-view`** (6.3).
   **Done when:** clippy for `aarch64-linux-android --all-features` is clean,
   the host tests (size selection, registry, manifest contributions) pass,
   and the generated manifest has the CAMERA permission only with the
   feature.
4. **libmorph: feature surface and API** (2.1, 3, stubs; no backends yet).
   `mobile-camera` is removed. `on_camera` and the event drain are wired
   into both runners.
   **Done when:** the build matrix in section 8 is clippy-clean, and a stub
   test sees `Unsupported` from `start` and `capture_photo` with the
   features off.
5. **libmorph: GPU frame path** (5).
   **Done when:** the `convert.rs` tests pass, and the ignored GPU test passes
   on the owner's Mac.
6. **libmorph: desktop backend** (4.1), plus `examples/camera-app` on desktop
   (7) in its own commit.
   **Done when:** the owner runs `cargo run` in `examples/camera-app` on
   macOS and sees the preview, a flip and a photo inset. A refused camera
   permission shows the denied state instead of crashing.
7. **libmorph: web backend** (4.2).
   **Done when:** the example, built for wasm with `web`, shows the preview in
   Chrome over WebGPU and over WebGL2.
8. **libmorph: Android backend** (4.3), plus the Android half of the example
   in its own commit.
   **Done when:** the example APK previews on a phone, survives background
   and resume, follows rotation, flips between cameras, and takes a photo
   through the system camera app.
9. **AGENTS.md: feature and Core API sections updated** (its own commit).
   **Done when:** AGENTS.md lists `camera-view`/`camera-capture` and the
   `Camera` API, with no `mobile-camera` left.

## 10. Tunable constants (living section)

| Name | Value | Where | Meaning |
|---|---|---|---|
| `CAMERA_SIZE` | 1280 × 720 | libmorph, libmobile | largest preview size requested |
| `CAMERA_FPS` | 30 | libmorph desktop | requested webcam rate |
| `IMAGE_READER_MAX` | 3 | libmobile | `AImageReader` buffer count |
| `DESKTOP_QUEUE` | 1 | libmorph desktop | frames buffered between threads |
| `PHOTO_INSET_SECS` | 2.0 | example | how long the photo inset shows |

## 11. Later: `camera-stream`

Reserved for webcam-style recording: encoded video, and audio with it, to
a file or a stream. It is named here so `camera-view` and `camera-capture`
are not stretched to cover it. It would build on the same backends, with
encoders on top: `MediaCodec` (Android), `MediaRecorder` (web), and a
platform encoder or ffmpeg on desktop. Not part of this plan; it needs its
own.

## Amendment — 2026-10-14 12:14 EDT: Decisions confirmed

The owner confirmed (2026-10-14) the five decisions put to them when the plan
was written. The body stands as written on each point:

1. Desktop and web `camera-capture` takes one frame from the webcam (4.1,
   4.2); there is no system camera app to hand off to.
2. Android `camera-view` uses the NDK camera (`ACamera*` and
   `AImageReader`), with a Kotlin plugin only for the CAMERA permission
   (6.2, 6.3), not CameraX. Frames are copied out each tick and converted from
   YUV on the GPU (4.3, 5).
3. On desktop, the first webcam serves both `Facing::Front` and
   `Facing::Back`, and `mirrored` is true for `Front` (4.1).
4. nokhwa is used without its default features. That leaves out mozjpeg and
   `output-wgpu` (wgpu 27). MJPEG webcams are decoded with zune-jpeg (2.1,
   4.1).
5. The example is the standalone crate `examples/camera-app/`, for desktop
   and Android (7).

## Amendment — 2026-10-14 12:14 EDT: Status Draft → Hold

The owner placed the plan on hold after confirming the decisions above; it is
not approved for implementation. No step of section 9 may start until an
amendment returns it to `Draft`.

## Amendment — 2026-10-14 12:32 EDT: Example app builds through libmorph-build

Follows docs/design/20261014_02_LIBMORPH_BUILD_CRATE_PLAN.md. This replaces
two items in section 7 (`examples/camera-app/`):

- `build.rs`: `libmorph_build::gradle_project("dev.highbay.libmorph.camera")
  .generate()`, replacing
  `libmobile::gradle::Project::new(…).env_prefix("libmorph").generate()`.
- `Cargo.toml`: `libmorph-build` as the build-dependency, from the
  same repository by path (`../../crates/libmorph-build`), in place of
  libmobile.

The example then names no libmobile dependency. The status stays `Hold`.
