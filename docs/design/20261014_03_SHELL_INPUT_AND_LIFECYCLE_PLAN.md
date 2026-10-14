---
title: Shell input and lifecycle additions (pressure, hover, keys, second finger, exit, data dir, surface format)
date: 2026-10-14
status: Draft
covers:
  - L1 `Touch::pressure`
  - L2 `MorphApp::on_hover`
  - L3 `MorphApp::on_key`, `Key`, `KeyStatus`
  - L4 a second mouse finger on desktop
  - L5 `MorphContext::exit` and a single `on_suspend` on every exit
  - L6 `MorphContext::data_dir`
  - L7 `MorphApp::SRGB_SURFACE`
  - One libmobile addition, `android::finish`, which L5 needs
sources:
  - Owner's request (2026-10-14), items L1–L7 with their Done when, "Ask me
    before changing any existing public signature", and "if any changes are
    needed in libdevice or libmobile tell me before implementing"
  - android-activity 0.6.1: `Pointer::pressure()` (input.rs),
    `Keycode::{Back, Escape}`, `KeyEvent::{action, key_code, repeat_count}`
    (native_activity/input.rs), `AndroidApp::internal_data_path()` and
    `activity_as_ptr()` (lib.rs). There is no finish method.
  - ndk 0.9 `native_activity::NativeActivity::{from_ptr, finish}`
    (`ANativeActivity_finish`)
  - winit 0.30.13: `WindowEvent::TouchpadPressure { pressure: f32, stage: i64 }`
    (macOS only), `KeyEvent { logical_key, state, repeat, .. }`,
    `Key::{Named, Character}`, `NamedKey::{Escape, BrowserBack, GoBack}`
  - libmobile 791b2ae `src/android/mod.rs`: `pump_input` returns the app's
    `InputStatus` to android-activity; the Kotlin activity does not override
    Back handling
related:
  - AGENTS.md "Core API", "Lifecycle", "Emulation & Input (desktop)"
    (step 9 updates them)
  - docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md (Hold): its
    section 3.4 relies on `on_suspend`/`on_resume`, and step 6 here makes
    those fire exactly once per transition
---

# Shell input and lifecycle additions

## 1. Aim

Seven additions to the shell, all additive: trait methods with default
bodies, new types, and new methods on `MorphContext`. The one change to an
existing public type is a new field on `Touch` (section 2), which the owner
decides on before step 2 starts. Hosts without a feature get `None`, a no-op
or an unchanged default.

## 2. Public API

```rust
// src/input.rs
pub struct Touch {
    pub id: u64,
    pub phase: TouchPhase,
    pub x: f32,
    pub y: f32,
    /// 0.0..=1.0 where the host reports it: Android, and macOS Force Touch
    /// while the left button is held. Otherwise `None`.
    pub pressure: Option<f32>,          // new (L1)
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Key { Character(char), Escape, Back }

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KeyStatus { Handled, Unhandled }

// src/app.rs, MorphApp
const SRGB_SURFACE: bool = true;                                        // L7
fn on_hover(&mut self, ctx: &mut MorphContext, x: f32, y: f32) {}      // L2
fn on_key(&mut self, ctx: &mut MorphContext, key: Key) -> KeyStatus {  // L3
    KeyStatus::Unhandled
}

// src/context.rs, MorphContext
pub fn exit(&mut self);                                                 // L5
pub fn data_dir(&self) -> Option<&Path>;                                // L6
```

Adding `pressure` to `Touch` breaks code outside the crate that builds a
`Touch` with a struct literal. Code that only reads touches, like the touch
example, is unaffected. Adding a field is the only way to provide L1 as
specified, so this is the owner's decision.

## 3. Desktop and web input (`src/input.rs`, `src/runner/winit.rs`)

`MouseTouch` tracks the left and right buttons, the cursor position and the
latest pressure, and returns zero or more `MouseOutput`s per event:

```rust
pub(crate) enum MouseOutput { Touch(Touch), Hover(f32, f32) }
```

| Event | Left up | Left held, right up | Left and right held |
|---|---|---|---|
| cursor moves | `Hover` (only if right is also up) | primary `Moved` | second `Moved`; the primary stays where it was |
| left press | primary `Started` | — | — |
| left release | — | primary `Ended` | primary `Ended`; the second carries on |
| right press | nothing | second `Started` at the cursor | — |
| right release | second `Ended`, if one is active | — | second `Ended`; the primary is unfrozen and moves with the cursor again |
| `TouchpadPressure` | stored | stored; primary `Moved` with the new pressure | stored |
| cursor leaves | — | primary `Cancelled` | both `Cancelled` |

- Ids: primary `MouseTouch::ID` (`u64::MAX`), second `MouseTouch::ID - 1`.
- While the second finger is active, the primary is frozen at its position
  when the right button was pressed. One finger holds; the cursor drives the
  other, so a rotate gesture has a fixed pivot. If both followed the cursor
  they would sit on the same point.
- Pressure: the latest `TouchpadPressure.pressure`, cleared to `None` on each
  left press until the next reading. Only the primary carries it; the second
  touch's is `None`. The table has the primary emit `Moved` on a new reading,
  so a press-harder-without-moving gesture is visible.
- Web: the same code runs. Browsers send no `TouchpadPressure`, so pressure
  stays `None`. Real touches from `WindowEvent::Touch` get
  `pressure: touch.force.map(normalized)`, where winit reports it.

Keys: on `WindowEvent::KeyboardInput` with `state == Pressed` and
`!repeat`, `key_from_winit(&event.logical_key)` maps:

| winit | `Key` |
|---|---|
| `Key::Character(s)` with exactly one `char` | `Character(c)` |
| `Key::Named(NamedKey::Escape)` | `Escape` |
| `Key::Named(NamedKey::BrowserBack \| NamedKey::GoBack)` | `Back` |
| anything else | not delivered |

The returned `KeyStatus` is ignored; winit takes no default action on keys.

## 4. Android input (`src/runner/android.rs`)

- L1: every `Touch` built from a `MotionEvent` pointer carries
  `Some(pointer.pressure())`.
- L3: `InputEvent::KeyEvent(key)`:
  - `Keycode::Back` maps to `Key::Back` and `Keycode::Escape` to
    `Key::Escape`. Other keycodes produce no `Key` and return
    `InputStatus::Unhandled`, as today. Characters from hardware keyboards
    are left for later: typed text already arrives through libmobile's IME
    bridge.
  - `KeyAction::Down` with `repeat_count() == 0` calls `on_key`, and the
    `KeyStatus` becomes the event's `InputStatus`.
  - Android acts on Back when the key comes **up**, so the status given on
    the press is remembered per keycode (`KeyRouting`, below). The repeats and
    the `Up` of that key return the same status. Otherwise a handled Back
    would still close the activity on release.

  ```rust
  /// Platform-free; host-tested.
  pub(crate) struct KeyRouting { last: [(Key, KeyStatus); 2] /* Back, Escape */ }
  impl KeyRouting {
      fn pressed(&mut self, key: Key, status: KeyStatus) -> KeyStatus;
      fn followup(&self, key: Key) -> KeyStatus;   // repeats and Up
  }
  ```

## 5. Exit and suspend (L5)

- `Shell` gains `suspended: bool`. `detach` calls `on_suspend` only when
  `!suspended`, then sets it. `attach` calls `on_resume` only when
  `suspended`, then clears it. Today, a lost surface that is recreated
  doesn't count as a suspension; it still won't.
- `Shell::shutdown(self)`: calls `on_suspend` if `!suspended`, then drops the
  surface, then the app. Every exit path ends here, so `on_suspend` runs
  exactly once per exit, before anything is dropped. An app suspended by
  `WindowDestroyed` is not told twice when `Destroy` follows.
- `MorphContext::exit` sets `exit_requested`. Each runner checks the flag
  after every call into the app.
- Desktop and web: `CloseRequested`, the flag, and winit's `exiting` (e.g.
  Cmd-Q) all call `shutdown`, drop the window, then `event_loop.exit()`.
- Android: the flag calls `libmobile::android::finish(app)` once (section 7).
  The activity then runs its normal teardown (`WindowDestroyed` → `on_suspend`,
  `Destroy`), and `MobileEvent::Destroy` calls `shutdown`, which does not
  suspend again. If `Destroy` arrives first for any reason, `shutdown` still
  suspends exactly once.

## 6. Data directory and surface format

- L6: `MorphContext` gains `data_dir: Option<PathBuf>`. On Android it is
  `app.internal_data_path()`, read at bring-up; elsewhere `None`.
- L7: `Gpu::configure(surface, size, previous, srgb: bool)`. The preferred
  format is the first one in the capabilities with `is_srgb() == srgb`, else
  the adapter default. A `previous` format the surface still offers wins over
  the preference, so resume keeps the same format, as today. `Shell` passes
  `A::SRGB_SURFACE`. The desktop overlay is unaffected: it blends black
  coverage, which is the same in either encoding.

## 7. libmobile: `android::finish`

android-activity 0.6 cannot finish the activity. libmorph could do it itself
through libmobile's re-exported `ndk` (`NativeActivity::from_ptr` on
`app.activity_as_ptr()`, then `finish()`), but that is an unsafe raw-pointer
call. libmobile already keeps that kind of platform call behind safe
functions, each with its own scoped `#[allow(unsafe_code)]`. So:

```rust
// libmobile src/android/mod.rs
/// Ask the activity to finish (`ANativeActivity_finish`): the normal
/// teardown follows (`Pause`, `Stop`, `WindowDestroyed`, `Destroy`).
pub fn finish(app: &AndroidApp);
```

It needs no new dependency, since ndk's `native_activity` module is not
feature-gated, and nothing changes in libdevice.

## 8. Example

`examples/touch.rs`:
- `on_touch` logs every `Touch`, including `pressure` and the second
  finger's id;
- `on_hover` logs at `debug` level, so the default `info` filter isn't
  flooded; `RUST_LOG=debug` shows it;
- `on_key` logs the key and returns `KeyStatus::Unhandled`.

## 9. Steps

Each step is one commit and passes `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets` and `cargo test --workspace`, with
default features and with `--no-default-features`, plus
`cargo check --target aarch64-linux-android --features mobile` (Rule 3.4).

1. **libmobile: `android::finish`** (section 7). This happens in the libmobile
   repository and is pushed before step 6.
   **Done when:** libmobile's clippy for `aarch64-linux-android
   --all-features` is clean and its tests pass.
2. **API surface** (section 2): the types, the trait methods with defaults,
   `exit`/`data_dir` storing their values, and `Touch::pressure: None`
   everywhere.
   **Done when:** the step's checks pass, and the touch example builds
   unchanged apart from nothing; it doesn't construct `Touch`.
3. **Desktop and web pointer input** (section 3: hover, second finger,
   pressure), with `MouseTouch` unit tests for every row of the table.
   **Done when:** those tests pass, and the step's checks pass for desktop and
   `wasm32 --features web`.
4. **Keys on desktop and web** (section 3 keys), with tests for
   `key_from_winit`.
   **Done when:** the mapping tests pass.
5. **Android input** (section 4), with host tests for `KeyRouting` (press,
   repeat, release, other keys).
   **Done when:** the routing tests pass and the Android check passes.
6. **Exit and suspend** (section 5), then **data dir** (section 6), in that
   order in one commit, with `Cargo.lock` re-resolved to the libmobile commit
   from step 1 and no local `[patch]`.
   **Done when:** a host test drives a `Shell` with a test app through
   suspend, resume, exit and exit-after-suspend, and counts exactly one
   `on_suspend` per exit. This test needs a GPU, so it is `#[ignore]` and the
   owner runs it on the Mac.
7. **Surface format** (section 6, L7).
   **Done when:** a unit test of the format choice (a pure function over a
   list of formats) covers both values of `srgb`, with and without
   `previous`.
8. **Touch example** (section 8).
   **Done when:** it builds, and it returns `KeyStatus::Unhandled` for every
   key. The owner checks the logs for pressure, hover, keys and the second
   finger on the Mac; agent sandboxes can't open windows.
9. **AGENTS.md** (its own commit): Core API, Lifecycle and Emulation & Input
   updated.
   **Done when:** AGENTS.md lists the new hooks, `Key`, `KeyStatus`, exit,
   data dir and `SRGB_SURFACE`.

Then the commits are pushed to github.com/tmzt/libmorph.

## Amendment — 2026-10-14 13:01 EDT: L4 removed; native trackpad touches get their own plan

The owner decided (2026-10-14) that mouse buttons should not emulate a
second finger. Desktop click-to-touch is a single-finger convenience.
Multi-finger input on the Mac should come from the trackpad itself, and that
will be planned on its own. `Touch`, with its ids and phases, already
carries it. (winit 0.30 reports no per-finger trackpad touches on macOS, only
pinch and rotation gestures, so it needs its own design and a trial.)

This replaces:

- **covers:** "L4 a second mouse finger on desktop" is removed.
- **Section 3:** `MouseTouch` tracks only the left button, the cursor
  position and the latest pressure. Every table row mentioning the right
  button or a second touch is removed, along with the note on freezing the
  primary and the id `MouseTouch::ID - 1`. What remains:

  | Event | Left up | Left held |
  |---|---|---|
  | cursor moves | `Hover` | `Moved` |
  | left press | `Started` | — |
  | left release | — | `Ended` |
  | `TouchpadPressure` | stored | stored; `Moved` with the new pressure |
  | cursor leaves | — | `Cancelled` |

  Hover fires whenever the left button is up; other buttons are ignored.
- **Section 8:** the example does not log a second finger.
- **Section 9, step 3:** "hover, second finger, pressure" becomes "hover,
  pressure". Its unit tests cover the rows of the table above.
- **Section 9, step 8:** the owner's Mac check covers pressure, hover and
  keys.

The owner's original Done when, "The touch example logs pressure, hover,
keys and the second finger", no longer includes the second finger.
