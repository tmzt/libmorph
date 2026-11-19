---
title: Disk space and disk pressure, told to the app when it changes
date: 2026-11-19
status: Draft
covers:
  - "`MorphContext::disk_space`, `disk_pressure` and `check_disk_now`, and a defaulted `MorphApp::on_disk_pressure` hook, in a new module `libmorph::disk`"
  - "`MorphApp::DISK_WATCH`, an associated const that turns the watch on, with the app's path, thresholds and interval"
  - "Android: libmobile's `disk-space` feature through a passthrough `mobile-disk-space`; the desktop: libdevice's `disk::space` and libmorph's own timer; scripted runs: a `disk-space` verb"
sources:
  - "ShotToss `docs/design/20261119_01_SHOTTOSS_GALLERY_PLAN.md` (Draft), through its coordinator on 2026-11-19: disk pressure to trim its two cache tiers (data_dir thumbnails 256 MiB LRU, cache_dir previews 128 MiB LRU) and to stop before batch writes; thresholds Low under max(1 GiB, 5 %), Critical under 256 MiB, 256 MiB of hysteresis"
  - "Owner (2026-11-19, in the ShotToss session): \"might be valuable as libmorph/libmobile plans\"; \"callbacks in libmobile that call JNI exist for things like the cutout code, though this would probably need periodic checking\"; \"the timer doesn't need to be every tick\""
  - "libmobile `docs/design/20261119_02_DISK_SPACE_PLAN.md` (Draft): `disk_space::{DiskSpace, DiskPressure, DiskThresholds, DiskPressureReport}`, `android::disk_space::{space, watch, unwatch, check_now}`, `MobileEvent::DiskPressure`, a Kotlin timer of `DISK_CHECK_INTERVAL` (60 s, bounds 15 s to 10 min) only while resumed"
  - "AGENTS.md, Conventions: libmorph is a wrapper; Android capabilities come from libmobile and desktop ones from libdevice; associated consts configure what libmorph manages (`OWNS`, `GPU_IN_BACKGROUND`)"
  - "libmorph today (0.9.2, `57f7864`): no disk call; system reports arrive as defaulted hooks (`on_low_memory`, `on_trim_memory`) with script verbs (`low-memory`, `trim-memory`)"
related:
  - libmobile docs/design/20261119_02_DISK_SPACE_PLAN.md (the Android half)
  - docs/design/20261119_03_MEMORY_PRESSURE_GRADES_PLAN.md (the memory counterpart, same day)
  - docs/design/20261023_03_APP_STORAGE_PLAN.md (`data_dir`, `cache_dir`; the watched volume by default)
  - ShotToss docs/design/20261119_01_SHOTTOSS_GALLERY_PLAN.md (the first user, dev.tmzt.shottoss)
---

# Disk space and disk pressure, told to the app when it changes

## 1. Hook, not pull alone

libmorph tells apps about the system's state through defaulted hooks
(`on_low_memory`, `on_trim_memory`, `on_service`), and lets them read the
current value through `MorphContext`. Disk pressure follows both: the app
reads it when it is about to write, and is told when it changes. libmorph
does not poll on Android: libmobile's Kotlin timer does, off the frame
loop, and libmorph only drains libmobile's event.

## 2. API

`libmorph::disk`, libmorph's own types, the same shape as libmobile's (as
`TrimLevel` is libmorph's own), so the desktop and scripted runs need no
libmobile:

```rust
pub struct DiskSpace { pub free: u64, pub total: u64, pub allocatable: Option<u64>, pub cache_quota: Option<u64> } // #[non_exhaustive]
pub enum DiskPressure { None, Low, Critical }                                                                       // ordered, #[non_exhaustive]
pub struct DiskThresholds { pub low_min: u64, pub low_fraction_permille: u32, pub critical: u64, pub hysteresis: u64 } // DEFAULT, grade()
pub struct DiskPressureReport { pub pressure: DiskPressure, pub space: DiskSpace }                                   // #[non_exhaustive]
pub enum DiskError { Path(String), Unsupported, Platform(String) }                                                 // #[non_exhaustive]

/// What DISK_WATCH asks libmorph to watch.
#[derive(Clone, Copy, Debug)]
pub struct DiskWatch {
    /// Which of the app's directories names the volume.
    pub volume: DiskVolume,                 // DataDir (default) or CacheDir
    pub thresholds: DiskThresholds,         // DiskThresholds::DEFAULT
    pub interval: Duration,                 // 60 s; clamped to 15 s ..= 600 s
}
impl DiskWatch { pub const DEFAULT: Self; }
```

`DiskThresholds::DEFAULT` and `grade` are the same values and the same
rule as libmobile's (Low under max(1 GiB, 5 % of total), Critical under
256 MiB, a grade clears only 256 MiB above its threshold). The grading is
about twenty lines, written in both crates with the same table of test
cases, because libmorph's desktop and scripted runs grade without
libmobile; a test in libmorph checks the two agree on Android builds.

On the app:

```rust
pub trait MorphApp {
    /// Watch the volume of the app's data (or cache) directory. None, the
    /// default: no watch, no timer, on_disk_pressure is never called.
    const DISK_WATCH: Option<DiskWatch> = None;

    /// The watched volume's grade changed. Called once after init with the
    /// starting grade, then only on a change, on the loop thread.
    fn on_disk_pressure(&mut self, ctx: &mut MorphContext, report: &DiskPressureReport) {
        let _ = (ctx, report);
    }
}
```

On the context:

```rust
impl MorphContext {
    /// The space of the volume holding `path`. On Android this also asks
    /// StorageManager for the allocatable bytes and the app's cache quota,
    /// which can take tens of milliseconds: call it when deciding, not per frame.
    pub fn disk_space(&self, path: &Path) -> Result<DiskSpace, DiskError>;
    /// The watched volume's last grade; DiskPressure::None without DISK_WATCH.
    pub fn disk_pressure(&self) -> DiskPressure;
    /// Check the watched volume now (before a batch write, say). Calls
    /// on_disk_pressure before returning if the grade changed.
    pub fn check_disk_now(&mut self) -> Result<DiskPressureReport, DiskError>;
}
```

## 3. Hosts

| Host | `disk_space` | The watch |
|---|---|---|
| Android, `mobile-disk-space` | libmobile's `android::disk_space::space` | libmobile's `watch` at the first `init`, with the directory's path; `MobileEvent::DiskPressure` calls the hook; `check_now` for `check_disk_now` |
| Android, without it | `Err(Unsupported)` | none; `DISK_WATCH` set without the feature logs one warning |
| Desktop | libdevice's `disk::space` (`allocatable`, `cache_quota` `None`) | libmorph's timer (below) |
| The web | `Err(Unsupported)` | none |
| Scripted runs | the scripted numbers (4) for any path | graded on each `disk-space` command and at `check_disk_now`; no timer |

**The desktop's timer.** libmorph owns the winit loop, so it keeps a
`next_disk_check: Instant`, compared once per loop iteration (one
`Instant` comparison, as the frame pacing already does); when due, it
calls `disk::space` and grades. It is not run while the app is suspended
(minimised or occluded, per the device-may-be-absent plan) and runs once
on resume. The check itself is a `statvfs`, microseconds.

**Why libdevice on the desktop.** AGENTS.md's layering puts desktop
platform calls in libdevice, and libmorph does not reach to the OS
itself. `statvfs` is POSIX, so libdevice's `disk::space` is the same call
libmobile's `statvfs_space` makes. libdevice has no such call yet (0.1.3);
it needs its own small plan (open question 1).

## 4. Scripted runs

```
disk-space <free> <total>      e.g. disk-space 900MiB 64GiB
```

sets the scripted volume's numbers (units `B`, `KiB`, `MiB`, `GiB`), grades
them against `DISK_WATCH`'s thresholds, and calls `on_disk_pressure` if
the grade changed. Before any such command a run reports 64 GiB free of
128 GiB (`None`). Accepted while suspended, as `low-memory` is.

## 5. Features and dependencies

- `mobile-disk-space = ["mobile", "libmobile/disk-space"]`, with a comment.
- libmobile at the release with `disk-space`; libdevice at the release
  with `disk::space` (feature `disk-space` there, on the macOS and other
  desktop dependency lines).
- libmorph-build does not change.

## 6. Files

- `src/disk.rs` (new): the types, `grade`, `DiskWatch`, the desktop timer
  state, the libmobile mapping (Android), the script state.
- `src/app.rs`: `DISK_WATCH`, `on_disk_pressure`.
- `src/context.rs`: the three methods.
- `src/runner/*`: the Android drain of `MobileEvent::DiskPressure`, the
  desktop timer, `watch` at the first `init`.
- `src/script.rs`: the verb.
- `Cargo.toml`, `README.md`, `AGENTS.md`'s Core API.

## 7. Versioning

Additive: a defaulted const and hook, three methods, a new module of
`#[non_exhaustive]` types, a feature and a script verb. A libmorph patch
release, numbered when released (0.9.4 if the all-files access plan's
0.9.3 goes first). libmobile's and libdevice's moves are semver-compatible.

## 8. Steps

1. **The module, the hook and scripted runs**, on every host: types,
   grade (with the shared test table), `DISK_WATCH`, the hook, the three
   methods, the verb; Android and desktop answer `Unsupported` until
   steps 2 and 3.
   **Done when:** `cargo fmt -p libmorph -- --check`; `cargo clippy -p
   libmorph --all-targets` with default features and `--features script`:
   no new warnings; `cargo test -p libmorph --features script` passes,
   with a script that crosses Low and Critical and back and sees exactly
   one hook call per change.
2. **Android**, after libmobile's release: the passthrough, `watch`, the
   drain, `check_now`, `space`.
   **Done when:** `cargo clippy -p libmorph --target aarch64-linux-android
   --features mobile,mobile-disk-space` and with `mobile` alone: no new
   warnings; on the S23, an example with `DISK_WATCH` and a raised
   `critical` logs one `Critical` hook after `init` and no other while
   resumed.
3. **The desktop**, after libdevice's release: `disk::space` and the
   timer.
   **Done when:** on the Mac, an example logs the starting grade and its
   numbers match `df -k` for the data directory's volume; a 15 s interval
   shows one check per 15 s in the debug log, none while minimised.
4. **Release** on the owner's word.
   **Done when:** tagged, and ShotToss builds against it.

## 9. Open questions

1. **libdevice's `disk::space`.** A small plan in libdevice (statvfs on
   macOS and Linux, `allocatable` and `cache_quota` `None`, `Unsupported` on Windows until
   asked). If the owner prefers, libmorph could call `statvfs` itself on
   the desktop as an agreed exception to the layering; this plan assumes
   libdevice.
2. **Default on?** `DISK_WATCH` is `None` by default so no app gains a
   timer it did not ask for. ShotToss sets `Some(DiskWatch::DEFAULT)`.

## Amendment — 2026-11-19 18:32 EST: the desktop through libdevice's plan; one engine file; open question 1 closed

The owner answered open question 1 on 2026-11-19, through the ShotToss
coordinator (verbatim): "just have the subagent do it and follow the
critical rules, must be tested and share API with libmorph". That answer
is the agreement for this amendment. It replaces the following:

- **Section 3, the desktop row and "Why libdevice on the desktop".** The
  desktop's `disk_space` is libdevice's `disk_space::space` (feature
  `disk-space`), planned in libdevice
  `docs/design/20261119_01_DISK_SPACE_PLAN.md` (Draft), for libdevice
  0.1.4. It is the name libdevice's plan uses; this plan's "`disk::space`"
  meant the same call.
- **Section 2, "libmorph's own types" and "written in both crates".** The
  grading is not written in libmorph. The types and `grade` live in one
  engine file, `src/disk_space/engine.rs`, byte-identical in libmobile and
  libdevice (libmobile's plan amended the same day; the key store
  precedent). libmorph's `disk` module re-exports libdevice's engine types
  on every host (libdevice is a dependency on every host, Android
  included), and on Android converts libmobile's `DiskSpace` and
  `DiskPressureReport` field by field into them. libmorph adds only
  `DiskWatch`, `DiskVolume` and `DiskError` (a re-export of the engine's
  `DiskSpaceError` replaces the latter if the owner prefers one error
  type; this plan keeps `DiskError` as named, mapped one to one).
  Scripted runs build their reports with `DiskThresholds::grade` and
  `DiskPressureReport::new`. The desktop timer's interval is clamped with
  the engine's `clamp_interval`.
- **A new test, in step 2's Done when:** built on the host with
  `--features mobile,mobile-disk-space`,
  `libmobile::disk_space::ENGINE_SOURCE == libdevice::disk_space::ENGINE_SOURCE`,
  beside the key store's in `tests/secure_store.rs` (or a new
  `tests/disk_space.rs`).
- **Step 1's "the shared test table"** is the engine's own tests, run in
  libmobile and libdevice; step 1 here tests libmorph's use of them (the
  script crossing grades, one hook call per change).
- **Section 5:** libdevice's dependency lines (every host, and macOS's)
  gain `"disk-space"`.
- **Open question 1 is closed:** libdevice, not a libmorph exception.
  Open question 2 stays open.
