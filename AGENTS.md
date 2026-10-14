# libmorph — Agent Instructions

You are an expert Systems Architect and Senior Rust Developer working on **libmorph**, a unified windowing, graphics, and device-emulation shell for `wgpu` applications.

## Architecture

`libmorph` gives an app one API (`MorphApp`) across three hosts:

| Host | Event loop owner | Surface source |
|---|---|---|
| Desktop (emulation) | `winit` 0.30 (`ApplicationHandler`) | `winit::window::Window` |
| Web (`wasm32`) | `winit` 0.30 (`ApplicationHandler`) | full-screen `<canvas>` injected into the DOM |
| Android | `libmobile::android::run` (`MobileApp`) | `libmobile::android::SurfaceTarget` |

iOS is **not supported**. Neither dependency targets it yet, so don't write iOS or Metal-lifecycle code paths.

### Dependencies (actual crates)

Neither crate is on crates.io. Use git dependencies:

```toml
libdevice = { git = "https://github.com/tmzt/libdevice" }
libmobile = { git = "https://github.com/tmzt/libmobile", optional = true }
```

**`libdevice`** (wgpu `30.0.1`): owns GPU bring-up.
- `Device` holds the `wgpu::Instance`, `Adapter`, `Device` and `Queue` together. Keep it alive for the whole app; only surfaces are recreated.
- `Device::create_instance()` → create the surface from that instance → `Device::new_with_surface(instance, config, Some(&surface)).await`.
- `DeviceConfig { geometry: PortraitGeometry, force_fallback_adapter }`. Only one preset exists: `GALAXY_S24_PORTRAIT` (1080×2340). `set_geometry` updates it after a resize.
- Limits: the instance uses `Backends::PRIMARY` (no GL), and devices require `max_storage_buffers_per_shader_stage: 8`. Consequences: **WebGPU only on web (no WebGL2)** and **no GLES-only Android devices**, until libdevice makes backends and limits configurable.
- `new_blocking*` spin-poll with a no-op waker. **Never call them on wasm**, where they hang. On web, use the async constructor through `wasm_bindgen_futures::spawn_local`.
- libmorph must use wgpu **30** so its `wgpu` types match libdevice's.

**`libmobile`** (Android-only runtime; its platform-free types build on any host):
- Entry point: `libmobile::android::run(app: AndroidApp, handler: impl MobileApp)`. It owns the `android-activity` poll loop and is the only thing that drains the Kotlin bridges (insets, intents, IME text, auth, camera). Those bridge modules are private, so **winit must not run the loop on Android**.
- `MobileApp` hooks: `event(&AndroidApp, MobileEvent)`, `input(&AndroidApp, &InputEvent) -> InputStatus`, `tick(&AndroidApp)`, `poll_timeout()`.
- `MobileEvent::WindowDestroyed` arrives **synchronously inside the poll callback**. Drop the `wgpu::Surface` before returning from the handler.
- `MobileEvent::Insets(Insets { safe_area: SafeArea, ime_bottom })` uses physical pixels. Only the newest report is delivered per tick.
- `SurfaceTarget::current(&app)` implements raw-window-handle 0.6, so pass it to wgpu's safe `create_surface`.
- Cargo features: `google-auth`, `camera`. No others exist.
- The app's activity must be `dev.highbay.libmobile.MobileNativeActivity` (Gradle wiring comes from `libmobile::gradle`; see Known Limitations).

### Cargo Features & Passthrough

```toml
[features]
default = ["audio"]
audio = ["dep:cpal"]
mobile = ["dep:libmobile"]
# Passthrough: each one also enables `mobile`, so `libmobile/<x>` is never
# requested without the dependency itself.
mobile-google-auth = ["mobile", "libmobile/google-auth"]
mobile-camera = ["mobile", "libmobile/camera"]
web = ["dep:wasm-bindgen", "dep:wasm-bindgen-futures", "dep:web-sys"]
```

Don't invent passthroughs for features libmobile doesn't have (e.g. `sensors`). Add one only once the upstream feature exists.

Make `winit` a target-specific dependency for `cfg(not(target_os = "android"))`. On Android, `android-activity`/`ndk` come in through libmobile's re-exports (`libmobile::android::{android_activity, ndk}`). Don't declare them separately, or the JavaVM singleton gets duplicated.

### Target Guards

Put these at the top of the runner module:

```rust
#[cfg(all(target_arch = "wasm32", not(feature = "web")))]
compile_error!("The 'web' feature must be enabled when compiling for the wasm32 target.");

#[cfg(all(target_os = "android", not(feature = "mobile")))]
compile_error!("The 'mobile' feature must be enabled when compiling for Android.");
```

### Core API

- **`MorphApp`**: a state-driven trait with `init`, `update`, `render`, `on_touch`, `on_suspend`, `on_resume`.
- **`MorphContext`**: passed to every hook. It provides:
  - `wgpu::Device` / `wgpu::Queue` (borrowed from `libdevice::Device`);
  - the current `PortraitGeometry`;
  - `safe_area() -> SafeArea`: libmorph's own `SafeArea` type, so the signature is identical with or without `mobile`. It is converted from `libmobile::SafeArea` on Android, emulated on desktop, and `SafeArea::NONE` on web;
  - `audio()`: a real handle under `audio`, a zero-sized no-op stub otherwise.

### Lifecycle

- **Android:** an internal `MobileApp` impl adapts libmobile's events to `MorphApp`:
  - `WindowCreated` → create the surface from `SurfaceTarget` and configure it. On the first window, also build the `libdevice::Device` (blocking is fine on Android: use `pollster` or libdevice's `new_blocking_with_surface`), then call `init`.
  - `WindowDestroyed` → drop the surface synchronously, then call `on_suspend`.
  - `Resume` with a window → call `on_resume`.
  - `Insets` → update `MorphContext`'s safe area.
  - `tick` → `update` + `render`.
  - `poll_timeout` → `Some(Duration::ZERO)` while rendering continuously.
- **Desktop:** winit `ApplicationHandler`. `resumed` creates the window and surface. `suspended` drops the surface. The `libdevice::Device` persists across both.
- **Web:** `ApplicationHandler`. The async device bring-up runs in `spawn_local` and its result comes back through an `EventLoopProxy` user event. Don't render until it arrives.

### Emulation & Input (desktop)

- Size the window from `DeviceConfig::geometry` (physical pixels). libdevice has **no** DPI, cutout, or safe-area data today. Keep emulated safe-area presets in libmorph, or add them upstream to libdevice. Don't pretend libdevice provides them.
- Translate winit mouse events into the same touch events that Android `InputEvent::MotionEvent` produces, so `on_touch` behaves the same on both.

### CLI Overrides

Strip `--morph-*` arguments from `std::env::args()` before the app sees them (e.g. `--morph-device`, `--morph-fallback-adapter` → `DeviceConfig::force_fallback_adapter`), and pass the cleaned list to the app. This applies to desktop only.

## Known Limitations

- **Gradle handoff through a facade:** libmobile publishes its Gradle script through `links` metadata (`DEP_LIBMOBILE_GRADLE_SCRIPT`), and Cargo passes that only to *direct* dependents. Until libmorph re-exports it (its own `links = "libmorph"` plus libmobile's `gradle` learning to read `DEP_LIBMORPH_*`), Android apps must still list `libmobile` as a **build-dependency** to call `libmobile::gradle::Project`.
- No iOS. No WebGL2. No GLES-only Android (see libdevice limits).

## Behavioral Guidelines

- **Direct & concise:** no preamble. Output code, module definitions, or architectural reasoning directly.
- **Ground truth over this file:** before using any libdevice or libmobile API, check it against the source in `~/src/libdevice` and `~/src/libmobile`. If they disagree with this document, the source wins. Note the discrepancy.
- **Feature-gate rigor:** gate subsystem code with `#[cfg(feature = "...")]`, and provide zero-sized stubs so `MorphContext` signatures don't change.
- **Idiomatic Rust:** safe, concise, zero-cost abstractions.
- **Lifecycle awareness:** never hold a surface past `WindowDestroyed` / `suspended`. Never block the wasm main thread.
- **Complete solutions:** provide compilable code. If something is ambiguous, state the assumption in one line and pick the best option.
