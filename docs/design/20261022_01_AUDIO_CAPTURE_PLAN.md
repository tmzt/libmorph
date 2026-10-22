---
title: Audio capture and the audio_setup hook
date: 2026-10-22
status: In implementation
covers:
  - A `MorphApp::audio_setup` hook, called before audio is opened, in
    which the app asks for a capture (input) stream and may turn the
    output stream off
  - Opening the default input device with cpal, on the hosts where cpal
    supports one, and pausing it with the app as the output is paused
  - Capture sinks: app callbacks that receive each captured block on the
    audio input thread, as `f32` samples with a frame count and a capture
    time
  - An `audiocapture` example that writes the captured audio to `.au`
    files named by millisecond timestamps and logs each file name
sources:
  - Owner's request (2026-10-22): "implement audio capture and an
    audio_setup hook. create an example audiocapture that captures and
    dumps audio frames to files with millis timestamps (au or raw pcm) and
    prints the filename to the log".
  - `src/audio.rs` today (plan 20261016_01): `Audio::open` builds a cpal
    output stream on the default device at its default config, with a
    `Mixer` driven by `Command`s over an `mpsc` channel that the callback
    only `try_recv`s; removed sources are sent back to the main thread to
    be dropped. Without `audio`, `Audio` is a zero-sized stub.
  - cpal 0.18.2: `DeviceTrait::default_input_config`,
    `build_input_stream::<T>(config, FnMut(&[T], &InputCallbackInfo), ..)`;
    `InputCallbackInfo::timestamp().capture` is a `StreamInstant` with
    `duration_since`. The webaudio host returns `UnsupportedOperation`
    for input. On macOS the first input stream triggers the system's
    microphone permission prompt.
  - libmobile 0.1.1: no runtime permission requests. `gradle::Project::
    permission(name)` adds a `uses-permission` to the manifest only.
  - Sun `.au` format: a 24-byte big-endian header (magic `.snd`, data
    offset, data size, encoding, sample rate, channels); encoding 3 is
    16-bit linear PCM, big-endian samples.
related:
  - docs/design/20261016_01_AUDIO_DEVICE_AND_SOURCES_PLAN.md (the output
    stream, sources, `audio-cpal`, pausing with the app)
  - docs/design/20261014_05_APP_LIMITS_REQUEST_PLAN.md (`limits`, the
    other hook called before `init` without `self`)
  - docs/design/20261014_04_SCRIPTED_RUNS_AND_PNG_CAPTURE_PLAN.md
    (scripted runs are silent)
---

# Audio capture and the audio_setup hook

## 1. Goal

An app can record from the default microphone. Opening an input device is
not free (on macOS it asks the user for permission; on Android it needs
`RECORD_AUDIO`), so capture is opt-in, chosen before libmorph opens any
audio. The app chooses in a new hook, `audio_setup`, which can also turn
the output stream off for apps that only record.

Nothing changes for apps that do not override the hook: libmorph opens
the output stream as today, and no input.

## 2. The hook

In `src/app.rs`:

```rust
pub trait MorphApp {
    /// What audio to open, before [`init`](Self::init). `setup` starts
    /// with output on and capture off.
    fn audio_setup(setup: &mut AudioSetup) {
        let _ = setup;
    }
}
```

Like `limits`, it takes no `self`: it runs before the app exists. Each
runner calls it once, right before it opens `Audio`, on the main thread.
The scripted runner calls it too, and ignores the answer (scripted runs
stay silent).

In `src/audio.rs`, exported from the crate root:

```rust
/// What [`MorphApp::audio_setup`] asks libmorph to open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct AudioSetup {
    /// Open the default output device. `true` by default.
    pub output: bool,
    /// Open the default input device. `false` by default.
    pub capture: bool,
}

impl Default for AudioSetup { /* output: true, capture: false */ }
```

`#[non_exhaustive]` lets later plans add fields (a sample rate, a channel
count, a device choice) without a breaking release; apps assign the fields
they want on the `&mut` they are given.

## 3. Capture types

```rust
/// The capture stream's format. Samples handed to a sink are always
/// `f32`, whatever the device delivers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// One block from the input device, as handed to a [`Sink`].
#[derive(Debug)]
pub struct Captured<'a> {
    /// Interleaved, at [`CaptureFormat`]'s rate and channel count.
    pub samples: &'a [f32],
    /// Frames captured before this block since the stream opened. It
    /// does not advance while the app is paused.
    pub frame: u64,
    /// When this block's first frame was captured, after the first
    /// block's, by the device's clock. It includes time spent paused.
    pub time: std::time::Duration,
}

/// A capture sink added with [`Audio::add_sink`]. Stale ids are ignored.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SinkId(u64);

/// Receives captured blocks on the audio input thread, and must not block.
pub type Sink = Box<dyn FnMut(&Captured<'_>) + Send>;
```

These are plain types and exist with and without `audio`.

## 4. `Audio` methods

With `audio`:

```rust
impl Audio {
    /// The capture stream's format; `None` without one.
    pub fn capture_format(&self) -> Option<CaptureFormat>;

    /// Hand every captured block to `sink` until it is removed.
    pub fn add_sink(&self, sink: Sink) -> SinkId;
    /// The sink is dropped on this thread on a later call, not on the
    /// audio thread.
    pub fn remove_sink(&self, id: SinkId);
}
```

With `audio-cpal`, in addition, mirroring the output accessors:

```rust
pub fn capture_device(&self) -> Option<&impl DeviceTrait>;
pub fn capture_stream_config(&self) -> Option<&cpal::StreamConfig>;
pub fn capture_sample_format(&self) -> Option<cpal::SampleFormat>;
```

The stub `Audio` without `audio` has `capture_format()` (`None`),
`add_sink` (drops the sink, returns `SinkId(0)`) and `remove_sink`.

`is_available()` keeps meaning "there is an output stream".

## 5. Opening and the input callback

`Audio::open(setup: AudioSetup)` replaces `Audio::open()`:

- `setup.output`: start the output stream as today, or leave it `None`.
- `setup.capture`: take `default_host().default_input_device()` and its
  `default_input_config()`, and build an input stream for `f32`, `i16`,
  `u16` or `i32` samples. Any failure (no device, permission refused,
  unsupported host, another sample format) logs
  `libmorph: audio capture unavailable: <error>` and leaves capture
  `None`; the app still runs.

The input callback owns a `Capture` state (in a `capture` module next to
`mixer`):

1. apply `CaptureCommand::{AddSink(u64, Sink), RemoveSink(u64)}` from an
   `mpsc` channel with `try_recv`. A removed sink is sent back over a
   second channel and dropped on the main thread, as sources are;
2. convert the block into a reused `Vec<f32>` with `f32::from_sample`
   (grown only when cpal's buffer grows);
3. compute `time` as `info.timestamp().capture.duration_since(first)`,
   where `first` is the first block's capture instant;
4. call every sink with the same `Captured`, then add the block's frames
   to `frame`.

The input stream pauses and resumes with the output stream, in the same
places (`Audio::pause` and `resume`: winit's `suspended`/`resumed`,
Android's `Pause`/`Resume`).

## 6. Hosts

- **macOS, Linux, Windows**: cpal's default host. macOS asks for
  microphone permission on first use, for the terminal or app bundle that
  runs the binary.
- **Android**: AAudio through libmobile's cpal. The app must declare
  `android.permission.RECORD_AUDIO` (`.permission(..)` on its Gradle
  project) and the permission must already be granted; libmobile has no
  runtime permission request, so until it does the stream fails to open
  and capture is `None`. Requesting the permission is not in this plan.
- **Web**: cpal's webaudio host has no input, so capture is always
  `None` and the warning is logged.
- **Scripted runs**: silent, no capture.

## 7. The `audiocapture` example

`examples/audiocapture.rs`, desktop only (it writes files):

- `audio_setup` sets `capture = true` and `output = false`.
- `init` reads `capture_format()`; with none it logs a warning and draws
  a dark red screen. Otherwise it starts a writer thread and adds a sink.
- The sink copies samples into a buffer of `chunk` frames (default
  1000 ms; the first app argument gives milliseconds). When it is full it
  sends the buffer and the chunk's first-frame `time` to the writer over
  an `mpsc` channel, and takes an empty buffer back from a second channel
  (allocating only when none has come back yet), so the audio thread does
  no file I/O.
- The writer names each file `<unix ms>.au`: the Unix time in
  milliseconds when the sink was added, plus the chunk's `time` in
  milliseconds. It writes it under `out/audiocapture/` (created at start;
  `out/` is gitignored), as a Sun `.au` file: 24-byte header, encoding 3
  (16-bit linear PCM), the capture rate and channels, big-endian samples
  clamped to -1..1. After each file it logs
  `wrote out/audiocapture/<unix ms>.au (<frames> frames, <channels> ch,
  <rate> Hz)`, and sends the buffer back.
- The screen shows the last chunk's peak level as brightness.
- On exit the partial last chunk is dropped; files hold whole chunks.

## 8. Steps

1. **Types, hook and opening.** `AudioSetup`, `CaptureFormat`,
   `Captured`, `SinkId`, `Sink` and `MorphApp::audio_setup`;
   `Audio::open(setup)` with optional output and capture; the capture
   callback and sinks; the stub. The runners call `A::audio_setup` and
   pass the result. Unit tests for `Capture` (sink fan-out, frame
   counting, time from the first block, removed sinks returned undropped)
   and for `AudioSetup::default()`.
   **Done when:** `cargo test -p libmorph` passes with and without
   default features, and an app that does not override `audio_setup`
   behaves as before.
2. **Pause and resume.** The input stream pauses and resumes in
   `Audio::pause`/`resume`.
   **Done when:** on desktop, sinks are not called while the app is
   suspended, and `frame` continues from where it stopped.
3. **Example.** `examples/audiocapture.rs` as in section 7, with a unit
   test of its `.au` header and sample encoding.
   **Done when:** `cargo run --example audiocapture` on macOS, after the
   permission is granted, logs one `wrote ….au` line per second, and the
   files play in `afplay` with the sound of the room.

## 9. Release

Everything is additive (a defaulted trait method, new types, new
methods), so it is released as libmorph 0.2.2 (Rule 5.4). `Audio::open`
is crate-private.

## Amendment — 2026-10-22 06:56 EDT: implementation has started; the document is now immutable

The owner asked for this to be implemented (2026-10-22), on the
`audio-capture` branch in its own worktree. Steps 1 and 2 are built in
this commit.

## Amendment — 2026-10-22 06:58 EDT: the example on Android

Agreed with the owner (2026-10-22, "yes, add it and push to the s23").
Replaces section 7's first line ("desktop only (it writes files)") and adds
a step 4.

- `examples/audiocapture.rs` writes to `ctx.data_dir()/audiocapture/`
  when there is a data directory (Android), and to `out/audiocapture/`
  otherwise. Its `main` is built on the desktop only.
- `examples/android/audiocapture/` is a new workspace member,
  `libmorph-example-audiocapture-android`, a cdylib named
  `audiocapture_android`. It includes the example's app with
  `#[path]`, and its `android_main` runs it with `run_android` and
  `android_logger` (tag `audiocapture`). Its `build.rs` calls
  `libmorph_build::gradle_project("dev.highbay.libmorph.example.audiocapture")`
  (libmorph-build by path) with `.permission("android.permission.RECORD_AUDIO")`.
  On a host it builds as an empty library.
- `examples/android/build.sh [--install]` builds the `.so` and the
  generated Gradle project, as libmobile's `examples/build.sh` does. With
  `--install` it installs the debug APK, grants `RECORD_AUDIO` with
  `adb shell pm grant` (libmobile has no runtime permission request), and
  launches the app. Files are fetched with
  `adb exec-out run-as <package> cat files/audiocapture/<name>`.

4. **On the S23.** **Done when:** the app on the owner's Galaxy S23 logs
   one `wrote …/audiocapture/<unix ms>.au` line per second, and a file
   pulled from the device plays with the sound of the room.

## Amendment — 2026-10-22 07:05 EDT: asking for RECORD_AUDIO, and on_capture_ready

Agreed with the owner (2026-10-22: "that's fine", and "hook" for a hook
rather than holding back `init` until the user answers). Replaces
section 6's Android item, and adds to sections 2, 4 and 7 and a step 5.
It depends on libmobile's
`docs/design/20261022_01_RUNTIME_PERMISSIONS_PLAN.md`, written the same
day.

- **Hook.** `MorphApp` gains
  `fn on_capture_ready(&mut self, ctx: &mut MorphContext)`, with an
  empty default. It is called once per run, after `init`, when the
  capture stream has opened: right after `init` when it opened with the
  rest of audio (desktop, or Android with the permission already
  granted), or later on Android, once the user allows the request. It is
  never called if capture was not asked for or did not open.
  `ctx.audio().capture_format()` is `Some` from then on.
- **Sinks before the stream.** `Audio::add_sink` keeps sinks added
  before the capture stream opens, and hands them to it when it does, so
  an app may add them in `init`. `remove_sink` removes a kept one. If
  capture never opens they are dropped with `Audio`.
- **Android.** With `AudioSetup::capture`, the runner checks
  `libmobile::android::permission_granted(app, "android.permission.RECORD_AUDIO")`
  before opening audio. If it is granted, capture opens as before. If
  not, audio opens without capture, and right after `init` the runner
  calls `request_permissions` for it. On `MobileEvent::Permissions`
  with `RECORD_AUDIO` granted, it opens the capture stream
  (`Audio::open_capture`, crate-private), starts it paused if the app is
  paused, and calls `on_capture_ready`. A denial logs
  `libmorph: audio capture: RECORD_AUDIO denied`, and capture stays
  closed for the run. The app still declares the permission in its
  manifest.
- **Example.** `audiocapture` adds its sink in `on_capture_ready`
  instead of `init`, and shows the dark red screen until then.
  `examples/android/build.sh` no longer grants permissions with
  `adb`, so installing shows the system dialog.

5. **Asking on Android.** The hook, kept sinks, the Android request and
   the example changes, on libmobile 0.1.2.
   **Done when:** on the owner's Galaxy S23, after
   `adb shell pm revoke dev.highbay.libmorph.example.audiocapture
   android.permission.RECORD_AUDIO`, launching the example shows the
   system dialog. Allowing it starts the `wrote …` lines, and denying it
   logs the denial and keeps the red screen.

Release: additive (a defaulted hook, crate-private opening), so it stays
in libmorph 0.2.2 if released together, or 0.2.3 after. Moving libmobile
from 0.1.1 to 0.1.2 is semver-compatible.

## Amendment — 2026-10-22 07:13 EDT: a scrolling spectrogram in the example

Agreed with the owner (2026-10-22: "yes, add it. it's more useful than
voice reactive plasma"). Replaces section 7's "The screen shows the last
chunk's peak level as brightness", and adds a step 6. Only the example
changes; libmorph's API does not.

- A second sink, added in `on_capture_ready` next to the file writer's,
  analyses the capture on the audio thread with no allocation after
  setup: the channels are averaged to mono, and every 512 frames the
  last 1024 go through a Hann window and a 1024-point radix-2 FFT
  written in the example (no new dependency). That is 93.75 columns a
  second at 48 kHz.
- Each column has 256 rows, log-spaced in frequency from 40 Hz to the
  Nyquist frequency. Each row is the strongest FFT bin it covers, in
  dBFS mapped from -100..-20 to 0..255.
- Columns go into a ring of 512 shared with the main thread under a
  `Mutex` that the audio thread only `try_lock`s. A column that meets
  a held lock is dropped. Each frame the main thread copies the ring
  into an `R8Unorm` texture (512 x 256) when it has changed.
- `examples/audiocapture.wgsl` draws it full screen, scrolling right to
  left with the newest column at the right edge and low frequencies at
  the bottom, through a black, blue, magenta, orange, white colour ramp.
  The dark red screen until capture is ready is kept.

6. **Spectrogram.** As above, with unit tests of the FFT (a sine's peak
   bin) and the row mapping.
   **Done when:** on the desktop and on the owner's S23, speech and
   whistling show as moving bands, and silence (as when Android silences
   the app behind Recents) shows black.

## Amendment — 2026-10-22 07:24 EDT: capture modes on Android

Agreed with the owner (2026-10-22). Asked "can we support the different
capture modes on android via the audio_setup hook?", the owner agreed to
AAudio driven from Rust ("that's how libmobile/libmorph is designed, small
kotlin plugins that expose functionality not available in ndk"), to
presets only for now ("the eventual goal is full-duplex stt/tts but
that's beyond this example, I just want to demonstrate it will work"),
and to switching at runtime ("that's fine, audio_setup is mostly for
format setup"). Adds to sections 2, 4, 5, 6 and 7, and a step 7.
Replaces section 4's statement that the `audio-cpal` capture accessors
mirror the output ones on every host.

- **Mode.** In `src/audio.rs`, exported from the crate root:

  ```rust
  /// Android's AAudio input preset. The desktop and the web ignore it.
  #[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
  #[non_exhaustive]
  pub enum CaptureMode {
      /// AAudio's own choice (VoiceRecognition).
      #[default]
      Default,
      Generic,
      Camcorder,
      VoiceRecognition,
      /// Echo cancellation, noise suppression and gain control.
      VoiceCommunication,
      /// The microphone without processing, where the device offers it.
      Unprocessed,
      /// Low latency, for live monitoring.
      VoicePerformance,
  }
  ```

  `AudioSetup` gains `pub capture_mode: CaptureMode` (`Default`), the
  mode capture opens with.
- **Methods.** `Audio::capture_mode() -> Option<CaptureMode>`: the mode
  the open stream has (on Android, the preset AAudio reports; elsewhere
  `Default`), `None` without capture. `Audio::set_capture_mode(&self,
  mode) -> Option<CaptureMode>`: on Android, close the capture stream and
  open it again with `mode`, at the same sample rate and channel count,
  keeping the sinks, the frame count and the time base, and paused if it
  was paused; it returns the mode granted. Elsewhere, and without
  capture, it does nothing and returns `capture_mode()`. The stub has
  both, returning `None`.
- **Android input.** cpal 0.18.2 cannot set an input preset, so on
  Android libmorph opens the capture stream itself with
  `ndk::audio::AudioStreamBuilder` (through `libmobile::android::ndk`):
  input, `PCM_Float`, `input_preset` from the mode (raw values 1, 5, 6,
  7, 9, 10, so `VoicePerformance` needs no API-level feature; `Default`
  sets none). It keeps the rate and channel count AAudio chooses, and the
  same ones on a reopen. If a mode fails to open, libmorph logs
  `libmorph: audio capture: <mode> unavailable (<error>), using Default`
  and opens with `Default`. The callback hands its `f32` frames to the
  same `Capture` state as on cpal, kept in an `Arc<Mutex<_>>` that only
  the callback locks while the stream is open, so a reopen can take the
  sinks back. `time` comes from a monotonic clock started when capture
  first opened. Pausing uses `request_stop`, since AAudio input streams
  have no pause. Output stays on cpal.
- **cpal accessors.** Under `audio-cpal`, `capture_device()`,
  `capture_stream_config()` and `capture_sample_format()` are `None`
  on Android, where no cpal device is involved.
- **libmobile.** Its `cpal` feature enables `ndk/audio` itself rather
  than through cpal's own dependency (libmobile `CHANGES.md`, released in
  0.1.2).
- **Example.** A tap cycles the mode through `set_capture_mode` in the
  order above, logging `mode: asked <mode>, got <mode>`. A band across
  the top of the screen shows the granted mode by colour, and each file
  is named `<unix ms>_<mode>.au`, the mode in snake case.

7. **Capture modes.** As above.
   **Done when:** on the owner's Galaxy S23, tapping through the modes
   logs each one granted, `dumpsys audio` shows the matching source
   (`CAMCORDER`, `VOICE_RECOGNITION`, `VOICE_COMMUNICATION`,
   `UNPROCESSED`, `VOICE_PERFORMANCE`, or `MIC` for `Generic`), and
   files keep coming across the switches.
