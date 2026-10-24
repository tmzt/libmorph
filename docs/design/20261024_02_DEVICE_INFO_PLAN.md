---
title: The device's vendor, make and model
date: 2026-10-24
status: In implementation
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

## Amendment — 2026-10-24 13:04 EDT: the name is `brand_info`

The owner's answer to question 1, relayed by the handlr session
(2026-10-24): "brand_info". This replaces `hardware()` and `Hardware` in
the front matter and section 1: the method is
`MorphContext::brand_info() -> Option<&BrandInfo>`, and the type is
`BrandInfo { vendor, make, model }`, otherwise as section 1 describes.
libmobile's `build_info()` keeps its name, which follows Android's
`Build`, unless the owner says otherwise. Questions 2 and 3 are open.

## Amendment — 2026-10-24 13:06 EDT: approved, for 0.4.0

The owner approved this plan and libmobile's
`docs/design/20261024_02_BUILD_INFO_PLAN.md` on 2026-10-24 ("approve,
plan a 0.4.0 with what we've planned"). Section 3:
1. The name is `brand_info` (the amendment before this one).
2. It ships in libmorph 0.4.0, after libmobile 0.2.4 with `build_info()`.
3. Desktop, the web and scripted runs return `None` ("for now it can
   return None", relayed by the handlr session).

The status stays `Draft` until the first commit that implements it.

## Amendment — 2026-10-24 13:08 EDT: the name is `build_info`

The owner's decision (2026-10-24: "should it just be build_info then?",
then "that works"). This replaces the amendment "the name is
`brand_info`": the method is `MorphContext::build_info() ->
Option<&BuildInfo>`, the same name as libmobile's `build_info()`, and
the type is libmorph's own `BuildInfo { vendor, make, model }`, mapped
from libmobile's `manufacturer`, `brand` and `model`. Each field's
rustdoc names its Android source. Everything else in the approval
amendment stands.

## Amendment — 2026-10-24 13:12 EDT: in 0.4.1

The owner's decision (2026-10-24): libmorph 0.4.0 is released without it,
so that `open_link` and the foreground service reach handlr sooner. This
replaces answer 2 of the approval amendment: `build_info()` ships in
libmorph 0.4.1, after libmobile 0.2.4. Additive.

## Amendment — 2026-10-24 13:35 EDT: implementation has started; the document is now immutable

Step 1 starts with this commit, against libmobile 0.2.4 (`f4d5c04`, tag
`v0.2.4`). The document now changes only on the owner's explicit
instruction.
