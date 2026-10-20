---
title: Persistent pipeline cache
date: 2026-10-16
status: Implemented
covers:
  - Requesting `wgpu::Features::PIPELINE_CACHE` when the adapter offers it
    and the app has a data directory (Android)
  - One `wgpu::PipelineCache` per run, offered to the app as
    `MorphContext::pipeline_cache()`
  - Loading its data from the app's data directory before `init`, and
    saving it after `init`, on suspend and on exit
sources:
  - Request from the tower defense game (2026-10-16, on the owner's
    instruction): on a Galaxy S23 (Adreno 740, Vulkan), creating one compute
    pipeline takes about 0.9 s on every launch (23–27 s before its shader
    was reduced). The game will pass the cache to its
    `create_compute_pipeline` and `create_render_pipeline` calls.
  - wgpu 30.0.1:
    - `Features::PIPELINE_CACHE`, implemented on Vulkan only (not DX12,
      Metal, GL or WebGPU);
    - `unsafe Device::create_pipeline_cache(&PipelineCacheDescriptor {
      label, data: Option<&[u8]>, fallback: bool })`. With `fallback: true`,
      data that cannot be used (another wgpu version, another adapter, a
      driver update) gives an empty cache instead of an error;
    - `PipelineCache::get_data() -> Option<Vec<u8>>`;
    - `util::pipeline_cache_key(&AdapterInfo) -> Option<String>`:
      `wgpu_pipeline_cache_vulkan_<vendor>_<device>` on Vulkan, `None`
      elsewhere;
    - the `PipelineCache` docs: desktop drivers mostly keep their own
      caches, Android drivers leave it to the app, and the data should be
      written to a temporary file and renamed over the old one.
  - `MorphContext::data_dir()`: Android's internal data path, `None` on
    desktop and the web (plan 20261014_03).
related:
  - docs/design/20261014_03_SHELL_INPUT_AND_LIFECYCLE_PLAN.md (`data_dir`,
    `on_suspend`, one suspend per exit)
  - docs/design/20261014_05_APP_LIMITS_REQUEST_PLAN.md (device bring-up)
---

# Persistent pipeline cache

## 1. Goal

On Android with Vulkan, pipelines an app has built once are not compiled
again on the next launch. libmorph creates one `wgpu::PipelineCache`, loads
it from the app's data directory, offers it to the app, and saves it back.
The app only passes `ctx.pipeline_cache()` as the `cache` field of its
pipeline descriptors.

Everywhere else (desktop, the web, Android on GL) `pipeline_cache()` is
`None` and nothing else changes. Passing `None` as `cache` is what apps do
today.

## 2. When there is a cache

All three must hold:

1. the host has a data directory (`ctx.data_dir()` is `Some`, so Android
   only);
2. `wgpu::util::pipeline_cache_key(&adapter.get_info())` is `Some` (Vulkan
   only);
3. the adapter offers `Features::PIPELINE_CACHE`.

Then `Gpu::new` adds `PIPELINE_CACHE` to `required_features`, alongside
nothing else (libmorph requests no features today). Desktop Vulkan
(Linux) fails rule 1, because its drivers keep their own caches and it has
no data directory.

`Gpu::new` gains a `want_pipeline_cache: bool` argument, `true` from the
Android runner and `false` elsewhere, because the data directory is known
to the runner, not to `Gpu`.

## 3. File

```
<data_dir>/libmorph/<pipeline_cache_key>.bin
```

for example
`/data/user/0/<package>/files/libmorph/wgpu_pipeline_cache_vulkan_20803_1124073472.bin`.
The `libmorph/` directory keeps libmorph's files apart from the app's own.
The key changes with the GPU, and wgpu's own header in the data rejects
data from another wgpu version or driver (see `fallback` in the sources).

## 4. Loading

In a new `src/pipeline_cache.rs`, after the device exists and before
`init`:

1. read the file. A missing file, or any read error, is no data (logged at
   debug level for a missing file, warn otherwise);
2. create the cache with `label: Some("libmorph")`, `data` from step 1 and
   `fallback: true`;
3. log `libmorph: pipeline cache: loaded <n> bytes` (or `new`) at info.

```rust
pub(crate) struct PipelineCacheStore {
    cache: wgpu::PipelineCache,
    path: PathBuf,
    /// Hash of the last data read or written, to skip unchanged saves.
    saved: Option<u64>,
}
```

`create_pipeline_cache` is `unsafe`: the data must come from an earlier
`get_data`. It does: libmorph is the only writer of the file, and writes it
whole through a rename (section 5). wgpu checks its header and, with
`fallback: true`, ignores data it cannot use. What it cannot detect is a
file damaged after its header, which its own docs accept as a risk of
keeping the data on disk. The safety comment records this.

## 5. Saving

`PipelineCacheStore::save()`:

1. `get_data()`; `None` means nothing to save;
2. skip if the hash equals `saved` (`std::hash::DefaultHasher`; it only has
   to match within one run);
3. create `<data_dir>/libmorph/` if needed, write `<key>.bin.tmp`, then
   rename it over `<key>.bin`;
4. on success set `saved` and log `saved <n> bytes` at debug; on failure,
   warn and keep running.

It runs on the main thread at three points:

- **after `init` returns** (in `Shell::with_app`), so pipelines built in
  `init` are kept even if the process is later killed without a clean
  suspend;
- **on suspend**, after the app's `on_suspend`, in `Shell::detach`. On
  Android this is `WindowDestroyed`, the last callback a backgrounded app
  can count on;
- **on exit**, in `Shell::shutdown`, when `on_suspend` runs there.

The cost is one `get_data` copy and, when the data has changed, one file
write. Pipelines built after `init` reach the file at the next suspend or
exit.

## 6. API

```rust
impl MorphContext {
    /// The pipeline cache to pass as `cache` in pipeline descriptors.
    /// `None` unless the host persists one (Android on Vulkan).
    #[must_use]
    pub fn pipeline_cache(&self) -> Option<&wgpu::PipelineCache>;
}
```

`PipelineCache` is cheap to clone (wgpu handles are reference counted), so
an app that builds pipelines on another thread can clone it.
`ctx.device().features()` shows whether `PIPELINE_CACHE` was granted.

## 7. Steps

1. **Bring-up and loading.** The `want_pipeline_cache` argument, the
   feature request, `src/pipeline_cache.rs` with loading, and
   `MorphContext::pipeline_cache()`. Unit tests, on a temporary directory:
   the path from a key; a missing file is no data; save writes through the
   temporary file and leaves no `.tmp` behind; an unchanged save is skipped.
   The file functions take bytes, so the tests need no GPU.
   **Done when:** `cargo test` passes and clippy passes on every host.
2. **Saving.** The three save points of section 5.
   **Done when:** the Android build passes clippy, and the tests of step 1
   still pass.
3. **Docs.** `MorphContext` in AGENTS.md, and the README.
   **Done when:** both describe `pipeline_cache()` and where the data is
   kept.
4. **Trial.** The tower defense game passes the cache to its pipelines.
   **Done when:** on the owner's S23, the second launch logs
   `pipeline cache: loaded <n> bytes` with `n > 0`, and the game's
   pipeline creation time drops from about 0.9 s. The result goes in
   CHANGES.md.

## 8. Open questions for the owner

1. **Opting out.** Recommended: no switch. A cache the app ignores costs a
   small file and one save per suspend. The alternative is a
   `const PIPELINE_CACHE: bool = true` on `MorphApp`, like
   `SRGB_SURFACE`.
2. **Desktop Vulkan.** Recommended: none, as in section 2. Linux and
   Windows drivers keep their own caches, and desktop has no data
   directory. It could be added with a cache directory when those hosts
   are supported.
3. **Cache growth.** wgpu cannot shrink a cache, so data from old shaders
   stays in the file after an app update. Recommended: accept it for now.
   The file is replaced whenever the driver or wgpu version changes. An
   app that needs it smaller can delete `<data_dir>/libmorph/`.

## Amendment — 2026-10-20 00:02 EDT: approved, the open questions answered as recommended

Owner (2026-10-20): "make the change, use the standard plan mechanism".
The plan is approved as written, with section 8's recommendations:

1. **Opting out:** no switch.
2. **Desktop Vulkan:** no cache.
3. **Cache growth:** accepted; an app that needs the file smaller deletes
   `<data_dir>/libmorph/`.

Step 4's trial is now the game's props pipeline, a render pipeline built
on a worker thread (about 1.3 s on the S23, unlocked), which clones
`ctx.pipeline_cache()` as section 6 allows; the compute pipeline named
in the sources may pass it too.

## Amendment — 2026-10-20 00:02 EDT: implementation has started; the document is now immutable

Implementation has started with step 1, and this document is now
immutable (Rule 1.5).

## Amendment — 2026-10-20 00:14 EDT: Implemented

Every step is done and its **Done when:** holds:

1. Bring-up and loading: a2b2f60.
2. Saving: a2b2f60 (Android clippy clean with `--features mobile`).
3. Docs: a2b2f60 (README), 2d0cda0 (AGENTS.md).
4. Trial: in the tower defense game on the S23, the second launch logged
   `pipeline cache: loaded 627724 bytes` and the slowest pipeline's build
   went from about 1.3 s to 34 ms (CHANGES, this commit's addendum).

Released in libmorph 0.2.1 (tag v0.2.1).
