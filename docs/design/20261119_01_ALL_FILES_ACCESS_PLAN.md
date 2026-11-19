---
title: All files access, the shared storage root and media scans on every host
date: 2026-11-19
status: Draft
covers:
  - "`MorphContext::all_files_access`, `open_all_files_access_settings`, `external_storage_dir` and `scan_media`"
  - "Android: libmobile's `all-files-access` and `media-scan` features, through passthroughs `mobile-all-files-access` and `mobile-media-scan`; the desktop, the web and scripted runs: fixed answers"
  - "A script verb `all-files-access granted|missing|not-required` for headless tests"
sources:
  - "ShotToss `docs/design/20261119_01_SHOTTOSS_GALLERY_PLAN.md` (Draft), section 4.2 and open question 2: the access check, the settings page and the media scan, asked for in libmobile and libmorph instead of ShotToss's own JNI"
  - "Owner (2026-11-19, in the ShotToss session, verbatim): \"generic JNI like access checks should be added to libmorph/libmobile as a plan (or through a session if one is open)\""
  - "AGENTS.md, Conventions: libmorph is a wrapper; a capability is built and released in libmobile first, and libmorph does not reach past it to JNI"
  - "libmobile `docs/design/20261119_01_ALL_FILES_ACCESS_PLAN.md` (Draft): `android::all_files_access::{state, open_settings, external_storage_dir}`, `android::media_scan::scan_paths`, `gradle::Project::all_files_access`, for libmobile 0.5.3"
  - "libmorph today (0.9.2, `57f7864`): no storage-access call; `MorphApp::on_resume` exists; libmorph-build re-exports `libmobile::gradle` and pins libmobile by rev"
related:
  - libmobile docs/design/20261119_01_ALL_FILES_ACCESS_PLAN.md (the Android half)
  - docs/design/20261023_03_APP_STORAGE_PLAN.md (the app's own directories; unchanged)
  - docs/design/20261022_02_APP_PERMISSIONS_PLAN.md (runtime permissions; All files access is not one)
  - ShotToss docs/design/20261119_01_SHOTTOSS_GALLERY_PLAN.md (the first user, dev.tmzt.shottoss)
---

# All files access, the shared storage root and media scans on every host

## 1. API

Four methods on `MorphContext`, on every host and with every feature set,
so signatures never change, and their types in a new public module
`libmorph::shared_storage`:

```rust
/// Whether the app may read and write all of shared storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AllFilesAccess {
    /// Android: the user turned All files access on.
    Granted,
    /// Android: it is off. Ask with `open_all_files_access_settings`.
    Missing,
    /// No such permission on this host: the process reads what its user can
    /// (the desktop), or nothing (the web).
    NotRequired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SharedStorageError {
    /// The build script did not call `gradle_project(id).all_files_access()`.
    NotDeclared,
    /// Android 10 or the app was built without `mobile-all-files-access`
    /// (or `mobile-media-scan` for `scan_media`).
    Unsupported,
    /// The settings page was asked for while the app was not visible.
    NotVisible,
    /// No settings page resolved on this device.
    NoSettingsPage,
    /// A path given to `scan_media` was not absolute or not UTF-8, or there
    /// were more than 1 024.
    BadPaths(String),
    Platform(String),
}
```

```rust
impl MorphContext {
    /// Whether the app holds All files access now. Read it in `init` and
    /// again in `on_resume`: Android says nothing when the user changes it.
    pub fn all_files_access(&self) -> Result<AllFilesAccess, SharedStorageError>;

    /// Open the system page where the user turns All files access on: this
    /// app's own switch, or the list of apps where that page does not exist.
    /// The app is paused while it shows; read `all_files_access` in `on_resume`.
    pub fn open_all_files_access_settings(&mut self) -> Result<(), SharedStorageError>;

    /// The root of the phone's shared storage (`/storage/emulated/0` on most
    /// phones); `None` where there is no such volume.
    pub fn external_storage_dir(&self) -> Option<PathBuf>;

    /// Tell the system's media index that these files were added, changed,
    /// moved (give the old and the new path) or deleted. Returns once queued.
    pub fn scan_media(&self, paths: &[&Path]) -> Result<(), SharedStorageError>;
}
```

| Host | `all_files_access` | `open_all_files_access_settings` | `external_storage_dir` | `scan_media` |
|---|---|---|---|---|
| Android, with the feature | libmobile's `state()` | libmobile's `open_settings()`; the page opened is logged | libmobile's `external_storage_dir()`; an error is logged and gives `None` | libmobile's `scan_paths` (`mobile-media-scan`) |
| Android, without it | `Err(Unsupported)` | `Err(Unsupported)` | `None` | `Err(Unsupported)` |
| Desktop | `Ok(NotRequired)` | `Err(Unsupported)` | `None` | `Ok(())`: there is no index to tell |
| The web | `Ok(NotRequired)` | `Err(Unsupported)` | `None` | `Ok(())` |
| Scripted runs | the scripted state (2), `NotRequired` by default | `Ok(())`, logged as `open all-files-access settings` | `None` | `Ok(())`, each path logged at debug |

The error variants map one to one from libmobile's `AllFilesAccessError`
and `MediaScanError`; the paths' check runs on every host, so a bad path is
`BadPaths` on the desktop too and a test there catches it.

The module is named `shared_storage`, not `storage`, because
`src/storage.rs` already holds the app's own directories (`data_dir`,
`cache_dir`), which this plan does not change.

## 2. Scripted runs

A script verb, beside `camera-deny` and `media-deny`:

```
all-files-access granted|missing|not-required
```

sets what `all_files_access` answers from the next frame on. A script for
ShotToss's access card then reads:

```
all-files-access missing
frames 2
screenshot card
tap 180 420            # the card's button: logs "open all-files-access settings"
suspend
all-files-access granted
resume                  # on_resume reads Granted
frames 2
screenshot granted
```

An unknown argument is a parse error naming the three accepted ones.

## 3. Features and dependencies

- `Cargo.toml`:
  `mobile-all-files-access = ["mobile", "libmobile/all-files-access"]` and
  `mobile-media-scan = ["mobile", "libmobile/media-scan"]`, with comments,
  after `mobile-media-request`.
- libmobile: the same git dependency at the release that has both features
  (0.5.3), in libmorph's and libmorph-build's `Cargo.toml` (both pin by
  rev); `Cargo.lock` moves to it.
- libmorph-build needs no code: it re-exports `libmobile::gradle`, so
  `libmorph_build::gradle_project(id).all_files_access()` exists once its
  libmobile moves. Its README names the method in its list of manifest
  options.

## 4. Files

- `src/shared_storage.rs` (new): the two types, `From` impls from
  libmobile's errors (Android only), the path check's mapping, and one
  `platform` module per host as `src/link.rs` does.
- `src/context.rs`: the four methods.
- `src/script.rs` and the runner: the verb and the scripted state.
- `src/lib.rs`: `pub mod shared_storage;` and re-exports of the two types.
- `Cargo.toml`: the two features and the libmobile rev.
- `crates/libmorph-build/Cargo.toml`: the libmobile rev; its README line.
- `README.md` and `AGENTS.md`'s Core API: one line each (release step).

## 5. Versioning

Additive: four methods, a new module with two `#[non_exhaustive]` types,
two features and a script verb. libmobile 0.5.2 to 0.5.3 is
semver-compatible. libmorph 0.9.2 to **0.9.3**, tagged `v0.9.3`, and
libmorph-build 0.5.2 to **0.5.3** (its libmobile rev moves, which adds
`Project::all_files_access` to its re-export), tagged
`libmorph-build-v0.5.3`, each with a `VERSIONS.md` entry.

## 6. Steps

1. **The wrapper**, after libmobile 0.5.3 is on GitHub: the module, the
   four methods on every host, the features, the script verb and the
   tests (scripted runs answer `NotRequired`, then each scripted state;
   the verb's parse errors; the desktop's `NotRequired`, `Unsupported`,
   `None` and `Ok(())`; `BadPaths` for a relative path on the desktop).
   **Done when:** `cargo fmt -p libmorph -- --check`; `cargo clippy -p
   libmorph --all-targets` with default features and with `--features
   script`; `cargo clippy -p libmorph --target aarch64-linux-android
   --features mobile,mobile-all-files-access,mobile-media-scan` and with
   `mobile` alone: no new warnings; `cargo test -p libmorph` and `cargo
   test -p libmorph --features script` pass.
2. **libmorph-build** on the same libmobile rev, with its README line.
   **Done when:** `cargo fmt -p libmorph-build -- --check`, `cargo clippy
   -p libmorph-build --all-targets` and `cargo test -p libmorph-build`
   pass, and a test build script calling `.all_files_access()` generates a
   manifest with `MANAGE_EXTERNAL_STORAGE`.
3. **Release** as libmorph 0.9.3 and libmorph-build 0.5.3 on the owner's
   word: the README and AGENTS.md lines, the versions, the `VERSIONS.md`
   entries, the tags.
   **Done when:** both commits are tagged, and ShotToss's access card,
   built on them, opens its own page on the S23 and shows the gallery after
   the switch is turned on and Back is pressed.

## 7. Risks

- **An app reads too early.** `all_files_access` in `init` and not in
  `on_resume` never sees the grant; the method's documentation and
  ShotToss's plan both say to read it in `on_resume`.
- **The desktop's `NotRequired`.** It says there is no permission, not
  that every path is readable; the app still handles the OS's own errors.
