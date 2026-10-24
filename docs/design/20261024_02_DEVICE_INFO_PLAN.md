---
title: The device's vendor, make and model
date: 2026-10-24
status: Draft
covers:
  - "`MorphContext::hardware() -> Option<&Hardware { vendor, make, model }>`"
  - Why it is not `ctx.device()`, and what desktop and the web return
sources:
  - "Owner (2026-10-24, relayed by the handlr session): \"needs libmorph
    support, does not expose private identifiers just vendor, make and
    model\". handlr asked for `ctx.device() -> Option<Device>` with
    `vendor` from `Build.MANUFACTURER`, `make` from `Build.BRAND` and
    `model` from `Build.MODEL`; `None` on desktop and the web; no serial,
    Android ID, IMEI, advertising ID or anything else identifying. It
    uses the vendor for notes such as Samsung's app sleeping."
  - "libmorph today: `MorphContext::device()` already returns
    `&libdevice::DeviceSpec` (the emulated or closest spec), used by apps
    and examples."
  - libmobile docs/design/20261024_02_BUILD_INFO_PLAN.md (`build_info()`)
related:
  - libmobile docs/design/20261024_02_BUILD_INFO_PLAN.md
---

# The device's vendor, make and model

## 1. API

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Hardware {
    /// Android: Build.MANUFACTURER ("samsung").
    pub vendor: String,
    /// Android: Build.BRAND ("samsung").
    pub make: String,
    /// Android: Build.MODEL ("SM-S918U").
    pub model: String,
}

impl MorphContext {
    /// The real device's vendor, make and model, and nothing that
    /// identifies the unit or its owner. Android only; None on the
    /// desktop, the web and in scripted runs.
    pub fn hardware(&self) -> Option<&Hardware>;
}
```

- **The name.** `ctx.device()` already returns the `DeviceSpec`, so
  changing it would break every app. `hardware()` is new and additive
  (section 3, question 1).
- **Android:** read once at bring-up from libmobile's `build_info()`
  (`manufacturer` to `vendor`, `brand` to `make`, `model` to `model`).
- **Desktop and the web:** `None`. The emulated device is `device()`;
  the host Mac's model is not what an app running in emulation should
  see. **Scripted runs:** `None`.
- Never logged by libmorph.

## 2. Steps

1. **The method**, after libmobile's release, with a test that it is
   `None` in scripted runs, and an example logging it.
   **Done when:** tests and clippy pass on every host, and on the S23 the
   example logs the three values libmobile's check found.
2. **Release**, on the owner's word, in the release after libmobile's.
   Additive.

## 3. Open questions for the owner

1. **The name.** `hardware()` and `Hardware`, since `device()` is taken.
   Recommended: yes. Alternatives: `device_info()`, `real_device()`.
2. **0.4.0.** It needs a libmobile release (0.2.4) first. Recommended:
   build it now and ship it in 0.4.0 with the other wrappers if the
   owner approves today; otherwise the release after.
3. **Desktop.** `None`, as handlr suggested. Recommended: yes; the Mac's
   model would be a libdevice plan if an app ever needs it.
