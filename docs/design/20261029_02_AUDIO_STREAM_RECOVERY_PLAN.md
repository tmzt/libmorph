---
title: Audio streams that recover after a resume or a lost device
date: 2026-10-29
status: Draft
covers:
  - "Reopening the capture stream, with the same mode, format and sinks,
    when starting it again on resume fails (`InvalidState`, `Disconnected`
    or any other error), when its stream reports it is lost, and when it
    stops delivering blocks while it should be running; a bounded retry,
    then a reported failure"
  - "The same for the output stream, which has the same flaw: a failed
    `play()` on resume is only logged today, and a lost device is never
    reopened; the mixer's voices and sources are kept"
  - "What the app is told: `on_capture_ready` stays once per run, the
    capture format never changes across a reopen, and a new
    `MorphApp::on_audio(ctx, &AudioEvent)` hook with
    `Audio::capture_state()`, `Audio::output_state()` and
    `Audio::restart(AudioStream)` (all additive)"
  - "Log lines for every interruption, attempt, recovery and failure,
    never audio content or levels"
  - "A unit-tested recovery state machine with no hardware, and an
    adb-only check on the S23 (no tapping) with the `audiocapture`
    example and a script"
  - "Release as libmorph 0.5.4 (additive), on libmobile 0.3.5 unchanged;
    RoundTwo as the first user"
sources:
  - "RoundTwo (branch ui-shell, pinning libmorph 0.5.3 at `70fd9f0`), on a
    Samsung S23 Ultra SM-S918U, Android 16 (API 36): capture opened in
    `CaptureMode::VoiceRecognition` (logged `libmorph: audio capture:
    asked VoiceRecognition, got VoiceRecognition`). After the user left the
    app and came back, logcat showed `W roundtwo:
    libmorph::audio::output: libmorph: audio capture resume failed:
    InvalidState` at 18:38:25 and again at 18:52:59 EDT. From then on no
    capture block reached the app's sinks: a whole mock interview round
    wrote nine 44-byte WAVs (header only, −120 dBFS) and libvoicexn's STT
    got nothing. Nothing retried; only restarting the app opened capture
    again."
  - "Owner (2026-10-29, in the RoundTwo session, told the above): \"yes,
    plan the libmorph fix and add the mic check\". The mic check is
    RoundTwo's own (its plan `20261029_06_MOCK_INTERVIEW_SESSION_PLAN.md`,
    amendment 19:01 EDT, which says \"A failed resume or open that
    libmorph reports, once its plan lands, is shown in the same way,
    without waiting\")."
  - "libmorph today (0.5.3, `70fd9f0`), `src/audio.rs`, `mod output`:
    `Audio::resume` calls `output.stream.play()` and
    `input.stream.play()` and on an error only logs `libmorph: audio
    resume failed: {error}` or `libmorph: audio capture resume failed:
    {error}`; `Audio::pause` likewise. Both are called from
    `runner/android.rs` on `MobileEvent::Resume`/`Pause` and from
    `runner/winit.rs` in `resumed`/`suspended`. `mod aaudio`:
    `AaudioInput::play` is `CaptureStream::start`, `pause` is
    `CaptureStream::stop`. The cpal error callbacks (`build`,
    `build_input`) only log. The mixer and the `Capture` state live inside
    each stream's callback closure, so a new stream would start without
    them."
  - "libmobile 0.3.5 (`67d2e8f`), `src/android/audio_capture.rs`:
    `CaptureStream::start` sets `started` and returns
    `request_start().map_err(error)`, where `error` formats the ndk
    `AudioError` with `{:?}` (hence the bare `InvalidState`);
    `set_preset(preset)` closes the stream, opens a new one at the same
    format with the same callback (the callback lives in an
    `Arc<Mutex<State>>` with a clock that keeps running) and starts it if
    `started`; on failure the stream stays closed and `start` reports
    \"the capture stream is closed\". The AAudio error callback only logs
    `libmobile: capture stream: {error:?}`; nothing above libmobile can
    see it."
  - "AAudio (NDK): `AAudioStream_requestStart` returns
    `AAUDIO_ERROR_INVALID_STATE` when the stream is in a state it cannot
    start from (already starting or started, or closing, closed or dead);
    a stream the system disconnects (`AAUDIO_STREAM_STATE_DISCONNECTED`,
    for example a route change or the input taken from the app) can never
    start again and must be closed and a new one opened, and that must not
    be done from the error callback's thread. cpal 0.18.2's AAudio host
    maps `Disconnected`, `Unavailable`, `NoService` and `InvalidHandle` to
    `ErrorKind::DeviceNotAvailable` and `InvalidState`/`Internal` to
    `StreamInvalidated`, and also reports `Xrun` through the same error
    callback."
  - "AGENTS.md, Platform Layering: AAudio calls belong in libmobile. This
    plan adds none; it uses only libmobile's existing `CaptureStream`
    (`set_preset`, `start`, `stop`) and cpal."
related:
  - docs/design/20261016_01_AUDIO_DEVICE_AND_SOURCES_PLAN.md (the output
    stream, the mixer and its section 6, Suspend and resume; Implemented,
    unchanged)
  - docs/design/20261022_01_AUDIO_CAPTURE_PLAN.md (capture, sinks,
    `on_capture_ready` and its amendments on Android modes and libmobile's
    `CaptureStream`; In implementation, frozen, unchanged)
  - libmobile docs/design/20261022_02_AUDIO_CAPTURE_PLAN.md (the
    `CaptureStream` this plan reopens through)
  - RoundTwo docs/design/20261029_06_MOCK_INTERVIEW_SESSION_PLAN.md
    (amendment 19:01 EDT: the mic check; the first user)
---

# Audio streams that recover after a resume or a lost device

## 1. The cause

The failure is in `src/audio.rs`, `output::Audio::resume`:

```rust
if let Some(input) = &self.input
    && let Err(error) = input.stream.play()
{
    log::warn!("libmorph: audio capture resume failed: {error}");
}
```

On Android `input.stream.play()` is `aaudio::AaudioInput::play`, which is
libmobile's `CaptureStream::start`, which is `AAudioStream_requestStart`
on the stream opened at launch. While the app was away the stream had
been stopped by libmorph (`Audio::pause`, `CaptureStream::stop`) and was
then left in a state AAudio will not start from, so `requestStart`
returned `AAUDIO_ERROR_INVALID_STATE`. Since no block arrived afterwards,
the stream was dead, not merely already started: a stream the system has
disconnected (or closed under the app) never starts again. libmorph logs
the error and keeps the dead stream for the rest of the run: nothing
closes it, opens a new one, retries, or tells the app.

Three more paths have the same flaw:

- **A capture stream lost while running** (a headset plugged or
  unplugged, another app taking the microphone): libmobile's AAudio error
  callback logs `libmobile: capture stream: Disconnected` and nothing
  else, and blocks simply stop.
- **The output stream on resume**: `output.stream.play()` failing is only
  logged as `libmorph: audio resume failed: {error}`.
- **The output stream, and the desktop's cpal capture stream, lost while
  running**: their cpal error callbacks only log `libmorph: audio stream:
  {error}` or `libmorph: audio capture stream: {error}`.

A reopen is not possible today without losing state: the `Mixer` (voices
and sources) and the `Capture` (sinks, frame count, time base) are moved
into each cpal stream's callback closure. On Android the `Capture` lives
in libmobile's callback, which `CaptureStream::set_preset` already keeps
across a reopen; that is the path this plan uses there.

## 2. What the app sees

All additive. On every host and with every feature set, so signatures
never change (the stub without `audio` answers `Closed` and does nothing).

```rust
/// One of the two streams `AudioSetup` can open.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum AudioStream {
    Output,
    Capture,
}

/// Where a stream is. Read with `Audio::output_state` and
/// `Audio::capture_state`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum AudioState {
    /// Not asked for in `audio_setup`, not open yet (Android before
    /// RECORD_AUDIO), or no device; also scripted runs and the stub.
    Closed,
    /// Open and delivering (or being fed), as far as libmorph can tell.
    Running,
    /// Stopped with the app (`on_suspend`); starts again with it.
    Paused,
    /// Lost, and libmorph is reopening it. Sinks get no blocks meanwhile.
    Recovering,
    /// Every attempt failed. Nothing more is tried until
    /// `Audio::restart` or the next resume of the app.
    Failed,
}

/// A change in a stream's health, passed to `MorphApp::on_audio`.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AudioEvent {
    /// The stream stopped when it should run; libmorph is reopening it.
    /// `reason` is the error or "no data for 1000 ms".
    Interrupted { stream: AudioStream, reason: String },
    /// Running again, at the same format. For capture, the sinks are
    /// still attached and `Captured::frame` continues from where it
    /// stopped; the missing time shows in `Captured::time`.
    Restored { stream: AudioStream, attempts: u32 },
    /// Every attempt failed. `reason` is the last error.
    Failed { stream: AudioStream, reason: String },
}
```

On `Audio` (`&self`, like its other methods):

```rust
pub fn output_state(&self) -> AudioState;
pub fn capture_state(&self) -> AudioState;
/// After `Failed`, try again with a fresh set of attempts (for a "Try
/// again" button). True if an attempt was started; false if the stream
/// is not `Failed` (nothing to do) or was never opened.
pub fn restart(&self, stream: AudioStream) -> bool;
```

On `MorphApp`, with a default that does nothing:

```rust
/// A stream was interrupted, restored or given up on. Delivered on the
/// main thread before `update`, oldest first, while the app is resumed.
fn on_audio(&mut self, ctx: &mut MorphContext, event: &AudioEvent) {
    let _ = (ctx, event);
}
```

**`on_capture_ready` is unchanged**: once per run, when capture first
opens. A reopen asks for the same rate and channel count as the first
stream (libmobile's `set_preset` does; the desktop reuses the
`StreamConfig`), so `capture_format()` and `format()` never change during
a run and sinks and sources need no new setup. If the device will not
open at that format, the attempt fails like any other. Apps add no sink
again: the sinks stay attached throughout.

Not changed either: `pause`/`resume` with the app, `set_capture_mode`
(which already reopens on Android), the `audio-cpal` accessors (see
section 5 for `device()` after a reopen), and the fact that a sink gets
no blocks while the app is paused.

## 3. The recovery state machine

New private file `src/audio/recovery.rs` (`mod recovery;` in
`src/audio.rs`, compiled with `audio`), with no cpal, AAudio or clock of
its own, so it is tested with no hardware and made-up instants. One
`Recovery` per open stream.

```rust
/// Delays before attempt 1, 2, 3 and 4 after a loss. Then `Failed`.
pub(crate) const RETRY_DELAYS: [Duration; 4] = [ms(0), ms(250), ms(1000), ms(3000)];
/// A stream that should run and has delivered no block (capture) or
/// called no callback (output) for this long is lost. The same limit
/// applies from each (re)start to its first block.
pub(crate) const STALL: Duration = ms(1000);

pub(crate) struct Recovery {
    stream: AudioStream,
    state: AudioState,
    /// Attempts made since the loss (0 while `Running`).
    attempts: u32,
    /// When the next attempt is due, while `Recovering`.
    retry_at: Option<Instant>,
    /// The callback counter last seen, and when it last changed (or when
    /// the stream was last started).
    seen: u64,
    seen_at: Instant,
    /// The last error, for `Failed`.
    reason: String,
    events: Vec<AudioEvent>,
}

/// What the owner of the stream must do now.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Act {
    Nothing,
    /// Close the stream and open a new one with the same configuration,
    /// then call `reopened` with the result.
    Reopen,
}

impl Recovery {
    /// The stream opened and started (or opened paused: `paused` next).
    pub fn opened(stream: AudioStream, now: Instant, counter: u64) -> Self;
    /// The app paused: no watching, no retries. Keeps `Failed`.
    pub fn paused(&mut self);
    /// The app resumed and the stream's start returned `result`. Ok:
    /// `Running`, watched from `now`. Err: a loss (`Interrupted`) and
    /// `Act::Reopen` at once. From `Failed`, the attempts start afresh.
    pub fn resumed(&mut self, now: Instant, counter: u64, result: Result<(), String>) -> Act;
    /// The stream's error callback reported a device loss (set by the
    /// callback in an atomic flag, read on the main thread).
    pub fn lost(&mut self, now: Instant, reason: String) -> Act;
    /// Once per frame. While `Running`: a counter unchanged for `STALL`
    /// is a loss. While `Recovering` after a successful reopen: the first
    /// change of the counter is `Restored`; none within `STALL` is a
    /// failed attempt. While `Recovering` with `retry_at <= now`:
    /// `Act::Reopen`.
    pub fn tick(&mut self, now: Instant, counter: u64) -> Act;
    /// The reopen asked for by `Act::Reopen` returned `result`. Ok: wait
    /// for the first block (see `tick`). Err: the next delay, or
    /// `Failed` after the fourth attempt.
    pub fn reopened(&mut self, now: Instant, counter: u64, result: Result<(), String>);
    /// `Audio::restart`: from `Failed`, attempts start afresh (`Act::Reopen`).
    pub fn restart(&mut self, now: Instant) -> Act;
    pub fn state(&self) -> AudioState;
    pub fn take_events(&mut self) -> Vec<AudioEvent>;
}
```

Rules, all in `Recovery`:

1. A loss while `Running` (failed start, `lost`, or a stall) sets
   `Recovering`, `attempts = 0`, `retry_at = now + RETRY_DELAYS[0]`, and
   queues one `Interrupted`. A further loss while already `Recovering`
   queues nothing.
2. An attempt counts when `Act::Reopen` is returned. A reopen that fails,
   or succeeds but delivers nothing within `STALL`, schedules the next
   attempt at `now + RETRY_DELAYS[attempts]`; after the fourth, the state
   is `Failed` and one `Failed` event is queued.
3. The first counter change after a successful reopen sets `Running` and
   queues `Restored { attempts }`.
4. `paused` stops the clock: no stall and no retry while paused.
   `resumed` from `Recovering` or `Failed` starts with a fresh set of four
   attempts (the app came back, which is when the RoundTwo case is
   repaired).
5. Errors that are not a loss (cpal's `Xrun`, `BufferUnderrun` and the
   like) are not passed to `lost`; only cpal's
   `ErrorKind::DeviceNotAvailable` and `StreamInvalidated`.

The counters are an `Arc<AtomicU64>` per stream, incremented with
`Ordering::Relaxed` once per callback by libmorph's own callback (the
capture callback in `mod aaudio` and `build_input`, the output callback
in `build`). The lost flag is an `Arc<AtomicBool>` set by libmorph's cpal
error callbacks. No callback allocates, locks a new lock, or logs anything
new.

## 4. Capture on Android

In `mod aaudio`:

- `AaudioInput` keeps `asked: Cell<CaptureMode>` (the mode the app last
  asked for, at open and in `set_mode`) and `blocks: Arc<AtomicU64>`
  (incremented in the callback passed to `CaptureStream::open`).
- `AaudioInput::reopen(&self) -> Result<(), String>` is
  `self.stream.set_preset(preset(self.asked.get()))`, logging the mode
  granted as `set_mode` does. libmobile closes the old stream (whatever
  its state), opens a new one at the same format with the same callback,
  and starts it, because the failed `start` already set its `started`.
  libmorph does not call `start` again after a successful reopen (it
  would answer `InvalidState` on a started stream).
- If the reopen falls back to the default preset, that is still a
  recovery; `capture_mode()` reports the granted mode as today.

In `mod output`, `Audio` gains `capture_recovery: RefCell<Option<Recovery>>`
and `output_recovery: RefCell<Option<Recovery>>`, created when each stream
opens (`open`, `open_capture`), and:

- `resume`: for each open stream, call its start (`play`) and pass the
  result to `Recovery::resumed`; on `Act::Reopen`, reopen at once and pass
  the result to `reopened`.
- `pause`: as today, then `Recovery::paused`.
- `poll(&self, now)` (crate-private): for each stream, read the lost flag
  (swap to false) into `lost`, then `tick` with the counter, and reopen on
  `Act::Reopen`. Called once per frame from `Shell::draw`, before the
  app's `on_audio` events are delivered.
- `take_events(&self) -> Vec<AudioEvent>` (crate-private): both
  recoveries' events, oldest first.
- `capture_state`, `output_state`, `restart` read and drive the
  recoveries; `Closed` where there is none.
- Commands sent while a stream is `Failed` (`play`, `add_source`, …) are
  still accepted; for capture, `add_sink` still reaches the `Capture`
  through the same channel, which the next working stream reads.

In `runner/shell.rs`, `Shell::audio_events()` calls `ctx.audio.poll(now)`
and passes each event to `app.on_audio`, and `draw` calls it before
`camera_events`. The Android and winit runners keep calling
`audio.resume()`/`pause()` where they do today.

## 5. Output, and capture on the desktop

The same flaw, the same fix, through cpal:

- `Mixer` and its `Receiver<Command>` move into an
  `Arc<Mutex<MixerState>>` shared by the stream's callback. The callback
  takes it with `try_lock`; if it is held (only during a reopen, when the
  old and new streams may overlap for one callback) it writes silence and
  returns. Likewise `Capture` and its `Receiver<CaptureCommand>` on the
  desktop, in `Arc<Mutex<CaptureState>>`, dropping that one block.
- `Output::stream` and the desktop `Input::stream` become
  `RefCell<cpal::Stream>`. A reopen drops the old stream first, then
  builds a new one on the host's current default device with the
  original `StreamConfig` and sample format, and plays it.
- `format()`, `stream_config()`, `sample_format()`, `capture_format()`,
  `capture_stream_config()` and `capture_sample_format()` never change.
  `device()` and `capture_device()` keep returning the device first
  opened, documented as such (the stream may now run on the new default
  device); see open question 2.
- Voices keep their positions and sources keep running across an output
  reopen; the time lost is not made up.

## 6. Logging

All from `src/audio.rs`'s modules (log targets `libmorph::audio::*`), never samples, levels or any audio
content. `<stream>` is `audio capture` or `audio output`.

| When | Level | Line |
| --- | --- | --- |
| A start fails on resume | warn | `libmorph: <stream> resume failed: <error>; reopening` |
| The error callback reports a loss | warn | `libmorph: <stream> lost: <error>; reopening` |
| A stall | warn | `libmorph: <stream> stalled: no data for 1000 ms; reopening` |
| Each attempt | info | `libmorph: <stream> reopen <n>/4: ok` (capture adds `, asked <mode>, got <mode>`) or warn `… reopen <n>/4 failed: <error>` |
| The first block after a reopen | info | `libmorph: <stream> restored after <n> attempt(s), <ms> ms after the loss` |
| Giving up | error | `libmorph: <stream> failed after 4 attempts: <error>; restart() or the next resume tries again` |
| `restart` | info | `libmorph: <stream> restart asked` |

The existing `libmorph: audio capture resume failed: <error>` text is
kept as the start of the first line, so a search for it still finds the
event. Xruns stay at their current level and wording.

## 7. Files

- `src/audio.rs`: the three public types, `output_state`,
  `capture_state`, `restart` (real and stub), `poll` and `take_events`,
  the reopen paths, the counters and flags, the shared mixer and desktop
  capture state.
- `src/audio/recovery.rs`: section 3, with its tests.
- `src/app.rs`: `on_audio`.
- `src/lib.rs`: re-export `AudioEvent`, `AudioState`, `AudioStream`.
- `src/runner/shell.rs`: `audio_events`, called from `draw`.
- `examples/audiocapture.rs`: logs each `AudioEvent` (`audio: <event>`)
  and each written file's frame count as today.
- `examples/android/capture_resume.sh`: the S23 check (step 5).
- `AGENTS.md` (own commit): the Core lines for `on_audio`, the three
  types and the three methods.

## 8. Versioning

libmorph 0.5.3 → **0.5.4**, a patch under Rule 5.4: the fix changes no
signature, and everything new is additive (a trait method with a
default, new `#[non_exhaustive]` types, new methods on `Audio`). No
dependency moves: libmobile stays at 0.3.5 (`67d2e8f`) and cpal at
0.18.2. The `VERSIONS.md` entry names the fix and lists the additions.

## 9. Steps

1. **The state machine.** `src/audio/recovery.rs` as in section 3, with
   tests on made-up instants and counters:
   `failed_start_reopens_at_once_and_reports_interrupted`,
   `first_block_after_reopen_is_restored_with_attempts`,
   `stall_while_running_is_a_loss`,
   `no_stall_and_no_retry_while_paused`,
   `retries_follow_the_delays_then_fail_once`,
   `reopen_without_blocks_counts_as_failed`,
   `resume_after_failed_starts_afresh`,
   `restart_only_from_failed`,
   `second_loss_while_recovering_reports_nothing`.
   **Done when:** `cargo test -p libmorph recovery` runs those nine tests
   and they pass on the Mac, with no device opened, and the formatter and
   linter gates of AGENTS.md pass.

2. **The API.** Section 2's types, `Audio` methods (real and stub),
   `MorphApp::on_audio`, `Shell::audio_events` and the call in `draw`;
   `Audio::open` and `open_capture` create the recoveries; `resume`,
   `pause`, `poll`, `take_events` drive them; no reopen code yet
   (`Act::Reopen` reopens nothing and counts as a failed attempt).
   **Done when:** a unit test opens `Audio` with no streams and finds both
   states `Closed`, `restart` false and no events; the crate builds with
   and without `audio`, on the Mac and for `aarch64-linux-android` with
   `mobile` (as `examples/android/build.sh` builds it), and the AGENTS.md
   gates pass.

3. **Capture on Android.** Section 4: `asked`, `blocks`,
   `AaudioInput::reopen`, the lines of section 6.
   **Done when:** `cargo clippy` for `aarch64-linux-android` with `mobile`
   adds no warning and step 5's check passes for capture.

4. **Output, and capture on the desktop.** Section 5: the shared mixer
   and capture state with `try_lock`, `RefCell` streams, the cpal error
   callbacks setting the lost flag for `DeviceNotAvailable` and
   `StreamInvalidated` only, reopen on the default device with the
   original config.
   **Done when:** the existing mixer and capture tests pass unchanged; a
   new test applies commands to a `MixerState` through two successive
   "streams" (two callbacks sharing the `Arc`) and shows a voice playing
   on across them; `cargo run --example tone` and `cargo run --example
   audiocapture` on the Mac play and record as before (one file per
   second logged); and on the S23 step 5's check also shows
   `audio output` lines only if output was lost, with
   `ctx.audio().output_state()` logged as `Running` after each relaunch.

5. **The S23 check, adb only.** `examples/audiocapture.rs` logs each
   `AudioEvent` and, once per second, `capture_state()` and
   `output_state()`. New `examples/android/capture_resume.sh <cycles>`
   (default 20):
   1. builds and installs the example (`examples/android/build.sh
      audiocapture --install`), grants the microphone with `adb shell pm
      grant dev.highbay.libmorph.example.audiocapture
      android.permission.RECORD_AUDIO`, clears logcat and launches with
      `adb shell am start -n
      dev.highbay.libmorph.example.audiocapture/dev.highbay.libmobile.MobileNativeActivity`;
   2. waits for two `wrote … frames` lines;
   3. per cycle, one of three ways out, in turn: `adb shell input keyevent
      KEYCODE_HOME` and 5 s away; `KEYCODE_HOME` and 60 s away;
      `KEYCODE_HOME`, then `adb shell am start -a
      android.speech.action.RECOGNIZE_SPEECH` (another app takes the
      microphone), 5 s, `adb shell input keyevent KEYCODE_BACK`; then the
      same `am start -n` to bring the example back;
   4. after each return, requires within 5 s a new `wrote … frames` line
      with a nonzero frame count and `capture_state` `Running`, and
      records whether a `resume failed`, `lost` or `stalled` line and its
      `restored` line appeared;
   5. saves the whole logcat (`-s audiocapture libmobile libmorph`) and a
      summary to `out/audiocapture/20261029_NN_capture_resume/`
      (`logcat.txt`, `summary.txt`), appends an entry to
      `out/audiocapture.md`, and exits non-zero on any cycle without
      samples.

   The script is first run on 0.5.3 (`70fd9f0`) to find a way out that
   reproduces `audio capture resume failed: InvalidState` and no samples
   afterwards; that run is kept as the baseline evidence.
   **Done when:** on the S23 (SM-S918U, Android 16), the script exits 0
   for 20 cycles on this plan's build, the baseline run on 0.5.3 shows the
   failure for at least one way out (or, if none reproduces it, that is
   recorded in the evidence entry and in `CHANGES.md`), and in the fixed
   run every `resume failed`/`lost`/`stalled` line is followed by a
   `restored` line and an `audio: Restored` event.

6. **Release** as libmorph 0.5.4 on the owner's word: `version` in
   `Cargo.toml`, the `VERSIONS.md` entry in the same commit, tag `v0.5.4`;
   the AGENTS.md Core lines in their own commit; this plan's status by
   amendment with the README row.
   **Done when:** `v0.5.4` names the version commit and `VERSIONS.md` has
   its entry.

7. **RoundTwo, the first user.** RoundTwo (its own work, under its plan
   `20261029_06_MOCK_INTERVIEW_SESSION_PLAN.md`) moves both libmorph pins
   from `70fd9f0` to the 0.5.4 commit, relies on capture resuming after
   the app returns, implements `on_audio` so `Failed` for capture shows
   its "The microphone isn't working" message at once (and
   `Interrupted`/`Restored` are logged in its own log), checks
   `capture_state()` in its mic check before a round, and may offer
   `restart(AudioStream::Capture)` instead of asking the user to restart
   the app.
   **Done when:** on the S23, RoundTwo is sent home with `adb shell input
   keyevent KEYCODE_HOME` and brought back with `am start` during a mock
   interview, and the next take is a WAV longer than its 44-byte header
   with libvoicexn's transcript not empty, with no restart of the app.

## 10. Risks

- **A stall that is not a loss.** Android delivers zeros, not nothing,
  to an app behind Recents or with the microphone toggle off, so those do
  not trip the stall check; a device that legitimately delivers no
  callbacks for a second while running would be reopened needlessly,
  which costs a short gap and a log line.
- **Reopen racing an AAudio callback.** libmobile's `set_preset` drops the
  old stream before opening the new one, and closing waits for the
  callback; the reopen always runs on the main thread, never in an error
  callback.
- **Overlapping cpal callbacks** during an output reopen are covered by
  `try_lock` with silence; one buffer is lost.
- **The cause may be other than a disconnect.** If the baseline run shows
  `InvalidState` with blocks still arriving (a stream already started),
  the reopen costs a short gap and is still correct.

## 11. Open questions

1. **libmobile's error callback.** libmorph sees an Android capture loss
   only as a failed start or a stall, because libmobile's
   `CaptureStream` keeps the AAudio error to its log. A later libmobile
   plan could add `CaptureStream::take_error() -> Option<CaptureError>`
   (set from the error callback), which would let libmorph react at once
   rather than after 1000 ms. This plan does not need it; should it be
   planned in libmobile now or after the S23 check?
2. **`device()` after a reopen.** The `audio-cpal` accessors return
   references, so they keep the device first opened. Is that acceptable
   (documented), or should a later, minor-version change make them return
   an owned value?
3. **The thresholds.** Four attempts at 0, 250, 1000 and 3000 ms, and a
   1000 ms stall: right for a voice app, or should `AudioSetup` carry
   them (a new field on the `#[non_exhaustive]` struct, still additive)?
4. **Scripted runs.** Should a script command (for example
   `audio-fail capture`) post `AudioEvent::Failed` so apps such as
   RoundTwo can test their message headless? Not in this plan unless the
   owner wants it.
