---
title: Audio output device and streaming sources
date: 2026-10-16
status: Draft
covers:
  - Exposing the cpal output device libmorph opens, and its stream format,
    through `ctx.audio()`, as wgpu is exposed through `MorphContext`
  - An `audio-cpal` feature for the `pub use cpal;` re-export and the
    device accessors, with passthroughs for cpal's host backends
  - Streaming sources: app callbacks that write samples on the audio
    thread, mixed with the existing `Sound` voices
  - Pausing sources with the app, on every host that pauses the stream
sources:
  - Owner's decision (2026-10-16), relayed by the tower defense game's
    session: "I think we decided on libmorph exposing cpal device and maybe
    a basic add_source wrapper, exposing cpal isn't too different from
    exposing wgpu and gives the game engine as much ownership as possible
    of the API."
  - Consumer: the tower defense game. Its audio is a node graph rendered by
    the app, so prerecorded `Sound`s are not enough. It needs the output
    rate, channel count and sample format, and a callback that fills the
    output buffer without blocking.
  - Owner (2026-10-16), relayed the same way: "exposing cpal needs a
    feature, like wgpu in libmorph (and it's deps)".
  - `src/audio.rs` today: `Audio::open` builds a cpal output stream on the
    default device at its default config (f32, i16, u16 or i32 samples),
    and a `Mixer` on the audio thread applies `Command`s from an `mpsc`
    channel with `try_recv`, sums the voices into an f32 buffer and clamps
    to -1..1. The stream is paused and resumed only on Android, on
    `MobileEvent::Pause` and `Resume`.
  - libmorph's existing passthroughs (`mobile-camera = ["mobile",
    "libmobile/camera"]`, `web` enabling `wgpu/webgl` and
    `cpal?/wasm-bindgen`). libmobile and libdevice do not depend on cpal.
  - cpal 0.18.2 features: `asio`, `jack`, `pipewire`, `pulseaudio`,
    `realtime`, `realtime-dbus`, `audioworklet`, `wasm-bindgen`, `custom`.
  - cpal 0.18.2: `Device` is `Clone`; `StreamConfig { channels: u16,
    sample_rate: u32, buffer_size }`; `SampleFormat`.
related:
  - docs/design/20261014_03_SHELL_INPUT_AND_LIFECYCLE_PLAN.md (`on_suspend`
    and `on_resume`)
  - docs/design/20261014_04_SCRIPTED_RUNS_AND_PNG_CAPTURE_PLAN.md (scripted
    runs are silent)
---

# Audio output device and streaming sources

## 1. Goal

An app that synthesises its own audio can run it on libmorph's output
stream. libmorph keeps opening and owning the cpal device and stream on
every host, as today, and the app gets:

- the cpal `Device` and the stream's format, to set up its own graph at the
  right rate and channel count;
- `add_source`, a callback libmorph calls on the audio thread to fill part
  of each output buffer, mixed with the `Sound` voices;
- `remove_source`.

The stream itself is not exposed. libmorph pauses and resumes it with the
app, and an app holding it could break that.

Nothing changes for apps that only play `Sound`s.

## 2. Types

In `src/audio.rs`, exported from the crate root next to `Audio`, `Sound` and
`Voice`:

```rust
/// The output stream's format. Samples handed to sources are always `f32`,
/// whatever the device takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// A streaming source added with [`Audio::add_source`]. Stale ids are
/// ignored.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceId(u64);

/// Fills interleaved `f32` samples at the output's rate and channel count.
/// The buffer arrives zeroed; the source writes or adds into it.
pub type Source = Box<dyn FnMut(&mut [f32]) + Send>;
```

`OutputFormat` holds plain numbers, so it exists with and without the
`audio` feature.

## 3. Features

`audio` keeps its meaning (`dep:cpal`, `Sound` playback) and now also
covers sources, which need no cpal types. Exposing cpal itself is a
separate feature, so cpal's API only becomes part of an app's dependencies
when the app asks for it:

```toml
audio = ["dep:cpal"]
# The cpal re-export and the output device it opened.
audio-cpal = ["audio"]
# Passthrough: cpal host backends. Each also enables `audio-cpal`, since an
# app choosing a backend is reaching into cpal.
audio-jack = ["audio-cpal", "cpal/jack"]
audio-pulseaudio = ["audio-cpal", "cpal/pulseaudio"]
audio-pipewire = ["audio-cpal", "cpal/pipewire"]
audio-asio = ["audio-cpal", "cpal/asio"]
```

`web` already enables `cpal?/wasm-bindgen`. libmobile and libdevice do not
use cpal, so nothing passes through to them; on Android cpal uses AAudio
with no feature.

The backend features only make a backend available. libmorph still opens
`cpal::default_host()`. Choosing another host is not in this plan (see
section 8).

## 4. `Audio` methods

With `audio`:

```rust
impl Audio {
    /// The output's format; `None` without an output stream.
    pub fn format(&self) -> Option<OutputFormat>;

    pub fn add_source(&self, source: Source) -> SourceId;
    pub fn remove_source(&self, id: SourceId);
}
```

With `audio-cpal`, in addition:

```rust
impl Audio {
    /// The output device libmorph opened; `None` without one.
    pub fn device(&self) -> Option<&cpal::Device>;

    /// The config the stream was built with.
    pub fn stream_config(&self) -> Option<&cpal::StreamConfig>;

    /// The device's sample type. libmorph converts from `f32`.
    pub fn sample_format(&self) -> Option<cpal::SampleFormat>;
}
```

and `lib.rs` has `#[cfg(feature = "audio-cpal")] pub use cpal;`, so apps
write `libmorph::cpal` and do not declare their own copy.

Without `audio`, the stub `Audio` has `format()` (always `None`),
`add_source` (drops the source and returns `SourceId(0)`) and
`remove_source` (does nothing), so code using sources compiles either way.

`format()` is the call apps use to size their graph. The cpal accessors are
for anything libmorph does not wrap.

## 5. The mixer

`Command` gains two variants, sent over the existing channel:

```rust
AddSource(u64, Source),
RemoveSource(u64),
```

`Mixer` gains `sources: Vec<(u64, Source)>`, created with capacity 16, and
`scratch: Vec<f32>`. `Mixer::mix(out)` becomes:

1. zero `out` and add the voices, as today;
2. for each source: resize `scratch` to `out.len()`, fill it with zeros,
   call the source with it, and add it into `out`;
3. clamp to -1..1, as today.

Each source has its own scratch pass, so a source that writes instead of
adding cannot overwrite the voices or another source. `scratch` grows only
when cpal's buffer grows, and cpal keeps buffers the same size between
callbacks.

A source that panics is not caught. A panic on the audio thread is a bug
in the app, as a panic in `render` is.

**Dropping off the audio thread.** A removed source may own a large graph,
and freeing it inside the callback costs time the callback does not have.
`RemoveSource` does not drop the source on the audio thread. The mixer
sends it back over a second `mpsc` channel (`Sender::send` on an unbounded channel does not block), and `Audio` drops
whatever is waiting there at the start of each of its own methods and when
it is dropped itself. The sources still installed when the stream is
dropped are dropped with the callback, as voices are today.

`stop_all` keeps its meaning (every `Sound` voice) and does not remove
sources.

## 6. Suspend and resume

A paused cpal stream does not call its callback, so pausing the stream
pauses the sources and voices together. What changes is where the stream
is paused:

- **Android:** as today, on `MobileEvent::Pause` and `Resume`.
- **Desktop and web:** the winit runner pauses the stream in `suspended`
  and plays it again in `resumed` after the first. On macOS, Windows,
  Linux and the web, winit does not send `suspended` for minimising or
  hiding the window, so sources keep running there, as `update` and
  `render` do.
- **Scripted runs:** there is no stream (`Audio::silent()`). Sources are
  accepted and never called, and `format()` is `None`.

## 7. Steps

1. **Types and the mixer.** `OutputFormat`, `SourceId`, `Source`, the two
   commands, `sources`, `scratch` and the return channel, with unit tests
   in `mixer::tests`: a source's output is added to a voice's; two sources
   that both write (not add) are summed; the sum is clamped; a removed
   source is no longer called and is returned, not dropped, by the mixer;
   `stop_all` leaves sources running; a stale `RemoveSource` is ignored.
   **Done when:** `cargo test` passes with and without `audio`.
2. **`Audio` methods and features.** `format`, `add_source`,
   `remove_source` and the stub's versions; the `audio-cpal` and backend
   features from section 3; under `audio-cpal`, `device`, `stream_config`,
   `sample_format` and `pub use cpal;`. `start()` keeps the device, config
   and sample format it chose.
   **Done when:** clippy passes for default, `--no-default-features`,
   `--features audio-cpal`, Android `mobile,audio-cpal` and wasm `web`.
3. **Desktop suspend.** Pause and play the stream in the winit runner's
   `suspended` and `resumed`.
   **Done when:** clippy passes on every host, and the Android behaviour is
   unchanged.
4. **Example.** `examples/tone.rs`: a sine whose pitch follows the touch's
   height, as a source, using `format()`.
   **Done when:** the owner hears it follow the finger or mouse on the
   MacBook Pro.
5. **Docs.** The `MorphContext` and feature sections of AGENTS.md, and the
   README.
   **Done when:** both describe `add_source`, `format()`, `audio-cpal`
   and the backend features.

## 8. Open questions for the owner

1. **Backend passthroughs.** Recommended: `jack`, `pulseaudio`,
   `pipewire` and `asio`, as in section 3, and none of cpal's others
   (`realtime` and `audioworklet` change how libmorph's own stream runs,
   and `custom` is for cpal hosts written by the app). The alternative is
   `audio-cpal` alone, with backends added when an app needs one.
2. **Per-source volume.** Recommended: none. The app controls its own
   level inside the source. `Sound` voices keep `set_volume`.
3. **Device changes.** If the default output changes (headphones
   unplugged), cpal reports a stream error and libmorph logs it, as today.
   Recommended: reopening the stream on a new device is out of scope for
   this plan. When it is added, sources will move to the new stream and
   `format()` may change.
4. **Choosing the host.** With a backend feature on, an app may want
   libmorph to open that host (JACK, say) instead of the default.
   Recommended: a later plan, with a `--morph-audio-host` option, once an
   app asks for it.

## Amendment — 2026-10-16 09:01 EDT: The device as `DeviceTrait`, not `cpal::Device`

Replaces the `device()` signature in section 4. On the owner's
instruction (2026-10-16): "it should just be the trait device, not the
underlying implementation".

cpal's `DeviceTrait` has associated types and generic methods, so it
cannot be a `dyn` trait object. `device()` returns it as an opaque type
instead:

```rust
/// The output device libmorph opened; `None` without one. Only
/// `DeviceTrait`'s methods are reachable through it.
pub fn device(&self) -> Option<&impl cpal::traits::DeviceTrait>;
```

The concrete `cpal::Device` (the platform's host-dispatch type) is not
part of libmorph's API, so libmorph can change how it opens the device,
such as the host chosen in section 8 question 4, without breaking apps.
Apps call `description()`, `id()`, `supported_output_configs()` and the
other trait methods, with `use libmorph::cpal::traits::DeviceTrait;`.

`stream_config()` and `sample_format()` are unchanged. `StreamConfig`
and `SampleFormat` are plain data in cpal, not a host's implementation.

Also: one line in section 5's "Dropping off the audio thread" paragraph
runs past the wrapping width. The text is correct.

## Amendment — 2026-10-16 09:07 EDT: libmobile owns cpal on Android

Replaces the feature table in section 3 and adds a step before section 7's
step 1. Agreed with the owner (2026-10-16): "libmobile own cpal on android
and libmorph owns it on desktop/web, feature passed down to libmobile so
that cpal dependency remains optional".

libmobile owns the Android platform layer, including the NDK. cpal reaches
AAudio through the `ndk` crate, so on Android cpal belongs to libmobile,
and libmorph uses libmobile's copy, as it already does for
`android_activity` and `ndk`. On desktop and the web, libmorph is the
platform layer and keeps its own cpal. libdevice stays data only.

### libmobile (its own plan, in its repository)

- A `cpal` feature: `cpal = ["dep:cpal"]`, with cpal 0.18 (the version
  libmorph uses) as an optional Android-only dependency.
- `libmobile::android::cpal`: a re-export, next to `android_activity`
  and `ndk`. libmobile adds no wrapper of its own.
- Without the feature, libmobile does not depend on cpal.

### libmorph features

```toml
# cpal on desktop and the web; libmobile's cpal on Android.
audio = ["dep:cpal", "libmobile?/cpal"]
# The cpal re-export and the output device it opened, on every host.
audio-cpal = ["audio"]
# Passthrough: cpal host backends (desktop). Each also enables
# `audio-cpal`. They enable nothing on Android or the web.
audio-jack = ["audio-cpal", "cpal/jack"]
audio-pulseaudio = ["audio-cpal", "cpal/pulseaudio"]
audio-pipewire = ["audio-cpal", "cpal/pipewire"]
audio-asio = ["audio-cpal", "cpal/asio"]
```

- libmorph's own cpal becomes a dependency for
  `cfg(not(target_os = "android"))` only. `web` keeps
  `cpal?/wasm-bindgen`.
- On Android, `mobile` is always on (the target guard), so `audio` turns
  on `libmobile/cpal`. Without `audio`, neither crate depends on cpal.
- In `src/audio.rs` and `lib.rs`, one alias picks the copy: on Android
  `libmobile::android::cpal`, elsewhere `::cpal`. `pub use cpal;`
  (under `audio-cpal`) re-exports that alias, so apps write
  `libmorph::cpal` on every host, and `device()`, `stream_config()`
  and `sample_format()` exist on Android too.

### Steps

Before section 7's step 1:

0. **libmobile.** The libmobile plan above is implemented and pushed, and
   libmorph pins it. Android audio then builds with libmobile's cpal.
   **Done when:** `cargo clippy --target aarch64-linux-android --features
   mobile` passes with one cpal in `Cargo.lock`, and Android playback
   is unchanged.

## Amendment — 2026-10-16 09:10 EDT: Approval and answers

The owner approved this plan (2026-10-16), with the answers to section 8
below. Where they disagree with earlier text, they win.

1. **Backend passthroughs: none.** `audio-jack`, `audio-pulseaudio`,
   `audio-pipewire` and `audio-asio` are dropped from section 3 and from
   the amendment "libmobile owns cpal on Android". Desktop development is
   on macOS, where cpal uses CoreAudio with no feature. They come back,
   each a one-line passthrough, when Linux or Windows is supported. The
   features are `audio` and `audio-cpal` only.
2. **Volume: one for the whole app.** No volume per source. `Audio`
   gains:

   ```rust
   /// Scales the whole mix (voices and sources) before it is clipped.
   /// 1.0 at start; negative values are treated as 0.
   pub fn set_master_volume(&self, volume: f32);
   ```

   It is a `Command::SetMasterVolume(f32)` to the mixer, which multiplies
   every sample by it after summing and before clamping. The stub has it
   too, doing nothing. `Sound` voices keep `set_volume`. Step 1 adds a
   test that it scales voices and sources together.
3. **Device changes:** out of scope, as recommended.
4. **Choosing the host:** out of scope, as recommended.

Section 7's step 2 checks clippy for default, `--no-default-features`,
`--features audio-cpal`, Android `mobile` and `mobile,audio-cpal`, and
wasm `web`.
