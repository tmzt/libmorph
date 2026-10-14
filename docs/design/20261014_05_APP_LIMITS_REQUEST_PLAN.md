---
title: App-requested device limits (LimitsRequest)
date: 2026-10-14
status: Draft
covers:
  - A public `LimitsRequest { required, preferred }` of whole `wgpu::Limits`
  - `MorphApp::limits(base)`, an associated function called before `init`
  - Resolving the request against the adapter in `Gpu::new`, with a pure,
    host-tested function and a new `GpuError::Limits`
  - Threading the request through the desktop, web and Android runners
sources:
  - Owner's request (2026-10-14), relayed by another Claude session: items
    1–7 and the final Done when, including "Never clamp `required`
    silently" and "Ask the owner before changing any other existing public
    signature"
  - Field report: Galaxy S23 (Adreno 740, Vulkan). A compute shader binding
    8 storage buffers fails with "Too many bindings of type StorageBuffers in
    Stage COMPUTE, limit is 4, count was 8", although the adapter supports
    more. libdevice's removed offscreen `Device` asked for
    `max_storage_buffers_per_shader_stage: 8` on top of the downlevel
    baseline. That bump was lost when bring-up moved into libmorph.
  - wgpu-types 30.0.1 `src/limits.rs`:
    - `downlevel_defaults()` has `max_storage_buffers_per_shader_stage: 4`,
      and `downlevel_webgl2_defaults()` has 0;
    - `check_limits(&allowed) -> bool`;
    - `check_limits_with_fail_fn(&allowed, fatal, fn(name, self, allowed))`;
    - `or_better_values_from(other)` and `or_worse_values_from(other)`.

    All four go through the `with_limits!` table, which marks each field as a
    maximum (`Ordering::Less`) or a minimum (`Ordering::Greater`, the
    alignments such as `min_uniform_buffer_offset_alignment`).
  - libmorph 13329c2:
    - `src/gpu.rs`: `GpuError` (not `#[non_exhaustive]`), and `Gpu::new`, which
      requests `downlevel_defaults` or `downlevel_webgl2_defaults`, then
      `.using_resolution(adapter.limits())`;
    - `src/runner/winit.rs` `Handler::boot` and `src/runner/android.rs` `boot`
      both call `Gpu::new`;
    - `src/context.rs` `MorphContext::limits`.
related:
  - AGENTS.md "GPU Bring-up" (step 3 updates it)
  - docs/design/20261014_04_SCRIPTED_RUNS_AND_PNG_CAPTURE_PLAN.md (Draft):
    its headless runner also calls `Gpu::new` (section 4.1 makes the surface
    optional). Whichever plan is implemented second adapts to the other's
    `Gpu::new` signature. See section 6.
---

# App-requested device limits

## 1. Problem

`Gpu::new` requests a fixed baseline: `downlevel_defaults()`, or
`downlevel_webgl2_defaults()` on a GL adapter, with the texture sizes raised
to the adapter's by `using_resolution`. Pipelines that need more than the
baseline are invalid, and the app shows a blank screen. A device's limits are
fixed at `request_device`. libmorph owns the only device that can render to
its surface, so an app cannot fix this after bring-up. A second device from
an exposed instance would not be able to present.

The app has to say what it needs before the device exists.

## 2. `LimitsRequest`

In `src/gpu.rs`, re-exported from `lib.rs` as `libmorph::LimitsRequest`:

```rust
/// What an app asks of the device, as whole `wgpu::Limits` values.
#[derive(Clone, Debug, PartialEq)]
pub struct LimitsRequest {
    /// Bring-up fails if the adapter can't meet these.
    pub required: wgpu::Limits,
    /// Asked for where the adapter allows; never below `required`.
    pub preferred: wgpu::Limits,
}

impl LimitsRequest {
    /// `required` and `preferred` both `base`: today's behaviour.
    #[must_use]
    pub fn base(base: &wgpu::Limits) -> Self;
}
```

The fields are named, not a tuple. They hold whole `wgpu::Limits` values, so
libmorph never mirrors wgpu's ~30 fields or tracks them across wgpu majors.
`wgpu::Limits` is `Clone + Debug + PartialEq`, so the derives hold.

## 3. `MorphApp::limits`

A new associated function on the trait, with a default body. It has no
`self`, because the app doesn't exist yet:

```rust
/// The device limits to ask for, before `init`. `base` is what libmorph
/// asks for by default: the downlevel defaults (WebGL2 defaults on a GL
/// adapter), with texture sizes raised to the adapter's.
fn limits(base: &wgpu::Limits) -> LimitsRequest {
    LimitsRequest::base(base)
}
```

Adding a trait item with a default body doesn't break existing
implementations, and the default leaves every existing app unchanged.

An app that needs more:

```rust
fn limits(base: &wgpu::Limits) -> LimitsRequest {
    let required = wgpu::Limits { max_storage_buffers_per_shader_stage: 8, ..base.clone() };
    LimitsRequest { preferred: required.clone(), required }
}
```

`base` depends on the adapter. On a GL adapter (WebGL2, older Android) it
has `max_storage_buffers_per_shader_stage: 0`, so the example above fails
there with `GpuError::Limits`, by design. An app with a fallback path keeps
`required` at `base` and puts the higher value only in `preferred`. It then
reads `ctx.limits()` in `init` to choose the path.

## 4. Resolution

A pure function in `src/gpu.rs`, with no GPU, so the host can test it:

```rust
pub(crate) fn resolve_limits(
    request: &LimitsRequest,
    adapter: &wgpu::Limits,
) -> Result<wgpu::Limits, GpuError>
```

1. Collect every field where `request.required` fails against `adapter`,
   with `required.check_limits_with_fail_fn(adapter, false, ...)`. `fatal`
   is `false`, so all failures are reported, not just the first. If there
   are any, return
   `GpuError::Limits(Vec<LimitFailure>)`. `required` is never clamped
   silently.
2. Otherwise return
   `request.preferred.clone().or_worse_values_from(adapter).or_better_values_from(&request.required)`:
   - `preferred`, clamped to what the adapter offers;
   - then raised back to `required` wherever the clamp went below it.

   Step 1 guarantees `required` is within `adapter`, so the result is too.

Both helpers follow wgpu-types' own direction table, so alignments, where
smaller is better, resolve the right way without any per-field code in
libmorph.

`base` itself isn't part of the resolution: it only feeds `A::limits`.

```rust
/// One limit the adapter can't meet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitFailure {
    /// The `wgpu::Limits` field name, e.g. `max_storage_buffers_per_shader_stage`.
    pub name: &'static str,
    pub required: u64,
    pub available: u64,
}
```

`GpuError::Limits(Vec<LimitFailure>)` displays as:

`the adapter can't meet the app's required limits: max_storage_buffers_per_shader_stage 8 (adapter 4)`

with the failures joined by `, `.

**Public enum change:** `GpuError` is not `#[non_exhaustive]`. Adding
`Limits` breaks any code outside the crate that matches `GpuError`
exhaustively. Section 7, decision 1, asks the owner how to handle this.

## 5. `Gpu::new` and the runners

`Gpu::new` gains a `limits: fn(&wgpu::Limits) -> LimitsRequest` argument.
The runners pass `A::limits`.

- A function pointer is `'static + Copy`, so it moves into the web's
  `spawn_local` future without making `Gpu::new` generic over the app.
- Inside, after the adapter is chosen:
  1. compute `base` as today;
  2. `request = limits(&base)`;
  3. `granted = resolve_limits(&request, &adapter.limits())?`;
  4. request the device with `required_limits: granted.clone()`;
  5. store `granted` in `Gpu::limits`, which `ctx.limits()` returns.
- `Gpu::new` is crate-private, so its signature can change freely.
  - Desktop and web: `Handler<A>::boot` passes `A::limits`. A `Limits`
    failure goes through the existing `fail` path, like any other bring-up
    error.
  - Android: `boot` passes `A::limits`. A failure is logged as
    `libmorph: <error>`, and the runner stops rendering (`failed = true`),
    like the other bring-up failures.
- `request_device` can still fail after a successful resolution (a driver
  refusal). That stays `GpuError::Device`.

## 6. Headless or offline GPU

There is no `OfflineConfig` in libmorph. The offscreen `Device` that
AGENTS.md says may come back is not planned. The nearest is plan 04 (Draft),
whose headless runner calls `Gpu::new` with no surface. That runner is
generic over `A: MorphApp`, so it passes `A::limits` like the others, and
`ScriptRun` needs no limits field. If an offline GPU without an app is
planned later, its config gains `limits: LimitsRequest`, defaulting to
`LimitsRequest::base(&base)` and resolved through `resolve_limits`.

## 7. Open decisions (owner)

1. **`GpuError::Limits` changes a public enum** that is not
   `#[non_exhaustive]`. Recommended: add the variant, and mark `GpuError`
   `#[non_exhaustive]` in the same commit. That is one break now, instead of
   one for every future variant. `MorphError` is already
   `#[non_exhaustive]`. The alternative is to add the variant alone.
   Either way, exhaustive matches outside the crate stop compiling.
   libmorph is 0.1.0, git-only, and has no known external matchers.
2. **The baseline stays as it is.** Recommended: `base` remains the
   downlevel defaults, so no existing app changes, and apps opt in through
   `limits`. The alternative is to restore libdevice's old bump of 8
   storage buffers in `base` itself. That would change the device every app
   gets, and on GL adapters, where the limit is 0, it would make bring-up
   fail.

## 8. Steps

Each step is one commit. It passes `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets` with no new warnings, and
`cargo test --workspace`, with default features and with
`--no-default-features`. It also passes
`cargo check --target aarch64-linux-android --features mobile` (Rule 3.4).
The first step sets `status: In implementation` (Rule 1.5).

1. **Types and resolution** (sections 2 and 4): `LimitsRequest`,
   `LimitFailure`, `GpuError::Limits` (with decision 1), and
   `resolve_limits`. `Gpu::new` resolves `LimitsRequest::base(&base)`
   through it, so behaviour is unchanged and nothing is dead code.
   **Done when:** unit tests of `resolve_limits` cover:
   - the base request, which resolves to the base;
   - `required` met;
   - `required` unmet, with the error naming
     `max_storage_buffers_per_shader_stage` and both values, and every
     failing field reported when several fail;
   - `preferred` above the adapter, clamped to it;
   - `preferred` below `required`, raised to `required`;
   - `min_uniform_buffer_offset_alignment`, where smaller is better: a
     preferred 64 on an adapter offering 256 resolves to 256, and a
     required 64 on that adapter fails.
2. **`MorphApp::limits` and the runners** (sections 3 and 5): the trait
   function, the `fn` pointer argument on `Gpu::new`, and the desktop, web
   and Android call sites. The touch example stays unchanged.
   **Done when:** the step's checks pass, including
   `cargo clippy --target wasm32-unknown-unknown --features web`, and the
   touch example builds without changes.
3. **AGENTS.md** (its own commit): the "GPU Bring-up" table's limits column
   and the text below it describe `LimitsRequest`, `MorphApp::limits` and
   `GpuError::Limits`. "Core API" lists the new trait function.
   **Done when:** AGENTS.md describes sections 2–5 as built. The checks
   above pass with default features and with `--no-default-features`, along
   with the Android `--features mobile` check. The commits are pushed to
   github.com/tmzt/libmorph, and the pushed commit hash is reported to the
   owner.

## Amendment — 2026-10-14 15:33 EDT: Approval, decisions, and the scripted runner

The owner asked for the work to be finished (2026-10-14), which approves the
plan. The owner did not choose otherwise on section 7, so its recommended
choices stand:

1. `GpuError` gains `Limits` and becomes `#[non_exhaustive]` in the same
   commit (step 1).
2. `base` stays as it is: the downlevel defaults, or the WebGL2 defaults on
   a GL adapter, with texture sizes raised. Apps opt in through `limits`.

Plan 04 is now implemented. `Gpu::new` already takes
`Option<&wgpu::Surface>`, and the headless runner (`src/runner/script.rs`)
calls it. This replaces section 6's "whichever plan is implemented second"
note: step 2 also passes `A::limits` from the headless runner, and
`ScriptRun` gains no limits field. Step 2's checks include
`--features script`, and its Done when adds that the scripted run test
passes.
