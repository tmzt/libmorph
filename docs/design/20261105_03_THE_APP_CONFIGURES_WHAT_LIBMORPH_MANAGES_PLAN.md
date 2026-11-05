---
title: The app configures what libmorph manages, and may construct its own frames
date: 2026-11-05
status: Superseded (by 20261105_04_OWNS_AND_THE_RENDER_STATUS_PLAN.md)
covers: >
  A first callback, `MorphApp::configure`, that runs before anything
  opens and returns a `MorphAppConfig`: per-subsystem preferences
  libmorph applies, each `None` by default. Its first subsystem is
  drawing: a `DrawingConfig` with the surface format, present mode and
  frame latency the app wants, and a `mode` that says whether libmorph
  constructs the frame (`Managed`, as today) or the app does
  (`ConstructFrame`, through `ctx.acquire` and `ctx.present`; `render` is
  then never called). The rule that makes it hold together: each
  subsystem is either managed, in which case libmorph opens it, pumps
  its events and calls its callbacks, or owned, in which case libmorph
  does none of those. The pumps leave `Shell::draw` for a per-iteration
  `Shell::iterate`. Audio is deliberately not configured here. The
  release is 0.7.0, and libdevice and libmobile are pinned by tag.
sources: >
  The owner, 2026-11-05: "I was wondering why libmorph had drawing code,
  my thinking was it handed back the wgpu device and left it at that. We
  probably built it for the simple demos like the plasma one, never
  intending it to be used for a game like this"; "we do need a simple
  plan to make libmorph drawing opt-out in the builder, without the full
  threading plan right now, it will still need event delivery and jni
  calls to work though"; "libmorph shouldn't change depending on
  device"; and that each optional call and each pump needs one clean
  place to check whether its subsystem is managed.
  `src/runner/shell.rs` at `e60bf90`: `Shell::frame` (acquire, `draw`,
  present) and `Shell::draw` (the audio, camera and media pumps, then
  `update`, then `render`); `reconfigure`, `resize`, `attach`.
  `src/runner/android.rs`: `Adapter::frame`, `tick`, `poll_timeout`, and
  the `WindowResized`, `ConfigChanged`, `WindowDestroyed` and
  `WindowCreated` arms. `src/runner/winit.rs`: `boot`, `ready`,
  `RedrawRequested`. `src/runner/script.rs`: `play`.
  `src/app.rs`: `SRGB_SURFACE`, `limits`, `audio_setup`, `init`,
  `update`, `render`, `on_suspend`, `on_resume`. `src/audio.rs`:
  `AudioSetup`. `src/context.rs`: `surface_size`, `surface_format`, and
  no accessor for the surface. `src/gpu.rs`: `Gpu::new`, `configure`,
  `choose_format`. `Cargo.toml`: libdevice and libmobile as git
  dependencies with no tag or rev, in three places.
  A game on libmorph, 2026-11-04/05: a 266 ms frame on an Adreno 740,
  steady, and a blocking readback of its own that was the actual
  freeze.
related: >
  `docs/design/20261105_02_AN_APP_MAY_PRESENT_ITS_OWN_FRAMES_PLAN.md`,
  which this supersedes: it kept the same opt-out as a bare
  `drawing() -> Drawing` and a `present` hook, and is carried forward
  here as the `mode` field of a config that also holds the surface
  preferences. `docs/design/20261105_01_THE_APP_RUNS_OFF_THE_LOOP_THREAD_PLAN.md`,
  which stays Draft and is not depended on: an app that constructs its
  own frames can thread and pace them without libmorph growing a
  routing layer, so most of that plan may prove unnecessary.
  `docs/design/20261014_05_APP_LIMITS_REQUEST_PLAN.md` (`limits`) and
  `docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md` (`audio_setup`), the
  two setup callbacks this one goes in front of.
---

# The app configures what libmorph manages

## 1. Why

`Shell::frame` (`src/runner/shell.rs`) acquires the surface texture,
calls `draw`, and presents. `draw` pumps the audio, camera and media
events, calls `update`, calls `render`, and draws the desktop overlay.
That is the whole of an app's frame, and libmorph owns every part of it.

For the demos libmorph was built around that is the feature: a plasma
shader has nothing to write but `render`. For a game it is a wall. The
app cannot choose when to submit, how deep to let the swapchain queue
get, whether to skip a frame, or what to do when one is slow. The same
boundary is why nothing in the live path at `e60bf90` ever called
`Device::poll` (only the scripted runner's readback did) and why an app
that wanted to could not have done it itself: it never held the surface
texture, so it never knew when a frame was over. A non-blocking poll
after present is being added to the managed path separately; under this
plan, an app that constructs its own frames polls when it decides to.

The owner's reading is the right one: libmorph should have handed back
the wgpu device and left the frame to the app, and the drawing code is
there because the demos needed it. This plan does not take the drawing
code away. It makes it one of two modes, chosen by the app, and puts the
choice beside the other things an app would want to say about its
surface before it exists.

## 2. The shape

### 2.1 `configure`, the first callback

```rust
/// What the app wants opened and driven, before anything is. `config`
/// arrives with every field at its default; return it with the app's
/// preferences set. Pure data: no GPU, window or surface exists yet,
/// because this decides what opens.
fn configure(config: MorphAppConfig) -> MorphAppConfig {
    config
}
```

An associated function with no `self`, like `limits` and `audio_setup`,
so it is a plain `fn` pointer and moves into the web's `spawn_local`
future the way `A::limits` already does. It is called exactly once per
run, first, in each runner's bring-up: `winit.rs` in `resumed` before
`boot`, `android.rs` at the top of `boot`, `script.rs` at the top of
`run`.

It is declarative, not imperative. The app does not open anything in
`configure`; it states preferences, and libmorph applies them - at
bring-up, and again every time it configures the surface. On Android
that is repeatedly: a rotation arrives as `WindowResized` and
`ConfigChanged` (libmobile's generated manifest declares
`configChanges` for orientation and size), and on some devices as a
`WindowDestroyed` and `WindowCreated` pair; each goes through
`Shell::resize` or `Shell::attach`, and each applies the same config.
The app says it once; libmorph keeps it true.

### 2.2 `MorphAppConfig` and `DrawingConfig`

```rust
/// Per-subsystem preferences. `None` is "no opinion": libmorph does
/// what it does today.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MorphAppConfig {
    pub drawing: Option<DrawingConfig>,
    // audio: Option<AudioConfig> is NOT in this plan; see section 6.
}

/// How the surface is configured, and who constructs the frame.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DrawingConfig {
    /// The surface format, used when the surface offers it; otherwise
    /// `SRGB_SURFACE`'s rule applies and a warning is logged. Read what
    /// was chosen from `ctx.surface_format()` in `init`.
    pub format: Option<wgpu::TextureFormat>,
    /// `wgpu::SurfaceConfiguration::present_mode`, when the surface
    /// offers it; otherwise the surface's default (`Fifo`), logged.
    pub present_mode: Option<wgpu::PresentMode>,
    /// `wgpu::SurfaceConfiguration::desired_maximum_frame_latency`:
    /// how many frames may be in flight on the swapchain. wgpu's
    /// default (2) when `None`; a game that wants the shallowest queue
    /// asks for 1.
    pub max_frame_latency: Option<u32>,
    pub mode: DrawingMode,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum DrawingMode {
    /// libmorph acquires the surface texture, calls `update` and
    /// `render`, draws the desktop overlay, and presents. Every app
    /// written so far.
    #[default]
    Managed,
    /// The app does, from `construct_frame`, through `ctx.acquire` and
    /// `ctx.present`. libmorph still opens the device, owns the window
    /// and the surface, configures it, and delivers every other
    /// callback. `render` is never called.
    ConstructFrame,
}
```

**Configuration and ownership are two axes, so ownership is a field of
the config, not the shape of it.** An app may want a `Bgra8Unorm`
surface while libmorph still drives the frame, and that is the common
case, not an edge: it is the plasma demo with a shader that
gamma-encodes itself. `format`, `present_mode` and `max_frame_latency`
apply under both modes, because libmorph configures the surface under
both. An earlier draft put them inside the `ConstructFrame` variant to
make "illegal combinations unrepresentable"; that was wrong, and is
recorded in section 5, because the combinations are not illegal.

**`SRGB_SURFACE` stays.** It is the cheap way to say the one thing most
apps want, and removing it would be breaking for nothing. The precedence
is: a kept `previous` format wins across a resume while the new surface
offers it (today's rule, unchanged, so pipelines stay valid); else
`format: Some(f)` when offered; else the first offered format whose
sRGB-ness matches `SRGB_SURFACE`; else the surface's default.
`choose_format` in `src/gpu.rs` grows the one middle step.

**The structs are exhaustive and their fields are `pub`.** This is a
deliberate choice against the crate's habit (`AudioSetup`, `Key`,
`GpuError` are `#[non_exhaustive]`), and the reason is how a config is
written. Functional record update is not allowed on a `#[non_exhaustive]`
struct from another crate, so a non-exhaustive `DrawingConfig` would
have to be built field by field with `let mut`. Exhaustive, the app
writes what it means:

```rust
fn configure(config: MorphAppConfig) -> MorphAppConfig {
    MorphAppConfig {
        drawing: Some(DrawingConfig {
            format: Some(wgpu::TextureFormat::Bgra8Unorm),
            max_frame_latency: Some(1),
            mode: DrawingMode::ConstructFrame,
            ..Default::default()
        }),
        ..config
    }
}
```

The cost is that every field added later is a minor bump under Rule 5.4
("a variant or field added to an exhaustive public enum or struct"),
which is honest, cheap while 0.x, and the strongest reason this is not
1.0 (section 8). `DrawingMode` is `#[non_exhaustive]` because an app
names a variant rather than building one, and should not match on it.
The owner is considering a macro or another shape for the struct; this
is the shape to build against until that is decided, and it is the one
part of the plan most likely to be amended.

### 2.3 The ordering rule

`configure` runs first. Then, for each subsystem: if it is managed,
libmorph opens it, pumps its events and calls its callbacks; if the app
owns it, libmorph opens nothing, pumps nothing, and calls none of its
callbacks. That is the whole rule, and it is what removes three things
an earlier draft needed:

- **precedence rules** between old callbacks and new config, because
  under `ConstructFrame` `render` is simply not called, and under
  `Managed` `construct_frame` and `acquire` are simply not used;
- **panicking on conflicting use**, because there is no conflict to
  detect: a `ConstructFrame` app that also implements `render` has
  written dead code, not an error, and `ctx.acquire` under `Managed`
  returns `Err(AcquireError::Managed)` and logs once at warn;
- **detecting whether a default trait method was overridden**, which
  stable Rust cannot do (no specialisation, no reflection on trait
  impls). An earlier draft wanted to infer the mode from whether
  `present` had a body. It cannot be done, and the config makes it
  unnecessary: the app says which mode it is in, in data.

The bring-up sequence, with the new step first and nothing else
reordered:

| | Step | Callback | Condition |
|---|---|---|---|
| 1 | preferences | `A::configure(MorphAppConfig::default())` | always |
| 2 | instance, window, surface created; adapter chosen against the surface | - | always |
| 3 | device | `A::limits(&base)` | always (section 3.4) |
| 4 | surface configured | `config.drawing` and `SRGB_SURFACE` | always |
| 5 | audio opened | `A::audio_setup(&mut AudioSetup::default())` | always in this plan (section 6) |
| 6 | the app | `A::init(ctx)` | always |
| 7 | per iteration with a surface | `Shell::iterate` (section 3.1) | mode decides what runs inside |

### 2.4 The register

The config is consulted at bring-up for the surface, and after that the
loop needs one answer per subsystem, "is this managed?", at the top of
each pump and each optional call. Re-deriving it from
`config.drawing.as_ref().map_or(true, |d| d.mode == Managed)` at every
site is the scattered `if let Some(cfg)` the owner wants gone. So it is
computed once, after `configure` returns, and kept:

```rust
/// Which subsystems libmorph drives this run. Decided once from the
/// app's `MorphAppConfig` and never changed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Managed {
    pub drawing: bool,
}

impl From<&MorphAppConfig> for Managed {
    fn from(config: &MorphAppConfig) -> Self {
        Self {
            drawing: config
                .drawing
                .as_ref()
                .is_none_or(|d| d.mode == DrawingMode::Managed),
        }
    }
}
```

It is a struct of named `bool`s, not a bitset, for three reasons. It is
read on one thread only, the loop thread, so there is nothing to make
atomic; plan `20261105_01`'s "routing atomic bitset" is a different
object with a different job. A named field cannot be confused with
another bit. And there are few of them: the loop touches four
subsystems (drawing, audio, camera, media), and a `u8` would buy nothing
a struct does not.

It has one field, because only drawing has a source for the other value
in this plan. Audio, camera and media are always managed here, and a
field nothing can set is a field no test can exercise and a reader will
take to mean more than it does. When the audio plan gives `audio` a
`None`/owned state, it adds `pub audio: bool` here and one `if` at the
top of `Shell::audio_events`; the shape is ready, the field is not
pre-declared.

Where the gates go matters as much as where the register is. **The
gate for a subsystem lives inside that subsystem's own entry point, not
at its callers.** `Shell::audio_events` would start with
`if !self.managed.audio { return; }`; the loop calls it unconditionally.
Then there is exactly one gate per subsystem, and a new call site cannot
forget one. For drawing, the one gate is the branch in `Shell::iterate`.

`Shell` holds both the `config` (for every later `Gpu::configure`) and
the `managed` register (for every iteration). On Android the config is
needed before the shell exists, for the first surface in `boot`, so
`Adapter` computes it and hands it to `Shell::start`.

## 3. What changes in the loop

### 3.1 The pumps move, and `iterate` is the one place

Today `Shell::draw` pumps audio, camera and media before `update`. They
are there only because `draw` was the one thing that ran every
iteration, not because they have anything to do with drawing - and
`audio_events` is not only a pump, it also ticks the recovery state
machine (`audio.poll(now)`). Under `ConstructFrame`, if the branch
skipped `draw`, all three would stop on the desktop and the web, and
`on_audio`, `on_camera` and `on_media` would never fire. (Android pushes
camera and media events straight from the loop and would be unaffected,
which is exactly the kind of per-host difference the owner does not
want.)

So the three pumps leave `draw` for the top of a new `Shell::iterate`,
and every runner calls `iterate` where it calls `frame` today
(`winit.rs` `RedrawRequested`, `android.rs` `Adapter::frame`,
`script.rs` `Action::Frames`):

```rust
pub fn iterate(&mut self) -> FrameStatus {
    if self.ctx.target.is_none() {
        return FrameStatus::Skipped;
    }
    self.audio_events();
    self.camera_events();
    self.media_events();
    if self.managed.drawing {
        return self.frame();
    }
    let dt = self.tick_dt();
    self.app.update(&mut self.ctx, dt);
    self.app.construct_frame(&mut self.ctx);
    if std::mem::take(&mut self.ctx.frame_held) {
        log::warn!("libmorph: a frame acquired in construct_frame was not presented or dropped before it returned");
    }
    if std::mem::take(&mut self.ctx.surface_lost) {
        FrameStatus::Lost
    } else {
        FrameStatus::Presented
    }
}
```

`Shell::frame` keeps its acquire, `update`, `render`, overlay and present
exactly as they are, with the three pump calls removed from `draw`. So
the `Managed` sequence is byte for byte today's, except that the pumps
run before the acquire rather than after it. The one observable
consequence: on an iteration whose acquire returns `Timeout`, `Occluded`
or `Outdated`, the pumps now run where before they were skipped with the
frame. Nothing documented changes (`on_audio` is "before `update`, oldest
first, while resumed", and still is); an event can arrive one iteration
earlier than it would have. Section 8 counts this.

The pumps run when there is a target, as the frame does today, so while
suspended nothing is pumped on the desktop, as now. The gate is the
iteration, not the frame.

### 3.2 The frame under `ConstructFrame`

```rust
/// Under `DrawingMode::ConstructFrame`: once per loop iteration while a
/// surface exists, after `update`. Acquire with `ctx.acquire()`, or do
/// not, and present with `ctx.present(frame)`, or drop the frame to skip
/// it. `render` is never called in this mode.
fn construct_frame(&mut self, ctx: &mut MorphContext) {
    let _ = ctx;
}
```

`MorphContext` exposes `surface_format()` and `surface_size()` but not
the surface, which lives in `Shell::target`. `Target` moves into
`MorphContext` as `pub(crate) target: Option<Target>` (with `resize`,
`reconfigure`, `frame`, `attach_offscreen` and `offscreen` following
it), and two methods hand out and take back one frame:

```rust
/// One frame of the surface, or of the offscreen target in a scripted
/// run. Present it with `MorphContext::present`, or drop it to skip the
/// frame.
pub struct SurfaceFrame {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    /// Physical pixels.
    pub size: (u32, u32),
    /// The surface wants reconfiguring; `present` does it after this
    /// frame, as the managed path does.
    pub suboptimal: bool,
    source: FrameSource, // Surface(wgpu::SurfaceTexture) | Offscreen
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AcquireError {
    /// Not under `DrawingMode::ConstructFrame`. Logged once at warn.
    Managed,
    /// No surface: suspended, or the window is not up yet.
    NoSurface,
    /// `Timeout` or `Occluded`: nothing to draw into this iteration.
    NotReady,
    /// The surface was outdated. libmorph has reconfigured it; acquire
    /// again next iteration.
    Outdated,
    /// The surface is lost. libmorph has dropped it and the runner
    /// recreates it before the next iteration; the next acquire that
    /// succeeds is on the new surface. The app hears nothing else,
    /// as under `Managed` today.
    Lost,
    /// This iteration's frame is still held.
    Held,
    /// wgpu reported a validation error acquiring. Logged.
    Validation,
}

impl MorphContext {
    /// The surface's next texture. At most one per `construct_frame`.
    pub fn acquire(&mut self) -> Result<SurfaceFrame, AcquireError>;
    /// Present `frame` (a no-op on the offscreen target), then
    /// reconfigure if it was suboptimal. The device is not polled: that
    /// is the app's, which is the point.
    pub fn present(&mut self, frame: SurfaceFrame);
}
```

`acquire` does the owner's share of the error handling - the share that
needs the surface's configuration or the runner - and leaves the app the
decision. `Outdated` reconfigures and reports; `Lost` drops the target,
sets `surface_lost`, and `iterate` returns `FrameStatus::Lost` so the
runner recreates the surface on the path it already has for the managed
frame (`winit.rs` and `android.rs` both have it). `Suboptimal` is
reported in the frame and acted on in `present`, after the app has
presented, exactly where the managed path reconfigures. The runner code
does not branch on the mode anywhere: both modes feed it the same
`FrameStatus`.

**One rule the app must keep: a frame acquired in `construct_frame` is
presented or dropped before `construct_frame` returns.** Two things
happen between iterations that the app cannot see coming: libmorph
reconfigures the surface (`Shell::resize` on a `WindowResized`, and the
suboptimal and outdated paths), and on Android the surface is dropped
synchronously inside the `WindowDestroyed` handler, before the activity
takes the window back. A swapchain image held across either is at best
an `Outdated` next time and at worst a texture of a window that no
longer exists. libmorph warns once when it happens (`frame_held` in
`iterate`) and does not police it further. What the app may do instead
is not acquire this iteration, which is the frame-skip the plan exists
to allow.

**Pacing is the app's, and the loop's rate is the host's.** When the app
presents every iteration, `Fifo` paces the loop as it does today. When
it does not acquire, the iteration returns at once and the host calls
the next one as soon as it can: `request_redraw` on winit,
`poll_timeout: Some(Duration::ZERO)` on Android. An app that wants to
idle - a paused game drawn once - acquires and presents its unchanged
frame, or blocks in `construct_frame`, which costs the same latency a
long frame costs today. A host-neutral pacing hint (a return from
`construct_frame`, or a `ctx` call, mapped to `ControlFlow::WaitUntil`
and `poll_timeout`) is a later, additive step if an app needs it; it is
not built on speculation here.

**The desktop overlay is not drawn under `ConstructFrame`.** It is
drawn into the frame libmorph constructs, after `render`, and under this
mode libmorph constructs none. An app that wants the emulated cutouts
over its own frame can be given `ctx.draw_overlay(&view)` later,
additively; the game does not need it to read a render.

**Scripted runs work under both modes**, because `acquire` and `present`
abstract over `Target::Offscreen`: `acquire` hands back the offscreen
texture with `source: Offscreen`, `present` presents nothing, and
`read_rgb` blocks for the GPU as it does now. This matters: the game's
desktop binary takes its headless captures through the `script`
feature, and the mode must not cost it that.

### 3.3 Surface preferences, applied at every configure

`Gpu::configure` gains the `DrawingConfig` (or its absence) as an
argument, and applies it after `get_default_config`: `present_mode` when
`get_capabilities().present_modes` contains it, else the default,
logged at warn; `desired_maximum_frame_latency` when given; and
`format` through `choose_format` in the precedence of section 2.2.
`Shell::start` and `Shell::attach` both call it, so a resume, a lost
surface and an Android rotation all apply the same preferences; `resize`
and `reconfigure` reuse the stored `SurfaceConfiguration`, which already
carries them.

In scripted runs `format` is honoured for its sRGB-ness only
(`offscreen_format` picks `Rgba8UnormSrgb` or `Rgba8Unorm`), because the
screenshot readback assumes four bytes a pixel; `present_mode` and
`max_frame_latency` have no meaning there and are ignored.

An unoffered `format` falls back and logs rather than failing bring-up.
Rejected alternative in section 5.

### 3.4 The existing callbacks under the new ordering

**`limits`** keeps `fn limits(base: &wgpu::Limits) -> LimitsRequest` and
is called unconditionally, as today, between the adapter and the device.
It is awkward in a way worth stating. Every other subsystem can in
principle be managed or owned, but the GPU cannot: libmorph must create
the instance, adapter and device in every mode, because the surface it
owns needs them, and `ctx.device()`, `ctx.queue()` and `ctx.limits()`
are what every app builds on. So the GPU is managed in a sense no flag
expresses, it has no field in the register, and `limits` has no
condition. Folding it into `configure` was considered and rejected
(section 5): `base` comes from the adapter, which does not exist when
`configure` runs. If a later plan wants pre-adapter choices - backends,
power preference, the fallback adapter that `--morph-fallback-adapter`
sets today - they belong in a `gpu: Option<GpuConfig>` on
`MorphAppConfig`, and `limits` stays at its phase, after the adapter.

**`audio_setup`** keeps `fn audio_setup(setup: &mut AudioSetup)` and is
called exactly as today, with today's defaults (`output: true`,
`capture: false`, `capture_mode: Default`), because audio is always
managed in this plan. When the audio plan gives `MorphAppConfig` an
`audio` field, `audio_setup` is called only under managed audio - the
ordering rule of section 2.3 applied to it - and until then nothing
about it moves. It is the one callback whose by-`&mut` shape differs
from `configure`'s by-value; the difference is left, because changing
`audio_setup` would be breaking for nothing, and because the audio plan
may fold its content into the config anyway.

**`SRGB_SURFACE`** keeps its meaning as the default when `format` is
`None` (section 2.2).

**`render`** stays required and is not called under `ConstructFrame`.
The alternative, a default body so an owning app need not implement it,
is rejected in section 5.

**`update`** is called every iteration under both modes, before `render`
or `construct_frame`, with `dt` since its last call. Its doc changes
from "once per frame before `render`" to say so. A `ConstructFrame` app
may leave it default and do everything in `construct_frame`, or keep
simulation in `update` where input results are already consumed; both
work, and keeping it called means an app that switches mode does not
have to move its simulation.

**`init`, `on_suspend`, `on_resume`** and every other hook are unchanged
in signature, timing and condition. Under `ConstructFrame`, `on_suspend`
is still the signal that the surface is going and `on_resume` that a new
one is configured; `ctx.acquire()` between them returns
`Err(NoSurface)`. A `ConstructFrame` app acquires through `ctx` each
iteration and so always sees the current surface, with no announcement
needed when it changes size or is recreated.

## 4. What the app takes on under `ConstructFrame`

Stated plainly, because this is the trade, and if an app finds it has
taken on more than this the split is in the wrong place:

- **Acquiring, and skipping.** Whether to acquire this iteration at all.
  `NotReady`, `Outdated` and `Lost` are reported, with libmorph's share
  already done; the app decides what the frame does about them.
- **Presenting, and pacing.** Whether to present, how deep to let the
  queue get (with `max_frame_latency` as the ceiling), and when to call
  `ctx.device().poll(..)`.
- **The one rule.** Present or drop within `construct_frame`.
- **Nothing else.** The device, queue and adapter; the window, its
  creation, resize and destruction; the surface and its configuration;
  every event, input, lifecycle, intent, permission, audio, camera,
  media; and every JNI call, which stays on the loop thread where it is
  cheap - all of it is libmorph's, unchanged.

## 5. Rejected, and why

- **Surface preferences inside the `ConstructFrame` variant** (`format`
  and `present_mode` as fields of the enum variant, to make illegal
  combinations unrepresentable). Wrong, because the combinations are not
  illegal: configuring the surface while libmorph draws is the main
  case. Ownership is a field of the config (section 2.2).
- **Precedence rules, conflict panics, and override detection.**
  Replaced by the ordering rule (section 2.3). Override detection is also
  impossible on stable Rust.
- **A default body for `render`.** It would let a `ConstructFrame` app
  omit it, and it would let a `Managed` app that mistypes `render`'s
  signature (a wrong lifetime, `&Frame` for `&Frame<'_>`, a missing
  `&mut`) compile to a black screen instead of failing to build. Trading
  a compile-time error for a runtime log is the wrong direction. Making
  a required method provided later is additive; the reverse is breaking.
  So it stays required now.
- **`ConstructSurface` and `ConstructWindow`.** Naming the variants by
  what the app constructs is the right scheme, and each further rung
  hands the app more of the Android window lifecycle: `WindowDestroyed`
  arriving synchronously inside the poll callback with the surface to be
  gone before the handler returns, the `SurfaceTarget` handoff, the
  density and inset reports. libmorph owning the surface is precisely
  what lets it absorb a rotation on the app's behalf. `ConstructWindow`
  is also meaningless on Android, where the activity owns the window.
  Neither is built; the enum is `#[non_exhaustive]` so either could be.
- **Synthesised events: turning a rotation into a fake
  `on_suspend`/`on_resume` pair.** The real events already arrive and
  are already handled: `WindowResized` to `Adapter::resized` and
  `Shell::resize`; `ConfigChanged` to the scale factor; and where a
  device does destroy and recreate the window, `WindowDestroyed` and
  `WindowCreated` to `detach` and `attach`, which already call the real
  `on_suspend` and `on_resume`. A synthetic pair would double those
  where they happen and invent them where they do not. An app may do
  real work on suspend - save state, release the camera - that is wrong
  for a rotation. And faking it on mobile only would make libmorph
  behave differently per device, which the owner has ruled out:
  "libmorph shouldn't change depending on device". A `ConstructFrame`
  app sees the current surface at each `acquire` and needs no
  announcement.
- **Failing bring-up on an unoffered `format`.** The app can read
  `ctx.surface_format()` in `init` and build for what it got, which is
  what every app does today; a phone whose compositor offers one format
  should not refuse to run a game over a preference. `limits` fails
  bring-up for `required` because a shader that needs eight storage
  buffers cannot run with four; a format is not of that kind. Falls back
  and logs.
- **A bitset, or atomics, for the register.** Read on one thread,
  four subsystems, named fields (section 2.4).
- **Folding `limits` into `configure`.** `base` does not exist yet
  (section 3.4).
- **A pacing return from `construct_frame`.** Deferred, not rejected:
  additive later if an app needs it (section 3.2).

## 6. Audio is out of scope

An `AudioConfig` is essentially today's `AudioSetup` plus an ownership
field, and it is not designed here. "The app takes cpal" needs a third
state `AudioSetup` cannot express: `output: false, capture: false`
already means "open nothing", and owning would mean "open nothing, and
also hand the app the cpal host and the stream lifecycle libmorph pauses
and recovers today" - on Android, libmobile's copy of cpal and its
`CaptureStream`. The owner has said it needs more thought. When it is
designed, it goes in `MorphAppConfig::audio`, adds `audio` to the
register, gates `audio_setup` and `Shell::audio_events` on it, and is
its own plan.

## 7. Relation to the drafts of the same day

**`20261105_02` is superseded by this plan.** It kept the same opt-out
as a bare associated function `drawing() -> Drawing` with `Managed` and
`Own`, and a `present` hook. Everything in it is carried forward: its
"why" is section 1; `Drawing::Own` is `DrawingMode::ConstructFrame`, a
field of a config that also holds the surface preferences it had no
place for; its `present` is `construct_frame`, and its `ctx.surface()`
is `ctx.acquire`/`ctx.present`, which hand out one frame rather than the
surface so scripted runs keep working; its sections 3 and 4 are sections
3.2 and 4 here; its three steps are steps 3 to 5 below; and its "not
justified by a slow frame" is section 10. What it lacked was the second
axis, the ordering rule, the register and the pump move, which is why it
could not say what happened to `on_audio` when `draw` was skipped.

**`20261105_01` stays Draft and is not changed.** The owner wants the
drawing opt-out "without the full threading plan right now". The two are
independent, and this one may make most of that one unnecessary: an app
that constructs its own frame can run its simulation and its drawing on
whatever threads it likes, hand the loop thread a finished frame to
present, and pace all of it, without libmorph growing per-thread event
loops and a routing bitset. What `_01` would still add - input delivered
to a thread other than the loop's - is not something a game has asked
for yet. That plan is left to be judged once a `ConstructFrame` app
exists.

## 8. The version: 0.7.0, and why not 1.0

Rule 5.4: "While the version is `0.x`, the bump follows the public API
change. A release whose public API changes are not strictly additive
bumps the minor version (`0.x.y` to `0.(x+1).0`): anything removed,
renamed or changed in signature or type, behaviour a caller can observe
changing, or a variant or field added to an exhaustive public enum or
struct. A release whose changes are strictly additive, or change no
public API, bumps the patch version."

Counted honestly:

- **Additive:** `MorphApp::configure` (default returns its argument),
  `MorphApp::construct_frame` (default empty), `MorphAppConfig`,
  `DrawingConfig`, `DrawingMode`, `SurfaceFrame`, `AcquireError`,
  `MorphContext::acquire`, `MorphContext::present`. Every subsystem
  defaults to managed, so an app that implements none of the new items
  takes today's path.
- **Internal:** `Shell::iterate`, the register, `Target` moving into
  `MorphContext`, `Gpu::configure`'s new argument. None is public.
- **Observable, narrowly:** the pumps run on iterations whose acquire
  fails, where they were skipped (section 3.1). No documented contract
  changes, and no app is known to depend on the old timing, but it is
  "behaviour a caller can observe changing" by the rule's letter.
- **Neutral:** pinning libdevice and libmobile to the versions the lock
  already resolves (section 9) moves nothing.

So the rule's letter asks for a minor bump on the strength of the pump
timing alone, and nothing else here would. If that one item were argued
away the rule would say 0.6.2, and 0.7.0 would be a choice rather than a
requirement. It is chosen anyway, and stated as such: this release
changes what libmorph is to an app - a thing that opens the device and
drives what the app has not taken - and the game, which takes libmorph
by tag, should see that in the number it opts into.

**Not 1.0.** A version number is a compatibility claim, not a
significance claim. 1.0 would promise that `MorphAppConfig` and
`DrawingConfig` keep their shape, and section 2.2 says the shape is the
open question: the owner is considering a macro or another form, the
structs are exhaustive so every added field is a minor bump, and the
audio field is known to be coming. 1.0 would freeze the config at exactly
the point intended for revision.

## 9. Pinning libdevice and libmobile by tag

`Cargo.toml` names both as git dependencies with no `tag` or `rev`, in
three places: `[dependencies]` (libdevice; libmobile optional),
`[build-dependencies]` (libmobile, the host copy for
`forward_metadata`), and `[target.'cfg(target_os = "macos")'.dependencies]`
(libdevice with `open-link` and `clipboard`). So each tracks its
default branch, and a breaking change in either reaches libmorph on the
next fresh resolution with no commit here recording it. That is the
opposite of how the game takes libmorph (`tag = "v0.6.1"`, a
deliberate opt-in), and it defeats Rule 5.5, which says: "When a crate
with a public API moves a dependency to a version that is not
semver-compatible with the one it used ... its own release is not
strictly additive and bumps its minor version ... libmorph re-exports
libdevice, libmobile's types, wgpu and cpal, so their incompatible bumps
are libmorph's too." A rule about moving a dependency needs the move to
be a thing that happens in a commit. With a tag, it is: every move is a
manifest diff, reviewed under 5.5, with its old and new versions in
`VERSIONS.md`.

The committed `Cargo.lock` softens this for libmorph's own builds, since
it pins a revision, but a dependent resolving libmorph `v0.6.1` fresh
takes each branch's head at that moment, which is how a libmobile 0.5.0
would break a libmorph release that never changed.

The current state, confirmed against the sibling checkouts: libmorph
0.6.1 (tag `v0.6.1`), libmobile 0.4.0 (tag `v0.4.0` at `9d22b31`),
libdevice 0.1.3 (tag `v0.1.3` at `e4a3a15`). Each crate's version matches
its newest tag, so none is mid-release. The lock at `e60bf90` resolves
libmobile at `0b2929c` (one commit past its tag, `CHANGES` only) and
libdevice at `948f924` (three past its tag: `CHANGES`, the README row
and a plan's status). Pinning by tag moves each back to the tagged
commit; no code differs.

The pin is `tag = "v0.4.0"` and `tag = "v0.1.3"`, in all three places,
with the same spelling in each: Cargo treats two git specifications that
differ as two sources, and two copies of libmobile would collide on its
`links` key. The lock is regenerated with `.cargo/config.toml` (the
gitignored patch to the siblings) moved aside, because a lock resolved
with a patch active loses the `source = "git+…"` line, which `AGENTS.md`
says is never committed. The manifest and lock are a repository-level
change and go in their own commit (Rule 3.1).

## 10. What this is not justified by

Not by a slow frame. The game renders its interior scene at 266 ms on an
Adreno 740, steady, and the thing that actually froze it was a blocking
GPU readback it had left in its own code: a one-off shader-corruption
check that was never put behind a flag. Nothing in this plan would have
caught that, and nothing in it makes a frame faster. It is justified by
the boundary being in the wrong place for anything larger than a demo,
which was true before any of that was measured and would be true if
the game drew in 2 ms.

## 11. Steps

**Step 1 - pin libdevice and libmobile by tag.** `Cargo.toml`'s three
entries gain `tag = "v0.1.3"` and `tag = "v0.4.0"`; the lock is
regenerated without the patch; one commit, repository-level.

**Done when:** `Cargo.lock`'s `source` for each reads
`git+https://github.com/tmzt/<crate>?tag=v<version>#<tagged commit>`;
`cargo tree -p libmorph | grep -E 'libdevice|libmobile'` shows 0.1.3 and
0.4.0 and no duplicate; `cargo clippy -p libmorph --all-targets`, the
same with `--features script`, and
`cargo clippy -p libmorph --target aarch64-linux-android --features mobile`
are clean; `cargo test -p libmorph --features script` passes.

**Step 2 - the config, the callback and the register.**
`MorphAppConfig`, `DrawingConfig`, `DrawingMode` in a new `src/app_config.rs`
(re-exported from `lib.rs`); `MorphApp::configure` with its default;
`Managed` with `From<&MorphAppConfig>`; each runner calls `configure`
first and hands the config and the register to `Shell::start` and
`start_offscreen`, which keep them. Nothing reads them yet except tests.

**Done when:** unit tests show `MorphAppConfig::default()` gives
`Managed { drawing: true }`, `DrawingMode::ConstructFrame` gives
`drawing: false`, and the example in section 2.2 compiles as written;
every existing example builds unchanged; a scripted run of `raymarch`
gives byte-identical PNGs to its run at the step 1 commit.

**Step 3 - the surface preferences.** `Gpu::configure` takes the
`Option<&DrawingConfig>` and applies `present_mode`,
`desired_maximum_frame_latency` and `format` (through `choose_format`,
in section 2.2's precedence, with the warn logs); `Shell::start` and
`attach` pass it; `offscreen_format` honours `format`'s sRGB-ness.

**Done when:** `choose_format`'s tests cover the new middle step
(requested and offered; requested and not offered; requested but a
`previous` kept); an example that asks for `Bgra8Unorm` under `Managed`
reports it from `ctx.surface_format()` on a Mac, and the same build asks
for `Rgba16Float`, is refused, logs the fallback and draws; a scripted
run of an unchanged app is byte-identical to step 2's.

**Step 4 - `iterate` and the pump move.** `Shell::iterate` as in section
3.1, with the pumps at its top and removed from `draw`; `frame` otherwise
untouched; `winit.rs`, `android.rs` and `script.rs` call `iterate` where
they called `frame`. The `ConstructFrame` branch is not yet reachable
(no `construct_frame` hook); `iterate` calls `frame` unconditionally and
the register is read by a test only.

**Done when:** a scripted run is byte-identical to step 3's; the
`audiocapture` and `tone` examples deliver `on_audio` and
`on_capture_ready` on a Mac as before; a test in `shell.rs` drives an
app whose acquire is made to fail (an offscreen shell with no target)
and shows the pumps still ran.

**Step 5 - `ConstructFrame`.** `Target` moves into `MorphContext`;
`SurfaceFrame`, `AcquireError`, `acquire`, `present`, `frame_held`,
`surface_lost`; `MorphApp::construct_frame`; `iterate`'s branch; a new
example `examples/ownframe.rs` that clears to a colour and draws one
triangle from `construct_frame`, presenting every iteration, skipping
every third when run with `--skip`.

**Done when:** `ownframe` on a Mac shows what the same triangle under
`Managed` shows, survives a window resize, a suspend and a resume (the
scripted `suspend`/`resume` and winit's) without special-casing; on the
S23 it survives a rotation and a backgrounding; `ctx.acquire()` from a
`Managed` app's `render` returns `Err(AcquireError::Managed)` and logs
once; the `frame_held` warning fires for a test app that keeps its
frame and does not fire for one that presents.

**Step 6 - scripted runs under `ConstructFrame`.** `acquire` and
`present` over `Target::Offscreen`; the script runner takes no other
change.

**Done when:** a scripted run of `ownframe` gives the same bytes as the
same triangle under `Managed`, and `run.json` is identical apart from
nothing (the mode is not recorded, because it does not change the
pixels).

**Step 7 - 0.7.0.** `Cargo.toml` version, `VERSIONS.md` entry listing
section 8's items with the pump timing marked **Breaking** and the pins
named, tag `v0.7.0` on the commit; `AGENTS.md`'s Core API list gains
`configure`, `construct_frame`, `acquire`, `present` and the config
types, and its Lifecycle table's `tick → update + render` becomes
`tick → iterate`; the README's intro mentions the two modes.

**Done when:** `git tag --points-at HEAD` shows `v0.7.0` on the version
commit; the game can move `libmorph`'s `tag` to `v0.7.0` and build
its desktop and Android binaries with no code change; the CHANGES
entry records the checks.

## Amendment — 2026-11-05 14:33 EST: the register is a bitset, at the owner's instruction

The owner, 2026-11-05, on section 2.4's choice of a struct of named
`bool`s over a bitset: "the bitset is internal to libmorph, not the
API". This amendment replaces the register's form and every passage
that depends on it. The visibility (`pub(crate)`), the single
computation from `MorphAppConfig` after `configure` returns, the rule
that the gate lives inside each subsystem's own entry point rather than
at its callers, and the decision to declare only the subsystem that has
a source for the other value all stand.

### What was considered, and why the bitset won

Section 2.4 gave three reasons for named fields: the register is read
on one thread so needs no atomics; a named field cannot be confused
with another bit; and four subsystems do not need a `u8`. The first is
true and is not an argument for a struct, only against atomics, which
the bitset does not use either. The third is neutral. The second is the
one that decided it, and it is a public-API concern misapplied: a
mis-bitted flag is a hazard where a type is defined in one place and
read by callers who cannot see the definition. This type is
`pub(crate)`, defined once in `src/runner/shell.rs` beside `Shell` and
`FrameStatus` (both already `pub(crate)`), and read in that crate on the
loop thread; there is no second definition for it to drift from and no
downstream caller to get it wrong.

What the bitset gives that the struct did not is **one call shape for
every subsystem.** The gate at each entry point reads identically -
`self.managed.has(Subsystem::Drawing)`, `self.managed.has(Subsystem::Audio)`
- where four differently-named fields are four shapes, and a struct of
fields does not generalise to iterating or logging the subsystems at
bring-up. That one shape is what the owner asked for: a clean place to
check whether a subsystem is managed.

It is also the same object, in shape, as plan `20261105_01`'s "routing
atomic bitset": a word of bits indexed by a `pub(crate)` enum. One
mental model across the two plans rather than two forms for the same
job. The semantic difference section 2.4 drew still holds and is kept
here so it is not lost: `_01`'s bitset says which of three loops gets
each event class and is read from several threads, so it is atomic;
this one has one bit per subsystem, set when managed, and is read on
the loop thread only, so it is a plain `u8`.

### The replacement for section 2.4's code and its explanation

The code block in section 2.4 (`pub(crate) struct Managed { pub drawing:
bool }` and its `From`) is replaced by:

```rust
/// The subsystems the loop drives, each one bit of `Managed`. A
/// subsystem is declared here when a `MorphAppConfig` field can make
/// it owned; until then it is managed and has no bit to clear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum Subsystem {
    Drawing = 1 << 0,
    // Audio, Camera, Media: added by the plan that gives each an owned
    // state, with its bit, its line in `From`, and its gate.
}

/// Which subsystems libmorph drives this run: one bit per
/// `Subsystem`, set when managed. Decided once from the app's
/// `MorphAppConfig` after `configure` returns, and never changed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Managed(u8);

impl Managed {
    /// Every declared subsystem managed: the default, and what a
    /// `MorphAppConfig::default()` gives.
    pub const ALL: Self = Self(Subsystem::Drawing as u8);

    pub fn has(self, subsystem: Subsystem) -> bool {
        self.0 & subsystem as u8 != 0
    }

    fn without(self, subsystem: Subsystem) -> Self {
        Self(self.0 & !(subsystem as u8))
    }
}

impl From<&MorphAppConfig> for Managed {
    fn from(config: &MorphAppConfig) -> Self {
        let mut managed = Self::ALL;
        if config
            .drawing
            .as_ref()
            .is_some_and(|d| d.mode == DrawingMode::ConstructFrame)
        {
            managed = managed.without(Subsystem::Drawing);
        }
        managed
    }
}
```

The paragraph beginning "It is a struct of named `bool`s, not a bitset,
for three reasons" is replaced by the reasoning above.

The paragraph beginning "It has one field, because only drawing has a
source for the other value in this plan" is replaced by: it declares one
subsystem, because only drawing has a source for the other value in
this plan. Audio, camera and media are always managed here, and a bit
nothing can clear is a bit no test can exercise and a reader will take
to mean more than it does. A bitset in which only one bit can presently
be cleared is honest and costs nothing. When the audio plan gives
`MorphAppConfig::audio` an owned state, it adds `Audio = 1 << 1` to
`Subsystem`, one line to `From`, and
`if !self.managed.has(Subsystem::Audio) { return; }` at the top of
`Shell::audio_events`; nothing is pre-declared for it.

The paragraph beginning "Where the gates go matters as much as where the
register is" stands, with its example gate now written
`if !self.managed.has(Subsystem::Audio) { return; }`.

### The other passages this changes

- Section 3.1, in `Shell::iterate`: `if self.managed.drawing {` is
  `if self.managed.has(Subsystem::Drawing) {`. Nothing else in the
  function changes.
- Section 5, the bullet "A bitset, or atomics, for the register" is
  replaced by: **Named `bool` fields for the register.** Their one
  advantage, that a field cannot be mis-bitted, protects callers who
  cannot see the definition, and this type has none; a bitset gives one
  call shape per subsystem and matches `20261105_01`'s. Atomics are
  still rejected: the register is read on the loop thread only.
- Section 6, "adds `audio` to the register" is "adds `Subsystem::Audio`
  to the register".
- Section 11, step 2's **Done when**: "unit tests show
  `MorphAppConfig::default()` gives `Managed { drawing: true }`,
  `DrawingMode::ConstructFrame` gives `drawing: false`" is "unit tests
  show `Managed::from(&MorphAppConfig::default()) == Managed::ALL`,
  that `Managed::ALL.has(Subsystem::Drawing)`, and that a config with
  `DrawingMode::ConstructFrame` gives `!has(Subsystem::Drawing)`".

The status stays Draft. The README row is unchanged: it names the
register's computation and where its gates live, neither of which
moved.

## Amendment — 2026-11-05 16:37 EST: superseded by the plan where the app declares what it owns

Superseded by `docs/design/20261105_04_OWNS_AND_THE_RENDER_STATUS_PLAN.md`. Nothing in this plan was
implemented. Worked out with the owner in discussion, 2026-11-05.

What replaced what, and why:

- `configure`, `MorphAppConfig`, `DrawingConfig`, `DrawingMode` and the
  register of section 2.4 and its amendment are all replaced by one
  associated const, `MorphApp::OWNS`, of a hand-rolled `Owned` flags
  type. The owner: "it also reads clearly the MorphApp OWNS these
  subsystems", and "we won't move anything else". A const has no
  position in the sequence, so nothing is computed, stored or read back
  through a register; and because the app type is monomorphized,
  `A::OWNS.contains(Owned::FRAME)` folds at compile time and the managed
  path compiles to what it is today with the other arm eliminated.
  `DrawingMode::ConstructFrame` becomes the `Owned::FRAME` bit.
- Section 2.2's `format`, `present_mode` and `max_frame_latency` fields
  are replaced by a `wgpu_config` hook, called at every attach when
  `FRAME` is owned, taking and returning a `wgpu::SurfaceConfiguration`
  the way `limits` takes and returns limits. The owner: "my concern is
  we're going to end up duplicating wgpu's configuration in morph's own
  structs, and have to follow the upstream api changes". A managed app's
  surface knob stays `SRGB_SURFACE`, which already covers the case
  section 2.2 cited for `format`.
- Section 2's argument for exhaustive `pub` fields is moot: there is no
  struct.
- Section 3.1's `Shell::iterate` and the pump move are not needed. The
  pumps stay where they are and keep firing for every subsystem libmorph
  still drives, so taking the frame cannot silence one - which was the
  flaw that superseded `20261105_02`. `Shell` is `pub(crate)`, so where
  the branch lives is internal.
- Section 3.2's frame is carried forward whole: `ctx.acquire` and
  `ctx.present` become `ctx.acquire_frame` and `ctx.present_frame`,
  because a method reads as its verb applied to its receiver and
  `ctx.acquire()` reads as acquiring the context. With them, the one
  rule and its Android reasoning, the pacing and
  overlay and scripted-run passages, and section 4's trade.
  `SurfaceFrame` is renamed `AcquiredFrame`, to pair with `acquire_frame`
  and
  to stop reading as a near-synonym of the `Frame` that `render` keeps;
  `AcquireError::Managed` becomes `NotOwned`.
- Sections 8 and 9, the version reasoning and the pinning by tag, are
  carried forward as the replacement's sections 9 and step 1.
- Section 10 is carried forward: the release is still not justified by a
  slow frame.

Added there and absent here: a default body for `render`, and a
`RenderStatus` return by which a managed app says whether the frame
needs presenting. The owner: "the value of a return is to tell the morph
render loop whether anything needs to be presented". The two ship as one
break because adding `OWNS` while `render` stayed required would leave
every owning app writing a dead method.

Also settled in the same discussion and recorded there rather than here:
a missed override is reported by a `log_once!` and not by a panic or a
build error, because Rust cannot ask whether a method was overridden and
the only mechanism reports against libmorph's own source at codegen
time. A lint callable from `build.rs` is the intended answer and is out
of scope.
