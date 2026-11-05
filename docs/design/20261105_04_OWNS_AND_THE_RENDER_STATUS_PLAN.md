---
title: OWNS - an app declares the subsystems it takes, and render says whether to present
date: 2026-11-05
status: Draft
covers: >
  Two changes to `MorphApp`, released together as one break. An
  associated const, `OWNS`, by which an app declares the subsystems it
  takes from libmorph; `Owned::FRAME` is the only flag built, and it
  turns off libmorph's acquire, draw and present in favour of
  `construct_frame`, `ctx.acquire_frame` and `ctx.present_frame`. And a
  return from
  `render`, `RenderStatus`, by which a managed app says whether the
  frame needs presenting at all - which is what lets `render` have a
  default body, so an owning app need not write one.
sources: >
  The owner, 2026-11-05, in discussion: "I intended libmorph to be the
  minimal required API for a mobile app using wgpu for rendering";
  "the value of a return is to tell the morph render loop whether
  anything needs to be presented"; "a no-op that logged on first use
  would be fine, it's enough to tell us why the screen is black";
  "overriddable behavior on MorphApp is just the old java applet API
  smells"; "we could also just make the bitset the public API returned
  in configure similar to SDLs subsystems ... but with the difference
  that setting the bit means the app takes ownership of the full
  subsystem with only a single hook called in the right place like
  wgpu_config at attach"; "it also reads clearly the MorphApp OWNS
  these subsystems"; "we won't move anything else"; and, on a build
  error for a missed override, "the likely answer will be a clippy-like
  lint added as an optional build.rs callable, but not part of 0.7.0 or
  in scope".
  `src/app.rs:19` (`MorphApp`), `:87` (`render`), `:10` (`Frame`),
  `:55` (`limits`), `:24` (`SRGB_SURFACE`).
  `src/runner/shell.rs:102` (`Shell`, `pub(crate)`), `:116` (`start`,
  which configures the surface before `with_app` calls `init` at `:131`),
  `:160` (`attach`), `:262` (`resize`), `:271` (`reconfigure`),
  `:332` (`frame`), `:392` (`draw`), `:406` (the one `app.render` call).
  `src/gpu.rs:205` (`configure`: `get_default_config`, then `format`
  only), `:230` (`choose_format`).
  `src/input.rs:41` (`KeyStatus`, the precedent for a status enum
  returned from a hook, and not `#[must_use]`).
  `wgpu-types-30.0.1/src/lib.rs:176` (`PollType`).
  `wgpu-core-30.0.1/src/device/resource.rs:5305`
  (`desired_maximum_frame_latency` clamped to the surface's range).
related: >
  `docs/design/20261105_03_THE_APP_CONFIGURES_WHAT_LIBMORPH_MANAGES_PLAN.md`,
  which this supersedes and which carried the design this replaces:
  `configure`, `MorphAppConfig`, `DrawingConfig`, `DrawingMode` and the
  internal register. What is carried forward from it is named in
  section 8.
  `docs/design/20261105_01_THE_APP_RUNS_OFF_THE_LOOP_THREAD_PLAN.md`,
  which stays Draft and is not depended on.
  `docs/design/20261014_05_APP_LIMITS_REQUEST_PLAN.md`, whose `limits`
  is the shape `wgpu_config` copies: here is the baseline, hand back
  what you want.
---

# OWNS - an app declares the subsystems it takes

## 1. Why

libmorph is meant to be the minimal API a mobile app needs to render
with wgpu. Two things in it are not minimal, and they are the same
thing twice.

`Shell::frame` acquires the surface texture, calls `update` and
`render`, draws the desktop overlay, and presents. That is the whole of
a frame, and libmorph owns all of it. For a plasma demo that is the
feature - there is nothing to write but a fragment shader. For anything
larger it is a wall: the app cannot choose when to submit, whether to
skip a frame, or what to do when one is slow.

And `render` is required. An app that draws its own frames must still
write one, so it writes an empty one, which compiles and means nothing.

Neither needs a new subsystem, a config object or a thread. What they
need is for the app to be able to **say** two things: which subsystems
it has taken, and whether this frame needs presenting.

## 2. `Owned` and `const OWNS`

A hand-rolled flags type, and one associated const:

```rust
/// The subsystems an app takes from libmorph. A set bit means libmorph
/// does not drive that subsystem: it does not open it, pump it, or call
/// its managed-path hooks.
///
/// Hand-rolled rather than `bitflags`, so no foreign type is in the
/// public API: under Rule 5.5 that would chain libmorph's version to
/// another crate's.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Owned(u32);

impl Owned {
    /// Nothing taken. libmorph drives everything, as it does today.
    pub const fn empty() -> Self { Self(0) }
    /// The frame: acquire, draw and present. See `construct_frame`.
    pub const FRAME: Self = Self(1 << 0);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub const fn union(self, other: Self) -> Self { Self(self.0 | other.0) }
}

impl core::ops::BitOr for Owned { /* const-compatible, so `FRAME | X` works in a const */ }
```

and on `MorphApp`:

```rust
/// The subsystems this app takes from libmorph. `Owned::empty()` - the
/// default - leaves libmorph driving everything, which is every app
/// written so far.
///
/// ```
/// # use libmorph::Owned;
/// const OWNS: Owned = Owned::FRAME;
/// ```
const OWNS: Owned = Owned::empty();
```

**Why a const and not a callback.** A const has no position in the
sequence: libmorph reads `A::OWNS` wherever it needs it, with nothing
computed and nothing stored. Because `A` is monomorphized,
`if A::OWNS.contains(Owned::FRAME)` folds at compile time and the other
arm is eliminated, so a managed app compiles to today's code path with
the owned branch gone. A returned config would have to be called at a
stated moment, stored, and read back through a register; none of that
exists here.

**What it costs.** Ownership is a compile-time property. `cfg!` works; a
runtime toggle does not. If that is ever needed, the way out is a
returned config, and nothing here forecloses it.

**Why only `FRAME`.** It is the only subsystem an app can take on the
target platform. The device, queue and surface cannot be the app's: the
surface comes from the `ANativeWindow` that libmobile's poll loop owns,
created and destroyed with the activity's lifecycle. An app making its
own instance and surface would be reimplementing libmorph. So `FRAME`
is the ceiling, and `Owned::GPU` would promise what the platform cannot
give.

`AUDIO`, `CAMERA` and `MEDIA` are the intended next flags and are **not
built** (section 7).

## 3. `RenderStatus`, and why `render` can then have a default

```rust
/// Whether the frame needs presenting.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RenderStatus {
    /// The app drew into `frame.view`; libmorph presents it.
    Drawn,
    /// Nothing changed. libmorph drops the texture without presenting,
    /// and the previous frame stays on screen.
    Skipped,
}
```

Not `#[must_use]`: `KeyStatus` is the same thing in the same trait and
is not, and the crate uses that attribute on getters. It would earn
nothing here - the signature change is a hard error at every impl,
which is stronger than a lint, and there is one caller.

```rust
/// Record and submit this frame's work to `frame.view`, and say whether
/// anything needs presenting.
///
/// The default draws nothing and returns `Skipped`. That is correct for
/// an app with `Owned::FRAME`, where `render` is never called; for a
/// managed app it means nothing is drawn at all, so it logs once.
fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) -> RenderStatus {
    let _ = (ctx, frame);
    log_once!("MorphApp::render is not implemented and the frame is managed, \
               so nothing is drawn. Implement render, or set \
               `const OWNS: Owned = Owned::FRAME;` and construct frames.");
    RenderStatus::Skipped
}
```

The return is what makes the default safe. Without it a default body
must either present an untouched surface texture or burn a clear pass
every frame; with it, libmorph presents nothing, which is exactly what
"I did not draw" means.

**In the shell.** `Shell::draw` returns the status; on `Skipped` it does
not draw the desktop overlay (there is no base under it) and `frame`
drops the `SurfaceTexture` without calling `queue.present`, reporting
the `FrameStatus::Skipped` it already has. The offscreen branch returns
the same status rather than unconditionally `Presented`, so both targets
report alike; the script runner discards it today either way
(`script.rs:95`).

**`update` still runs on a skipped frame.** Skip means "do not present",
not "do not simulate".

**What `Skipped` does not cover.** It is the app's claim about its own
output. Two things sit outside it, and are documented rather than
special-cased: the desktop overlay, which is not presented either, so a
pressed dpad button does not appear while frames are skipped; and a
freshly attached surface, which has no previous frame to keep, so an app
must draw the first frame after a surface change.

A skipped frame on the offscreen target keeps the texture's prior
contents, which is what a device does. A never-written texture reads
black: wgpu zero-initializes on first read
(`wgpu-core-30.0.1/src/init_tracker/mod.rs:4`). No clear is needed and
the script runner needs no special case.

## 4. `log_once!`, and the contract hooks

```rust
/// Log `msg` at warn the first time this call site is reached.
/// One `AtomicBool` per expansion.
macro_rules! log_once { ($($arg:tt)*) => { /* ... */ } }
```

Internal. It is used by the defaults of hooks a contract **requires**,
and by nothing else, because not every default is a hole:

| default | reachable when | treatment |
| --- | --- | --- |
| `render` | the frame is managed and it was not implemented | `log_once!` |
| `construct_frame` | `OWNS` has `FRAME` and it was not implemented | `log_once!` |
| `wgpu_config` | always; returning the baseline is a real choice | silent |
| `limits`, `audio_setup`, every `on_*` | always; a no-op is a real choice | silent |

No panic and no `debug_assert`: one behaviour in debug and release.

**No build error.** Rust offers no supported way to ask whether a method
was overridden. The only mechanism is a post-monomorphization const
assert forced from the default body, which reports against libmorph's
own source, surfaces at codegen rather than type-check, and relies on
instantiation behaviour that is not a language guarantee. A lint
callable from `build.rs` can read the impl block and say exactly what is
missing; that is the right tool and it is out of scope here.

The two logs interlock usefully in both directions. An app that
implements `construct_frame` and forgets `OWNS` has a managed frame, so
libmorph calls `render`, whose default fires and names the real mistake.

## 5. `wgpu_config`, under `Owned::FRAME`

```rust
/// The surface configuration to apply. Called at every attach when
/// `OWNS` contains `Owned::FRAME`: the first window, and again after a
/// rotation, a resume or a lost surface. `default` is what libmorph
/// would have used.
///
/// libmorph overrides `width` and `height` after this returns, because
/// it knows the window's size and a wrong one breaks the surface.
fn wgpu_config(
    default: wgpu::SurfaceConfiguration,
    caps: &wgpu::SurfaceCapabilities,
) -> wgpu::SurfaceConfiguration {
    default
}
```

**By value in, by value out, like `limits`.** The baseline arrives as a
*value*, not as a method body that would have to be replicated to
extend, so an app that wants one field writes
`SurfaceConfiguration { present_mode, ..default }` and there is no
question of what libmorph set before or after: it set all of it, and the
app replaced it. A `&mut` to a half-filled struct is the loose version
of the same hook.

**Associated, not a method.** The first attach precedes `init`
(`shell.rs:121` configures, `:131` builds the app), so there is no
instance to call it on, and it cannot be a `ctx` capability.

**Not called for a managed frame.** A managed app's surface knob is
`SRGB_SURFACE`, which already covers the case that motivated a `format`
field - a shader that gamma-encodes itself needs a non-sRGB surface, not
a named format. Keeping the hook to owning apps is what stops libmorph
mirroring `wgpu::SurfaceConfiguration` field by field and tracking it
upstream.

**Not called offscreen**, where there is no surface. `SRGB_SURFACE`
drives `offscreen_format()` under both, so scripted runs stay faithful
to the device.

Note for whoever implements an app with it: `desired_maximum_frame_latency`
is a request, clamped to the surface's reported range
(`wgpu-core-30.0.1/src/device/resource.rs:5305`). Asking for 1 gets 1
only if the range allows. `caps` is in hand to check.

## 6. The frame under `Owned::FRAME`

```rust
/// Construct and present this iteration's frame. Called once per
/// iteration when `OWNS` contains `Owned::FRAME`, after `update`, in
/// place of libmorph's acquire, `render`, overlay and present.
///
/// Acquire with `ctx.acquire_frame()`, or do not, and present with
/// `ctx.present_frame(frame)`, or drop the frame to skip this iteration.
fn construct_frame(&mut self, ctx: &mut MorphContext) {
    let _ = ctx;
    log_once!("`OWNS` contains `Owned::FRAME` but construct_frame is \
               not implemented, so no frame is drawn or presented.");
}
```

```rust
impl MorphContext {
    /// The surface's next texture. At most one per `construct_frame`.
    pub fn acquire_frame(&mut self) -> Result<AcquiredFrame, AcquireError>;
    /// Present `frame` (a no-op on the offscreen target), then
    /// reconfigure if it was suboptimal. The device is not polled: that
    /// is the app's, which is the point.
    pub fn present_frame(&mut self, frame: AcquiredFrame);
}
```

`AcquiredFrame` owns its texture, because the app presents **or drops**
it; `Frame<'a>`, which `render` keeps, lends a view libmorph will
present itself. The names are the ownership difference and should stay
distinct - `AcquiredFrame` pairs with `acquire_frame`.

**Why `acquire_frame` and `present_frame`, not `acquire` and `present`.**
A method reads as its verb applied to its receiver, and `ctx.acquire()`
reads as acquiring the context. The three hooks and calls then say the
same noun: `construct_frame`, `acquire_frame`, `present_frame`.

```rust
pub struct AcquiredFrame {
    pub view: wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    /// Physical pixels.
    pub size: (u32, u32),
    /// The surface wants reconfiguring; `present_frame` does it after this
    /// frame, as the managed path does.
    pub suboptimal: bool,
    source: FrameSource, // Surface(wgpu::SurfaceTexture) | Offscreen
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AcquireError {
    /// `OWNS` does not contain `Owned::FRAME`. Logged once.
    NotOwned,
    /// No surface: suspended, or the window is not up yet.
    NoSurface,
    /// `Timeout` or `Occluded`: nothing to draw into this iteration.
    NotReady,
    /// The surface was outdated. libmorph has reconfigured it; acquire
    /// again next iteration.
    Outdated,
    /// The surface is lost. libmorph has dropped it and the runner
    /// recreates it before the next iteration. The app hears nothing
    /// else, as a managed app does not today.
    Lost,
    /// This iteration's frame is still held.
    Held,
    /// wgpu reported a validation error acquiring. Logged.
    Validation,
}
```

**One rule the app must keep: a frame acquired in `construct_frame` is
presented or dropped before `construct_frame` returns.** Two things
happen between iterations that the app cannot see coming - libmorph
reconfigures the surface (`resize` on a `WindowResized`, and the
suboptimal and outdated paths), and on Android the surface is dropped
synchronously inside the `WindowDestroyed` handler before the activity
takes the window back. A swapchain image held across either is at best
`Outdated` next time and at worst a texture of a window that no longer
exists. libmorph logs once when it happens and does not police it
further. What the app may do instead is not acquire this iteration,
which is the skip this plan exists to allow.

**Pacing is the app's, and the loop's rate is the host's.** Presenting
every iteration lets `Fifo` pace the loop as today. Not acquiring
returns at once and the host calls the next iteration as soon as it can.
An app that wants to idle presents its unchanged frame, or blocks, which
costs the latency a long frame costs today. A host-neutral pacing hint is
additive later and is not built on speculation.

**The desktop overlay is not drawn**, because it is drawn into the frame
libmorph constructs and libmorph constructs none. `ctx.draw_overlay(&view)`
is additive later if an app wants the emulated cutouts.

**Scripted runs work under both**, because `acquire_frame` and
`present_frame` abstract over the offscreen target: `acquire_frame` hands
back the offscreen texture, `present_frame` presents nothing, and
`read_rgb` blocks for the GPU
as it does now. The RPG game's desktop build takes its headless
captures through the `script` feature, and taking the frame must not
cost it that.

## 7. What the app takes on, and what it does not

Under `Owned::FRAME`:

- **Acquiring, and skipping.** Whether to acquire at all. `NotReady`,
  `Outdated` and `Lost` are reported with libmorph's share already done.
- **Presenting and pacing**, including when to call
  `ctx.device().poll(..)`.
- **The surface's configuration**, through `wgpu_config`.
- **The one rule.** Present or drop within `construct_frame`.
- **Nothing else.** The device, queue and adapter; the window and its
  creation, resize and destruction; the surface object and its
  lifecycle; every event - input, lifecycle, intent, permission, audio,
  camera, media; and every JNI call, which stays on the loop thread
  where it is cheap. All of it stays libmorph's, unchanged.

Deliberately not in this plan:

- **`AUDIO`, `CAMERA`, `MEDIA`.** Named as the next flags, not built.
  They are not the same shape as each other: camera and media ownership
  means the app pumps and handles events where libmorph would have
  called `on_camera`/`on_media`, which wants a returning pump on `ctx`
  (`while let Some(event) = ctx.pump_camera()`), and leaves those hooks
  as managed-mode conveniences. Audio ownership means libmorph never
  opens cpal at all, because `audio_events` checks streams for stalls,
  losses and attempts due rather than draining a queue.
- **A config object, a builder, or an internal register.** `OWNS` needs
  none, and moving `SRGB_SURFACE`, `APP_ID` and `SECURE_STORE_ID` into
  one is a separate break that may never be wanted.
- **A device poll on the managed path.** Nothing in the live path polls
  the device today and that does not change here. Under `Owned::FRAME`
  the app polls when it decides to.
- **Anything about a slow frame.** The RPG game's Android build renders
  an interior scene at 266 ms on an Adreno 740, steady; what actually
  froze it was a blocking readback of its own. This plan is justified by the boundary
  being in the wrong place for anything larger than a demo, which was
  true before that frame was measured and stays true if it drops to 2 ms.
- **The applet shape.** `MorphApp` has ~22 defaulted methods and Rust has
  no `@Override`, so a misspelled hook compiles as an inherent method and
  the default keeps running. That is a known provisional design; the exit
  is capabilities on `ctx` rather than hooks on the trait, which is why
  the later subsystems take a returning pump, and the lint is the
  detector. It is why this is 0.7.0 and not 1.0.

## 8. Relation to the drafts of the same day

`20261105_03` is superseded by this plan. Nothing in it was implemented.

Carried forward, in substance: its section 3.2's frame (its `acquire`
and `present` are `acquire_frame` and `present_frame` here; its
`SurfaceFrame` is `AcquiredFrame`; `AcquireError` keeps its name), its one rule and the Android reasoning behind it, its
pacing and overlay and scripted-run passages, its section 4 trade, its
version reasoning, and its step 1 on pinning libdevice and libmobile by
tag.

Replaced: `configure`, `MorphAppConfig`, `DrawingConfig`, `DrawingMode`
and the internal `Managed` register are all gone, replaced by
`const OWNS`. Its `format`, `present_mode` and `max_frame_latency`
fields are gone, replaced by `wgpu_config`, because mirroring
`wgpu::SurfaceConfiguration` field by field would make libmorph track
wgpu's API and bump for it. Its section 2's argument for exhaustive
`pub` fields is moot with no struct. Its `Shell::iterate` and pump move
are not needed: the pumps stay where they are and keep firing for every
subsystem libmorph still drives, in both cases, so taking the frame
cannot silence a subsystem - which was the flaw that superseded
`20261105_02`.

Added, and not in any earlier draft: `render`'s default and
`RenderStatus`.

## 9. The version: 0.7.0

`render`'s signature changes, so the release is not strictly additive
and bumps the minor version under Rule 5.4. Everything else in it is
additive - a new const with a default, new hooks with defaults, a new
flags type and two new `ctx` methods - and would have been a patch on
its own.

Both parts ship in the one break deliberately. Adding `OWNS` while
`render` stayed required would leave every owning app writing a dead
method, and giving `render` a default is what removes that; they are one
change seen from two sides.

Not 1.0: the trait's shape is provisional (section 7), and a release
whose own design note says so should not claim stability.

Adding a flag to `Owned` later is strictly additive - a patch bump -
which is the property that makes deferring `AUDIO`, `CAMERA` and `MEDIA`
cheap.

## 10. Steps

**Step 1 - pin libdevice and libmobile by tag.** `Cargo.toml`'s git
dependencies carry no tag or rev, in three places, so a fresh
resolution takes whatever their default branch holds with no commit
here recording it. Pin each to its current tag.

**Done when:** `cargo tree` shows both at a tag, `Cargo.lock` records
it in its own commit, and a clean checkout builds the same code twice.

**Step 2 - `Owned`, `OWNS` and `log_once!`.** The flags type, the const
defaulting to `Owned::empty()`, and the macro. No call site reads `OWNS`
yet.

**Done when:** every app and example builds and behaves exactly as
before, `cargo test -p libmorph` passes, and `Owned::FRAME | Owned::FRAME`
is usable in a `const`.

**Step 3 - `RenderStatus` and `render`'s default.** The signature, the
default body, and the shell honouring `Skipped`: no overlay, no present,
the texture dropped, the offscreen branch reporting alike.

**Done when:** the 17 drawing impls in this repo return `Drawn` and draw
what they did; the 8 headless ones return `Skipped` explicitly rather
than being deleted, so the default's log stays a real signal; an app with
no `render` logs once and presents nothing; and a scripted capture is
byte-identical to before.

**Step 4 - `wgpu_config`.** `Gpu::configure` takes it, calls it when
`OWNS` has `FRAME`, and overrides `width` and `height` after.
`Shell::start` and `Shell::attach` both reach it, so a resume, a lost
surface and a rotation all apply it.

**Done when:** a managed app's surface configuration is byte-identical
to today's, and an owning app that asks for a different `present_mode`
gets it or learns from `caps` why not.

**Step 5 - `construct_frame`, `acquire_frame` and `present_frame`.** `Target` reachable
from `MorphContext`; the branch on `A::OWNS` in `Shell::frame`; the
held-frame log.

**Done when:** an example with `const OWNS: Owned = Owned::FRAME;`
presents a cleared frame, skips by not acquiring, survives a rotation
and a backgrounding, and every managed app is untouched - and the
managed branch is the only one compiled into it.

**Step 6 - scripted runs under `Owned::FRAME`.** `acquire_frame` and
`present_frame` over the offscreen target.

**Done when:** one app under each setting produces identical bytes from
a scripted run.

**Step 7 - 0.7.0.** `Cargo.toml`'s version, a `VERSIONS.md` entry
listing the additions and marking `render`'s signature **Breaking**, and
the tag on that commit.

**Done when:** `git tag --points-at HEAD` shows `v0.7.0` on the version
commit, and the RPG game can move its `tag` to `v0.7.0` and build its
Android and desktop targets with no change but `render`'s return.
