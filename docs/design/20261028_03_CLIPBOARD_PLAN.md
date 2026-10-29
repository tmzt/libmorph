---
title: The clipboard's text on every host
date: 2026-10-28
status: Draft
covers:
  - "`MorphContext::clipboard_text(&self) -> Option<String>` and
    `MorphContext::clipboard_has_text(&self) -> bool`"
  - "Android: libmobile's `clipboard` feature, through a passthrough
    `mobile-clipboard`; macOS: libdevice's `clipboard::read_text`; other
    desktops, the web and scripted runs: nothing"
  - "Read only: nothing here writes to the clipboard"
sources:
  - "flankr `docs/design/20261028_06_BOTTOM_BAR_UPDATE_PLAN.md` (Draft),
    decision 11.4: \"`MorphContext::clipboard_text(&self) ->
    Option<String>` in libmorph, the plain text of the clipboard's first
    item, read on the UI thread on the user's tap. Android: a new
    libmobile `clipboard` feature [...]; macOS: libdevice's
    `clipboard::read_text()`; elsewhere `None`.\" Paste text is disabled
    while the clipboard has no text (decision 8)."
  - "Owner (2026-10-28, through the flankr lead, verbatim): \"plan those
    in appropriate repos with subagent, then impl\"."
  - "AGENTS.md, Platform Layering: Android capabilities are built in
    libmobile first and released, desktop ones in libdevice; libmorph
    wraps both with one API."
  - "libmobile `docs/design/20261028_03_CLIPBOARD_PLAN.md`:
    `android::clipboard::read_text(&AndroidApp) ->
    Result<Option<String>, ClipboardError>` and `has_text`, feature
    `clipboard`, for libmobile 0.3.2."
  - "libdevice `docs/design/20261024_01_CLIPBOARD_PLAN.md` (In
    implementation): feature `clipboard`, `clipboard::read_text() ->
    Option<String>`, macOS through `NSPasteboard`, `None` on other
    desktops. Step 1 is libdevice `31cc51a`, not yet on GitHub and not yet
    released (libdevice is at 0.1.2)."
  - "libmorph today (0.5.0, `9ab1c3a`): no clipboard call; the text input
    plan's desktop Cmd+V names libdevice's `clipboard` but is not built."
related:
  - libmobile docs/design/20261028_03_CLIPBOARD_PLAN.md (the Android half)
  - libdevice docs/design/20261024_01_CLIPBOARD_PLAN.md (the macOS half)
  - docs/design/20261024_06_TEXT_INPUT_PLAN.md (its Cmd+V uses the same
    libdevice call; unchanged by this plan)
  - flankr docs/design/20261028_06_BOTTOM_BAR_UPDATE_PLAN.md (the user)
---

# The clipboard's text on every host

## 1. API

Two methods on `MorphContext`, on every host and with every feature set,
so signatures never change:

```rust
/// The clipboard's text, for the app's own Paste button: the plain text
/// of the clipboard's first item. `None` when it holds no text, on hosts
/// without a clipboard call, and when the platform will not give it.
///
/// Call it when the user asks to paste. On Android 12 and later the
/// system shows a "pasted from your clipboard" toast for text another app
/// copied; on Android 10 and later only the app in front (with window
/// focus) may read it. libmorph never logs the text.
pub fn clipboard_text(&self) -> Option<String>;

/// Whether the clipboard holds text, for enabling a Paste button. On
/// Android it reads only the clip's description, so no toast is shown.
pub fn clipboard_has_text(&self) -> bool;
```

| Host | `clipboard_text` | `clipboard_has_text` |
|---|---|---|
| Android, `mobile-clipboard` | libmobile's `android::clipboard::read_text`; an error is logged at debug and gives `None` | libmobile's `has_text`; an error gives `false` |
| Android, without it | `None` | `false` |
| macOS | libdevice's `clipboard::read_text()` | `clipboard_text().is_some()` (no toast on macOS) |
| Other desktops, the web | `None` | `false` |
| Scripted runs | `None` | `false` (a run never reads the host's clipboard) |

An empty string is `None` on every host. The error's reason
(`NotFocused`, `Platform`) is logged, not returned, as flankr's plan names
an `Option`; the log line names the reason, never the text or its length.

## 2. Features and dependencies

- `Cargo.toml`: `mobile-clipboard = ["mobile", "libmobile/clipboard"]`,
  with a comment, after `mobile-battery`.
- macOS: libdevice's macOS dependency line gains `"clipboard"` beside
  `"open-link"`, always on for macOS builds, as `open-link` is. It adds no
  crate: `objc2`, `objc2-foundation` and `objc2-app-kit` are already
  there for `open-link`.
- libmobile: the same git dependency, at the release with the `clipboard`
  feature (0.3.2); `Cargo.lock` moves to it.

## 3. Files

- `src/clipboard.rs` (new, private): `text(&MorphContext)` and
  `has_text(&MorphContext)` with one `platform` module per host, as
  `src/link.rs` does, and `non_empty(Option<String>) -> Option<String>`.
- `src/context.rs`: the two methods.
- `src/lib.rs`: `mod clipboard;`.
- `Cargo.toml`: the feature and libdevice's feature.
- `README.md` and `AGENTS.md`'s Core API: one line each (release step).

## 4. Versioning

Additive: two methods on `MorphContext` and a new feature. libmobile 0.3.1
to 0.3.2 is semver-compatible. libmorph 0.5.0 to **0.5.1**, tagged
`v0.5.1`, with a `VERSIONS.md` entry. libmorph-build does not change
(its libmobile moves in `Cargo.lock` only).

## 5. Steps

1. **The wrapper**, after libmobile 0.3.2 and libdevice's `clipboard` are
   on GitHub: the feature, the two methods on every host, the tests
   (scripted runs give `None` and `false`; `non_empty`; on hosts other
   than macOS without the feature, `None`).
   **Done when:** `cargo fmt -p libmorph -- --check`; `cargo clippy -p
   libmorph --all-targets` with default features and with `--features
   script,secure-store,mobile-clipboard`; `cargo clippy -p libmorph
   --target aarch64-linux-android --features mobile,mobile-clipboard` and
   with `mobile` alone: no new warnings; `cargo test -p libmorph` passes;
   on the Mac, a copied line comes back from `clipboard_text` in an
   example or a test the owner runs by hand.
2. **Release** as libmorph 0.5.1 on the owner's word: the README and
   AGENTS.md lines, the version, the `VERSIONS.md` entry, the tag.
   **Done when:** the commit is tagged `v0.5.1` and flankr's companion
   builds against it.

## 6. Risks

- **libdevice's release.** libdevice's `clipboard` exists only in the
  local checkout (`31cc51a`). Until it is on GitHub, a build from git
  fails on the unknown feature. If libdevice is not ready when libmorph
  is released, the macOS row falls back to `None` by dropping the feature
  and the macOS platform module (a recorded departure).
- **A reason lost.** `None` cannot tell "empty" from "not in front". The
  Paste button is shown only while the app is in front, so this is rare;
  a `Result` would be a later, breaking change.

## Amendment — 2026-10-28 23:43 EDT: approved

The owner approved planning and implementing the clipboard calls that
flankr's bottom bar plan needs, on 2026-10-28, through the flankr lead
(verbatim): "plan those in appropriate repos with subagent, then impl".
This plan is built with libmobile's `docs/design/20261028_03_CLIPBOARD_PLAN.md`,
libmobile first, for libmobile 0.3.2 and libmorph 0.5.1. The status stays
`Draft` until the first commit that implements it.
