---
title: Memory pressure in grades, shared from flankr
date: 2026-11-19
status: Draft
covers:
  - "`MemoryPressure` (`UiHidden`, `Background`, `Low`), its mapping from `TrimLevel` and from `on_low_memory`, lifted from flankr's companion"
  - "A defaulted `MorphApp::on_memory_pressure` hook and `MorphContext::memory_pressure`, the highest grade since the last resume"
  - "How flankr would adopt it later (not part of this plan)"
sources:
  - "flankr `apps/companion/src/memory.rs` and `crates/flankr-companion/src/client.rs` (`Pressure`), read on 2026-11-19: the grades, their order, the table from libmorph's `TrimLevel`, `LOW_MEMORY`, and clearing the grade on resume; from flankr plan 20261022_03, the L3 amendments of 2026-10-24 12:21 and 13:45 EDT"
  - "ShotToss `docs/design/20261119_01_SHOTTOSS_GALLERY_PLAN.md` (Draft), through its coordinator on 2026-11-19: graded memory pressure to trim its thumbnail tiers and its GPU atlas"
  - "Owner (2026-11-19, in the ShotToss session): disk and graded memory pressure \"might be valuable as libmorph/libmobile plans\""
  - "docs/design/20261024_01_MEMORY_PRESSURE_PLAN.md (Implemented): `on_low_memory`, `on_trim_memory(TrimLevel)`, the `low-memory` and `trim-memory` verbs; from API 34 Android delivers only `UI_HIDDEN` and `BACKGROUND`"
related:
  - docs/design/20261024_01_MEMORY_PRESSURE_PLAN.md (built on; unchanged)
  - docs/design/20261111_03_THE_DEVICE_MAY_BE_ABSENT_PLAN.md (its GPU release keeps its own trigger)
  - docs/design/20261119_02_DISK_PRESSURE_PLAN.md (the disk counterpart, same day)
  - flankr apps/companion/src/memory.rs (the source)
---

# Memory pressure in grades, shared from flankr

## 1. Why

`on_trim_memory` hands an app Android's seven levels, whose order is not
the order of what they ask: `RunningCritical` sorts below `UiHidden` but
asks for more. flankr's companion grades them into three steps of what to
drop, and ShotToss needs the same three. The grading is generic, so it
moves here, and each app keeps only what it drops at each grade.

No libmobile change is needed: libmobile already gives every level and
low memory, and libmorph's `TrimLevel` maps them one to one.

## 2. API

`src/memory.rs`, beside `TrimLevel`:

```rust
/// What an app should drop, in increasing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum MemoryPressure {
    /// The UI is hidden, or the device is getting low while running: keep
    /// what is near; drop what is far.
    UiHidden,
    /// In the background, or running low: keep only what is close to the
    /// current view.
    Background,
    /// Low memory, a critical running level or `Complete`: keep only what
    /// is on screen, so the process is not killed.
    Low,
}

impl MemoryPressure {
    /// The grade for a trim level; None for a level libmorph adds later.
    pub const fn from_trim(level: TrimLevel) -> Option<Self>;
    /// on_low_memory's grade.
    pub const LOW_MEMORY: Self = Self::Low;
}
```

The table, flankr's exactly:

| `TrimLevel` | `MemoryPressure` |
|---|---|
| `RunningModerate`, `UiHidden` | `UiHidden` |
| `RunningLow`, `Background`, `Moderate` | `Background` |
| `RunningCritical`, `Complete`, `on_low_memory` | `Low` |

On the app and the context:

```rust
pub trait MorphApp {
    /// A graded memory report, called right after on_trim_memory or
    /// on_low_memory for the same report, with its grade. Every report is
    /// passed on, also one no higher than the last, so an app may trim again.
    fn on_memory_pressure(&mut self, ctx: &mut MorphContext, pressure: MemoryPressure) {
        let _ = (ctx, pressure);
    }
}

impl MorphContext {
    /// The highest grade reported since the app last resumed (or started);
    /// None when there has been none. Cleared just before on_resume, as
    /// flankr clears its own on resume: back in front, the app may grow again.
    pub fn memory_pressure(&self) -> Option<MemoryPressure>;
}
```

`on_trim_memory` and `on_low_memory` stay as they are; an app uses
whichever it likes. Scripted runs need no new verb: `trim-memory <level>`
and `low-memory` call the new hook too, and `resume` clears the grade.

## 3. What stays with the app

What each grade drops. flankr keeps its views and screens (the most recent
closed view and ±5 screens at `UiHidden`, ±1 at `Background`, the screen
at `Low`); ShotToss its tiers (at `UiHidden` nothing, at `Background` the
in-memory decode queue and preview texture, at `Low` the atlas's slot
table as well). libmorph does not free anything for the app at any grade.
The GPU release of the device-may-be-absent plan keeps its own trigger
and is not driven by this type.

## 4. How flankr would adopt it (not in this plan)

flankr's `memory.rs` becomes `MemoryPressure::from_trim` and `LOW_MEMORY`,
and its `memory_pressure` is called from `on_memory_pressure`.
`flankr_companion::Pressure` is platform-free and need not depend on
libmorph: a one-to-one `From<MemoryPressure>` in the app keeps it, or the
client takes libmorph's type. Either is flankr's own plan, after this is
released.

## 5. Files

- `src/memory.rs`: the type, `from_trim`, tests (each level's grade,
  flankr's test that `RunningCritical` grades above `UiHidden`).
- `src/app.rs`: the hook.
- `src/context.rs`: `memory_pressure` and the slot it reads.
- The runners: the call after each `on_trim_memory` and `on_low_memory`,
  the clearing before `on_resume`.
- `src/lib.rs`: the re-export. `README.md`, `AGENTS.md`'s Core API.

## 6. Versioning

Additive: a new `#[non_exhaustive]` type, a defaulted hook and a method.
A libmorph patch release, numbered when released (after the all-files
access and disk pressure plans, if they go first). libmobile and
libmorph-build do not change.

## 7. Steps

1. **The grades, the hook and the context's value.**
   **Done when:** `cargo fmt -p libmorph -- --check`; `cargo clippy -p
   libmorph --all-targets` with default features and `--features script`,
   and `--target aarch64-linux-android --features mobile`: no new
   warnings; `cargo test -p libmorph --features script` passes, including
   a script of `trim-memory running-critical`, `trim-memory ui-hidden`,
   `resume`, `low-memory` that sees the hook with `Low`, `UiHidden`, `Low`
   and `memory_pressure()` as `Low`, `Low`, then `None` after the resume,
   then `Low`; on the S23, sending the app to the background logs
   `on_memory_pressure(UiHidden)` and, a while later or with
   `adb shell am send-trim-memory <pkg> BACKGROUND`, `Background`.
2. **Release** on the owner's word.
   **Done when:** tagged, and ShotToss builds against it.
