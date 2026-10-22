# libmorph — Agent Instructions

You are an expert Systems Architect and Senior Rust Developer working on **libmorph**, a unified windowing, graphics, and device-emulation shell for `wgpu` applications.

## CRITICAL RULE 0: THE CHANGES.MD PROTOCOL

Before writing, modifying or deleting code for a new planned work item:

1. Only planned new work, feature plans and architectural changes are
   appended to the bottom of `CHANGES.md`. Routine implementation work that
   follows an existing plan does not need its own addendum.
2. Each addendum is timestamped (`## YYYY-MM-DD HH:MM TZ — <subject>`, from the
   system clock) and names the plan it belongs to. Entries are appended, never
   edited.
3. Departures from a frozen plan (Rule 1.7) are recorded as addenda here.
4. Updates are small, incremental steps. No large, unannounced redesigns of
   the codebase.

## CRITICAL RULE 1: SPECS AND PLANS

1. Specs and plans live in `docs/design/`, named `YYYYMMDD_NN_NAME.md`. `NN` is
   the sequence within the day, and plans end in `_PLAN`. Each starts with YAML
   front matter: `title`, `date`, `status`, what it covers, its sources, and
   related or dependent documents.
2. Plans are written in plain engineering language that a person could build
   from alone: concrete interfaces, data layouts, numbers and paths, and
   numbered steps, each ending with **Done when:** stating what works at that
   point.
3. `status` is one of:
   - `Draft`: being worked out, changed only by amendments;
   - `Hold`: no work may start or continue until an amendment returns it to
     `Draft`;
   - `In implementation`: frozen.
   - `Implemented`: every step is done and its **Done when:** holds. It stays
     frozen, and living sections stay open. It is set by an amendment that lists
     the commits implementing each step, in the commit that completes the last
     step or in a later one.
   - `Superseded (by <plan>)`: replaced before implementation by the named
     plan, which carries its content forward. Nothing in it is implemented.
     It is set with an amendment naming the replacement and what moved where.

   The front-matter `status` is the only part of a document edited in place,
   and every status change gets its own amendment.
4. An amendment is appended to the end of the document as
   `## Amendment — YYYY-MM-DD HH:MM TZ: <subject>`. The body is never rewritten.
   An amendment states which earlier text it replaces; where they disagree, the
   later amendment wins. Timestamps come from the system clock, never typed by
   hand, and a wrong one is corrected by a later note, not by editing.
5. The first commit that implements a plan sets `status: In implementation`
   and appends an amendment saying implementation has started and the document
   is now immutable. After that it changes only on the owner's explicit
   instruction.
6. A plan may name **living sections** in its front matter. These stay open
   after implementation starts: new entries are appended as amendments, and an
   entry is fixed once its item is implemented. Implementation records, such as
   which commits implemented an entry, are appended as amendments, not filled in
   place.
7. When implementation departs from a frozen plan:
   - small departures needed to make the plan work are recorded in an addendum
     to this file, naming the plan, the step and the reason;
   - a departure that changes behaviour or an interface others depend on is
     discussed with the owner first (Rule 2), then recorded;
   - a frozen plan is never edited to match the code. A larger change of
     direction becomes a new dated plan that names the one it replaces.

## CRITICAL RULE 2: DISCUSS BEFORE CHANGING A PLAN

1. A change to a spec or plan is proposed to the owner first: what changes,
   where and why. Nothing is written to the document until the owner agrees.
2. Refinements made during the discussion go into the proposal, not the file.
   Once agreed, the whole change is written as **one** amendment, not a series
   of amendments that replace one another.

## CRITICAL RULE 3: COMMITS AND EVIDENCE

1. At most one crate or subproject per commit. Repository-level files go in
   their own commits: this file, the workspace manifest, evidence logs, tooling
   config and scripts. The one exception is adding a crate to the workspace in
   that crate's first commit.
2. A plan's status change and its immutable marker go in the commit that starts
   implementing it.
3. Commits name their paths explicitly (`git add <paths>`,
   `git commit -- <paths>`), so unrelated work in progress, including another
   agent's, is never swept in. Commits already on the shared branch are never
   amended or rewritten.
4. Before each code commit, the crate passes `cargo fmt -p <crate> -- --check`,
   `cargo clippy -p <crate> --all-targets` with no new warnings, and
   `cargo test -p <crate>`.
5. Renders and other generated evidence go under the gitignored `out/`. Each
   area keeps its own log file. Entries are appended under dated headings, with
   `./`-relative links and descriptions that stand on their own without the
   images. Earlier entries are never edited.

## CRITICAL RULE 4: DECISION STATUS IN THE README

1. `README.md` ends with an `## Architectural decisions` section: one GitHub
   Markdown table listing every spec and plan in `docs/design/`, oldest first.
   Nothing follows the table.
2. Columns: **Document** (the title, as a relative link such as
   `[Camera view and capture](docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md)`;
   no absolute or GitHub URLs), **Date** (from its front matter), **Status**
   (exactly its front-matter `status`, including `Superseded (by <link>)`),
   and **Decision** (one line: what it decides, and why it is at that status).
3. A row is added in the commit that adds the document, and updated in the
   same commit as every status-change amendment. This is an exception to
   Rule 3.1. The table is a summary: the documents and their amendments stay
   the record.

## CRITICAL RULE 5: VERSIONS AND TAGS

Adopted from libcontrols (its Rule 5) on the owner's instruction,
2026-10-16; see CHANGES.md. The workspace has two crates with public APIs,
each versioned on its own: `libmorph` (tags `v<version>`) and
`crates/libmorph-build` (tags `libmorph-build-v<version>`).

1. The commit that changes a crate's `version` in its `Cargo.toml` is tagged
   (for example `git tag v0.2.0 <commit>`) as soon as it is committed. A tag
   that has been pushed is never moved or deleted.
2. `VERSIONS.md` records each version change, and only version changes:
   one entry per version, appended oldest first, headed
   `## <crate> <version> — YYYY-MM-DD (<short commit>, tag <tag>)`. The
   entry lists the public API changes since the previous version: what was
   added, changed or removed, with breaking changes marked **Breaking**.
   Internal changes are not listed, and commits that do not change a version
   get no entry. Earlier entries are never edited. An entry written in the
   version commit itself names only the tag, since a commit cannot contain
   its own hash.
3. The `VERSIONS.md` entry is in the same commit as the version change. This
   is an exception to Rule 3.1.
4. While the version is `0.x`, the bump follows the public API change. A
   release whose public API changes are not strictly additive bumps the minor
   version (`0.x.y` to `0.(x+1).0`): anything removed, renamed or changed in
   signature or type, behaviour a caller can observe changing, or a variant or
   field added to an exhaustive public enum or struct. A release whose changes
   are strictly additive, or change no public API, bumps the patch version
   (`0.x.y` to `0.x.(y+1)`). This applies to releases after 0.1.0.
5. The public API rule is recursive through dependencies. When a crate with
   a public API moves a dependency to a version that is not semver-compatible
   with the one it used (for a `0.x` dependency, a new minor version; from
   1.0, a new major version), its own release is not strictly additive and
   bumps its minor version under Rule 5.4 (its major version from 1.0), even
   when its own code does not change. Every crate with a public API that
   depends on it does the same when it takes that release, and so on up the
   chain. libmorph re-exports libdevice, libmobile's types, wgpu and cpal,
   so their incompatible bumps are libmorph's too. The `VERSIONS.md` entry
   names the dependency with its old and new versions and marks the change
   **Breaking**.

## Architecture

`libmorph` gives an app one API (`MorphApp`) across three hosts:

| Host | Event loop owner | Surface source |
|---|---|---|
| Desktop (emulation) | `winit` 0.30 (`ApplicationHandler`) | `winit::window::Window` |
| Web (`wasm32`) | `winit` 0.30 (`ApplicationHandler`) | full-screen `<canvas>` injected into the DOM |
| Android | `libmobile::android::run` (`MobileApp`) | `libmobile::android::SurfaceTarget` |

Native iOS is **not supported**. libmobile is Android-only. libdevice does have iOS *device specs*, so desktop can emulate an iPhone's geometry and safe area, but don't write native iOS or Metal-lifecycle code paths.

### Platform Layering (libmobile and libdevice first)

**libmorph is a wrapper; it does not reach past the platform crates.**
Owner's rule (2026-10-22): "libmorph is supposed to be a wrapper for
libmobile so the features we added should have been in libmobile first",
extended the same day to libdevice: "that should be extended to libdevice
with the same rule, libmorph is a wrapper only (with a few exceptions)".

- Any Android capability (NDK or AAudio calls, Kotlin plugins, permissions,
  intents, Keystore) is planned and built in libmobile first, with a
  libmobile example that genuinely uses it, and released. Then libmorph is
  amended to wrap it.
- Any desktop platform capability (the macOS Keychain, other OS services)
  is planned and built in libdevice first, behind a feature, and released.
  Then libmorph wraps it. Where libmobile and libdevice both provide a
  capability, they expose the same API, and libmorph wraps both.
- libmorph does not call `libmobile::android::ndk` (or `android_activity`)
  or OS frameworks to implement such a feature. It keeps only what a
  wrapper adds: one API on every host, lifecycle handling (pausing with the
  app, asking for permissions it needs), fan-out to several consumers, and
  its own types so signatures do not change with or without `mobile`.
- The exceptions are what libmorph exists to own: wgpu and GPU bring-up,
  the winit event loop and window on desktop and the web (with device
  emulation, input mapping and the macOS trackpad), cpal on desktop and
  the web, and scripted runs. Any new exception is agreed with the owner
  first.
- Examples ask only for permissions they use.

### wgpu Ownership

**libmorph is the only crate in the stack that depends on wgpu.** libdevice is data only and has no wgpu dependency or feature. libmobile is also wgpu-free and hands over its window through raw-window-handle 0.6. libmorph:

- performs adapter/device bring-up itself, choosing backends, limits and async strategy per host;
- re-exports it as `pub use wgpu;` so apps write `libmorph::wgpu` and never declare their own copy;
- can therefore bump wgpu majors without coordinating with libdevice or libmobile.

libdevice's old offscreen `Device` (instance, adapter, device, queue and color target, used by standalone tools such as librendertest) was removed upstream. It will probably come back in libmorph later as an optional feature. **Don't add it yet.**

### Dependencies (actual crates)

Neither crate is on crates.io. `Cargo.toml` uses git dependencies only:

```toml
libdevice = { git = "https://github.com/tmzt/libdevice" }
libmobile = { git = "https://github.com/tmzt/libmobile", optional = true }
```

To build against sibling checkouts, use a local, **uncommitted** `.cargo/config.toml` (it is gitignored):

```toml
[patch."https://github.com/tmzt/libdevice"]
libdevice = { path = "../libdevice" }
```

Add the same kind of entry for `https://github.com/tmzt/libmobile` to work against a local libmobile.

Never commit a `path` dependency or a `Cargo.lock` that was resolved with a patch active. A patch strips the `source = "git+…"` line from the patched entry.

**`libdevice`**: simulated-device data, built into the binary as constants, and the desktop's platform services behind features (see Platform Layering). No GPU code. libmorph re-exports it as `libmorph::libdevice`.
- `DeviceSpec { id, name, platform, geometry: PortraitGeometry, pixel_ratio, ppi, insets: SystemInsets, cutouts, corners }`. All lengths are `u32` physical pixels of the portrait display. `ppi: f32` is the display's density; `px_per_mm()`, `mm_to_px`, `px_to_mm` and `physical_size_mm()` convert in device pixels.
- Presets are generated by its `build.rs` from `devices.toml`. `GALAXY_S24` is first and is `DEFAULT_DEVICE`; there is also `IPHONE_15`. `DEVICES` lists them all, and `DeviceSpec::by_id` finds one. Presets are compile-time only, so a downstream crate can't add devices (apart from CLI overrides; see CLI Overrides).
- `spec.insets(InsetType::…)` returns `Insets { top, right, bottom, left }` (`u32`, physical pixels). The combined types are derived: `SafeDrawing` (system bars ∪ cutout), `SafeGestures`, `SafeContent`, `IosSafeArea`, `CssSafeArea`. `Insets::to_logical(pixel_ratio)` converts to dp/pt/CSS px.
- `Cutout { edge, shape, bounds }` describes cutouts (circle, capsule, notch, rect, SVG path). `Corners` gives the corner radii, with a circular or continuous shape.
- CLI: `DeviceSpec::extract_args(args) -> (DeviceSpec, Vec<String>)` takes out `--device*` options (see CLI Overrides).

**`libmobile`** (Android-only runtime; its platform-free types build on any host):
- Entry point: `libmobile::android::run(app: AndroidApp, handler: impl MobileApp)`. It owns the `android-activity` poll loop and is the only thing that drains the Kotlin bridges (insets, intents, IME text, auth, camera, permissions). Those bridge modules are private, so **winit must not run the loop on Android**.
- `MobileApp` hooks: `event(&AndroidApp, MobileEvent)`, `input(&AndroidApp, &InputEvent) -> InputStatus`, `tick(&AndroidApp)`, `poll_timeout()`.
- `MobileEvent::WindowDestroyed` arrives **synchronously inside the poll callback**. Drop the `wgpu::Surface` before returning from the handler.
- `MobileEvent::Insets(Insets { safe_area: SafeArea, ime_bottom })` uses `f32` physical pixels, fields `left, top, right, bottom`. Only the newest report is delivered per tick. `safe_area` corresponds to libdevice's `InsetType::SafeDrawing`.
- `SurfaceTarget::current(&app)` implements raw-window-handle 0.6, so pass it to wgpu's safe `create_surface`.
- Runtime permissions: `request_permissions(&app, &[&str]) -> PermissionRequest` (one request at a time, in order), answered by `MobileEvent::Permissions(PermissionsOutcome { request, permissions })`; `permission_granted`, `should_show_rationale`. Each permission must be declared in the manifest.
- `CaptureStream` (under `cpal`): AAudio capture with an `InputPreset`, which cpal cannot set: `open(preset, callback)`, `format()`, `preset()`, `set_preset` (reopens with the same callback and format), `start`/`stop`. Needs `RECORD_AUDIO`.
- Cargo features: `google-auth`, `camera`, `cpal` (cpal re-exported as `libmobile::android::cpal`, plus `CaptureStream`; needs API 28). No others exist.
- The app's activity must be `dev.highbay.libmobile.MobileNativeActivity` (Gradle wiring comes from `libmobile::gradle`; see Gradle Handoff).

Two crates use the name `Insets`: `libdevice::Insets` (`u32`, top/right/bottom/left) and `libmobile::Insets` (`f32` safe area plus `ime_bottom`). Both convert into libmorph's fractional `SafeArea` (`src/safe_area.rs`) through `From`, field by field, never by position:
- `impl From<libdevice::Insets> for SafeArea` exists.
- `impl From<libmobile::SafeArea> for SafeArea` exists under `#[cfg(feature = "mobile")]`.

### Cargo Features & Passthrough

```toml
[features]
default = ["audio"]
# cpal on desktop and the web; on Android, libmobile's copy.
audio = ["dep:cpal", "libmobile?/cpal"]
# The cpal re-export (`libmorph::cpal`) and the output device it opened.
audio-cpal = ["audio"]
mobile = ["dep:libmobile"]
# Passthrough: each one also enables `mobile`, so `libmobile/<x>` is never
# requested without the dependency itself.
mobile-google-auth = ["mobile", "libmobile/google-auth"]
mobile-camera = ["mobile", "libmobile/camera"]
web = ["dep:wasm-bindgen", "dep:wasm-bindgen-futures", "dep:web-sys", "wgpu/webgl"]
# Scripted headless runs with PNG screenshots; png and crc32fast are
# desktop-only dependencies, so this enables nothing on Android and the web.
script = ["dep:png", "dep:crc32fast"]
```

`build.rs` sets `cfg(script_runner)` when `script` is on and the target is neither Android nor wasm32. Gate runner code on `#[cfg(script_runner)]` rather than repeating the feature-and-target condition.

Don't invent passthroughs for features libmobile doesn't have (e.g. `sensors`). Add one only once the upstream feature exists.

wgpu's default features already include `webgpu`. `web` adds `webgl` so WebGL2 works as a fallback.

cpal is owned like the NDK: libmobile owns it on Android (its `cpal` feature, re-exported as `libmobile::android::cpal`), and libmorph owns it on desktop and the web (a `cfg(not(target_os = "android"))` dependency). `src/audio.rs` aliases whichever copy applies as `audio::cpal`, and `lib.rs` re-exports that alias under `audio-cpal`. Without `audio`, neither crate depends on cpal. cpal's host backends (JACK, PulseAudio, PipeWire, ASIO) get passthroughs only when Linux or Windows is supported.

Make `winit` a target-specific dependency for `cfg(not(target_os = "android"))`. On Android, `android-activity`/`ndk` come in through libmobile's re-exports (`libmobile::android::{android_activity, ndk}`). Don't declare them separately, or the JavaVM singleton gets duplicated.

### Gradle Handoff (Android)

libmobile publishes its generated Gradle script through `links` metadata, and Cargo passes that only to *direct* dependents. libmorph declares `links = "libmorph"`, and its `build.rs` calls `libmobile::gradle::forward_metadata()` (only under `mobile`) to republish the metadata as `DEP_LIBMORPH_GRADLE_SCRIPT` / `DEP_LIBMORPH_FEATURES`. That is why libmobile is also an optional build-dependency of libmorph, enabled by the same `dep:libmobile`.

Android apps name libmorph crates only: libmorph as a dependency, and `crates/libmorph-build` as the build-dependency. libmorph-build depends only on libmobile, so it is light on the host. It re-exports `libmobile::gradle`, and `gradle_project()` presets libmorph's `links` key (`LINKS`, checked against the root manifest by a test):

```toml
[dependencies]
libmorph = { git = "https://github.com/tmzt/libmorph", features = ["mobile"] }
[build-dependencies]
libmorph-build = { git = "https://github.com/tmzt/libmorph" }
```

```rust
// build.rs
libmorph_build::gradle_project("com.example.app")
    .generate()
    .expect("gradle project");
```

Both come from the same git URL, so one commit pins them together. Don't make libmorph itself the build-dependency: that would build wgpu and winit for the host. Examples inside this repository use `libmorph-build` by path.

### Target Guards

Put these at the top of the runner module:

```rust
#[cfg(all(target_arch = "wasm32", not(feature = "web")))]
compile_error!("The 'web' feature must be enabled when compiling for the wasm32 target.");

#[cfg(all(target_os = "android", not(feature = "mobile")))]
compile_error!("The 'mobile' feature must be enabled when compiling for Android.");
```

### GPU Bring-up (libmorph-owned)

| Host | Backends | Limits | Await strategy |
|---|---|---|---|
| Desktop | `PRIMARY` | `A::limits(base)`, resolved | blocking (`pollster`) is fine |
| Android | `VULKAN \| GL` | `A::limits(base)`, resolved | blocking on the `android_main` thread is fine |
| Web | `BROWSER_WEBGPU \| GL` | `A::limits(base)`, resolved | `wasm_bindgen_futures::spawn_local`; **never block** |

`base` is `downlevel_defaults`, or `downlevel_webgl2_defaults` on any GL adapter (WebGL2, older Android), with texture sizes raised by `.using_resolution(adapter.limits())`. The app's `MorphApp::limits(base) -> LimitsRequest { required, preferred }` (default: both `base`) is resolved against the adapter by `resolve_limits` in `src/gpu.rs`:

- every field of `required` the adapter can't meet is reported as `GpuError::Limits(Vec<LimitFailure { name, required, available }>)`, and bring-up fails. `required` is never clamped;
- otherwise the device gets `preferred.or_worse_values_from(&adapter).or_better_values_from(&required)`.

wgpu-types' helpers know which fields are minima (the alignments), so libmorph has no per-field code. `Gpu::new` takes `A::limits` as a `fn` pointer, so it moves into the web's `spawn_local` future. `GpuError` is `#[non_exhaustive]`.

Expose whatever was granted on `MorphContext` (`ctx.limits()`) so apps can feature-detect.

### Core API

- **`MorphApp`**: a state-driven trait with `init`, `update` and `render`, plus hooks with default bodies:
  - `on_touch(Touch)`. `Touch { id, phase, x, y, pressure: Option<f32> }`; pressure comes from Android, and from macOS Force Touch while the mouse is held. `TouchPhase::Cancelled` only means the contact ended without a release; libmorph gives it no further meaning.
  - `on_hover(x, y)`: the cursor moved with no button held (desktop and web).
  - `on_key(Key) -> KeyStatus`, on press only. `Key` is `#[non_exhaustive]`: `Character(char)`, `Escape`, `Back`. On Android, the status decides whether the system acts on Back; desktop ignores it.
  - `on_suspend` and `on_resume`: exactly once per transition. Every exit calls `on_suspend` once, unless the app is already suspended, before the surface and the app are dropped.
  - `const SRGB_SURFACE: bool = true`: `false` prefers a non-sRGB surface, for apps that gamma-encode in their own shaders.
  - `fn limits(base: &wgpu::Limits) -> LimitsRequest` (no `self`, called before `init`): the device limits to ask for; see GPU Bring-up.
  - `fn audio_setup(&mut AudioSetup)` (no `self`, called before audio opens; plan `20261022_01_AUDIO_CAPTURE_PLAN.md`): `output` (default `true`), `capture` (default `false`) and `capture_mode: CaptureMode` (Android input preset). `AudioSetup` is `#[non_exhaustive]`.
  - `on_capture_ready`: once, after `init`, when capture has opened: right after `init`, or on Android once the user allows `RECORD_AUDIO`, which libmorph asks for itself when `capture` is set.
  - `on_permissions(&Permissions)`: the answer to each `ctx.request_permissions` call (plan `20261022_02_APP_PERMISSIONS_PLAN.md`); libmorph's own requests are not passed on.
- **`MorphContext`**: passed to every hook. It provides:
  - `wgpu::Device` / `wgpu::Queue`, plus the granted `wgpu::Limits`;
  - `device() -> &libdevice::DeviceSpec`: the emulated spec on desktop. On Android and web, it is the closest data available: the CLI or default spec, with geometry updated from the real surface;
  - `safe_area() -> SafeArea`: libmorph's own type (`f32` physical px, top/right/bottom/left), so the signature is the same with or without `mobile`. Its source depends on the host:
    - desktop: `SafeArea::from(spec.insets(InsetType::SafeDrawing))`;
    - Android: `libmobile::SafeArea`, converted;
    - web: zero for now (the browser's `env(safe-area-inset-*)` isn't reachable from wasm without CSS plumbing);
  - `scale_factor()`: surface pixels per dp/point/CSS pixel. On desktop, `spec.pixel_ratio` scaled by surface width / device width;
  - `px_per_mm()`: surface pixels per millimetre of the device's display, `spec.px_per_mm()` scaled the same way;
  - `audio()`: a real handle under `audio`, a zero-sized no-op stub otherwise (plan `20261016_01_AUDIO_DEVICE_AND_SOURCES_PLAN.md`):
    - `play`/`play_with` a `Sound` (in-memory `f32` samples, resampled), giving a `Voice` for `set_volume` and `stop`; `stop_all` stops voices only;
    - `add_source(Source) -> SourceId` and `remove_source`: a `Box<dyn FnMut(&mut [f32]) + Send>` called on the audio thread with a zeroed, interleaved buffer at `format()`'s rate and channel count, mixed with the voices. Removed sources are sent back and dropped on the main thread;
    - `set_master_volume(f32)` scales the whole mix before clipping to -1..1; `format() -> Option<OutputFormat { sample_rate, channels }>`;
    - under `audio-cpal`: `device() -> Option<&impl DeviceTrait>` (the concrete `cpal::Device` is not exposed), `stream_config()`, `sample_format()`. The stream itself is never exposed;
    - capture (`AudioSetup::capture`): `add_sink(Sink) -> SinkId` and `remove_sink`; a `Box<dyn FnMut(&Captured) + Send>` called on the audio input thread with `Captured { samples, frame, time }` (interleaved `f32` at `capture_format()`). Sinks added before capture opens are kept for it; removed ones are dropped on the main thread. `capture_mode()` and `set_capture_mode(CaptureMode)` (Android: reopens with the preset, keeping the sinks; elsewhere `Default`). On Android capture goes through libmobile's `CaptureStream`, so the `audio-cpal` capture accessors (`capture_device()`, `capture_stream_config()`, `capture_sample_format()`) are `None` there;
    - the streams pause with the app: on Android at `Pause`/`Resume`, on desktop and web in winit's `suspended`/`resumed`. Scripted runs have no streams, so sources and sinks are never called;
  - `exit()`: ask the runner to stop. On desktop and web the window closes; on Android libmorph calls `libmobile::android::finish` and the activity tears down normally;
  - `data_dir() -> Option<&Path>`: Android's internal data path; `None` on desktop and web.
  - `request_permissions(&[&str]) -> Option<PermissionRequest>`, `permission_granted`, `should_show_rationale`: libmorph's own types over libmobile's, the same on every host. Off Android there are no runtime permissions: `None`, `true` and `false`.
  - `pipeline_cache() -> Option<&wgpu::PipelineCache>` (plan `20261016_02_PIPELINE_CACHE_PLAN.md`): `Some` only on Android with Vulkan when the adapter offers `PIPELINE_CACHE`, which `Gpu::new` then requests (its `want_pipeline_cache`, `true` from the Android runner only). Apps pass it as `cache` in their pipeline descriptors, cloning it for pipelines built on another thread; a pipeline built with `cache: None` is not cached. The data is kept at `<data_dir>/libmorph/<wgpu::util::pipeline_cache_key>.bin`, loaded before `init` (`fallback: true`, so stale data gives an empty cache), and saved through a `.tmp` rename after `init`, after `on_suspend` and on exit, skipped when unchanged (`src/pipeline_cache.rs`).
- **`libmorph::script`** (plan `20261014_04_SCRIPTED_RUNS_AND_PNG_CAPTURE_PLAN.md`): scripted, headless runs.
  - `Script::parse` / `load` / `from_commands` build a checked script from plain text: one English command per line, space-separated operands, `#` comments. Commands: `fps`, `frames`, `touch <id> <start|move|end|cancel> <x> <y> [<pressure>]`, `tap`, `drag`, `hover`, `key <escape|back|space|char|U+XXXX>`, `screenshot [<name>]`, `suspend`, `resume`, `exit`. Ordering mistakes (touch not down, screenshot before a frame or while suspended, duplicate names) are rejected with the line number before any GPU work. The parser builds without the feature.
  - `script::run::<A>(&script, ScriptRun)` (`script` feature, desktop): no window or event loop, so it runs from `cargo test` and agent sandboxes. It renders into an offscreen `Rgba8UnormSrgb`/`Rgba8Unorm` texture at `spec.geometry`, with a fixed `dt` of `1 s / fps`, silent audio, the emulated safe area and, unless disabled, the overlay. It returns a `Report`.
  - Screenshots are RGB PNGs from the `png` crate with fixed settings (level 6, adaptive filter, `sRGB` chunk, no time or text chunks). `run.json` records the device, size, format, fps, adapter, how the run ended, and each screenshot's frame, line and pixel CRC-32. Nothing in the output depends on when the run happened. The same script, app, adapter and `Cargo.lock` give byte-identical files.
  - `OutDir::Exact(dir)`, or `OutDir::Sequenced` (default `out/morph/YYYYMMDD_NN_<script stem>/`, dated in UTC, named like the plans).

### Lifecycle

- **Android:** an internal `MobileApp` impl adapts libmobile's events to `MorphApp`:
  - `WindowCreated` → create the surface from `SurfaceTarget` and configure it. On the first window, also bring up the adapter, device and queue, then call `init`.
  - `WindowDestroyed` → drop the surface synchronously, then call `on_suspend`.
  - `Resume` with a window → call `on_resume`.
  - `Insets` → update the safe area.
  - `tick` → `update` + `render`.
  - `poll_timeout` → `Some(Duration::ZERO)` while rendering continuously.
  - `Destroy` → shut down (no second `on_suspend` after `WindowDestroyed`).
  - Back and Escape go to `on_key` on their first press. The status the app returns also answers that key's repeats and release (`KeyRouting`), because Android acts on Back when it comes up.
- **Desktop:** winit `ApplicationHandler`. `resumed` creates the window and surface. `suspended` drops the surface. The device and queue persist across both. Close, `exit()`, a failure and winit's `exiting` all end in `Shell::shutdown`.
- **Web:** `ApplicationHandler`. The async bring-up runs in `spawn_local` and its result comes back through an `EventLoopProxy` user event. Don't render until it arrives.

### Emulation & Input (desktop)

- Size the window from `spec.geometry` (physical pixels), shrunk to fit the primary display (`min(1, 0.9 × display width / w, 0.85 × display height / h)`) unless `--morph-window-scale <n>` gives an exact multiple. The emulated scale factor is `spec.pixel_ratio` times surface width / device width, independent of the host monitor's DPI; the safe area, overlay and `px_per_mm()` scale the same way.
- Draw an optional overlay from `spec.cutouts` and `spec.corners` (masked corners, cutout shapes) so layouts can be checked against the real obstructions.
- Translate winit mouse events into the same touch events that Android `InputEvent::MotionEvent` produces, so `on_touch` behaves the same on both. The left button is a single finger (id `u64::MAX`). Moving with no button held is a hover. A held touch is cancelled once when it leaves the window (by position or `CursorLeft`), and nothing more is reported until the next press.
- Multi-finger input on desktop comes from real touch events (`WindowEvent::Touch`) and, on macOS, the trackpad, never from mouse buttons (plan `20261015_01_MAC_TRACKPAD_TOUCH_PLAN.md`):
  - `src/runner/trackpad.rs` (macOS only, all the `objc2` code) allows indirect touches on winit's view and reads `NSTouch` fingers from gesture events through a local event monitor; `PadTouch` in `src/input.rs` turns them into touches with ids from `1 << 32`.
  - A session starts when two or more fingers are on the pad with the cursor over the surface, reports every finger until the last one lifts, and holds the mouse off (no hover, left presses ignored, a held mouse touch cancelled once). One finger is the mouse. Focus loss and `suspended` cancel the fingers.
  - `--morph-trackpad physical` (default) places fingers so pad distances equal distances on the device (`NSTouch::deviceSize` in points × `spec.px_per_mm()`), centred on the cursor; `fit` maps the whole pad to the surface; `off` installs nothing.

### CLI Overrides

Desktop only. Two separate namespaces, so they never clash:

- **Device selection:** libdevice's own `--device` prefix, unchanged:

  ```rust
  let (spec, args) = libdevice::DeviceSpec::extract_args(std::env::args())?;
  // --device iphone_15 | --device-width 720 | --device-status-bars-top 80 ...
  ```

  Append `libdevice::DEVICE_ARGS_HELP` to libmorph's help text.
- **libmorph's own options:** `--morph-*` (e.g. `--morph-fallback-adapter`), stripped by libmorph.
  - `--morph-script <path>` runs the script headless instead of opening a window (`MorphError::Script(ScriptError::Disabled)` without the `script` feature). `--morph-out <dir>` gives its exact output directory and is an error without `--morph-script`. Both also take `--opt=value`.
  - `--morph-trackpad <physical|fit|off>` and `--morph-window-scale <fit|n>`; see Emulation & Input. Both are accepted on every desktop host.

The order of the two passes doesn't matter. Pass the remaining `args` to the app.

## Known Limitations

- No native iOS.
- Device presets are compile-time constants in libdevice's `devices.toml`, and the figures are marked ESTIMATED there. Treat emulated insets as approximate until they have been checked on hardware.

## Behavioral Guidelines

- **Direct & concise:** no preamble. Output code, module definitions, or architectural reasoning directly.
- **Ground truth over this file:** before using any libdevice or libmobile API, check it against the source in `~/src/libdevice` and `~/src/libmobile`. If they disagree with this document, the source wins. Note the discrepancy.
- **Feature-gate rigor:** gate subsystem code with `#[cfg(feature = "...")]`, and provide zero-sized stubs so `MorphContext` signatures don't change.
- **Idiomatic Rust:** safe, concise, zero-cost abstractions.
- **Lifecycle awareness:** never hold a surface past `WindowDestroyed` / `suspended`. Never block the wasm main thread.
- **Complete solutions:** provide compilable code. If something is ambiguous, state the assumption in one line and pick the best option.
