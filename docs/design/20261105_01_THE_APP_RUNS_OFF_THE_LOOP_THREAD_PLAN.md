---
title: The app runs off the loop thread - a slow frame becomes slow, not fatal
date: 2026-11-05
status: Draft
covers: >
  Moving a `MorphApp` off the thread that services Android's looper, so a
  frame that takes seconds costs frames rather than the process. The
  loop keeps polling events and pumping input at full rate and forwards
  them to the app's own thread; the window's teardown is a stop signal
  the app checks between passes, acknowledged before the handler
  returns. `MorphApp` is unchanged.
sources: >
  A game on libmorph, 2026-11-04/05: an Adreno 740 exceeds Android's
  watchdog drawing an interior scene and the system records an ANR
  against that app's package. The same frame is 2.6 ms on an M4. Its
  `20261105_01_WALLS_BAKED_AND_THE_MARCH_BOUNDED_PLAN.md` cannot run its
  own step 0, because the app dies before it prints a frame time.
  The owner, 2026-11-05: "does drawing have to be on the main thread?",
  "will it need a drawing lock/signal?", and "an atomic stop signal
  checked between cpu passes".
  `libmobile::android::run` (`src/android/mod.rs:131`): one loop, which
  polls, pumps input, then calls `handler.tick`.
  `src/runner/android.rs:447` (`Adapter::tick`) and `:87`
  (`MorphApp::render`).
related: >
  `docs/design/20261104_01_INTENT_EXTRAS_PLAN.md`, whose intents arrive
  on the loop thread and would be forwarded like every other event.
---

# The app runs off the loop thread

## 1. Why

`libmobile::android::run` is a single loop. It polls the activity's
events, pumps input, and calls `handler.tick`, which is where a
`MorphApp` both simulates and draws. The three are one iteration, so
while a frame is drawn **no input is consumed**.

Android does not watch frame times. It watches whether an app takes its
input, and when it does not within five seconds it kills it. So a slow
frame is not reported as slowness: it is reported as an ANR, and the
process goes.

That is what the game is hitting. The failure is also the wrong shape
to debug: its frame log reports at frames 1, 2 and then every 60, and
the app dies before the sixtieth, so the thing that would say how slow
the frame is never prints. **The bug hides the measurement that would
explain it.**

Nothing here makes a frame faster. What it changes is what a slow frame
costs: frames, rather than the process and the evidence.

## 2. Two shapes, and why the app moves rather than the draw

**Split the trait.** `MorphApp::render` becomes an extract on the loop
thread and a draw on a render thread, Bevy's arrangement. It is the
better long-run shape - the renderer then holds only a snapshot and
cannot touch the world - and it is a **breaking change to every app on
libmorph**, each of which has to be divided along a line it was not
written for.

**Move the app.** The whole `MorphApp` lives on its own thread. The loop
thread polls, pumps input, and forwards events down a channel. The app
simulates and draws exactly as it does now.

The second is the one to build. `MorphApp` is untouched, the change is
confined to `Adapter`, and `libmobile` needs nothing - it already calls
`tick`, and `tick` becomes a post rather than a frame. What it costs is
that the app must be `Send`, and that the renderer still has the world
in hand while drawing, so the trait split remains worth doing later and
this does not foreclose it.

## 3. What synchronises, and what does not

**The event handoff: a channel, no lock.** Events are small and ordered
and the loop thread never waits for them to be handled.

**The frame: nothing.** The app owns its world on its own thread and
draws from it. There is no second reader, so there is nothing to lock.
This is what moving the app rather than the draw buys.

**The surface: a stop signal, acknowledged.** `TerminateWindow` arrives
on the loop thread, and Android expects the window released before the
handler returns. A flag alone is not enough - the loop has to know the
app has actually let go.

The owner's answer is the shape: **an atomic stop signal checked between
CPU passes**. Not between frames. A frame is the thing that takes
seconds, so a stop checked at the top of one bounds the wait by exactly
the interval that is the problem. Checked between passes, the worst case
is one pass, and the loop's wait for the acknowledgement is bounded by
something short enough not to be a second ANR in the one path where the
alternative is a use-after-free on the native window.

**Resize and configuration: a flag.** Read at the top of a frame, no
handshake. A frame drawn at the old size is a frame, not a fault.

## 4. What the app sees that it did not

**The pose that drew the frame is not the newest pose.** The game
unprojects a tap through `camera::pose_of(&self.world)`. With the app on
its own thread that is still its own world, so this one is unchanged -
which is another point for moving the app rather than the draw, where it
would have needed the snapshot to carry its pose.

**Input arrives between frames rather than within one.** It already
does; what changes is that it keeps arriving while a frame is drawn, and
the app sees a burst at the next tick instead of one event per frame. An
app that assumed one touch per frame will see several.

## 5. Steps

**Step 1 - the app's thread, with no teardown.** Spawn it, forward
events, draw there. Window destruction is left as it is, which is a
known hole: this step is not shippable and exists to get the frame off
the loop thread and find out whether the ANR goes.

**Done when:** the game's scene runs on an Adreno without an ANR,
its frame log prints past frame 60, and a frame time is finally
readable.

**Step 2 - the stop signal.** An `AtomicBool` the app checks between CPU
passes, set on `WindowDestroyed`, acknowledged before the handler
returns, with a bounded wait and a stated fallback if it expires.

**Done when:** the window can be destroyed mid-frame - rotate the
device, background the app - without a crash and without the loop
blocking longer than one pass.

**Step 3 - resize and configuration.** The flag, read at the top of a
frame.

**Done when:** a rotation changes the surface without a frame drawn at
the wrong size reaching the screen.

**Step 4 - the desktop runner.** `winit`'s loop has no watchdog and no
reason to change, so it keeps the app on its own thread or does not, as
suits it. What matters is that one `MorphApp` behaves the same on both.

**Done when:** a scripted run produces the same bytes on the desktop as
it did before this plan.

## 6. What this does not cover

- **Making a frame faster.** That is the app's, and the game's
  `20261105_01` is where it is being worked out. This plan exists so
  that work can be measured at all.
- **The trait split.** Still the better long-run shape, and still open.
- **Multiple windows.** One app, one surface, as now.

## Amendment — 2026-11-05 12:27 EST: event classes move, not the app; and this is not justified by a multi-second frame

Worked out with the owner in discussion, 2026-11-05. This replaces
section 2's choice, section 3's synchronisation, and the whole of
section 5's steps. The document's title is now wrong and is left as it
is: the body is never rewritten, and what follows is what the plan
means.

### The correction that comes first

Section 1 argues from a frame that takes seconds on an Adreno. **That
number should not have been treated as a condition to design around.**
Desktop to mobile on the same content is normally ten to thirty times;
the M4 draws this frame in 2.6 ms, so a bad mobile case is of the order
of 130 ms. The 1286 ms that prompted all of this is about five hundred
times, which is not a degree of slowness but a different kind of thing -
a stall, a fallback, or a measurement that is not measuring what it is
read as.

Every large reading was taken while the app was losing focus to another
app, and a surface that is not being composited can block on
acquisition. That alone could account for it, and would make the figure
an artefact.

So: **this plan is not justified by a multi-second frame.** It is
justified by an app that cannot be measured, because Android kills it
for not taking its input and the frame log never reaches the interval
that would print a number. The cost of a frame is a separate fault with
a separate cause, and it is the one to fix.

### What the design is

Not "the app runs off the loop thread". What moves is **classes of
event**, not the app.

Each thread runs its **own event loop**, the way wgpu's do, and events
are delivered to it. A **routing atomic bitset** says which loop gets
which class. Reading it is lock-free, which matters when every event
consults it.

Three destinations:

| destination | what it takes |
|---|---|
| the loop thread | lifecycle, input, and everything by default |
| a sim tick | a timer, at a fixed step |
| a scheduled draw | frames, which may be skipped or coalesced |

**The default routes everything to the loop thread**, which is what
happens today: one thread, no contention, nothing for an app to reason
about. A builder call requests any other arrangement, and an app that
asks for sim and draw apart is declaring it can hand the data across.
That is why splitting sim from draw inside libmorph is not enough on its
own - the threads are the easy half, and ownership of the world is the
other.

### JNI stays where it is

Basic JNI operations stay on the loop thread. No other thread needs
attaching to the VM, which removes the question of scoped attachment,
of detaching before a thread exits, and of whether `AndroidApp` is
`Send`. It is also the strongest argument against moving the whole app,
which would have reached for JNI from wherever it ran.

### Teardown is an event, not a rendezvous

Section 3 proposed a stop flag with an acknowledgement, and an earlier
sketch proposed flipping the draw class back to the loop thread. Both
are more than is needed.

The stop is **delivered as an ordinary event** on the same mechanism as
everything else. The draw thread quiesces - which is the absence of
scheduled work rather than an operation it performs - and the loop
thread then takes the real terminate. Android's own ordering is what
makes it safe: a pause precedes a stop precedes a window going away.

These are Android apps and not an SDL quit. The likely case is the app
going to the background, and that just means the next frame is not
drawn.

**Rotation is not a special case.** It is a teardown of drawing followed
by a setup of drawing, and the draw thread handles both as the same two
events it already has - the same code as backgrounding, then the same
code as a first launch.

An acknowledgement is added **only if it turns out to be needed**.
Handling the stop between CPU passes may be enough on its own, and that
is the thing to try first.

### Why ordering is preferred to a rendezvous

A rendezvous is where a second ANR hides: the loop thread waiting on a
draw thread that is mid-frame. The trade is deliberate - **a Rust crash
can be recovered from and an ANR cannot** - so a narrow window in which
a destroyed surface is touched is the better risk.

One thing to confirm rather than assume: whether that window produces a
Rust panic, which is recoverable, or a segfault in native code, which is
not. It depends on whether `android-activity` frees the `ANativeWindow`
once the handler returns or holds it until the app acknowledges. That is
a contract question and is answerable by reading it.

### Out of scope

Work scheduling for a complex game - an async loop and a waker - is the
game's and not libmorph's. Drawing that line is what keeps libmorph's
default a single thread with nothing to reason about.

### Steps

Section 5's steps are replaced:

**Step 0 - find the real cost.** Read what the frame timer brackets, and
get one settled foreground number. Nothing below is worth building if
the frame is 146 ms and the readings were an artefact of a backgrounded
surface.

**Done when:** the frame's cost on the device is a number taken while
the app is foreground and settled, and what the timer includes is
written down.

**Step 1 - per-thread event loops and the routing bitset**, with every
class routed to the loop thread. No behaviour changes.

**Done when:** every app on libmorph runs exactly as it does now, and
the routing is a thing that can be read and set.

**Step 2 - the builder's alternate arrangement**, and a scheduled draw
on its own loop.

**Done when:** the game's frame log prints past frame 60 on the
device with a slow frame, instead of the app being killed.

**Step 3 - the sim tick on a timer.**

**Done when:** a slow frame costs frames rather than a lurch in `dt`.
