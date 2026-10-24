---
title: Persistent storage and cache directories on every host
date: 2026-10-23
status: Draft
covers:
  - "`MorphContext::storage_dir()` and `cache_dir()`: a private per-app
    directory on every host, created before `init`"
  - "`MorphApp::APP_ID`, the directory name on the desktop"
  - "What `data_dir()` and the pipeline cache become"
  - Scripted runs and the web
sources:
  - "Owner (2026-10-23, relayed by the handlr session): \"libmorph should
    provide a persistent storage location on all platforms, with
    mkdir_all\". Plan first, for the owner's approval."
  - "handlr's request: its companion keeps the phone's scrollback in a
    redb file in the app's data directory (handlr plan 20261023_09), and
    the desktop window should do the same. It already reads `vocab.txt`,
    its models and `dictation_silence.txt` from there. It asked for the
    directory to exist before `init`, a matching cache directory, a clear
    error rather than a panic, and suggested a new method rather than
    changing `data_dir()`."
  - "libmorph today: `data_dir() -> Option<&Path>`, Android's internal
    data path and `None` elsewhere; the pipeline cache lives at
    `<data_dir>/libmorph/<key>.bin` (plan 20261016_02);
    `MorphApp::SECURE_STORE_ID` names the Keychain service
    (`libmorph.<id>`, default the executable's stem)."
  - "libmobile docs/design/20261023_04_APP_DIRECTORIES_PLAN.md
    (`android::dirs::files_dir`, `cache_dir`) and libdevice
    docs/design/20261023_02_APP_DIRECTORIES_PLAN.md (`dirs::data_dir`,
    `cache_dir` by app id)."
  - AGENTS.md, Platform Layering, and Rule 5.4 (new methods are additive)
related:
  - docs/design/20261016_02_PIPELINE_CACHE_PLAN.md
  - libmobile docs/design/20261023_04_APP_DIRECTORIES_PLAN.md
  - libdevice docs/design/20261023_02_APP_DIRECTORIES_PLAN.md
---

# Persistent storage and cache directories on every host

## 1. API

```rust
pub trait MorphApp {
    /// The app's directory name on the desktop: its storage is
    /// ~/Library/Application Support/<APP_ID> on macOS. 1 to 64 of
    /// [A-Za-z0-9._-]; empty (the default) uses the executable's file
    /// stem. Android uses the package's own directories instead.
    const APP_ID: &'static str = "";
}

impl MorphContext {
    /// A private directory for files the app keeps, created before
    /// `init`. Android: the app's files directory; macOS: Application
    /// Support/<APP_ID>; Linux: $XDG_DATA_HOME/<APP_ID>; Windows:
    /// %APPDATA%\<APP_ID>; scripted runs: an empty directory in the run's
    /// output; the web: None. Also None, with an error logged, if it
    /// could not be created.
    pub fn storage_dir(&self) -> Option<&Path>;
    /// The same for files the app can recreate; the system may delete them
    /// (Android's cache directory, ~/Library/Caches/<APP_ID>, ...).
    pub fn cache_dir(&self) -> Option<&Path>;
}
```

- **Created before `init`** with `std::fs::create_dir_all`, by the runner,
  on every host. A failure is logged at error
  (`libmorph: storage: cannot create <path>: <error>`) and gives `None`;
  bring-up continues.
- **Android:** libmobile's `files_dir()` and `cache_dir()`.
- **Desktop:** libdevice's `data_dir(id)` and `cache_dir(id)`, with `id`
  being `APP_ID`, or the executable's stem with any other character
  replaced by `_`, as `SECURE_STORE_ID`'s default is.
- **Web:** `None` for both. Browser storage (OPFS) is a later plan.
- **Scripted runs:** `storage/` and `cache/` inside the run's output
  directory, emptied at the start of each run, so runs stay repeatable
  and never touch the user's real directory.
- **`data_dir()` does not change**, so no caller sees a difference: it
  stays Android's internal data path and `None` elsewhere. Its rustdoc
  points to `storage_dir()`, which on Android is the same directory.
- **The pipeline cache stays** at `<data_dir>/libmorph/` on Android, so
  existing caches are kept (section 3, question 3).

## 2. Steps

1. **The wrapper**, after libmobile's and libdevice's releases.
   `APP_ID`, the two methods, creation in each runner (desktop, Android,
   scripted), and tests: the id rule, scripted runs creating and
   emptying both directories, and a failure giving `None` (a path under a
   file).
   **Done when:** tests and clippy pass on every host.
2. **Checked.** The `securestore` example (or a new small `storage`
   example) writes a counter file in `storage_dir()`.
   **Done when:** the counter survives a restart on the Mac (under
   `~/Library/Application Support/<id>`) and on the owner's S23 (under
   `/data/user/0/<package>/files`), and a scripted run starts from an
   empty directory each time.
3. **Release**, on the owner's word, taking the libmobile and libdevice
   releases. Additive: a defaulted const and two methods. Then the handlr
   session gets the revision.

## 3. Open questions for the owner

1. **New methods, not `data_dir()`.** Changing `data_dir()` to `Some` on
   the desktop would be a behaviour change (Rule 5.4, a minor bump).
   Recommended: the new `storage_dir()` and `cache_dir()`, with
   `data_dir()` unchanged and pointing to them.
2. **`APP_ID` or `SECURE_STORE_ID`.** A new const keeps the two names
   apart, and both default to the executable's stem. Recommended: the new
   `APP_ID`, and `SECURE_STORE_ID` left as it is.
3. **The pipeline cache.** It is rebuildable, so `cache_dir()` suits it,
   but moving it loses each phone's current cache once. Recommended:
   leave it in `<data_dir>/libmorph/`.
4. **The web.** Recommended: `None` now; OPFS in a later plan if an app
   needs storage there.
5. **Linux and Windows.** libmorph supports only the Mac on the desktop
   today. libdevice computes their paths anyway (they are pure and
   tested), so nothing is lost. Recommended: keep them in libdevice;
   libmorph uses whatever libdevice returns.

## Amendment — 2026-10-23 23:30 EDT: `data_dir()` itself becomes cross-platform

The owner's correction, relayed by the handlr session (2026-10-23): "no,
it was intended to be cross platform, the agent chose option".
`data_dir()` was always meant to be the app's persistent directory on
every host; `None` off Android was an earlier implementation choice.
This replaces `storage_dir()` in sections 1 and 2, section 1's bullet
"`data_dir()` does not change", and open question 1.

- **`data_dir() -> Option<&Path>`** keeps its signature and becomes what
  section 1 gave `storage_dir()`: Android's files directory (as now,
  through libmobile's `files_dir()`), the desktop's per-app directory
  from libdevice, the run's `storage/` in scripted runs, created with
  `create_dir_all` before `init`. There is no `storage_dir()`.
- **`None` only where there is no filesystem or creation failed:** the
  web, and a directory that could not be created (logged at error).
  Callers that handle `Some` keep working; on the desktop they now get
  it. Keeping `Option` avoids a fallback path that would look writable
  and is not.
- **`cache_dir()`** stays a new method, as section 1 defines it.
- **Version.** `data_dir()` returning `Some` on the desktop is behaviour
  a caller can observe, so the release bumps libmorph's minor version
  (0.3.x to 0.4.0) under Rule 5.4, marked **Breaking** in VERSIONS.md.
- **The pipeline cache is unaffected.** It is created only when the
  runner asks for it (`want_pipeline_cache`, Android only), so a desktop
  `data_dir()` does not start one; it stays at `<data_dir>/libmorph/`.
- Section 2's checks use `data_dir()`; step 1 adds a test that scripted
  runs return `Some` for it.

Open questions 2 to 5 stand. A new question 1: keep `Option<&Path>`
(recommended), or return `&Path` with a documented fallback (for
example a temporary directory) where there is no storage, which would
change the signature and break callers.
