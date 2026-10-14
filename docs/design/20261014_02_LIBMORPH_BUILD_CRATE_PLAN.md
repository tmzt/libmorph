---
title: libmorph-build, the build-script crate for Android apps
date: 2026-10-14
status: Implemented
covers:
  - A crate `crates/libmorph-build` in the libmorph repository, re-exporting
    `libmobile::gradle` with libmorph's `links` key preset
  - Turning the repository root into a Cargo workspace
  - Android apps naming `libmorph-build` instead of libmobile as their
    build-dependency
sources:
  - Owner's question (2026-10-14), on the README's Android section: "is this
    language needed now?" Answer: yes, because the app's build.rs calls
    libmobile::gradle, so the app must name libmobile as a build-dependency;
    making libmorph the build-dependency would build wgpu, winit and cpal
    for the host
  - Owner's direction (2026-10-14): "what does the libmorph-build approach
    look like, a ./crates/libmorph-build ?" then "write a short plan and
    approve"
  - libmobile 791b2ae: `gradle::Project::env_prefix` and
    `gradle::forward_metadata`
related:
  - AGENTS.md "Gradle Handoff (Android)": its app setup changes (step 3)
  - README.md "Android": its app setup changes (step 2)
  - docs/design/20261014_01_CAMERA_VIEW_AND_CAPTURE_PLAN.md (Hold): section 7
    has the example app depend on libmobile with `env_prefix("libmorph")`.
    Moving that to libmorph-build needs an amendment there, agreed separately
    (Rule 2).
---

# libmorph-build

## 1. Aim

An Android app built on libmorph names only libmorph crates:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
libmorph = { git = "https://github.com/tmzt/libmorph", features = ["mobile"] }

[build-dependencies]
libmorph-build = { git = "https://github.com/tmzt/libmorph" }
```

```rust
// build.rs
fn main() {
    libmorph_build::gradle_project("com.example.app")
        .label("Example")
        .generate()
        .expect("gradle project");
}
```

Nothing changes in how the Gradle script reaches the app. libmorph's
`build.rs` still republishes libmobile's metadata as
`DEP_LIBMORPH_GRADLE_SCRIPT`, which Cargo sets in the app's build-script
process because the app depends on libmorph directly. libmorph-build's code
runs in that process and reads it there.

## 2. Layout

```
Cargo.toml                       [package] libmorph + [workspace]
crates/libmorph-build/Cargo.toml
crates/libmorph-build/src/lib.rs
```

Root `Cargo.toml` gains:

```toml
[workspace]
members = [".", "crates/libmorph-build"]
```

`crates/libmorph-build/Cargo.toml`: name `libmorph-build`, version
`0.1.0` (kept equal to libmorph's), edition 2024, license
`MIT OR Apache-2.0`. Its only dependency is
`libmobile = { git = "https://github.com/tmzt/libmobile" }`, so a host build
compiles libmobile and `log` and nothing else.

## 3. API (`crates/libmorph-build/src/lib.rs`)

```rust
pub use libmobile::gradle;

/// libmorph's `links` key.
pub const LINKS: &str = "libmorph";

/// A Gradle project that reads libmorph's forwarded metadata.
pub fn gradle_project(application_id: &str) -> gradle::Project {
    gradle::Project::new(application_id).env_prefix(LINKS)
}
```

`LINKS` must equal `links` in the root `Cargo.toml`. A unit test reads
the root manifest (`../../Cargo.toml` from `CARGO_MANIFEST_DIR`) and checks
the two match.

## 4. Resolution

- Cargo finds `libmorph-build` in the git repository by package name; apps
  give no path.
- Both dependencies use the same URL, so one lockfile entry pins one commit
  for libmorph and libmorph-build together.
- The workspace's `Cargo.lock` at the root covers both crates. The
  local-override rule (no lockfile resolved with a `[patch]` active) is
  unchanged.

## 5. Steps

1. **Add the crate and the workspace** (one commit, the Rule 3.1 exception
   for a crate's first commit): section 2 and section 3, with the `LINKS` test.
   **Done when:** `cargo test -p libmorph-build` and `cargo test -p libmorph`
   pass and clippy is clean for both. Also, a scratch app with only libmorph
   (`mobile`) and libmorph-build, checked for `aarch64-linux-android` with
   no local `[patch]`, writes its Gradle project, and that project applies
   libmobile's `libmobile.gradle`.
2. **README.md: the Android section uses libmorph-build** (section 1).
   **Done when:** the README names no libmobile dependency in the app setup.
3. **AGENTS.md: the Gradle Handoff section uses libmorph-build.**
   **Done when:** AGENTS.md's app setup matches section 1, and libmobile
   appears only as libmorph's own build-dependency and libmorph-build's
   dependency.

## Amendment — 2026-10-14 12:31 EDT: Approved for implementation

The owner approved this plan as written (2026-10-14). Its status stays
`Draft` until the commit that starts step 1 sets `In implementation`, as
Rule 1.5 says.

## Amendment — 2026-10-14 12:33 EDT: Implementation started

Step 1 is committed with this amendment. The status is `In implementation`,
and this document is now immutable (Rule 1.5).

## Amendment — 2026-10-14 12:36 EDT: Status In implementation → Implemented

Every step in section 5 is done and meets its **Done when:** check:

- Step 1, the crate and the workspace: `933c5be`
- Step 2, README.md: `d146bff`
- Step 3, AGENTS.md: `48962bf`
