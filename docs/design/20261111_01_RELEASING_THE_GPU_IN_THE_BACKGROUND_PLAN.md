---
title: Releasing the GPU in the background - an app may give back its device while it is off screen
date: 2026-11-11
status: Draft
covers: >
  An associated const, `MorphApp::GPU_IN_BACKGROUND`, by which an app
  says whether libmorph keeps its device while the app is off screen
  (`Keep`, the default and today's behaviour) or releases it
  (`Release { after }`): after `after` without a surface, or at once on
  a trim at `Background` or above or a low-memory report, libmorph calls
  `on_gpu` with `GpuEvent::Release`, drops its own GPU objects and the
  device, queue and adapter, and waits for wgpu to free them; on the
  next window it requests a device again with the same limits and calls
  `on_gpu` with `GpuEvent::Restored` before `on_resume`. On the desktop,
  for a releasing app, an occluded or minimised window stops frames and
  starts the same clock. A second const, `MEMORY_HINTS`, passes wgpu's
  own `MemoryHints` to the device request. A log of wgpu's allocator
  report and a check on the S23 measure what each step gives back.
  Not covered: the companion app's adoption, which is flankr's own plan;
  releasing the `wgpu::Instance`; the web.
sources: >
  The owner, 2026-11-10: "plan to handle backgrounding including freeing
  gpu memory, plan in libmorph as needed, currently using flankr while
  developing other gpu applications can cause the gpu app to run oit of
  memory and gpu res9urces"; then, on the proposal in that discussion
  (decisions 1 to 8 below, the measurement first, `Keep` the default,
  10 s): "yes" to writing it as a draft. The owner, 2026-11-05, on the
  OWNS plan: "overriddable behavior on MorphApp is just the old java
  applet API smells", which is why this adds one hook and not two.
related:
  - "docs/design/20261105_04_OWNS_AND_THE_RENDER_STATUS_PLAN.md (OWNS
    and `Owned::FRAME`: an app that owns the frame is released the same
    way; this plan adds a const beside `OWNS`, not a flag in it)"
  - "docs/design/20261029_02_AUDIO_STREAM_RECOVERY_PLAN.md (`on_audio`
    and `AudioEvent`, the shape `on_gpu` and `GpuEvent` follow; audio
    runs on without a device)"
  - "flankr: apps/companion, the first user, in a plan of its own once
    this one is approved"
---

# Releasing the GPU in the background

## 1. Goal

An app built on libmorph keeps its GPU device for the whole run. On
Android, leaving the screen drops only the surface, and `on_suspend`
promises the app that "the device, queue and every resource built on them
stay valid". An app that keeps its process alive off screen, such as
flankr's companion with its foreground service, therefore holds its
device memory for as long as it runs. Its atlases, textures, buffers and
pipelines, and the blocks wgpu's allocator keeps under
`MemoryHints::Performance`, all stay allocated while another GPU app runs
in front of it, and that app runs out of GPU memory and resources.

An app should be able to say that it gives its device back while it is off
screen, and libmorph should do the releasing and the restoring, so that
an app off screen holds no GPU memory at all.

## 2. Findings (read 2026-11-10, libmorph 0.7.1 at `2cc85df`)

1. `src/gpu.rs`: `Gpu { instance, adapter, device, queue, limits }`, built
   once by `Gpu::new` with `required_limits` from the app's
   `LimitsRequest` and `memory_hints: wgpu::MemoryHints::Performance`.
   `MorphContext` holds it as `pub(crate) gpu: Gpu`; apps reach it only
   through `ctx.device()`, `ctx.queue()`, `ctx.adapter_info()` and
   `ctx.limits()`.
2. `src/runner/android.rs`: `WindowDestroyed` calls `shell.detach()`,
   which drops the surface and calls `on_suspend`. `Pause` and `Resume`
   only set `resumed` and pause the audio. `LowMemory` and
   `TrimMemory(level)` are passed to the app. `poll_timeout` is zero while
   resumed with a surface, else `DEFAULT_POLL_TIMEOUT`.
3. `src/runner/winit.rs`: `suspended` detaches as on Android, which on
   macOS happens only when the app is hidden by the system, not when the
   window is minimised or covered. `RedrawRequested` requests the next
   frame every time, and `WindowEvent::Occluded` is not handled, so a
   covered or minimised window keeps drawing. `CurrentSurfaceTexture::
   Occluded` is treated as "nothing to draw this iteration".
4. libmorph's own GPU objects beyond the surface are the offscreen target
   of scripted runs, the overlay, and the camera's viewfinder.
5. The companion's `on_suspend` drops only its drawer's textures, and its
   memory pressure closes the drawer. Its msdf atlases, terminal textures,
   preview and pipelines are never dropped.
6. The script runner has `suspend`, `resume`, `low-memory`, `trim-memory
   <level>` and `frames <n>` at its `fps`.

## 3. Decisions

1. **The policy is a const**, beside `OWNS`, with nothing stored:

   ```rust
   /// What libmorph does with the device while the app is off screen.
   #[derive(Clone, Copy, Debug, PartialEq, Eq)]
   #[non_exhaustive]
   pub enum GpuInBackground {
       /// Keep it: resources stay valid and resuming is instant. Today's
       /// behaviour, and the default.
       Keep,
       /// Give it back once the app has been off screen for `after`, or
       /// at once under memory pressure (decision 3).
       Release { after: Duration },
   }

   pub trait MorphApp {
       const GPU_IN_BACKGROUND: GpuInBackground = GpuInBackground::Keep;
       // ...
   }
   ```

   `Duration::from_secs(10)` is the value the proposal names for an app
   that releases; it is the app's to choose.
2. **One hook**, following `on_audio`:

   ```rust
   #[derive(Clone, Debug, PartialEq, Eq)]
   #[non_exhaustive]
   pub enum GpuEvent {
       /// The device is about to go. Drop every wgpu object the app holds
       /// before returning; `ctx.device()` is valid until then.
       Release,
       /// A new device is up, with the same limits. Rebuild before the
       /// next frame; `on_resume` follows.
       Restored,
       /// Requesting a device again failed; libmorph tries again at the
       /// next window. `reason` is wgpu's error.
       RestoreFailed { reason: String },
   }

   fn on_gpu(&mut self, ctx: &mut MorphContext, event: &GpuEvent) {
       let _ = (ctx, event);
   }
   ```

   It is called only for an app whose `GPU_IN_BACKGROUND` is `Release`.
3. **When the device is released**, for `Release { after }`:
   - the surface has been gone for `after` (Android: from
     `WindowDestroyed`; desktop: from `suspended`, or from decision 7's
     occlusion), with no new window since;
   - at once, while there is no surface, on `TrimMemory` at
     `TrimLevel::Background` or above, or on `LowMemory`;
   - a trim or low-memory report while the app is on screen releases
     nothing.

   The Android runner's `poll_timeout` wakes at the deadline instead of
   waiting the full `DEFAULT_POLL_TIMEOUT`; the desktop runner uses
   `ControlFlow::WaitUntil`.
4. **Releasing**, in order: `on_gpu(Release)`; libmorph drops its
   offscreen target, overlay and viewfinder; it calls
   `device.poll(wgpu::PollType::Wait)` (or this wgpu's equivalent) so the
   queue's work and the dropped objects are finished; it drops the queue,
   device and adapter. The `wgpu::Instance` stays: it holds no device
   memory and keeps the next request quick. `on_trim_memory` and
   `on_low_memory` are still called after the release, as now.
5. **While released**: no `update` or `render`. The app's other callbacks
   (`on_service`, `on_intent`, camera, media, audio, trim) still arrive.
   The audio, the foreground service and the app's own threads are not
   touched. `ctx.device()`, `ctx.queue()`, `ctx.adapter_info()` and
   `ctx.limits()` keep their signatures and panic with "libmorph: the GPU
   is released (GPU_IN_BACKGROUND)" if called. A new
   `ctx.gpu_available() -> bool` says whether they may be called. Only an
   app that chose `Release` can reach that state, so for every other app
   nothing changes.
6. **Restoring**, at the next `WindowCreated` (Android) or `resumed`
   (desktop) after a release: libmorph requests an adapter and a device
   with the limits it was first granted (not the app's request again, so
   the app never sees different limits) and its memory hints, configures
   the new surface, then calls `on_gpu(Restored)` and then `on_resume`.
   If either request fails, the app gets `on_gpu(RestoreFailed)`, nothing
   is drawn, and libmorph tries again at the next window, logging each
   failure. The surface format is chosen again and can differ; the app
   reads `ctx.surface_format()` in `Restored`, as it does in `init`.
7. **Desktop occlusion**, for a releasing app only:
   `WindowEvent::Occluded(true)`, or a minimised window, stops requesting
   frames and starts decision 3's clock as a missing surface would.
   `Occluded(false)` requests a frame again, restoring first if the
   device was released. The surface is kept while occluded. A `Keep`
   app's desktop loop is unchanged.
8. **Memory hints are a const**, wgpu's own type:

   ```rust
   const MEMORY_HINTS: wgpu::MemoryHints = wgpu::MemoryHints::Performance;
   ```

   passed to every device request, the first included. `Performance` is
   today's value; an app may choose `MemoryUsage` or `Manual`.
9. **Measuring**: at each suspend, release and restore, libmorph logs
   `device.generate_allocator_report()` once, as the totals of allocated
   and reserved bytes (or that the backend gives none). A script verb,
   `gpu-report`, logs the same in a scripted run. On the S23, `dumpsys
   meminfo <package>`'s graphics lines are read by hand (step 5).
10. **Scripted runs**: `suspend` starts the clock for a releasing app, and
    `frames <n>` at the script's `fps` advances it, so a script can reach
    the release and test it headlessly. `resume` after a release restores.
11. **Versions.** Each piece is additive: a defaulted associated const,
    a defaulted trait method, a `#[non_exhaustive]` enum, a new context
    method, a new script verb, and behaviour that changes only for an app
    that chooses `Release`. That is a patch, 0.7.2, under Rule 5.4; the
    `README.md` lifecycle section says what `Release` changes. Open
    question 1 is the alternative.

## 4. Steps

1. **The allocator report**: decision 9, the log lines and `gpu-report`.
   **Done when:** a scripted run of `examples/` logs a report at
   `suspend` and on `gpu-report`, and a test checks the log line's
   wording on a backend without a report.
2. **The consts and the hook**: `GpuInBackground`, `GPU_IN_BACKGROUND`,
   `MEMORY_HINTS`, `GpuEvent`, `on_gpu`, `gpu_available`, with the
   device request using `MEMORY_HINTS`. **Done when:** a `Keep` app's
   tests pass unchanged, and a test checks the defaults.
3. **Release and restore in the shell and the script runner**: decisions
   3 to 6 and 10. **Done when:** scripted tests cover: a releasing app
   released after `after` and not before; at once on `trim-memory
   background` and on `low-memory` while suspended, and not while
   resumed; no `update` or `render` while released; `Restored` before
   `on_resume`, with the same limits; a `Keep` app never released; the
   panic message from `ctx.device()` while released.
4. **The Android and desktop runners**: decision 3's timeouts on both,
   decision 7's occlusion on the desktop, and `RestoreFailed`. **Done
   when:** an example app with `Release { after: 10 s }` is released and
   restored on macOS by minimising its window, as its log shows.
5. **On the S23**: the same example, off screen past `after`, gives back
   its graphics memory in `dumpsys meminfo` (recorded before and after),
   and comes back on screen drawing correctly. **Done when:** recorded in
   `CHANGES.md`, with the numbers, and the version and its `VERSIONS.md`
   entry are committed.

## 5. Risks

- An app that misses a wgpu object in `Release` keeps the device alive
  through it: wgpu frees the device only when the last handle goes. The
  allocator report after the release shows it (step 1), and the
  `README.md` says so.
- Requesting a device again takes time (tens of milliseconds on the
  S23, to be measured in step 5) and the app's rebuild adds to it; the
  first frame after a long absence is late. `after` keeps quick switches
  from paying it.
- A restore can be refused (`RestoreFailed`), for example while another
  app holds what the driver has; the app stays undrawn until a later
  window succeeds.
- Under Vulkan, dropping the device may not return every page to the
  system at once; step 5 measures what the S23 actually shows.

## 6. Open questions

1. **`ctx.device()` while released**: a panic on an opted-in contract
   keeps this a patch (decision 5). Returning `Option<&wgpu::Device>`
   from the accessors would make the absence visible in the types, at the
   cost of every app's code and a break, 0.8.0.
2. **The default**: `Keep` (decision 1) keeps every app as it is.
   `Release` by default would free memory for every libmorph app off
   screen and make each one handle `on_gpu`.
3. **The delay**: 10 s is the proposal's value for flankr; whether
   libmorph should offer a named default for it.
