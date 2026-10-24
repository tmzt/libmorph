---
title: Text input and the soft keyboard
date: 2026-10-24
status: Draft
covers:
  - "`MorphContext::start_text_input(value, &TextInputOptions)` and
    `stop_text_input()`"
  - "`MorphApp::on_text_input(&mut self, ctx, &TextInputEvent)`:
    `Changed(String)`, `Action`, `Dismissed`"
  - "`MorphContext::keyboard_inset()`: the keyboard's height"
  - "The desktop: winit's IME and keys, with paste through libdevice; the
    script commands"
sources:
  - "handlr (request, 2026-10-24, with the owner's \"yes\", from the handlr
    session): `ctx.text_input(Option<&str>)` with options (single or
    multiple lines, text, URL or number, Done or Send, autocorrect); a hook
    with `Changed(String)` (the full text, composition included), `Action`
    and `Dismissed`; the keyboard's height through the insets; and the
    same events on the desktop from winit's IME and keys, with Backspace
    and paste, so one code path serves both."
  - "libmobile docs/design/20261024_06_TEXT_INPUT_OPTIONS_PLAN.md
    (options, `TextAction`, `TextInputDismissed`) and today's
    `set_text_input`, `TextChanged` and `Insets::ime_bottom`, none of
    which libmorph passes on."
  - "libdevice docs/design/20261024_01_CLIPBOARD_PLAN.md (`read_text`)."
  - "winit 0.30: `Window::set_ime_allowed`, `set_ime_cursor_area`,
    `WindowEvent::Ime(Preedit | Commit | Enabled | Disabled)`,
    `KeyboardInput` with `logical_key` and `text`, `ModifiersChanged`."
related:
  - libmobile docs/design/20261024_06_TEXT_INPUT_OPTIONS_PLAN.md
  - libdevice docs/design/20261024_01_CLIPBOARD_PLAN.md
---

# Text input and the soft keyboard

## 1. API

```rust
impl MorphContext {
    /// Start editing `value`: on Android the soft keyboard opens; on the
    /// desktop typed text and the IME are routed to the app. Calling it
    /// again replaces the value and options.
    pub fn start_text_input(&mut self, value: &str, options: &TextInputOptions);
    /// Stop editing; the keyboard closes. No event follows.
    pub fn stop_text_input(&mut self);
    pub fn text_input_active(&self) -> bool;
    /// The keyboard's height over the bottom of the surface, in physical
    /// pixels; 0 when it is closed or there is none. Keep the field above it.
    pub fn keyboard_inset(&self) -> f32;
}

pub trait MorphApp {
    fn on_text_input(&mut self, ctx: &mut MorphContext, event: &TextInputEvent) {
        let _ = (ctx, event);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextInputEvent {
    /// The whole text after an edit, composition included.
    Changed(String),
    /// The action key (Done, Send, Go or Search; Enter on a single line).
    Action(TextAction),
    /// The keyboard was closed by the user or the system while editing.
    Dismissed,
}
```

`TextInputOptions`, `TextKind` and `TextAction` are libmorph's own, as in
libmobile's plan, mapped to libmobile's on Android.

- **Android:** libmobile's `set_text_input_with`, `TextChanged`,
  `TextAction` and `TextInputDismissed`; `keyboard_inset()` is
  `Insets::ime_bottom`. Events reach the app after `init`, suspended or
  not.
- **Desktop:** libmorph keeps the text and a caret at its end.
  - Typed text (`KeyboardInput`'s `text`) and the IME's `Commit` are
    inserted; `Preedit` is shown as part of the text in `Changed` until it
    is committed or cleared, so composition is visible.
  - Backspace deletes the last character (a grapheme cluster, by the
    `unicode-segmentation` crate) or the preedit's; Enter is
    `Action` on a single line, a newline on multiple lines with
    `TextAction::None`, and `Action` otherwise (Shift+Enter is a newline
    there); Cmd+V (Ctrl+V elsewhere) inserts libdevice's
    `clipboard::read_text()` (its `clipboard` feature, on for macOS
    builds, as `open-link` is).
  - Caret movement, selection and copy are not in this plan.
  - `set_ime_allowed(true)` while editing, with the IME's candidate
    window at the bottom of the surface (`set_ime_cursor_area`).
  - Keys typed while editing do not also reach `on_key`; Escape ends
    editing and sends `Dismissed`.
  - `keyboard_inset()` is 0.
- **The web:** the same as the desktop, through winit's web backend;
  paste is not available (no clipboard read without a user gesture), and
  is left out.
- **Scripted runs:** `type <text>` (the rest of the line, verbatim)
  inserts text, `text-action` sends `Action`, `text-dismiss` sends
  `Dismissed`; they require editing to be active, which the app starts.

## 2. Steps

1. **The wrapper**, after libmobile's and libdevice's releases: the
   types, methods, hook, the Android, desktop and scripted paths, tests
   (the desktop editing model: insert, preedit, Backspace over a
   grapheme cluster, Enter by options, paste; scripted typing).
   **Done when:** tests and clippy pass on every host; on the Mac an
   example's field takes typed text, an IME composition (Japanese or
   emoji picker), Backspace, Enter and Cmd+V; on the owner's S23 the same
   example's field takes the keyboard's text, Send and Back.
2. **Release** as the next libmorph version, on the owner's word.
   Additive.

## 3. Open questions for the owner

1. **Desktop editing scope.** Text at the end only, no caret movement or
   selection, as handlr's uses need (replies, a code, search).
   Recommended: yes; a caret is a later amendment if needed.
2. **Paste through libdevice.** A small `clipboard` feature, macOS only.
   Recommended: yes, following the layering rule.
3. **`unicode-segmentation`** for Backspace on the desktop. Recommended:
   yes; a small, dependency-free crate.

## Amendment — 2026-10-24 16:18 EDT: approved

The owner approved the text input plans in libmobile, libdevice and
libmorph on 2026-10-24 ("yes"), with libmorph's recommended answers: on
the desktop, text is edited at its end only (no caret movement or
selection); paste through libdevice's `clipboard`; `unicode-segmentation`
for Backspace. They are built after the notification and dark mode work
and released after it (libmobile 0.2.7, the next libdevice release,
libmorph 0.4.4), so they do not hold it up. The status stays `Draft`
until the first commit that implements it.
