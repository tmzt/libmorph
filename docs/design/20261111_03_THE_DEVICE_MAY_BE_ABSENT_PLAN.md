---
title: The device may be absent - an app may give back its GPU off screen, and the accessors say whether there is one
date: 2026-11-11
status: Draft
covers: >
  The background release of 20261111_01 and 20261111_02, built on one
  context whose device accessors return `Option`. `MorphContext` holds
  the device and queue as an `Option`; `ctx.gpu()` (both handles, cloned),
  `device()`, `queue()` and `adapter_info()` return `None` while they are
  released, and `limits()` stays infallible because the limits never
  change. Each callback decides for itself what to do without a device:
  exit early when the GPU is the work, defer it, or carry on when it
  needs none. Intents and links are delivered when they arrive, also
  while suspended, instead of after `on_resume`. libmorph documents, and
  enforces, where a device is always present. With it,
  `GPU_IN_BACKGROUND` (`Keep`, the default, or `Release { after }`), one
  `on_gpu` hook, desktop occlusion as a suspension for a releasing app,
  `MEMORY_HINTS` and the allocator report. A break, 0.8.0. Not covered:
  the companion app's adoption, flankr's own plan; releasing the
  `wgpu::Instance`; the web.
sources: >
  The owner, 2026-11-10: "plan to handle backgrounding including freeing
  gpu memory, plan in libmorph as needed, currently using flankr while
  developing other gpu applications can cause the gpu app to run out of
  memory and gpu resources". The owner, 2026-11-11: "rewrite around
  option B, succeed. review actual impact carefully. flankr is the first
  app that will adopt this change and it comes as 0.x break so compat
  shouldn't be a problem, argue otherwise"; on 20261111_02's unwrap
  count: "why not exit early in a callback if no device?"; then "if gpu
  is required to do meaningful work it makes sense to exit and expect
  shouldn't be preferred, causing a panic instead. its still up to the
  consumer to write the logic. for intents, some may be background and
  require no gpu"; then "rewrite again, clearly argue for the preferred
  version". The impact review of 2026-11-11 (section 3), and an
  independent review of this draft against the code the same day, whose
  findings are folded in (section 3.4, decisions 1, 3, 4, 7 to 10). The
  owner, 2026-11-05, on the OWNS plan: "overriddable behavior on MorphApp
  is just the old java applet API smells", which is why the release adds
  one hook.
supersedes: "docs/design/20261111_02_THE_GPU_IN_THE_TYPES_PLAN.md"
related:
  - "docs/design/20261111_01_RELEASING_THE_GPU_IN_THE_BACKGROUND_PLAN.md
    (the first draft, superseded by 20261111_02)"
  - "docs/design/20261105_04_OWNS_AND_THE_RENDER_STATUS_PLAN.md (OWNS and
    `Owned::FRAME`, 0.7.0: an owning app is released the same way)"
  - "docs/design/20261104_01_INTENT_EXTRAS_PLAN.md (`on_intent`; this
    plan changes when it is called, decision 6)"
  - "docs/design/20261029_02_AUDIO_STREAM_RECOVERY_PLAN.md (`on_audio`,
    the shape `on_gpu` follows; audio runs on without a device)"
  - "docs/design/20261105_01_THE_APP_RUNS_OFF_THE_LOOP_THREAD_PLAN.md (a
    draft; if it lands, the release stops the draw thread around it, as
    that plan does for teardown)"
  - "flankr: apps/companion, the first app to adopt it, in a plan of its
    own"
---

# The device may be absent

## 1. Goal

An app built on libmorph keeps its GPU device for the whole run. On
Android, leaving the screen drops only the surface. An app that stays
alive off screen, as flankr's companion does with its foreground
service, therefore holds its atlases, textures, buffers, pipelines and
the device itself while another GPU app in front of it runs out of GPU
memory. An app should be able to give its device back while it is off
screen, with libmorph doing the release and the restore.

Once a device can be released, some code will find none. This plan
decides how an app learns that, and argues for one answer: **the device
accessors return `Option`, and the app decides what each callback does
without a device.**

## 2. Why `Option` accessors

Four designs were weighed. Each answers the same question: what happens
when an app's code asks for the device and there is none.

| Design | Where absence shows | What an app writes | A mistake shows as |
|---|---|---|---|
| A. Accessors panic (20261111_01) | at run time only | nothing new | a crash, off screen, on the phone |
| B. Accessors return `Option` (this plan) | in each accessor's type | its own choice, per callback | a compile error until handled |
| C. Two contexts by callback (20261111_02) | in each callback's context type | a new type on the callbacks that can lack a device | a compile error; and a type change on every callback whose delivery changes |
| D. A context generic over the app | in the app's type | a generic on every context | a larger break, for the check B makes |

**B is preferred**, for these reasons, in order of weight:

1. **The type stays true when libmorph's delivery rules change.** Which
   callbacks can run without a device is not a property of the callback.
   It follows from libmorph's delivery rules, and those move. This plan
   moves one already: intents and links are delivered on arrival, also
   while released (decision 6), where today they wait for `on_resume`.
   Under C that moves `on_intent` and `on_open_url` from one context type
   to the other, a break for every app that overrides them, and so does
   every later change of the same kind, in either direction. Under B the
   type already says "there may be no device" and no signature changes;
   only the documentation of decision 4 does.
2. **The callback knows what its work needs; libmorph does not.** An
   intent that opens a scene needs the GPU; an intent that changes a
   setting, starts a download or answers a share does not. A service
   event, an audio event, a trim or a permission answer usually need
   none. Under B each callback says so in its own code:

   ```rust
   fn on_intent(&mut self, ctx: &mut MorphContext, intent: &Intent) {
       self.settings.apply(intent);              // needs no GPU: always runs
       let Some(gpu) = ctx.gpu() else {
           self.pending_scene = scene_of(intent); // the GPU part waits
           return;
       };
       self.scene.load(&gpu.device, &gpu.queue, intent);
   }
   ```

   C decides by callback instead: it gives a callback a device whenever
   libmorph happens to have one at that point of the lifecycle, and none
   otherwise, so the app's own split between GPU and non-GPU work has to
   follow libmorph's lifecycle instead of its own.
3. **No panic is ever needed.** A turns the one condition this plan
   introduces into a crash that happens off screen, where nobody sees it
   until the process is gone. B makes the absence a value. `expect`
   remains possible and is not recommended or shown anywhere in
   libmorph's documentation.
4. **The cost is one line per callback that does GPU work.** It is not
   one check per call: an app takes the handles once at the top, `let
   Some(gpu) = ctx.gpu() else { return };`. `ctx.gpu()` returns both
   handles as one value, so there is no `(Some, None)` pair to handle,
   and wgpu 30's `Device` and `Queue` are `Clone` (an `Arc` each), so the
   value holds no borrow of `ctx` and the rest of the callback keeps
   `&mut ctx`. `limits()` stays infallible (decision 1), so code that only
   reads limits changes nothing. Reading the `Option` is a load and a
   predictable branch; every wgpu call behind it costs more.

What B gives up, stated plainly:

- **In `init`, `update` and `render` the `else` can never run.** libmorph
  guarantees and enforces a device there (decision 4), so for those
  callbacks the line is a formality, and a `Keep` app pays it everywhere
  it does GPU work. That is the price of one type whose meaning does not
  depend on the lifecycle (reason 1); decision 4 says where it is a
  formality, so an app knows which of its exits can never be taken.
- **An early exit where the work should have been deferred drops it
  silently.** That is the app's logic to get right - "its still up to
  the consumer to write the logic" - and the type puts the choice in front
  of it at every accessor, which is as far as a library can take it.

A variant of C was weighed as well: only the callbacks that can lack a
device receive a view type with `Option` accessors, and the rest keep
infallible ones. It removes B's formality in `init`, `update` and
`render`, at the cost of reason 1: every change to the delivery rules
moves callbacks between the two types, beginning with decision 6.

## 3. The impact, reviewed (2026-11-11, libmorph 0.7.1 at `2cc85df`)

Every crate on the owner's machine that depends on libmorph was read:
nine apps and libraries, besides libmorph's own examples. Apps other than
flankr are named here only by count.

1. **Accessor calls.** 142 calls to `ctx.device()`, `ctx.queue()`,
   `ctx.limits()` and `ctx.adapter_info()` in all. Grouped by the function
   they sit in, for flankr's companion and three games, every one is in
   `init`, `update`, `render`, a helper those call, or `on_suspend`.
2. **What B costs an app**, then, is one `let Some(gpu) = ctx.gpu() else
   { … }` at the top of each callback that does GPU work - for most apps
   `init`, `update` and `render`, for flankr's companion the same three -
   with the helpers those pass `ctx` to (flankr's `render_screen`, 17
   calls) taking `&Gpu` or checking again; the calls inside change from
   `ctx.device()` to `gpu.device`. Calls to `ctx.limits()` do not change.
3. **The callbacks that can arrive with no device**, once a release
   exists and decision 6 is built: `on_capture_ready`, `on_permissions`,
   `on_service`, `on_camera`, `on_media`, `on_low_memory`,
   `on_trim_memory`, and now `on_intent` and `on_open_url`. `on_audio` is
   not among them: it is delivered from the frame loop
   (`Shell::audio_events`, from `pump_and_update`), so only while there is
   a device. The overrides of these callbacks across the dependents do not
   use the GPU today, nor do the helpers flankr's call from them
   (`run_camera`, `start_morph_audio`); under B they need no change.
4. **What libmorph itself holds or calls on the GPU**, beyond the four
   accessors (from the independent review, checked):
   - `MorphContext::pipeline_cache`, a `wgpu::PipelineCache` opened in
     `boot` and saved on suspend and exit (`src/context.rs`,
     `src/pipeline_cache.rs`), public through `ctx.pipeline_cache()`; it
     keeps the device alive through a release unless dropped;
   - the desktop overlay (`src/runner/overlay.rs`), built in `boot`;
   - `Viewfinder` (`src/camera/viewfinder.rs`), a public type the app owns
     and libmorph does not, whose `upload` and `update` call
     `ctx.device()` and `ctx.queue()` and can be reached from `on_camera`;
   - the instance and the granted limits live inside `Gpu` today
     (`src/gpu.rs`), and the runners create surfaces from
     `shell.gpu().instance`.
5. **Input does not need a surface today.** `Shell::touch`, `key`,
   `text_input` and `hover` reach the app whenever a shell exists, so a
   window without a device (a failed restore, decision 10) would pass
   input to an app with no device.
6. **No dependent stores a `MorphContext`** in a struct; every use is a
   parameter.
7. **Compatibility.** Each dependent pins libmorph by revision, tag or
   lock file, at versions from 0.2.1 to 0.7.1, so 0.8.0 reaches none of
   them until it moves; as a 0.x minor, that is the expected cost of the
   break. Three cases are not that simple:
   - one app builds through an uncommitted `[patch]` to the libmorph
     checkout on the machine (`.cargo/config.toml`), so it breaks as soon
     as that checkout carries step 2, whatever it pins;
   - two depend on libmorph's git URL with no revision; only their lock
     files hold them, and a `cargo update` moves them to 0.8.0, as it
     would to 0.7.0's break today;
   - flankr's companion and flankr-audio are on 0.6.0, so adopting this
     takes 0.7.0's break as well (OWNS and `RenderStatus`), and
     flankr-audio, whose public API carries libmorph's `Sink` and
     `Source`, takes a minor of its own under Rule 5.5 when it moves.
8. **What libmorph has today otherwise**: the device is requested with
   `MemoryHints::Performance`; the Android runner drops only the surface
   on `WindowDestroyed`; the desktop runner detaches only on winit's
   `suspended`, which the desktop does not send for a covered or
   minimised window, and redraws continuously without handling
   `WindowEvent::Occluded`; intents and links that arrive while suspended
   are queued and delivered after `on_resume`; the script runner has
   `suspend`, `resume`, `low-memory`, `trim-memory` and `frames`.

## 4. Decisions

1. **The context's layout.** The instance and the limits granted at the
   first request move out of `Gpu` onto `MorphContext`, and stay for the
   whole run. What can be released is the rest:

   ```rust
   /// The device and queue, cheap to clone (an `Arc` each).
   #[derive(Clone, Debug)]
   pub struct Gpu {
       pub device: wgpu::Device,
       pub queue: wgpu::Queue,
   }

   impl MorphContext {
       /// Both handles, cloned: `None` while the device is released.
       pub fn gpu(&self) -> Option<Gpu>;
       pub fn device(&self) -> Option<&wgpu::Device>;
       pub fn queue(&self) -> Option<&wgpu::Queue>;
       pub fn adapter_info(&self) -> Option<wgpu::AdapterInfo>;
       /// Always the limits first granted; a restore requests the same.
       pub fn limits(&self) -> &wgpu::Limits;
       /// `None` while released, as before when there is no data folder.
       pub fn pipeline_cache(&self) -> Option<&wgpu::PipelineCache>;
   }
   ```

   The adapter is held beside the device and released with it. Each of
   these returns `None` only while the device is released, which only an
   app with `Release` can reach; for a `Keep` app they are always `Some`.
2. **The idiom**, in the README and on each accessor: `let Some(gpu) =
   ctx.gpu() else { return };` once at the top of a callback that does
   GPU work, `RenderStatus::Skipped` from `render`; defer the GPU work a
   callback cannot do yet; do the rest regardless. `expect` is not shown.
3. **libmorph's own GPU users follow it.** `Viewfinder::upload` and
   `update` do nothing without a device and say so once in the log
   (`upload` returns `false`); the overlay draws nothing.
4. **Where a device is always present**, documented on each callback
   and enforced by the shell: `init`, `update`, `render`,
   `construct_frame`, `on_audio`, `on_touch`, `on_hover`, `on_key`,
   `on_text_input`, `on_suspend` (the release comes later), `on_resume`
   (after any restore) and `on_gpu`. The shell calls none of them while
   released: no frames, and input that arrives with a window but no
   device (decision 10) is dropped, with one log line per stretch of
   dropped input.
5. **The policy is a const**, beside `OWNS`:

   ```rust
   #[derive(Clone, Copy, Debug, PartialEq, Eq)]
   #[non_exhaustive]
   pub enum GpuInBackground {
       /// Keep the device off screen: today's behaviour, the default.
       Keep,
       /// Give it back after `after` off screen, or at once under
       /// memory pressure (decision 8).
       Release { after: Duration },
   }
   const GPU_IN_BACKGROUND: GpuInBackground = GpuInBackground::Keep;
   ```

   flankr's companion is the first to choose `Release`, at 10 s.
6. **Intents and links on arrival.** `on_intent` and `on_open_url` are
   called when the intent arrives, also while suspended, in the order they
   arrive, instead of after the next `on_resume`. This is the owner's
   case of an intent that needs no GPU ("some may be background and
   require no gpu"): it is handled at once, and one that needs the GPU
   finds `ctx.gpu()` `None` while released and defers. It changes when
   every app's `on_intent` runs, `Keep` apps included (they always have a
   device, but may now be called while suspended), and is part of the
   0.8.0 break. An intent before `init` still waits for `init`.
7. **One hook**, called only for an app that chose `Release`, and always
   with a device:

   ```rust
   #[derive(Clone, Debug, PartialEq, Eq)]
   #[non_exhaustive]
   pub enum GpuEvent {
       /// The device goes when this returns: drop every wgpu object,
       /// cloned `Gpu` values included.
       Release,
       /// A new device, with the same limits: rebuild before the next
       /// frame. `on_resume` follows.
       Restored,
   }
   fn on_gpu(&mut self, ctx: &mut MorphContext, event: &GpuEvent) {}
   ```

8. **When the device is released**, for `Release { after }`: after
   `after` with no surface (Android: from `WindowDestroyed`; desktop: from
   `suspended` or decision 11's occlusion) and no new surface since; or
   at once, with no surface, on `TrimMemory` at `TrimLevel::Background` or
   above, or `LowMemory`. A report while the app has a surface releases
   nothing. The Android runner's `poll_timeout` wakes at the deadline; the
   desktop runner uses `ControlFlow::WaitUntil`.
9. **Releasing**, in order: `on_gpu(Release)`; libmorph saves the pipeline
   cache's data and drops the cache, and drops the overlay; it waits on
   the device (`device.poll(wgpu::PollType::Wait { submission_index:
   None, timeout: Some(Duration::from_secs(2)) })`, logging a timeout) so
   queued work and dropped objects finish; it drops the queue, device and
   adapter, and the accessors of decision 1 return `None`. The instance
   and the limits stay. Memory returns when the last handle goes, so a
   handle the app kept keeps it (section 6); `Device::destroy` is no
   shortcut, since it only marks the device invalid.
10. **Restoring**, at the next surface: Android's `WindowCreated`, the
    desktop's `resumed` or decision 11's un-occlusion. In order: the
    surface from the kept instance; an adapter compatible with it
    (`request_adapter` takes the surface); a device with the limits first
    granted and `MEMORY_HINTS`; the pipeline cache reopened from its saved
    data; the overlay rebuilt; the surface configured; `on_gpu(Restored)`;
    then `on_resume`. The surface format is chosen again and may differ;
    the app reads `ctx.surface_format()` in `Restored`, as in `init`. If
    any step fails, it is logged, the accessors stay `None`, nothing is
    drawn, input is dropped (decision 4), and libmorph tries again from
    the poll loop every 2 s while the window is up, and at the next
    window.
11. **Desktop occlusion**, for a releasing app only:
    `WindowEvent::Occluded(true)` or a minimised window is a suspension -
    the shell detaches the surface, the app gets `on_suspend`, and frames
    stop - and starts decision 8's clock. `Occluded(false)` re-attaches,
    restoring first if released, then `on_resume`. A `Keep` app's desktop
    loop is unchanged.
12. **Memory hints**, wgpu's own type, for every device request:
    `const MEMORY_HINTS: wgpu::MemoryHints = wgpu::MemoryHints::Performance;`
    (`MemoryHints` is not `Copy`; libmorph clones it for each request).
13. **Measuring**: at each suspend, release and restore, libmorph logs
    `device.generate_allocator_report()` once, as its
    `total_allocated_bytes` and `total_reserved_bytes`, or that the
    backend gives none; a script verb, `gpu-report`, does the same.
14. **Scripted runs**: `suspend` starts the clock for a releasing app and
    `frames <n>` at the script's `fps` advances it; `resume` after a
    release restores; `intent` while suspended delivers at once
    (decision 6).
15. **Versions.** `device()`, `queue()` and `adapter_info()` change their
    return types, and `on_intent` and `on_open_url` change when they are
    called: a break, 0.8.0, under Rule 5.4. The `VERSIONS.md` entry marks
    both **Breaking**, gives decision 2's idiom as the change an app
    makes, and lists the additions (`Gpu`, `ctx.gpu()`, `GpuInBackground`,
    `GPU_IN_BACKGROUND`, `MEMORY_HINTS`, `GpuEvent`, `on_gpu`,
    `gpu-report`). `README.md`'s lifecycle section describes the release,
    decision 4's guarantees and decision 6's delivery.

## 5. Steps

1. **The allocator report**: decision 13. **Done when:** a scripted run
   logs a report at `suspend` and on `gpu-report`, and a test checks the
   line's wording on a backend without one.
2. **The layout and the accessors**: decisions 1 to 3, 5, 7 and 12, with
   every example and test in the crate moved to decision 2's idiom.
   **Done when:** the crate and its examples build, their tests pass with
   no behaviour change for a `Keep` app, and a test checks the defaults.
3. **Intents and links on arrival**: decision 6, and decision 14's
   `intent`. **Done when:** a scripted test delivers an intent and a link
   while suspended at once, in order, and an intent before `init` after
   it.
4. **Release and restore in the shell and the script runner**: decisions
   4 and 8 to 10, and 14. **Done when:** scripted tests cover: released
   after `after` and not before; at once on `trim-memory background` and
   `low-memory` while suspended, not while resumed; no `update`, `render`
   or input while released; `ctx.gpu()` `None` in `on_intent` while
   released and `Some` while resumed; the pipeline cache saved, dropped
   and reopened; `Restored` before `on_resume`, with the same limits; a
   failed restore retried; a `Keep` app never released.
5. **The runners**: decision 8's timeouts on Android and the desktop, and
   decision 11's occlusion. **Done when:** an example app with `Release {
   after: 10 s }` is released and restored on macOS by minimising its
   window, as its log shows.
6. **On the S23**: the same example off screen past `after` gives back its
   graphics memory in `dumpsys meminfo` (before and after recorded), and
   draws correctly when it returns. **Done when:** recorded in
   `CHANGES.md` with the numbers, and 0.8.0 and its `VERSIONS.md` entry
   are committed and tagged.

## 6. Risks

- An app that keeps any wgpu object past `on_gpu(Release)` - a cloned
  `Gpu` included - keeps the device alive through it, since wgpu frees a
  device with its last handle. The allocator report after the release
  shows it.
- An early exit where the work should have been deferred drops that work
  without a trace (section 2). The README's idiom shows the deferral next
  to the exit.
- Decision 6 changes when every app's `on_intent` and `on_open_url` run;
  an app that assumed it was on screen when they ran must now check.
- The first frame after a release waits for a new device and the app's
  rebuild; `after` keeps quick switches from paying it.
- A restore can fail while another app holds what the driver has; the app
  stays undrawn, retrying, until one succeeds.
- Under Vulkan, dropping the device may not return every page at once;
  step 6 measures what the S23 shows.
- The app built through a local `[patch]` (section 3.7) breaks as soon as
  the libmorph checkout carries step 2, before 0.8.0 is tagged.
