---
title: The system's dark mode
date: 2026-10-24
status: Draft
covers:
  - "`MorphContext::dark_mode() -> Option<bool>` on every host"
  - "`MorphApp::on_dark_mode(&mut self, ctx, dark: bool)` when it changes"
  - "A `dark-mode` script command; the system bars' icon colour is left to
    a later plan"
sources:
  - "handlr (request, 2026-10-24, with the owner's \"yes\"): its themes
    plan's Appearance setting has System, Always light and Always dark;
    System needs the system's dark mode, and a change while running.
    Until then handlr treats System as dark. It suggested
    `ctx.dark_mode() -> Option<bool>` and `on_dark_mode`, and, separately,
    setting the system bars' icon colour for a light theme."
  - "libmobile docs/design/20261024_05_NIGHT_MODE_PLAN.md
    (`android::night_mode`)."
  - "winit 0.30: `Window::theme() -> Option<Theme>` (macOS, Windows, the
    web) and `WindowEvent::ThemeChanged(Theme)`; winit is libmorph's own
    on the desktop and the web (AGENTS.md, Platform Layering)."
related:
  - libmobile docs/design/20261024_05_NIGHT_MODE_PLAN.md
---

# The system's dark mode

## 1. API

```rust
impl MorphContext {
    /// Whether the system is in dark mode: Android's night mode, the
    /// desktop's or browser's appearance. None when the host does not say.
    pub fn dark_mode(&self) -> Option<bool>;
}

pub trait MorphApp {
    /// The system switched between light and dark while the app runs.
    /// Not called for the value at start; read `ctx.dark_mode()` in `init`.
    fn on_dark_mode(&mut self, ctx: &mut MorphContext, dark: bool) {
        let _ = (ctx, dark);
    }
}
```

- **Android:** read with libmobile's `night_mode` before `init` and again
  on each `ConfigChanged`; `on_dark_mode` is called when it changes to a
  defined value, suspended or not.
- **Desktop and the web:** winit's `Window::theme()` when the window is
  created, and `WindowEvent::ThemeChanged`. On macOS that follows the
  system appearance. Where winit gives `None` (some Linux setups),
  `dark_mode()` is `None`. The emulated device does not change this.
- **Scripted runs:** `None` at start, and a command `dark-mode <on|off>`
  that sets it and calls `on_dark_mode`, so apps can test both themes
  headlessly and take screenshots of each.
- **System bars:** setting the status and navigation bar icons light or
  dark is a separate, later plan (it needs a Kotlin call in libmobile).

## 2. Steps

1. **The wrapper**, after libmobile's release: the method, the hook, the
   Android, winit and scripted paths, tests (a scripted run switching
   modes, and the hook called only on a change).
   **Done when:** tests and clippy pass on every host; on the Mac,
   switching System Settings → Appearance logs the change in an example;
   on the owner's S23, `cmd uimode night yes` and `no` do the same
   (restored afterwards).
2. **Release** as the next libmorph version, on the owner's word.
   Additive.

## 3. Open questions for the owner

1. **A hook, or system events.** Recommended: the hook, which reaches
   the app on the main loop where it draws; a `SystemEvent` can be added
   later if a background thread needs it.
2. **The system bars.** Recommended: a later plan, written when handlr's
   light theme needs it.

## Amendment — 2026-10-24 15:37 EDT: the system bars follow the app's theme

The owner's direction on scope, relayed by the handlr session
(2026-10-24): the system bars "should follow our theme background and
titlebar colors", here rather than in a later plan. This replaces
section 1's "System bars" bullet and open question 2. Proposed for the
owner's approval.

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct SystemBars {
    /// Painted behind the status bar (the app's title bar colour). Linear
    /// RGBA 0..1, like wgpu::Color; alpha 0 paints nothing there.
    pub status: [f32; 4],
    /// Painted behind the navigation bar (the app's background).
    pub navigation: [f32; 4],
    /// Dark icons on the status bar (for a light colour behind it).
    pub status_icons_dark: bool,
    pub navigation_icons_dark: bool,
}
impl SystemBars { pub fn new(status: [f32; 4], navigation: [f32; 4]) -> Self; } // icons from each colour's luminance

impl MorphContext {
    /// Set the bars' colours and icons; kept until set again. Typically
    /// called from init and on_dark_mode, with the app's theme colours.
    pub fn set_system_bars(&mut self, bars: SystemBars);
}
```

- **Painting.** The app already draws behind the bars (edge-to-edge). After
  the app's `render`, libmorph fills the bar areas in the same frame: the
  top inset of the system bars with `status`, and the bottom, left or
  right inset (wherever the navigation bar is) with `navigation`, so it
  looks the same on every Android version. The areas come from
  libmobile's insets (the system bars, not the IME). Alpha blends over the
  app's pixels, so a translucent colour lets the app show through.
- **Icons.** libmobile's `set_system_bar_icons`. `SystemBars::new` picks
  dark icons for a colour whose relative luminance is above 0.5; the
  fields override it.
- **Desktop emulation.** The same bar areas, from the emulated device's
  insets, are painted with the same colours, so screenshots and the
  window show the bars as on the phone. The icons are not drawn. The web
  ignores `set_system_bars`.
- **Scripted runs** paint the bars too, so screenshots include them.
- Step 1 adds: on the S23, handlr-like colours (a dark title bar, a light
  background) show behind the bars with contrasting icons, in light and
  dark mode, with gestures and with 3-button navigation; a scripted
  screenshot shows the painted bars at the emulated insets.
