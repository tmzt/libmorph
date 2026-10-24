---
title: Opening a link in another app
date: 2026-10-23
status: Draft
covers:
  - "`MorphContext::open_link(url)`: one call on every host, wrapping
    libmobile on Android and libdevice on the Mac"
  - The web, scripted runs and logging
sources:
  - "handlr (request, 2026-10-23, from the handlr session): repowiki
    links (`http://<mac>:8088/#/src--handlr/docs/design/<plan>`) in the
    companion's agent view should open in the browser on a tap. It asked
    for `libmorph::open_link(ctx, url) -> Result<(), Error>`: the system
    opener on the desktop, logged in scripted runs, the scheme checked,
    nothing logged past the path, and an error when nothing handles it.
    For the owner's approval."
  - "libmobile docs/design/20261023_03_OPEN_LINK_PLAN.md and libdevice
    docs/design/20261023_01_OPEN_LINK_PLAN.md: `check_link`, `open_link`
    and their `OpenLinkError`s."
  - "AGENTS.md, Platform Layering: libmorph wraps both; the web is
    libmorph's own (winit and the browser)."
  - docs/design/20261022_07_URL_SCHEMES_PLAN.md (the inbound side,
    `on_open_url`)
related:
  - libmobile docs/design/20261023_03_OPEN_LINK_PLAN.md
  - libdevice docs/design/20261023_01_OPEN_LINK_PLAN.md
---

# Opening a link in another app

## 1. API

```rust
impl MorphContext {
    /// Open `url` (http or https) in the app that handles it: the
    /// browser, normally. Android: libmobile; macOS: libdevice; the web: a
    /// new tab; scripted runs: logged, not opened.
    pub fn open_link(&self, url: &str) -> Result<(), OpenLinkError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenLinkError {
    BadUrl,
    NoHandler,
    /// Android: the app is hidden.
    NotVisible,
    /// No opener on this host.
    Unsupported,
    Platform(String),
}
```

- libmorph's own error type, so the signature does not change with or
  without `mobile`, mapped from libmobile's and libdevice's.
- **Android:** `libmobile::android::link::open_link`.
- **macOS:** libdevice's, with its `open-link` feature, which libmorph
  turns on for macOS builds (no libmorph feature, since the call is small
  and always wanted). Other desktops: `Unsupported`.
- **Web:** `window.open(url, "_blank", "noopener")`, after the same
  check; a blocked pop-up (`None`) is `Platform("blocked")`. libmorph's
  own, since the web is.
- **Scripted runs:** the URL is checked, logged redacted at info
  (`libmorph: script: open link …/path<redacted>`), and not opened, so
  runs stay headless and repeatable.
- libmorph never logs the URL past its path. A test checks that
  libmobile's and libdevice's `check_link` agree on a shared list.

## 2. Steps

1. **The wrapper**, after libmobile and libdevice release theirs. The
   method, the error, the web path, the scripted path, and the agreement
   test.
   **Done when:** tests and clippy pass on every host.
2. **Checked.** The `urlopen` example opens `https://example.com/` on a
   tap.
   **Done when:** on the Mac and on the owner's S23, the browser opens
   it; in a scripted run (`tap`), the log shows the redacted link and no
   browser opens.
3. **Release**, on the owner's word, taking the libmobile and libdevice
   releases. Additive. Then the handlr session gets the revision.

## 3. Open question for the owner

1. **A method or a free function.** handlr asked for
   `open_link(ctx, url)`. A `MorphContext` method lets scripted runs log
   instead of opening. A free function could be called from any thread,
   but could not tell a scripted run apart. Recommended: the method; an
   app's other threads send the URL to the main thread.

## Amendment — 2026-10-23 22:02 EDT: approved

The owner approved the open-link plans in libmobile, libdevice and
libmorph on 2026-10-23 ("approve plans"), without separate answers, so
their recommendations stand: http and https only; libmobile's call is
core, with no Cargo feature; the Mac uses `NSWorkspace`; libmorph's is a
`MorphContext` method. The status stays `Draft` until the first commit
that implements it.
