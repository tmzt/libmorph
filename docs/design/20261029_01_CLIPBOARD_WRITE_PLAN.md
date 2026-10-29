---
title: Writing text to the clipboard on every host
date: 2026-10-29
status: Draft
covers:
  - "`MorphContext::set_clipboard_text(&self, text: &str) ->
    Result<ClipboardWrite, ClipboardError>`, which tells the app whether
    the system already confirmed the copy, and
    `MorphContext::clipboard_can_write(&self) -> bool`"
  - "`libmorph::ClipboardWrite` (`SystemNotice`, `Silent`) and
    `libmorph::ClipboardError` (`Unsupported`, `Platform(String)`)"
  - "Android: libmobile's `android::clipboard::write_text` through the
    existing `mobile-clipboard`; macOS: libdevice's
    `clipboard::write_text`; scripted runs: logged, never the host's
    clipboard; other desktops and the web: `Unsupported`"
  - "RoundTwo as the first user"
sources:
  - "Owner (2026-10-29, in the RoundTwo session, verbatim): \"also add a
    copy button (copy icon if available or Copy in 0.8 size) to the
    question and transcript in the review card\"."
  - "Owner (2026-10-29, in the RoundTwo session), asked how to proceed since
    no library writes the clipboard, chose: \"Plan it in the libraries:
    write a plan in libmobile for writing the clipboard (Android's
    ClipboardManager), then one in libmorph for ctx.set_clipboard_text
    (Android and macOS), each under its own rules and version. Then move
    RoundTwo's libmorph pin and add the buttons.\""
  - "`docs/design/20261028_03_CLIPBOARD_PLAN.md` (In implementation):
    `clipboard_text` and `clipboard_has_text`, \"Read only: nothing here
    writes to the clipboard\"; `None` and `false` on other desktops, the
    web and in scripted runs (\"a run never reads the host's clipboard\").
    That plan is frozen, so the write is this new plan (AGENTS.md Rule
    1.7)."
  - "AGENTS.md, Platform Layering: Android capabilities are built in
    libmobile first and released, desktop ones in libdevice; libmorph
    wraps both with one API."
  - "libmobile `docs/design/20261029_01_CLIPBOARD_WRITE_PLAN.md` (Draft):
    `android::clipboard::write_text(&AndroidApp, &str) -> Result<Copied,
    ClipboardError>`, `Copied { system_notice }` true on Android 13 (API
    33) and later, in the existing `clipboard` feature; for libmobile
    0.3.5."
  - "libdevice `docs/design/20261029_01_CLIPBOARD_WRITE_PLAN.md` (Draft):
    `clipboard::write_text(&str) -> Result<(), ClipboardError>`
    (`Unsupported`, `Platform`), macOS through `NSPasteboard`; for libdevice
    0.1.3, with the read."
  - "libmorph today (0.5.2, `1f35530`): `src/clipboard.rs` (private, one
    `platform` module per host, `ctx.scripted` short-circuits), the
    `mobile-clipboard` passthrough, libdevice's macOS line with
    `features = [\"open-link\", \"clipboard\"]`; `open_link` in scripted
    runs checks and logs instead of opening. RoundTwo pins libmorph
    `c958897` (0.5.1)."
related:
  - docs/design/20261028_03_CLIPBOARD_PLAN.md (the read; unchanged)
  - libmobile docs/design/20261029_01_CLIPBOARD_WRITE_PLAN.md (the Android
    half)
  - libdevice docs/design/20261029_01_CLIPBOARD_WRITE_PLAN.md (the macOS
    half)
  - libmobile docs/design/20261028_03_CLIPBOARD_PLAN.md and libdevice
    docs/design/20261024_01_CLIPBOARD_PLAN.md (the reads they extend)
  - RoundTwo docs/design/20261029_06_MOCK_INTERVIEW_SESSION_PLAN.md (the
    first user: Copy on the results' question text and each answer's
    transcript)
---

# Writing text to the clipboard on every host

## 1. API

Two methods on `MorphContext` and two types at the crate root, on every
host and with every feature set, so signatures never change:

```rust
/// What happened after a copy the system accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipboardWrite {
    /// The system showed its own confirmation (Android 13 and later). The
    /// app shows none, so the user is not told twice.
    SystemNotice,
    /// Copied, and the system said nothing (macOS, Android 10 to 12L,
    /// scripted runs). The app may show its own short "Copied" notice.
    Silent,
}

/// Why the text was not copied.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipboardError {
    /// No clipboard write on this host: desktops other than macOS, the web,
    /// and Android built without `mobile-clipboard`.
    Unsupported,
    /// Anything the platform reported (never containing the text).
    Platform(String),
}
// Display and std::error::Error.
```

```rust
impl MorphContext {
    /// Put `text` on the clipboard as plain text, replacing what was there,
    /// for the app's own Copy button. Call it on the user's tap.
    ///
    /// `Ok(ClipboardWrite::SystemNotice)` when the system confirmed the
    /// copy itself (Android 13 and later); `Ok(ClipboardWrite::Silent)`
    /// when it did not, and the app may show its own notice. An empty
    /// `text` is written as given. libmorph never logs the text or its
    /// length.
    pub fn set_clipboard_text(&self, text: &str) -> Result<ClipboardWrite, ClipboardError>;

    /// Whether [`set_clipboard_text`](Self::set_clipboard_text) can work on
    /// this host, for showing or hiding a Copy button. Decided at build
    /// time: `true` on macOS, Android with `mobile-clipboard`, and scripted
    /// runs; `false` elsewhere.
    pub fn clipboard_can_write(&self) -> bool;
}
```

Both types are re-exported at the crate root (`libmorph::ClipboardWrite`,
`libmorph::ClipboardError`), as `OpenLinkError` is; the module stays
private. The read methods are unchanged and keep their `Option` and
`bool`; the read never returns this error.

`ClipboardWrite` is an enum, not a `bool`, so a later host with another
kind of confirmation (a browser's own notice, say) is a new variant of a
`#[non_exhaustive]` enum, not a change of meaning.

| Host | `set_clipboard_text` | `clipboard_can_write` |
|---|---|---|
| Android, `mobile-clipboard` | libmobile's `write_text`; `Copied { system_notice: true }` is `SystemNotice`, `false` is `Silent`; its `ClipboardError` maps to `Platform(<its Display>)` (a write has no `NotFocused`; any other variant also maps to `Platform`) | `true` |
| Android, without it | `Err(Unsupported)` | `false` |
| macOS | libdevice's `clipboard::write_text`; `Ok(())` is `Silent`; `Unsupported` and `Platform(m)` map across, any other variant to `Platform(<its Display>)` | `true` |
| Other desktops, the web | `Err(Unsupported)` | `false` |
| Scripted runs | `Ok(Silent)`, after logging `libmorph: script: clipboard written` at info (no text, no length); the host's clipboard is never touched | `true` |

Scripted runs come first, on every host, as `open_link`'s does: a run is
headless and must not overwrite the owner's clipboard. It keeps no
stand-in text: `clipboard_text` in a scripted run stays `None`, as the read
plan fixes (open question 7.1). A script's log shows that the Copy button
wrote; RoundTwo's scripted evidence relies on that line.

On Android the call needs the `AndroidApp` (`ctx.android`); a context
without one (which a running app always has) gives `Platform("no
activity")`. Errors are logged at debug with their reason only.

## 2. Features and dependencies

- No new feature. `mobile-clipboard` (`["mobile", "libmobile/clipboard"]`)
  now gives the write as well, since libmobile adds it to the same
  feature; its comment in `Cargo.toml` changes from "Read only" to "read
  and written".
- libmobile: the same git dependency; `Cargo.lock` moves to the release
  with `write_text` (0.3.5, compatible with 0.3.4).
- libdevice: the macOS line is unchanged (`clipboard` is already on);
  `Cargo.lock` moves to libdevice 0.1.3 (compatible with 0.1.2).
- The web adds no `web-sys` feature (open question 7.2).

## 3. Files

- `src/clipboard.rs`: `ClipboardWrite`, `ClipboardError` with `Display`
  and `Error`, `pub(crate) fn write(ctx: &MorphContext, text: &str)`,
  `pub(crate) fn can_write(ctx: &MorphContext) -> bool`, a `write` in each
  existing `platform` module, the module docs ("read and written"), and the
  host-tested mappings `from_copied(system_notice: bool) -> ClipboardWrite`.
- `src/context.rs`: the two methods, after `clipboard_has_text`.
- `src/lib.rs`: `pub use clipboard::{ClipboardError, ClipboardWrite};`.
- `Cargo.toml`: the comment of `mobile-clipboard`.
- `examples/clipcopy.rs` (new, desktop): a window whose tap calls
  `ctx.set_clipboard_text("libmorph-clipboard-check-29Q")` and logs the
  `Result` (never the text), as `examples/urlopen.rs` does for a link;
  `examples/scripts/clipcopy.morph` taps once and takes a screenshot.
- `AGENTS.md`'s Core API: one line (release step, its own commit under
  Rule 3.1). The README has no API list; it gains only this plan's row.

## 4. Versioning

Additive: two methods on `MorphContext` and two new `#[non_exhaustive]`
types; libmobile 0.3.4 to 0.3.5 and libdevice 0.1.2 to 0.1.3 are
semver-compatible (libdevice is re-exported, and its change is additive).
libmorph 0.5.2 to **0.5.3**, tagged `v0.5.3`, with a `VERSIONS.md` entry.
libmorph-build does not change (its libmobile moves in `Cargo.lock`
only); libmorph republishes the Gradle sources of its own libmobile
dependency, so an app's APK gets the `writeText` Kotlin method once its
`Cargo.lock` resolves libmobile 0.3.5.

## 5. Steps

1. **The wrapper**, after libmobile's write (0.3.5) and libdevice's
   (0.1.3) are on GitHub: the types, the two methods on every host, the
   scripted row, the tests (`from_copied` maps both values; a scripted
   context's write is `Ok(Silent)` and leaves `clipboard_text` `None`;
   on hosts other than macOS without the feature, `Unsupported` and
   `clipboard_can_write() == false`; `Display` of both errors).
   **Done when:** `cargo fmt -p libmorph -- --check`; `cargo clippy -p
   libmorph --all-targets` with default features and with `--features
   script,secure-store,mobile-clipboard`; `cargo clippy -p libmorph
   --target aarch64-linux-android --features mobile,mobile-clipboard` and
   with `mobile` alone; `cargo clippy -p libmorph --target
   wasm32-unknown-unknown --features web`: no new warnings; `cargo test -p
   libmorph` and `cargo test -p libmorph --features script` pass.
2. **The Mac check**, with `examples/clipcopy.rs`.
   **Done when:** on the owner's Mac, `cargo run --example clipcopy`, a
   click, then `test "$(pbpaste)" = libmorph-clipboard-check-29Q` exits 0
   and the log shows `Ok(Silent)`; and the scripted run
   `cargo run --example clipcopy --features script -- --morph-script
   examples/scripts/clipcopy.morph` logs `libmorph: script: clipboard written`
   while `pbpaste` still prints what was there before. Recorded in
   `CHANGES.md`.
3. **Release** as libmorph 0.5.3 on the owner's word: the AGENTS.md Core
   API line, the version, the `VERSIONS.md` entry, the tag `v0.5.3`.
   **Done when:** the commit is tagged `v0.5.3`; it builds for macOS and
   for `aarch64-linux-android` with `mobile,mobile-clipboard`.
4. **RoundTwo, the first user.** RoundTwo (its own work, under its plan
   `docs/design/20261029_06_MOCK_INTERVIEW_SESSION_PLAN.md` and its own
   rules) moves both libmorph pins in `apps/roundtwo/Cargo.toml` from
   `c958897` (0.5.1) to the commit tagged `v0.5.3`, its `Cargo.lock` to
   libmobile 0.3.5 and libdevice 0.1.3, and adds the Copy buttons on the
   results' question text and each answer's transcript, showing its own
   "Copied" only on `ClipboardWrite::Silent`. Nothing is changed in
   RoundTwo by this plan.
   **Done when:** RoundTwo builds on the Mac and for Android against
   `v0.5.3` (with `mobile-clipboard`, which its Android target already
   enables); on the Mac, Copy puts the transcript on the pasteboard
   (`pbpaste`) and RoundTwo shows its own notice; on the owner's S23
   (Android 15 or 16), Copy shows Android's confirmation and no second
   notice from RoundTwo, and the text pastes in another app; `dexdump` of
   RoundTwo's APK shows `clipboard/ClipboardPlugin.writeText`. The result
   is recorded here by an implementation amendment.

## 6. Risks

- **Release order.** libmobile 0.3.5 and libdevice 0.1.3 must be on
  GitHub first; until then a build from git fails to find `write_text`.
  The work can be built against local checkouts through the gitignored
  `.cargo/config.toml`, as the read was.
- **RoundTwo's other pins.** RoundTwo pins libmorph-build `b8eb101`
  (0.3.0). It needs no move for this, since the Kotlin comes through
  libmorph's libmobile, but step 4 checks the APK with `dexdump`.
- **Vendor overlays.** `SystemNotice` follows the Android version, not
  what was drawn (libmobile's plan, risks). The S23 check in step 4 shows
  whether One UI's confirmation is visible.

## 7. Open questions

1. **A scripted stand-in.** Should scripted runs keep the written text so
   that a later `clipboard_text` in the same run returns it (a
   round-trip test)? That changes the read plan's fixed scripted answer
   (`None`), which is behaviour a caller can observe, so it would be a
   minor bump (0.6.0) and its own plan. Proposed: no; the log line is
   enough for RoundTwo's evidence.
2. **The web.** The read plan has no web clipboard, so this plan answers
   `Unsupported` there. `navigator.clipboard.writeText` would fit (it
   needs a secure context and a recent user gesture, and is a Promise, so
   the call would return `Silent` before the browser answers). Proposed:
   a later additive amendment when a web app needs it.
3. **Sensitive text.** libmobile's `EXTRA_IS_SENSITIVE` and libdevice's
   `org.nspasteboard.ConcealedType` are open questions in their plans; a
   libmorph `set_clipboard_text_with(text, &ClipboardOptions)` would follow
   them, additively. Proposed: not now.
