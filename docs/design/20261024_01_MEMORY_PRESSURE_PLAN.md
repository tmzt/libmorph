---
title: Telling the app about memory pressure
date: 2026-10-24
status: In implementation
covers:
  - "Stage 1: `MorphApp::on_low_memory`, from libmobile's existing
    `MobileEvent::LowMemory`"
  - "Stage 2: `MorphApp::on_trim_memory(level)`, from Android's
    `onTrimMemory` levels, built in libmobile first"
sources:
  - "handlr (request, 2026-10-24, on the owner's instruction, from the
    handlr session): the companion app should respond to memory pressure
    by paging cached views to its redb store and trimming contents (handlr
    plan 20261022_03, living section L3). It asked for a defaulted hook,
    or the event through system events, never firing on desktop or the
    web; and, optionally, `onTrimMemory`'s levels as a graded event. It
    can manage with plain low memory."
  - "Owner (2026-10-24): \"approve them, the new hook in stages\"."
  - "libmobile today: `MainEvent::LowMemory` from android-activity
    becomes `MobileEvent::LowMemory` (`src/android/mod.rs`); there is no
    `onTrimMemory`. libmorph's Android runner ignores the event."
  - "Android: `ComponentCallbacks2.onTrimMemory(int)` with
    `TRIM_MEMORY_UI_HIDDEN` (20), `TRIM_MEMORY_BACKGROUND` (40), and the
    older `RUNNING_MODERATE` (5), `RUNNING_LOW` (10), `RUNNING_CRITICAL`
    (15), `MODERATE` (60) and `COMPLETE` (80); from API 34 only
    `UI_HIDDEN` and `BACKGROUND` are delivered. `onLowMemory()` is the
    older, ungraded call."
related:
  - libmobile docs/design/20261024_01_TRIM_MEMORY_PLAN.md (stage 2's
    libmobile half)
  - docs/design/20261023_01_FOREGROUND_SERVICE_PLAN.md (a backgrounded
    app with a service is the one that hears these)
---

# Telling the app about memory pressure

## 1. Stage 1: low memory

```rust
pub trait MorphApp {
    /// The system is low on memory (Android's onLowMemory). Free what can
    /// be rebuilt: caches, offscreen resources, decoded data. Never
    /// called on the desktop or the web, or in scripted runs unless the
    /// script says `low-memory`.
    fn on_low_memory(&mut self, ctx: &mut MorphContext) {
        let _ = ctx;
    }
}
```

- **Android runner:** `MobileEvent::LowMemory` calls it, once per event,
  if the app exists (a report before `init` is dropped, since there is
  nothing to free yet). It is called whether or not the app is suspended.
- **Scripted runs:** a `low-memory` command calls it, so apps can test
  their response headlessly. It is accepted while suspended.
- **libmorph itself** does nothing more on low memory: the pipeline cache
  is saved at suspend already, and the GPU device stays.
- Additive: a defaulted hook and a script command (`Command` is
  `#[non_exhaustive]`).

## 2. Stage 2: trim levels

After libmobile's `docs/design/20261024_01_TRIM_MEMORY_PLAN.md` is
released:

```rust
/// How hard Android is asking the app to trim (onTrimMemory).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum TrimLevel {
    RunningModerate,
    RunningLow,
    RunningCritical,
    UiHidden,
    Background,
    Moderate,
    Complete,
}

pub trait MorphApp {
    /// Android asked the app to trim memory. Levels are ordered, so
    /// `level >= TrimLevel::Background` means "free everything you can".
    /// From Android 14 only UiHidden and Background arrive.
    fn on_trim_memory(&mut self, ctx: &mut MorphContext, level: TrimLevel) {
        let _ = (ctx, level);
    }
}
```

- `TrimLevel` is libmorph's own type, mapped from libmobile's, so the
  signature is the same with or without `mobile`.
- `on_low_memory` stays; apps can use either.
- Scripted runs: `trim-memory <level>`, with the level in lowercase with
  hyphens (`ui-hidden`, `background`, ...).

## 3. Steps

1. **Stage 1.** The hook, the Android runner's call, the `low-memory`
   script command, and a scripted test that the hook runs in order and
   while suspended.
   **Done when:** tests and clippy pass on every host, and on the
   owner's S23 `adb shell am send-trim-memory <package> COMPLETE` (or
   `RUNNING_CRITICAL`) makes an example log `on_low_memory`, if
   android-activity forwards it; whichever happens is recorded.
2. **Stage 1 released** with libmorph 0.4.0, on the owner's word.
3. **Stage 2**, after libmobile's trim-memory release: `TrimLevel`, the
   hook, the runner's mapping, `trim-memory <level>`, tests.
   **Done when:** tests and clippy pass, and on the S23
   `am send-trim-memory <package> UI_HIDDEN` and `BACKGROUND` reach
   `on_trim_memory` with those levels.
4. **Stage 2 released**, on the owner's word. Additive.

## Amendment — 2026-10-24 12:17 EDT: approved

The owner approved this plan on 2026-10-24 ("approve them, the new hook
in stages"). Stage 1 (`on_low_memory`) is built first, in libmorph
only; stage 2 (trim levels) follows libmobile's trim-memory release. The
status stays `Draft` until the first commit that implements it.

## Amendment — 2026-10-24 13:04 EDT: implementation has started; the document is now immutable

Step 1 (stage 1: `on_low_memory`) starts with this commit, over
libmobile 0.2.3's `MobileEvent::LowMemory`. Stage 2 waits for libmobile's
trim-memory release. The document now changes only on the owner's
explicit instruction.

## Amendment — 2026-10-24 13:12 EDT: stage 1 released in 0.4.0; stage 2 in 0.4.1

Step 1 is `a72a3b4`, `2c690ed`, `7ac722d` and `efaba1f`. On the S23
`on_low_memory` did not fire: android-activity's `LowMemory` comes only
from `onLowMemory`, and `am send-trim-memory` refused `COMPLETE` for a
process with a foreground service, while `RUNNING_CRITICAL` reached the
process as `trimMemory(15)`; the step allows recording either. Step 2 is
libmorph 0.4.0 (`586ef89`, tag `v0.4.0`). Stage 2 ships in 0.4.1, after
libmobile 0.2.4, on the owner's word (2026-10-24: release 0.4.0 now and
the rest as 0.4.1).
