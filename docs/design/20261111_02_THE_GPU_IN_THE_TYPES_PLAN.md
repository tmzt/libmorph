---
title: The GPU in the types - an app may give back its device off screen, and only callbacks with a device can reach it
date: 2026-11-11
status: Draft
covers: >
  The background release of 20261111_01, rebuilt around the device's
  absence being visible in the types instead of a panic. The context
  splits in two: `HostContext`, everything libmorph gives an app except
  the GPU, and `MorphContext`, which derefs to it and adds the device,
  queue, adapter and limits, and exists only while there is a device.
  Callbacks that run only with a device (`init`, `update`, `render`,
  `construct_frame`, input, `on_suspend`, `on_resume`, `on_gpu`) keep
  `&mut MorphContext`, so the calls inside them are unchanged; the ten
  that can arrive while the device is released take `&mut HostContext`,
  where `device()` does not exist. With that, `GPU_IN_BACKGROUND`
  (`Keep` or `Release { after }`), `on_gpu`, desktop occlusion,
  `MEMORY_HINTS` and the allocator report, as in 20261111_01. A break,
  0.8.0. Not covered: the companion app's adoption, flankr's own plan;
  releasing the `wgpu::Instance`; the web.
sources: >
  The owner, 2026-11-11, on 20261111_01's open question 1: "rewrite
  around option B, succeed. review actual impact carefully. flankr is
  the first app that will adopt this change and it comes as 0.x break so
  compat shouldn't be a problem, argue otherwise"; and, on the
  discussion before it, that `gpu_available` should cost no more than a
  plain read. The impact review of 2026-11-11 (section 2), on every
  libmorph dependent on the owner's machine. The owner, 2026-11-05, on
  the OWNS plan: "overriddable behavior on MorphApp is just the old java
  applet API smells", which is why the release adds one hook.
supersedes: "docs/design/20261111_01_RELEASING_THE_GPU_IN_THE_BACKGROUND_PLAN.md"
related:
  - "docs/design/20261105_04_OWNS_AND_THE_RENDER_STATUS_PLAN.md (OWNS
    and `Owned::FRAME`, 0.7.0: an app that owns the frame is released the
    same way; `construct_frame` keeps `MorphContext`)"
  - "docs/design/20261029_02_AUDIO_STREAM_RECOVERY_PLAN.md (`on_audio`,
    the shape `on_gpu` follows; audio runs on without a device)"
  - "docs/design/20261105_01_THE_APP_RUNS_OFF_THE_LOOP_THREAD_PLAN.md
    (a draft; if it lands, the release stops the draw thread around it,
    as that plan does for teardown)"
  - "flankr: apps/companion, the first app to adopt it, in a plan of its
    own"
---

# The GPU in the types

## 1. Goal

As in 20261111_01: an app built on libmorph keeps its GPU device for the
whole run, so an app that stays alive off screen, as flankr's companion
does with its foreground service, holds its device memory while another
GPU app in front of it runs out. An app should be able to give its
device back while it is off screen, with libmorph doing the release and
the restore.

What changes from 20261111_01 is what the app can write while there is
no device. There, `ctx.device()` kept its signature and panicked; here,
a callback that can run without a device is given a context with no
`device()` on it, so the mistake does not compile.

## 2. The impact, reviewed (2026-11-11, libmorph 0.7.1 at `2cc85df`)

Every crate on the owner's machine that depends on libmorph was read:
nine apps and libraries, besides libmorph's own examples. The other apps
are named here only by count.

1. **Where the device is used.** 142 calls to `ctx.device()`,
   `ctx.queue()`, `ctx.limits()` and `ctx.adapter_info()` in all. Grouped
   by the function they sit in, for flankr's companion and three games:
   all are in `init`, `update`, `render` or helpers those call (flankr's
   26: 17 in `render_screen`, 6 in `render`, 3 in `init`), one is in
   `on_suspend`, and none is in a callback that can arrive while the
   device is released.
2. **So making the accessors return `Option`** would put an
   `.expect()` or a `?` at 142 places where a device always exists, and
   catch nothing the apps actually do. That is the cost the plain
   form of option B carries; decision 1 puts the absence where it can
   happen instead.
3. **The callbacks that can arrive with no device**, once a release
   exists: `on_capture_ready`, `on_audio`, `on_permissions`,
   `on_open_url`, `on_intent`, `on_service`, `on_camera`, `on_media`,
   `on_low_memory`, `on_trim_memory`. Across the dependents they are
   overridden 13 times, 8 of them in flankr's companion; none of those
   bodies calls a GPU accessor today.
4. **Compatibility.** Each dependent pins libmorph by revision, tag or
   lock file, at versions from 0.2.1 to 0.7.1, so 0.8.0 reaches none of
   them until it moves. Three cases are not that simple:
   - one app builds through an uncommitted `[patch]` to the libmorph
     checkout on the machine (`.cargo/config.toml`); it breaks the moment
     that checkout is on 0.8.0, whatever it pins;
   - two depend on libmorph's git URL with no revision; only their lock
     files hold them, and a `cargo update` moves them to 0.8.0 (as it
     would to 0.7.0's break today);
   - flankr's companion and flankr-audio are on 0.6.0, so adopting this
     also takes 0.7.0's break (OWNS and `RenderStatus`), and flankr-audio,
     whose public API carries libmorph's `Sink` and `Source`, takes a
     minor of its own under Rule 5.5 when it moves.
   The break is otherwise a 0.x minor, taken by each app when it chooses
   to move.
5. Unchanged from 20261111_01's findings: `Gpu` is built once with
   `MemoryHints::Performance`; the Android runner drops only the surface
   on `WindowDestroyed`; the desktop runner redraws continuously and does
   not handle `WindowEvent::Occluded`; libmorph's own GPU objects beyond
   the surface are the offscreen target, the overlay and the viewfinder;
   the script runner has `suspend`, `resume`, `low-memory`,
   `trim-memory` and `frames`.

## 3. Decisions

1. **Two contexts.**

   ```rust
   /// Everything libmorph gives an app except the GPU: safe area, scale,
   /// text input, clipboard, permissions, audio, camera, media, exit.
   pub struct HostContext { /* today's MorphContext, without `gpu` */ }

   /// A HostContext with a device. Exists only while there is one.
   pub struct MorphContext<'a> {
       host: &'a mut HostContext,
       gpu: &'a Gpu,
   }
   impl Deref for MorphContext<'_> { type Target = HostContext; /* … */ }
   impl DerefMut for MorphContext<'_> { /* … */ }
   impl MorphContext<'_> {
       pub fn device(&self) -> &wgpu::Device;
       pub fn queue(&self) -> &wgpu::Queue;
       pub fn adapter_info(&self) -> wgpu::AdapterInfo;
       pub fn limits(&self) -> &wgpu::Limits;
       // and the frame and surface methods that need the device
   }
   ```

   The shell keeps `host: HostContext` and `gpu: Option<Gpu>` side by
   side and builds a `MorphContext` view, two borrows, for each callback
   that has a device. There is no flag to read and nothing to unwrap: the
   view cannot be built without a `Gpu`. An app writing `&mut
   MorphContext` in a signature keeps compiling, because the new lifetime
   is elided.
2. **Which callback gets which.**
   - `&mut MorphContext`: `init`, `update`, `render`, `construct_frame`,
     `on_touch`, `on_hover`, `on_key`, `on_text_input` (each needs a
     window, and a window means a device, decision 7), `on_suspend` (the
     device is still there; the release comes later), `on_resume` (after
     any restore), and `on_gpu`.
   - `&mut HostContext`: the ten of section 2.3. A callback there that
     wants GPU work done records it and does it in the next `update`.
   - `HostContext::gpu_available() -> bool`, a plain field the shell
     sets, says whether a device exists, for an app that wants to log or
     decide something; it gives no access to the device.
3. **The policy is a const**, as in 20261111_01:

   ```rust
   #[derive(Clone, Copy, Debug, PartialEq, Eq)]
   #[non_exhaustive]
   pub enum GpuInBackground {
       Keep,                         // the default: today's behaviour
       Release { after: Duration },
   }
   const GPU_IN_BACKGROUND: GpuInBackground = GpuInBackground::Keep;
   ```

   flankr's companion is the first to choose `Release`, at 10 s.
4. **One hook**, called only for an app that chose `Release`:

   ```rust
   #[derive(Clone, Debug, PartialEq, Eq)]
   #[non_exhaustive]
   pub enum GpuEvent {
       /// The device goes when this returns: drop every wgpu object.
       Release,
       /// A new device, with the same limits: rebuild before the next
       /// frame. `on_resume` follows.
       Restored,
   }
   fn on_gpu(&mut self, ctx: &mut MorphContext, event: &GpuEvent) {}
   ```

   Both events come with a device. A failed restore has none, so it is
   not a `GpuEvent`: it is logged, `gpu_available()` stays false, nothing
   is drawn, and libmorph tries again at the next window.
5. **When the device is released**, for `Release { after }`: after
   `after` with no surface (Android: from `WindowDestroyed`; desktop:
   from `suspended` or decision 8's occlusion) and no new window since;
   or at once, with no surface, on `TrimMemory` at `TrimLevel::Background`
   or above, or `LowMemory`. A report while the app is on screen releases
   nothing. The Android runner's `poll_timeout` wakes at the deadline; the
   desktop runner uses `ControlFlow::WaitUntil`.
6. **Releasing**, in order: `on_gpu(Release)`; libmorph drops its
   offscreen target, overlay and viewfinder; it waits on the device
   (`device.poll` with wgpu 30's wait) so queued work and dropped objects
   finish; it drops the queue, device and adapter; `gpu_available()`
   becomes false. The `wgpu::Instance` stays. While released there is no
   `update` or `render`; the HostContext callbacks still arrive, and the
   audio, the foreground service and the app's threads are untouched.
7. **Restoring**, at the next `WindowCreated` (Android) or `resumed`
   (desktop): an adapter and a device with the limits first granted (the
   app never sees different limits) and `MEMORY_HINTS`; the new surface
   configured; `on_gpu(Restored)`; then `on_resume`. The surface format
   is chosen again and may differ; the app reads `ctx.surface_format()`
   in `Restored`, as in `init`.
8. **Desktop occlusion**, for a releasing app only:
   `WindowEvent::Occluded(true)` or a minimised window stops requesting
   frames and starts decision 5's clock; `Occluded(false)` requests a
   frame, restoring first if released. A `Keep` app's desktop loop is
   unchanged.
9. **Memory hints**, wgpu's own type, for every device request:
   `const MEMORY_HINTS: wgpu::MemoryHints = wgpu::MemoryHints::Performance;`
10. **Measuring**: at each suspend, release and restore, libmorph logs
    `device.generate_allocator_report()` once, as allocated and reserved
    totals, or that the backend gives none; a script verb, `gpu-report`,
    does the same.
11. **Scripted runs**: `suspend` starts the clock for a releasing app and
    `frames <n>` at the script's `fps` advances it; `resume` after a
    release restores.
12. **Versions.** The callback signatures of decision 2 change, and
    `MorphContext` gains a lifetime parameter: a break, 0.8.0, under Rule
    5.4. The `VERSIONS.md` entry marks it **Breaking** and says what an
    app changes: the ten callbacks' context type, any helper it passes
    that context to, and a `MorphContext` stored in a struct (none of
    the dependents does). `README.md`'s lifecycle section describes the
    release.

## 4. Steps

1. **The allocator report**: decision 10. **Done when:** a scripted run
   logs a report at `suspend` and on `gpu-report`, and a test checks the
   line's wording on a backend without one.
2. **The split**: decisions 1 and 2, with every callback, example and
   test moved to its context, `MEMORY_HINTS` in the device request, and
   `GPU_IN_BACKGROUND`, `GpuEvent` and `on_gpu` declared. **Done when:**
   the crate and its examples build and their tests pass with no
   behaviour change, and a compile-fail doc test shows `ctx.device()`
   refused in `on_service`.
3. **Release and restore in the shell and the script runner**: decisions
   4 to 7 and 11. **Done when:** scripted tests cover: released after
   `after` and not before; at once on `trim-memory background` and
   `low-memory` while suspended, not while resumed; no `update` or
   `render` while released; `gpu_available()` false in a HostContext
   callback while released; `Restored` before `on_resume`, with the same
   limits; a `Keep` app never released.
4. **The runners**: decision 5's timeouts on Android and the desktop,
   decision 8's occlusion, and a failed restore retried. **Done when:**
   an example app with `Release { after: 10 s }` is released and restored
   on macOS by minimising its window, as its log shows.
5. **On the S23**: the same example off screen past `after` gives back
   its graphics memory in `dumpsys meminfo` (before and after recorded),
   and draws correctly when it returns. **Done when:** recorded in
   `CHANGES.md` with the numbers, and 0.8.0 and its `VERSIONS.md` entry
   are committed and tagged.

## 5. Risks

- An app that misses a wgpu object in `Release` keeps the device alive
  through it, since wgpu frees a device with its last handle. The
  allocator report after the release shows it.
- The first frame after a release waits for a new device and the app's
  rebuild; `after` keeps quick switches from paying it.
- A restore can fail while another app holds what the driver has; the
  app stays undrawn until a later window succeeds.
- Under Vulkan, dropping the device may not return every page at once;
  step 5 measures what the S23 shows.
- The app built through a local `[patch]` (section 2.4) breaks as soon as
  the libmorph checkout carries step 2, before 0.8.0 is tagged.

## 6. Alternatives considered

1. **The accessors return `Option`** on one context (20261111_01's open
   question 1, in its plain form). Rejected on section 2's count: 142
   unwraps where a device always exists, and the ten callbacks that can
   lack one are no better guarded than with decision 2.
2. **A panic on an opted-in contract** (20261111_01, decision 5).
   Rejected by the owner for this plan: the mistake shows at run time, in
   the background, on the phone.
3. **Making `MorphContext` generic over the app**, so the check folds
   away for a `Keep` app. Rejected: a larger break than decision 1, for a
   check decision 1 does not need.
