---
title: App icons through libmorph-build
date: 2026-10-24
status: Draft
covers:
  - "libmorph-build turning on libmobile's `icons` and re-exporting
    `gradle::Icon`, so `gradle_project(id).icon(Icon::png(..))` works in
    an app's build script"
  - Desktop icons, left to a later plan
sources:
  - "Owner (2026-10-24, relayed by the handlr session): \"add icon
    support with png-based at various sizes (vector later) and convert
    them to Android resources automatically, should be compatible with
    desktop use also (later)\"."
  - "libmobile docs/design/20261024_03_APP_ICON_PLAN.md: `Icon`,
    `Project::icon`, the resources and manifest attributes, feature
    `icons` (host-only)."
  - "libmorph-build today: depends only on libmobile, re-exports
    `libmobile::gradle`, and `gradle_project(id)` returns its `Project`."
related:
  - libmobile docs/design/20261024_03_APP_ICON_PLAN.md
---

# App icons through libmorph-build

## 1. What changes

- `crates/libmorph-build` depends on libmobile with `features =
  ["icons"]`. libmorph-build is a build-dependency only, so the `image`
  crate is built for the host and never reaches the APK.
- `libmorph_build::gradle::Icon` is libmobile's, through the existing
  re-export, so an app writes:

  ```rust
  libmorph_build::gradle_project("dev.tmzt.handlr")
      .icon(Icon::png("icon/icon-1024.png").background_color([0x10, 0x14, 0x1c]))
      .generate()?;
  ```
- libmorph itself does not change. The window icon on the desktop and the
  macOS bundle icon are a later plan, reusing the same declaration
  through libdevice.

## 2. Steps

1. **libmorph-build** with the feature, after libmobile's release, and an
   icon in the `urlopen` Android example.
   **Done when:** `cargo test -p libmorph-build` and clippy pass, and the
   example's APK lists its icons (`aapt dump badging`).
2. **Release** libmorph-build, on the owner's word. Additive (a new
   re-exported type and builder method).

## Amendment — 2026-10-24 13:55 EDT: approved

The owner approved this plan with libmobile's
`docs/design/20261024_03_APP_ICON_PLAN.md` on 2026-10-24, with its
recommended answers and the owner's note that PNG describes the bitmap
sources, while the generated files are in whatever format each platform
takes (that plan's amendment "approved; sources are PNGs, outputs are what
the platform takes"). The status stays `Draft` until the first commit
that implements it.
