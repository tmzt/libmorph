---
title: Mac trackpad fingers as touches
date: 2026-10-15
status: In implementation
covers:
  - Reading the fingers on a Mac trackpad (AppKit `NSTouch`) in the desktop
    runner and delivering them as `Touch` events with stable ids, as Android does
  - Mapping trackpad positions onto the emulated screen at physical scale,
    anchored at the cursor, using the device's pixel density from libdevice
  - When trackpad touches are reported, and how they coexist with the
    single-finger mouse touch
  - A `--morph-trackpad <physical|fit|off>` option
sources:
  - Owner's request (2026-10-15), relayed by the Noble's Keep session: "the
    touchpad/mobile emulation should be added to libdevice and libmorph ...
    it should emulate the same behavior as mobile using the desktop
    multi-finger support on Mac"
  - Plan 20261014_03, amendment of 2026-10-14 13:01 EDT: mouse buttons do
    not emulate a second finger; native trackpad touches get their own plan
  - Split agreed with the libdevice session (2026-10-15): libdevice holds
    only facts about the emulated phone (`ppi`, `px_per_mm`), libmorph holds
    everything about host input
  - winit 0.30.13 `src/platform_impl/macos/view.rs`: the view implements
    `magnifyWithEvent:`, `rotateWithEvent:` and `pressureChangeWithEvent:`,
    but no `touchesBegan/Moved/Ended/CancelledWithEvent:`. winit reports no
    per-finger trackpad touches on macOS.
  - objc2-app-kit 0.2.2 (the version winit already uses):
    - `NSView::setAllowedTouchTypes(NSTouchTypeMask)`;
    - `NSEvent::touchesMatchingPhase_inView`;
    - `NSEvent::addLocalMonitorForEventsMatchingMask_handler`;
    - `NSTouch`: `identity`, `phase`, `normalizedPosition` (0..1, origin
      bottom left), `isResting`, `deviceSize` (points, 1/72 inch).
  - Consumer: Noble's Keep `docs/design/20261015_01_INITIAL_UI_PLAN.md`,
    sections 3.1, 3.3 and 7. It needs two-finger pinch zoom, and pixel
    density for an offset tool drawn 3 mm above a 9 mm touch circle.
related:
  - libdevice docs/design/20261015_01_DEVICE_PIXEL_DENSITY_PLAN.md (being
    written by the libdevice session): `DeviceSpec::ppi` and `px_per_mm()`.
    This plan depends on it for the physical mapping.
  - docs/design/20261014_03_SHELL_INPUT_AND_LIFECYCLE_PLAN.md (`Touch`,
    `MouseTouch`)
  - docs/design/20261014_04_SCRIPTED_RUNS_AND_PNG_CAPTURE_PLAN.md (scripted
    touches remain the headless way to test multi-finger input)
---

# Mac trackpad fingers as touches

## 1. Goal

On macOS desktop, the fingers on the trackpad become `Touch` events: each
finger has its own id and its own `Started`, `Moved`, then `Ended` or
`Cancelled`. This is the stream Android produces. An app's two-finger pinch
and pan code then runs unchanged on the phone and in desktop emulation.

Nothing changes on Android or the web, or in scripted runs. No public type
changes: `Touch`, `TouchPhase` and `MorphApp::on_touch` already carry
several fingers.

## 2. Reading the fingers

AppKit sends trackpad fingers only to a view that allows indirect touches.
libmorph:

1. after the window is created, gets its `NSView` from the window's
   raw-window-handle (`RawWindowHandle::AppKit`, `ns_view`) and calls
   `setAllowedTouchTypes(NSTouchTypeMask::Indirect)`. Resting touches stay
   off (`wantsRestingTouches` is false by default), so a thumb resting on
   the pad is not a finger;
2. installs a local event monitor
   (`NSEvent::addLocalMonitorForEventsMatchingMask_handler`) for
   `Gesture | Magnify | Rotate | Swipe | BeginGesture | EndGesture |
   ScrollWheel`. The handler returns the event unchanged, so winit still sees
   it. For each event, it calls `touchesMatchingPhase_inView(NSTouchPhase::Any,
   view)` and pushes one `PadFrame` (section 5) into a queue shared with the
   runner;
3. removes the monitor (`NSEvent::removeMonitor`) when the window is
   dropped.

The runner drains the queue in `about_to_wait` and before each redraw, so
fingers reach the app no later than the next frame.

Step 1 is a trial. It confirms that the monitor sees per-finger touches on
winit's view, and which event types carry them. If it does not, the fallback
is to add the four `touches…WithEvent:` methods to winit's view class at
run time (`class_addMethod`). winit's view does not define them, so nothing
is replaced. The fallback is recorded in CHANGES.md if it is needed.

All Objective-C calls are in one module, `src/runner/trackpad.rs`, under
`#[cfg(target_os = "macos")]`. Each `unsafe` block has a safety comment.

New macOS-only dependencies, at the versions winit 0.30.13 already locks, so
nothing is built twice:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.5.2"
objc2-foundation = { version = "0.2.2", features = ["NSSet", "NSGeometry"] }
objc2-app-kit = { version = "0.2.2", features = ["NSEvent", "NSTouch", "NSView", "NSResponder"] }
block2 = "0.5.1"
raw-window-handle = "0.6"
```

## 3. When fingers are reported

A trackpad behaves differently from a screen: one finger on it moves the
cursor, and a click is a press. So:

- **One finger** stays the mouse. Its click is the single-finger touch
  (`MouseTouch`, id `u64::MAX`), and its movement is hover, exactly as
  today.
- **A trackpad session** starts when two or more non-resting fingers are on
  the pad and the cursor is inside the surface. In that frame, every finger
  on the pad gets `Started`, in the order AppKit lists them.
- During a session, every finger is reported, including one that lands
  later (`Started`) and the last one left after the others lift. That
  matches a phone, where lifting one finger of a pinch leaves the other
  touching.
- The session ends when its last finger lifts. A finger AppKit reports as
  cancelled gets `Cancelled`. Losing window focus (`Focused(false)`) and
  `suspended` cancel every finger still down.

While a session is active, the mouse:

- reports no hover, and ignores left presses. macOS's three-finger drag
  turns three fingers into a left drag, so ignoring presses keeps that from
  arriving twice;
- if its touch was held when the session started, cancels that touch once
  (`Cancelled`, then nothing until the left button is released). This is the
  existing rule for leaving the window.

Fingers have no pressure: `pressure` is `None`. `NSTouch` reports no force.

## 4. Mapping onto the emulated screen

`PadMapping` is chosen with `--morph-trackpad`:

- **`physical`** (default). Distances on the pad equal distances on the
  emulated phone. At session start, the anchor is the cursor position and
  the origin is the centroid of the starting fingers. Each finger is placed
  at

  ```
  device px per pad unit = pad size in points × (25.4 / 72) × spec.px_per_mm()
  x = anchor.x + (finger.x − origin.x) × pad_width_pt  × 25.4/72 × px_per_mm × surface_w / spec.width
  y = anchor.y + (finger.y − origin.y) × pad_height_pt × 25.4/72 × px_per_mm × surface_h / spec.height
  ```

  `finger.y` is flipped to grow downwards (`1 − normalizedPosition.y`). The
  last factor converts device pixels to surface pixels when the window
  manager did not grant the full device size. `emulated_safe_area` already
  uses that same ratio. The fingers' centroid starts under the cursor, and
  a 30 mm pinch on the pad is a 30 mm pinch on the phone. For a Galaxy S24
  (416 ppi), that is 491 device pixels.
- **`fit`**. The whole pad maps to the whole surface:
  `x = finger.x × surface_w`, `y = finger.y × surface_h`. There is no
  anchor. This is the fallback while libdevice has no `ppi`.
- **`off`**: no monitor is installed, and nothing changes from today.

Positions are clamped to `0..=surface − 1` on each axis. Touches are not
cancelled at the edge, because the fingers are still on the pad.

The anchor stays fixed for the whole session. macOS does not move the
cursor during two-finger gestures, and a single finger left at the end of a
session moves the touch, not a new anchor.

## 5. Types

These are all crate-private, in `src/input.rs`, which builds and is tested
on every host. Only `src/runner/trackpad.rs` is macOS-only.

```rust
/// One finger on the pad: AppKit's identity turned into a key, and its
/// position, 0..1 from the pad's top left.
pub(crate) struct PadFinger { pub key: u64, pub x: f32, pub y: f32 }

/// Every finger on the pad after one AppKit event.
pub(crate) struct PadFrame {
    pub fingers: Vec<PadFinger>,
    /// Fingers AppKit reported as cancelled in this event.
    pub cancelled: Vec<u64>,
    /// `NSTouch::deviceSize`, in points (1/72 inch).
    pub pad_size: (f32, f32),
}

pub(crate) enum PadMapping { Physical { px_per_mm: f32 }, Fit }

/// Turns pad frames into touches (section 3), placed by `PadMapping`.
pub(crate) struct PadTouch { /* mapping, session, next id */ }

impl PadTouch {
    pub fn new(mapping: PadMapping) -> Self;
    /// `cursor` is the last cursor position inside the surface, if any;
    /// `scale` is surface size / device size.
    pub fn frame(&mut self, frame: &PadFrame, cursor: Option<(f32, f32)>,
                 surface: (u32, u32), scale: (f32, f32)) -> Vec<Touch>;
    /// Cancels every finger still down (focus lost, suspended).
    pub fn cancel(&mut self) -> Vec<Touch>;
    /// A session is active: the mouse is held off (section 3).
    pub fn active(&self) -> bool;
}
```

`MouseTouch` gets one new method, `interrupt(&mut self) -> Option<MouseOutput>`.
It cancels a held touch, as leaving the window does.

**Ids.** `src/runner/trackpad.rs` keeps the `identity` objects of the
fingers on the pad, compares them with `isEqual`, and gives each new one the
next `key`. `PadTouch` gives each finger in a session the next id from
`PadTouch::ID_BASE = 1 << 32`, counting up. These ids are kept apart from
`MouseTouch::ID` and from winit touchscreen ids, which count from zero.

## 6. Option

`--morph-trackpad <physical|fit|off>` (also `--morph-trackpad=…`) is
stripped with the other `--morph-*` options. Any other value is
`MorphError::InvalidOption`. On other hosts, the option is accepted and has
no effect, so one command line works everywhere. It is added to libmorph's
help text.

## 7. libdevice dependency

`physical` needs `DeviceSpec::px_per_mm()` from libdevice's density plan.
Until a libdevice commit with it is published and pinned in `Cargo.lock`,
this plan's steps 1–4 build with `fit` as the only mapping. Step 5 adds
`physical` and makes it the default.

## 8. Open questions for the owner

1. **Session start.** Recommended: two or more fingers start a session, and
   one finger stays the mouse. The alternative reports every finger,
   including the first, as a touch. A single finger resting on the pad would
   then be a held touch, and hover would be gone.
2. **Mouse touch held when a session starts.** Recommended: cancel it. The
   alternative lets it join the session as one of the fingers, which needs
   the mouse touch to move under pad control.
3. **Default mapping.** Recommended: `physical`, anchored at the cursor.
   `fit` is simpler, but its scale depends on the trackpad's size.
4. **Android density.** Noble's Keep also needs millimetres on the phone.
   On Android, `ctx.device()` is the CLI or default spec, so its `ppi` is
   the emulated device's, not the real one. Reporting the real `xdpi` needs
   `DisplayMetrics` through libmobile, which should be a separate plan.
   Recommended: leave it out of this one.

## 9. Steps

1. **Trial.** In `src/runner/trackpad.rs`: allow indirect touches on the
   view, install the monitor, and log each frame's fingers (`log::debug!`)
   with no touches delivered. Record which event types carry touches.
   **Done when:** running the touch example on the owner's Mac with
   `RUST_LOG=libmorph=debug` logs two fingers with stable keys through a
   pinch, and the result (or the switch to the fallback in section 2) is in
   CHANGES.md.
2. **`PadTouch` and `fit`.** The types in section 5, with unit tests: a
   session starts at two fingers; a late finger joins; the last finger
   keeps reporting; it ends on the last lift; AppKit cancels; `cancel()`;
   ids from `ID_BASE`; clamping; and `fit` positions.
   **Done when:** `cargo test` passes on all hosts.
3. **Runner wiring.** The queue, draining in `about_to_wait` and before
   redraw, `Focused(false)` and `suspended`, and the mouse held off with
   `MouseTouch::interrupt` (unit-tested).
   **Done when:** on the owner's Mac, a two-finger pinch in the touch
   example logs two touches with distinct ids, `Started` to `Ended`, and one
   finger still clicks and hovers as before.
4. **Option.** `--morph-trackpad` parsing, with `fit` and `off`, plus help
   text and tests.
   **Done when:** `--morph-trackpad=off` restores today's behaviour, and a
   bad value is `InvalidOption`.
5. **`physical`.** Bump the libdevice pin, and add the physical mapping
   with tests: a 30 mm spread on a 160 × 100 mm pad lands 491 device pixels
   apart on a Galaxy S24; the centroid starts at the cursor; scaled
   surfaces. Make it the default.
   **Done when:** the tests pass, and on the owner's Mac a pinch zoom in
   an example tracks the fingers at phone scale.
6. **Docs.** Update the "Emulation & Input" section in AGENTS.md and the
   README.
   **Done when:** both describe trackpad touches and `--morph-trackpad`.

## Amendment — 2026-10-15 23:35 EDT: Approval, answers, and fitting the window to Retina displays

The owner approved the plan for implementation (2026-10-15), and accepted the
recommendations in section 8:

1. A session starts at two or more fingers; one finger stays the mouse.
2. A mouse touch that is held when a session starts is cancelled.
3. `physical`, anchored at the cursor, is the default mapping.
4. Android's real display density is left to a separate libmobile plan.

The owner also asked, in the same instruction, for the mapping between the
host's physical pixels and the device's pixels to be fixed on Retina
displays: "Opening tinytrowel earlier was larger than the screen." The owner
suggested the pixel density plan. That plan is libdevice's and is data only,
and window size is host behaviour, so the work is added here as a new
section 10 and step 7. It needs nothing from libdevice.

### 10. Fitting the window to the display

Today the desktop window is created at `spec.geometry` in host physical
pixels. A Galaxy S24 (1080 × 2340) on a 14-inch MacBook Pro (3024 × 1964
physical pixels, scale factor 2) is taller than the display.

- **Window size.** Before the window is created, the runner takes
  `event_loop.primary_monitor()` and its physical size `(mw, mh)`, and
  computes `fit = min(1, 0.9 × mw / w, 0.85 × mh / h)` for the device
  size `(w, h)`. The margins leave room for the menu bar, Dock and title
  bar. The window is requested at `(round(w × fit), round(h × fit))`
  physical pixels. With no monitor, `fit` is 1. The S24 on the 14-inch
  MacBook Pro gets `fit` 0.713, a 770 × 1669 window (385 × 834 points),
  which is drawn at full Retina resolution.
- **The device's pixels on a smaller surface.** The surface keeps the size
  the window got, as it does today when the window manager shrinks it. The
  safe area and the overlay already scale by `surface / spec.geometry`.
  `MorphContext::scale_factor()` is now scaled the same way: it is
  `spec.pixel_ratio × surface_width / spec.geometry.width`, so a layout in
  dp keeps the phone's proportions. `ctx.device_spec()` still reports the
  emulated device itself. The `physical` trackpad mapping (section 4)
  already includes that ratio.
- **Millimetres.** Step 5 adds `MorphContext::px_per_mm() -> Option<f32>`:
  libdevice's `px_per_mm()` scaled by surface width / device width on
  desktop. It is the spec's own value on Android and the web, and `None`
  until the libdevice pin has `ppi`. This is a new method, not a change to
  an existing signature.
- **`--morph-window-scale <fit|factor>`**. The default is `fit`. A number
  such as `1` or `0.5` sets the window to that multiple of the device size
  exactly; `1` gives today's behaviour. A value that does not parse, or is
  not above 0, is `InvalidOption`. It is ignored on the web, as are scripted
  runs, which always render at `spec.geometry`.

Showing the phone at its real size on screen would need the display's
physical size (`CGDisplayScreenSize`). It is not part of this plan.

Step added:

7. **Window fit.** The fit computation, as a pure function with tests (the
   S24 on 3024 × 1964 gives 770 × 1669; a small device is not enlarged; no
   monitor gives 1), plus `--morph-window-scale` and the scaled
   `scale_factor`.
   **Done when:** the touch example opens on the owner's MacBook Pro fully
   on screen at the default device, and
   `--morph-window-scale 1` opens it at device size as before.

Step 6 also describes `--morph-window-scale`.

## Amendment — 2026-10-15 23:40 EDT: Implementation started

Implementation has started. This document is now immutable, and changes
only on the owner's explicit instruction.
