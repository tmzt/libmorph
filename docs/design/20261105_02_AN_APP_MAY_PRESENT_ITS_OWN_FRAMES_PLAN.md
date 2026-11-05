---
title: An app may present its own frames - libmorph keeps the device, the events and the JNI
date: 2026-11-05
status: Draft
covers: >
  A way for an app to take the frame: libmorph still opens the device,
  owns the window, delivers every event and makes every JNI call, but
  does not acquire, draw or present. Opted into the way the other setup
  choices are, an associated function on `MorphApp`, and off by default
  so nothing already written changes.
sources: >
  The owner, 2026-11-05: "I was wondering why libmorph had drawing code,
  my thinking was it handed back the wgpu device and left it at that. We
  probably built it for the simple demos like the plasma one, never
  intending it to be used for a game like this", and "we do need a
  simple plan to make libmorph drawing opt-out in the builder, without
  the full threading plan right now, it will still need event delivery
  and jni calls to work though".
  `src/runner/shell.rs:331` (`Shell::frame`: acquire, `draw`, present) and
  `:376` (`Shell::draw`: `update` then `render`).
  `src/app.rs:55` (`limits`) and `:73` (`audio_setup`), which are where a
  setup choice already goes.
related: >
  `docs/design/20261105_01_THE_APP_RUNS_OFF_THE_LOOP_THREAD_PLAN.md`,
  which this is deliberately smaller than and does not depend on. Most
  of what that plan proposes is only needed because libmorph drives the
  frame; an app that drives its own can pace and thread it however it
  likes without libmorph growing a routing layer.
---

# An app may present its own frames

## 1. Why

`Shell::frame` acquires the surface texture, calls `update` and `render`,
and presents. That is the whole of an app's frame, and libmorph owns all
of it. For a plasma demo that is the feature: there is nothing to write
but a fragment shader. For a game it is a wall - the app cannot choose
when to submit, how deep to let the queue get, whether to skip a frame,
or what to do when one is slow.

It also means libmorph gets to be wrong about things that are not its
business. Two from one day on a game built on it:

- Nothing in the live path ever called `Device::poll`. Only the scripted
  runner did. On a backend that needs it, submissions are not retired
  until something maintains the device, and an app that wanted to could
  not have done it itself.
- An app with a 266 ms frame has no way to tell libmorph to stop handing
  it another one.

Neither is a deep fault. Both are the same shape: the app knows what it
wants and has no way to say so.

## 2. What it is

One associated function on `MorphApp`, beside `limits` and
`audio_setup`, which is where a setup choice already goes:

```rust
/// Who presents. `Managed` is libmorph, and is the default.
pub enum Drawing {
    /// libmorph acquires the surface texture, calls `update` and
    /// `render`, and presents. Every app written so far.
    Managed,
    /// The app does. libmorph opens the device, owns the window and
    /// delivers everything else; `render` is never called, and the app
    /// drives its frame from `present`.
    Own,
}

fn drawing() -> Drawing {
    Drawing::Managed
}
```

Under `Drawing::Own`, `Shell::frame` stops acquiring and presenting and
calls one new hook instead:

```rust
/// Called once per loop iteration when `drawing()` is `Drawing::Own`.
///
/// The app acquires, draws and presents. `ctx.surface()` hands back the
/// configured surface; `ctx.gpu()` the device and queue it already had.
fn present(&mut self, ctx: &mut MorphContext) {}
```

`update` keeps its place and its meaning. Only the frame moves.

## 3. What libmorph keeps

Everything that is not the frame, and the owner's point is that this is
most of it:

- **The device, queue and adapter.** Opening them is libmorph's, and an
  app that wanted to do that itself would be writing libmorph.
- **The window.** Creation, resize, destruction and the surface
  configuration. The app is told; it does not have to watch.
- **Every event.** Input, lifecycle, intents, permissions, audio,
  capture - the whole of `MorphApp`'s other hooks, unchanged.
- **Every JNI call.** Clipboard, safe area, media, intents. They stay on
  the loop thread, which is where they already are and where they are
  cheap, and no app taking its own frame has to think about the VM.

## 4. What the app takes on

Stated plainly, because this is the trade:

- **Acquiring, and what to do when it fails.** `Suboptimal`, `Lost` and
  `Timeout` become the app's to handle. libmorph still reconfigures on
  resize, so `Lost` is the one that needs thought.
- **Presenting, and pacing.** Whether to present every iteration, how
  deep to let the queue get, whether to poll the device.
- **Nothing else.** If an app finds it has taken on more than this, the
  split is in the wrong place and the plan is wrong.

## 5. What it does not do

- **It does not move anything to a thread.** `present` is called from
  the loop thread like everything else. An app that wants its frame on
  another thread may now do that for itself, which is the point: it no
  longer needs libmorph to grow a routing layer first.
- **It does not change `Managed`.** Every app that does not implement
  `drawing()` behaves exactly as it does today, by the same code path.
- **It does not split `update` from `render`.** That remains the better
  long-run shape and remains open.

## 6. Steps

**Step 1 - the choice and the hook.** `Drawing`, `MorphApp::drawing`,
`MorphApp::present`, and `Shell::frame` branching on it. `ctx.surface()`
to hand the surface back.

**Done when:** an app that returns `Drawing::Own` and presents a cleared
frame from `present` draws the same thing libmorph would have, and every
existing app and example is untouched and unchanged.

**Step 2 - the surface lifecycle, as the app sees it.** What `Own` needs
to be told and when: created, resized, lost, destroyed. Most of it
already arrives through existing hooks, so this step is mostly about
saying which, and filling what is missing.

**Done when:** an `Own` app survives a rotation and a backgrounding
without special-casing either.

**Step 3 - the desktop and the scripted runner.** `winit` and the script
runner take the same branch, so one app behaves the same on all three.

**Done when:** a scripted run of an `Own` app produces the same bytes as
the same app under `Managed`.

## 7. What this is not justified by

Not by a slow frame. The game's is 266 ms on an Adreno and steady,
and the thing that actually stopped it was a blocking readback it had
left in itself - not libmorph owning the frame. This is justified by the
boundary being in the wrong place for anything larger than a demo, which
was true before that frame was ever measured.
