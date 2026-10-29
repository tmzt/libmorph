//! In-memory sample playback and microphone capture. With the `audio`
//! feature, sounds are mixed into a `cpal` output stream and captured blocks
//! are handed to sinks; without it [`Audio`] is a zero-sized no-op, so code
//! calling it compiles either way.

use std::sync::Arc;
use std::time::Duration;

/// What [`MorphApp::audio_setup`](crate::MorphApp::audio_setup) asks
/// libmorph to open. Assign the fields on the `&mut` the hook is given.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct AudioSetup {
    /// Open the default output device. `true` by default.
    pub output: bool,
    /// Open the default input device. `false` by default: on macOS it asks
    /// for microphone permission, and Android needs `RECORD_AUDIO`.
    pub capture: bool,
    /// The mode capture opens with; change it later with
    /// [`Audio::set_capture_mode`].
    pub capture_mode: CaptureMode,
}

impl Default for AudioSetup {
    fn default() -> Self {
        Self {
            output: true,
            capture: false,
            capture_mode: CaptureMode::Default,
        }
    }
}

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

/// One of the two streams [`AudioSetup`] can open.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum AudioStream {
    Output,
    Capture,
}

/// Where a stream is. Read with [`Audio::output_state`] and
/// [`Audio::capture_state`].
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
    /// [`Audio::restart`] or the next resume of the app.
    Failed,
}

/// A change in a stream's health, passed to
/// [`MorphApp::on_audio`](crate::MorphApp::on_audio).
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

/// When to reopen a lost stream.
#[cfg(feature = "audio")]
mod recovery;

/// The capture stream's format. Samples handed to a [`Sink`] are always
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
    /// Frames captured before this block since the stream opened. It does
    /// not advance while the app is paused.
    pub frame: u64,
    /// When this block's first frame was captured, after the first block's,
    /// by the device's clock. It includes time spent paused.
    pub time: Duration,
}

/// A capture sink added with [`Audio::add_sink`], for
/// [`Audio::remove_sink`]. Stale ids are ignored.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SinkId(u64);

/// Receives captured blocks on the audio input thread, and must not block.
pub type Sink = Box<dyn FnMut(&Captured<'_>) + Send>;

/// Interleaved `f32` samples held in memory. Cloning shares the samples.
#[derive(Clone, Debug)]
pub struct Sound {
    samples: Arc<[f32]>,
    channels: u16,
    sample_rate: u32,
}

impl Sound {
    /// # Panics
    ///
    /// If `channels` or `sample_rate` is zero.
    pub fn new(samples: impl Into<Arc<[f32]>>, channels: u16, sample_rate: u32) -> Self {
        assert!(channels > 0, "a sound needs at least one channel");
        assert!(sample_rate > 0, "a sound needs a sample rate");
        Self {
            samples: samples.into(),
            channels,
            sample_rate,
        }
    }

    #[must_use]
    pub fn channels(&self) -> u16 {
        self.channels
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Whole frames (one sample per channel).
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }

    #[must_use]
    pub fn duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f64(self.frames() as f64 / f64::from(self.sample_rate))
    }
}

/// A playing instance of a [`Sound`], for [`Audio::stop`] and
/// [`Audio::set_volume`]. Stale handles are ignored.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Voice(u64);

/// The output stream's format. Samples handed to a [`Source`] are always
/// `f32`, whatever the device takes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// A streaming source added with [`Audio::add_source`], for
/// [`Audio::remove_source`]. Stale ids are ignored.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceId(u64);

/// Fills interleaved `f32` samples at the output's rate and channel count,
/// on the audio thread. The buffer arrives zeroed; the source writes or adds
/// into it, and must not block.
pub type Source = Box<dyn FnMut(&mut [f32]) + Send>;

#[cfg(all(feature = "audio", not(target_os = "android")))]
pub use ::cpal;
/// The cpal libmorph opens its stream with: libmobile's on Android, its own
/// elsewhere.
#[cfg(all(feature = "audio", target_os = "android"))]
pub use libmobile::android::cpal;

#[cfg(feature = "audio")]
pub use output::Audio;

#[cfg(not(feature = "audio"))]
pub use stub::Audio;

#[cfg(not(feature = "audio"))]
mod stub {
    use super::{
        AudioEvent, AudioSetup, AudioState, AudioStream, CaptureFormat, CaptureMode, OutputFormat,
        Sink, SinkId, Sound, Source, SourceId, Voice,
    };

    /// The `audio` feature is off: every call is a no-op.
    #[derive(Debug, Default)]
    pub struct Audio;

    impl Audio {
        pub(crate) fn open(_setup: AudioSetup) -> Self {
            Self
        }

        #[cfg_attr(not(target_os = "android"), allow(dead_code))]
        pub(crate) fn open_capture(&mut self) -> bool {
            false
        }

        #[cfg(script_runner)]
        pub(crate) fn silent() -> Self {
            Self
        }

        #[must_use]
        pub fn is_available(&self) -> bool {
            false
        }

        pub fn play(&self, _sound: &Sound) -> Voice {
            Voice(0)
        }

        pub fn play_with(&self, _sound: &Sound, _volume: f32, _looping: bool) -> Voice {
            Voice(0)
        }

        pub fn set_volume(&self, _voice: Voice, _volume: f32) {}

        pub fn stop(&self, _voice: Voice) {}

        pub fn stop_all(&self) {}

        #[must_use]
        pub fn format(&self) -> Option<OutputFormat> {
            None
        }

        /// Drops the source: there is no output to call it from.
        pub fn add_source(&self, _source: Source) -> SourceId {
            SourceId(0)
        }

        pub fn remove_source(&self, _id: SourceId) {}

        pub fn set_master_volume(&self, _volume: f32) {}

        #[must_use]
        pub fn capture_format(&self) -> Option<CaptureFormat> {
            None
        }

        #[must_use]
        pub fn capture_mode(&self) -> Option<CaptureMode> {
            None
        }

        pub fn set_capture_mode(&self, _mode: CaptureMode) -> Option<CaptureMode> {
            None
        }

        /// Drops the sink: there is no capture to feed it.
        pub fn add_sink(&self, _sink: Sink) -> SinkId {
            SinkId(0)
        }

        pub fn remove_sink(&self, _id: SinkId) {}

        /// Always [`AudioState::Closed`]: there is no output.
        #[must_use]
        pub fn output_state(&self) -> AudioState {
            AudioState::Closed
        }

        /// Always [`AudioState::Closed`]: there is no capture.
        #[must_use]
        pub fn capture_state(&self) -> AudioState {
            AudioState::Closed
        }

        /// Always false: there is no stream to restart.
        pub fn restart(&self, _stream: AudioStream) -> bool {
            false
        }

        pub(crate) fn poll(&self, _now: web_time::Instant) {}

        pub(crate) fn take_events(&self) -> Vec<AudioEvent> {
            Vec::new()
        }

        pub(crate) fn pause(&self) {}

        pub(crate) fn resume(&self) {}
    }
}

#[cfg(feature = "audio")]
mod output {
    use std::cell::{Cell, RefCell};
    use std::fmt;
    use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, mpsc};

    use web_time::Instant;

    use super::cpal;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample};

    #[cfg(not(target_os = "android"))]
    use super::capture::CaptureState;
    use super::capture::{Capture, CaptureCommand};
    use super::mixer::{Command, Mixer, MixerState};
    use super::recovery::{Act, RETRY_DELAYS, Recovery};
    use super::{
        AudioEvent, AudioSetup, AudioState, AudioStream, CaptureFormat, CaptureMode, OutputFormat,
        Sink, SinkId, Sound, Source, SourceId, Voice,
    };

    /// The open stream, a channel into its mixer, and one back for the
    /// sources it has removed. The audio callback only ever `try_recv`s and
    /// sends on unbounded channels, so neither side blocks the other.
    struct Output {
        /// `None` between a reopen's close and its open, and after a reopen
        /// that failed.
        stream: RefCell<Option<cpal::Stream>>,
        /// The mixer, shared by every stream opened for this output, so a
        /// reopen keeps its voices and sources.
        shared: Arc<Mutex<MixerState>>,
        /// Output callbacks so far, for the stall check.
        blocks: Arc<AtomicU64>,
        /// A loss reported by the error callback ([`LOST_DEVICE`] or
        /// [`LOST_STREAM`]), taken on the main thread.
        lost: Arc<AtomicU8>,
        commands: mpsc::Sender<Command>,
        /// Removed sources, dropped here rather than on the audio thread.
        removed: mpsc::Receiver<Source>,
        #[cfg_attr(not(feature = "audio-cpal"), allow(dead_code))]
        device: cpal::Device,
        config: cpal::StreamConfig,
        #[cfg_attr(not(feature = "audio-cpal"), allow(dead_code))]
        sample_format: SampleFormat,
    }

    /// The open input stream and its channels, laid out like [`Output`].
    /// On Android the stream is AAudio's, opened by libmorph to set the
    /// input preset; elsewhere it is cpal's.
    struct Input {
        stream: InputStream,
        /// The capture state, shared by every stream opened for this input,
        /// so a reopen keeps its sinks and frame count. On Android libmobile
        /// keeps it in its callback instead.
        #[cfg(not(target_os = "android"))]
        shared: Arc<Mutex<CaptureState>>,
        /// Captured blocks so far, for the stall check.
        blocks: Arc<AtomicU64>,
        /// As [`Output::lost`]. Never set on Android, where libmobile keeps
        /// the AAudio error to its log.
        lost: Arc<AtomicU8>,
        commands: mpsc::Sender<CaptureCommand>,
        /// Removed sinks, dropped here rather than on the audio thread.
        removed: mpsc::Receiver<Sink>,
        format: CaptureFormat,
        #[cfg(not(target_os = "android"))]
        #[cfg_attr(not(feature = "audio-cpal"), allow(dead_code))]
        device: cpal::Device,
        #[cfg(not(target_os = "android"))]
        #[cfg_attr(not(feature = "audio-cpal"), allow(dead_code))]
        config: cpal::StreamConfig,
        #[cfg(not(target_os = "android"))]
        #[cfg_attr(not(feature = "audio-cpal"), allow(dead_code))]
        sample_format: SampleFormat,
    }

    #[cfg(not(target_os = "android"))]
    type InputStream = RefCell<Option<cpal::Stream>>;
    #[cfg(target_os = "android")]
    type InputStream = super::aaudio::AaudioInput;

    /// [`Output::lost`] and [`Input::lost`]: nothing reported.
    const LOST_NONE: u8 = 0;
    /// cpal's `ErrorKind::DeviceNotAvailable`.
    const LOST_DEVICE: u8 = 1;
    /// cpal's `ErrorKind::StreamInvalidated`.
    const LOST_STREAM: u8 = 2;

    fn lost_reason(code: u8) -> String {
        match code {
            LOST_DEVICE => "the device is no longer available",
            _ => "the stream was invalidated",
        }
        .into()
    }

    /// A cpal error callback: logged as before, and a device loss or an
    /// invalidated stream set in `lost` for the main thread. Xruns and other
    /// errors are only logged.
    fn on_error(lost: Arc<AtomicU8>, what: &'static str) -> impl FnMut(cpal::Error) + Send {
        move |error| {
            log::error!("libmorph: {what}: {error}");
            let code = match error.kind() {
                cpal::ErrorKind::DeviceNotAvailable => LOST_DEVICE,
                cpal::ErrorKind::StreamInvalidated => LOST_STREAM,
                _ => LOST_NONE,
            };
            if code != LOST_NONE {
                lost.store(code, Ordering::Relaxed);
            }
        }
    }

    impl Output {
        fn play(&self) -> Result<(), String> {
            match &*self.stream.borrow() {
                Some(stream) => stream.play().map_err(|e| e.to_string()),
                None => Err("the output stream is closed".into()),
            }
        }

        fn pause(&self) -> Result<(), String> {
            match &*self.stream.borrow() {
                Some(stream) => stream.pause().map_err(|e| e.to_string()),
                None => Ok(()),
            }
        }
    }

    impl Input {
        #[cfg(not(target_os = "android"))]
        fn play(&self) -> Result<(), String> {
            match &*self.stream.borrow() {
                Some(stream) => stream.play().map_err(|e| e.to_string()),
                None => Err("the capture stream is closed".into()),
            }
        }

        #[cfg(not(target_os = "android"))]
        fn pause(&self) -> Result<(), String> {
            match &*self.stream.borrow() {
                Some(stream) => stream.pause().map_err(|e| e.to_string()),
                None => Ok(()),
            }
        }

        #[cfg(target_os = "android")]
        fn play(&self) -> Result<(), String> {
            self.stream.play()
        }

        #[cfg(target_os = "android")]
        fn pause(&self) -> Result<(), String> {
            self.stream.pause()
        }
    }

    pub struct Audio {
        output: Option<Output>,
        input: Option<Input>,
        /// Sinks added before the capture stream opened, handed to it when
        /// it does.
        pub(super) waiting: RefCell<Vec<(u64, Sink)>>,
        /// The mode a later [`open_capture`](Self::open_capture) opens with.
        #[cfg_attr(not(target_os = "android"), allow(dead_code))]
        capture_mode: CaptureMode,
        next: Cell<u64>,
        /// One per open stream: when to reopen it, and what to tell the app.
        output_recovery: RefCell<Option<Recovery>>,
        capture_recovery: RefCell<Option<Recovery>>,
        /// Between `pause` and `resume`, when `restart` does nothing.
        paused: Cell<bool>,
    }

    impl fmt::Debug for Audio {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Audio")
                .field("available", &self.is_available())
                .field("format", &self.format())
                .field("capture_format", &self.capture_format())
                .finish()
        }
    }

    impl Audio {
        /// Open what `setup` asks for. Without a device, the app still runs
        /// and the calls for it are no-ops.
        pub(crate) fn open(setup: AudioSetup) -> Self {
            let output = setup
                .output
                .then(|| {
                    start()
                        .map_err(|error| log::warn!("libmorph: audio unavailable: {error}"))
                        .ok()
                })
                .flatten();
            let input = setup
                .capture
                .then(|| {
                    start_capture(setup.capture_mode)
                        .map_err(|error| {
                            log::warn!("libmorph: audio capture unavailable: {error}");
                        })
                        .ok()
                })
                .flatten();
            let now = Instant::now();
            Self {
                output_recovery: RefCell::new(output.as_ref().map(|output| {
                    let recovery = Recovery::opened(
                        AudioStream::Output,
                        now,
                        output.blocks.load(Ordering::Relaxed),
                    );
                    // The browser runs no output callback before the page's
                    // first user gesture, which is not a loss.
                    #[cfg(target_arch = "wasm32")]
                    let recovery = recovery.without_stall();
                    recovery
                })),
                capture_recovery: RefCell::new(input.as_ref().map(|input| {
                    Recovery::opened(
                        AudioStream::Capture,
                        now,
                        input.blocks.load(Ordering::Relaxed),
                    )
                })),
                output,
                input,
                waiting: RefCell::default(),
                capture_mode: setup.capture_mode,
                next: Cell::new(1),
                paused: Cell::new(false),
            }
        }

        /// Open the capture stream after [`open`](Self::open) left it closed
        /// (Android, once `RECORD_AUDIO` is granted), and hand it the sinks
        /// added so far. True if it is open.
        #[cfg_attr(not(target_os = "android"), allow(dead_code))]
        pub(crate) fn open_capture(&mut self) -> bool {
            if self.input.is_none() {
                self.input = start_capture(self.capture_mode)
                    .map_err(|error| log::warn!("libmorph: audio capture unavailable: {error}"))
                    .ok();
            }
            let Some(input) = &self.input else {
                return false;
            };
            let recovery = self.capture_recovery.get_mut();
            if recovery.is_none() {
                *recovery = Some(Recovery::opened(
                    AudioStream::Capture,
                    Instant::now(),
                    input.blocks.load(Ordering::Relaxed),
                ));
            }
            for (id, sink) in self.waiting.get_mut().drain(..) {
                let _ = input.commands.send(CaptureCommand::AddSink(id, sink));
            }
            true
        }

        /// No streams, as on a machine without an audio device: scripted
        /// runs are silent.
        #[cfg(script_runner)]
        pub(crate) fn silent() -> Self {
            Self {
                output: None,
                input: None,
                waiting: RefCell::default(),
                capture_mode: CaptureMode::Default,
                next: Cell::new(1),
                output_recovery: RefCell::default(),
                capture_recovery: RefCell::default(),
                paused: Cell::new(false),
            }
        }

        #[must_use]
        pub fn is_available(&self) -> bool {
            self.output.is_some()
        }

        /// The output's format; `None` without an output stream.
        #[must_use]
        pub fn format(&self) -> Option<OutputFormat> {
            self.output.as_ref().map(|output| OutputFormat {
                sample_rate: output.config.sample_rate,
                channels: output.config.channels,
            })
        }

        /// The output device libmorph first opened; `None` without one. After
        /// a reopen (see [`output_state`](Self::output_state)) the stream may
        /// run on the host's new default device, but this stays the first.
        #[cfg(feature = "audio-cpal")]
        #[must_use]
        pub fn device(&self) -> Option<&impl DeviceTrait> {
            self.output.as_ref().map(|output| &output.device)
        }

        /// The config the stream was built with.
        #[cfg(feature = "audio-cpal")]
        #[must_use]
        pub fn stream_config(&self) -> Option<&cpal::StreamConfig> {
            self.output.as_ref().map(|output| &output.config)
        }

        /// The device's sample type. libmorph converts from `f32`.
        #[cfg(feature = "audio-cpal")]
        #[must_use]
        pub fn sample_format(&self) -> Option<SampleFormat> {
            self.output.as_ref().map(|output| output.sample_format)
        }

        pub fn play(&self, sound: &Sound) -> Voice {
            self.play_with(sound, 1.0, false)
        }

        pub fn play_with(&self, sound: &Sound, volume: f32, looping: bool) -> Voice {
            let voice = self.id();
            self.send(Command::Play {
                voice,
                sound: sound.clone(),
                volume,
                looping,
            });
            Voice(voice)
        }

        pub fn set_volume(&self, voice: Voice, volume: f32) {
            self.send(Command::SetVolume(voice.0, volume));
        }

        /// Cut a voice off immediately.
        pub fn stop(&self, voice: Voice) {
            self.send(Command::Stop(voice.0));
        }

        /// Stop every voice. Sources keep running.
        pub fn stop_all(&self) {
            self.send(Command::StopAll);
        }

        /// Mix `source` into the output until it is removed.
        pub fn add_source(&self, source: Source) -> SourceId {
            let id = self.id();
            self.send(Command::AddSource(id, source));
            SourceId(id)
        }

        /// The source is dropped on this thread on a later call, not on the
        /// audio thread.
        pub fn remove_source(&self, id: SourceId) {
            self.send(Command::RemoveSource(id.0));
        }

        /// Scales the whole mix (voices and sources) before it is clipped.
        /// 1.0 at start; negative values are treated as 0.
        pub fn set_master_volume(&self, volume: f32) {
            self.send(Command::SetMasterVolume(volume.max(0.0)));
        }

        /// The capture stream's format; `None` without one.
        #[must_use]
        pub fn capture_format(&self) -> Option<CaptureFormat> {
            self.input.as_ref().map(|input| input.format)
        }

        /// The mode the capture stream has: on Android, the input preset
        /// AAudio reports; elsewhere [`CaptureMode::Default`]. `None` without
        /// capture.
        #[must_use]
        pub fn capture_mode(&self) -> Option<CaptureMode> {
            #[cfg(target_os = "android")]
            return self.input.as_ref().map(|input| input.stream.mode());
            #[cfg(not(target_os = "android"))]
            return self.input.as_ref().map(|_| CaptureMode::Default);
        }

        /// On Android, reopen the capture stream with `mode`, at the same
        /// format, keeping its sinks, and return the mode granted (a mode
        /// that fails to open falls back to [`CaptureMode::Default`]).
        /// Elsewhere, and without capture, nothing changes and this returns
        /// [`capture_mode`](Self::capture_mode).
        pub fn set_capture_mode(&self, mode: CaptureMode) -> Option<CaptureMode> {
            #[cfg(target_os = "android")]
            if let Some(input) = &self.input {
                input.stream.set_mode(mode);
            }
            #[cfg(not(target_os = "android"))]
            let _ = mode;
            self.capture_mode()
        }

        /// The input device libmorph first opened; `None` without one, and on
        /// Android, where capture does not go through cpal. After a reopen
        /// the stream may run on the host's new default device, but this
        /// stays the first.
        #[cfg(feature = "audio-cpal")]
        #[must_use]
        pub fn capture_device(&self) -> Option<&impl DeviceTrait> {
            #[cfg(target_os = "android")]
            return None::<&cpal::Device>;
            #[cfg(not(target_os = "android"))]
            return self.input.as_ref().map(|input| &input.device);
        }

        /// The config the capture stream was built with; `None` on Android.
        #[cfg(feature = "audio-cpal")]
        #[must_use]
        pub fn capture_stream_config(&self) -> Option<&cpal::StreamConfig> {
            #[cfg(target_os = "android")]
            return None;
            #[cfg(not(target_os = "android"))]
            return self.input.as_ref().map(|input| &input.config);
        }

        /// The input device's sample type, which libmorph converts to `f32`;
        /// `None` on Android.
        #[cfg(feature = "audio-cpal")]
        #[must_use]
        pub fn capture_sample_format(&self) -> Option<SampleFormat> {
            #[cfg(target_os = "android")]
            return None;
            #[cfg(not(target_os = "android"))]
            return self.input.as_ref().map(|input| input.sample_format);
        }

        /// Hand every captured block to `sink` until it is removed. Before
        /// the capture stream opens, the sink is kept for it.
        pub fn add_sink(&self, sink: Sink) -> SinkId {
            let id = self.id();
            if self.input.is_some() {
                self.send_capture(CaptureCommand::AddSink(id, sink));
            } else {
                self.waiting.borrow_mut().push((id, sink));
            }
            SinkId(id)
        }

        /// The sink is dropped on this thread on a later call, not on the
        /// audio thread.
        pub fn remove_sink(&self, id: SinkId) {
            self.waiting
                .borrow_mut()
                .retain(|(waiting, _)| *waiting != id.0);
            self.send_capture(CaptureCommand::RemoveSink(id.0));
        }

        fn id(&self) -> u64 {
            let id = self.next.get();
            self.next.set(id + 1);
            id
        }

        /// Drop what the audio threads have handed back since the last call.
        fn drain(&self) {
            if let Some(output) = &self.output {
                while output.removed.try_recv().is_ok() {}
            }
            if let Some(input) = &self.input {
                while input.removed.try_recv().is_ok() {}
            }
        }

        fn send(&self, command: Command) {
            self.drain();
            if let Some(output) = &self.output {
                let _ = output.commands.send(command);
            }
        }

        fn send_capture(&self, command: CaptureCommand) {
            self.drain();
            if let Some(input) = &self.input {
                let _ = input.commands.send(command);
            }
        }

        /// Where the output stream is: [`AudioState::Closed`] without one.
        #[must_use]
        pub fn output_state(&self) -> AudioState {
            state(&self.output_recovery)
        }

        /// Where the capture stream is: [`AudioState::Closed`] without one
        /// (and on Android until `RECORD_AUDIO` is granted).
        #[must_use]
        pub fn capture_state(&self) -> AudioState {
            state(&self.capture_recovery)
        }

        /// After [`AudioState::Failed`], try again with a fresh set of
        /// attempts (for a "Try again" button). True if an attempt was
        /// started; false if the stream is not `Failed` (nothing to do), was
        /// never opened, or the app is paused (its resume tries again).
        pub fn restart(&self, stream: AudioStream) -> bool {
            if self.paused.get() {
                return false;
            }
            let now = Instant::now();
            let act = match self.recovery(stream).borrow_mut().as_mut() {
                Some(recovery) => recovery.restart(now),
                None => return false,
            };
            if act == Act::Reopen {
                self.reopen(stream, now);
                true
            } else {
                false
            }
        }

        /// Once per frame: losses reported by the error callbacks, the
        /// stall check, and attempts that are due.
        pub(crate) fn poll(&self, now: Instant) {
            for stream in [AudioStream::Output, AudioStream::Capture] {
                let lost = match stream {
                    AudioStream::Output => self.output.as_ref().map(|output| &output.lost),
                    AudioStream::Capture => self.input.as_ref().map(|input| &input.lost),
                }
                .map_or(LOST_NONE, |lost| lost.swap(LOST_NONE, Ordering::Relaxed));
                if lost != LOST_NONE {
                    let act = match self.recovery(stream).borrow_mut().as_mut() {
                        Some(recovery) => recovery.lost(now, lost_reason(lost)),
                        None => Act::Nothing,
                    };
                    if act == Act::Reopen {
                        self.reopen(stream, now);
                    }
                }
                let Some(blocks) = self.blocks(stream) else {
                    continue;
                };
                let act = match self.recovery(stream).borrow_mut().as_mut() {
                    Some(recovery) => recovery.tick(now, blocks),
                    None => continue,
                };
                if act == Act::Reopen {
                    self.reopen(stream, now);
                }
            }
        }

        /// What the recoveries have to tell the app, output's first.
        pub(crate) fn take_events(&self) -> Vec<AudioEvent> {
            let mut events = Vec::new();
            for stream in [AudioStream::Output, AudioStream::Capture] {
                if let Some(recovery) = self.recovery(stream).borrow_mut().as_mut() {
                    events.extend(recovery.take_events());
                }
            }
            events
        }

        fn recovery(&self, stream: AudioStream) -> &RefCell<Option<Recovery>> {
            match stream {
                AudioStream::Output => &self.output_recovery,
                AudioStream::Capture => &self.capture_recovery,
            }
        }

        /// The stream's callback counter; `None` if it is not open.
        fn blocks(&self, stream: AudioStream) -> Option<u64> {
            match stream {
                AudioStream::Output => self.output.as_ref().map(|output| &output.blocks),
                AudioStream::Capture => self.input.as_ref().map(|input| &input.blocks),
            }
            .map(|blocks| blocks.load(Ordering::Relaxed))
        }

        /// One attempt, as the stream's recovery asked: close the stream,
        /// open a new one with the same configuration, and report back.
        fn reopen(&self, stream: AudioStream, now: Instant) {
            let result = match stream {
                AudioStream::Output => self.reopen_output(),
                AudioStream::Capture => self.reopen_capture(),
            };
            let blocks = self.blocks(stream).unwrap_or(0);
            if let Some(recovery) = self.recovery(stream).borrow_mut().as_mut() {
                if let Ok(detail) = &result {
                    log::info!(
                        "libmorph: {} reopen {}/{}: ok{detail}",
                        recovery.name(),
                        recovery.attempts(),
                        RETRY_DELAYS.len()
                    );
                }
                recovery.reopened(now, blocks, result.map(drop));
            }
        }

        /// Drop the old stream, then build one on the host's current default
        /// output device with the first stream's config and sample format,
        /// sharing its mixer, and play it.
        fn reopen_output(&self) -> Result<String, String> {
            let output = self.output.as_ref().ok_or("no output stream")?;
            drop(output.stream.borrow_mut().take());
            output.lost.store(LOST_NONE, Ordering::Relaxed);
            let device = cpal::default_host()
                .default_output_device()
                .ok_or("no output device")?;
            let stream = open_output(
                &device,
                output.config,
                output.sample_format,
                &output.shared,
                &output.blocks,
                &output.lost,
            )?;
            stream.play().map_err(|e| e.to_string())?;
            *output.stream.borrow_mut() = Some(stream);
            Ok(String::new())
        }

        /// Android: libmobile's `set_preset` with the mode last asked for.
        #[cfg(target_os = "android")]
        fn reopen_capture(&self) -> Result<String, String> {
            let input = self.input.as_ref().ok_or("no capture stream")?;
            input.stream.reopen()
        }

        /// As [`reopen_output`](Self::reopen_output), on the default input
        /// device, sharing the capture state.
        #[cfg(not(target_os = "android"))]
        fn reopen_capture(&self) -> Result<String, String> {
            let input = self.input.as_ref().ok_or("no capture stream")?;
            drop(input.stream.borrow_mut().take());
            input.lost.store(LOST_NONE, Ordering::Relaxed);
            let device = cpal::default_host()
                .default_input_device()
                .ok_or("no input device")?;
            let stream = open_input(
                &device,
                input.config,
                input.sample_format,
                &input.shared,
                &input.blocks,
                &input.lost,
            )?;
            stream.play().map_err(|e| e.to_string())?;
            *input.stream.borrow_mut() = Some(stream);
            Ok(String::new())
        }

        pub(crate) fn pause(&self) {
            self.paused.set(true);
            if let Some(output) = &self.output
                && let Err(error) = output.pause()
            {
                log::warn!("libmorph: audio pause failed: {error}");
            }
            if let Some(input) = &self.input
                && let Err(error) = input.pause()
            {
                log::warn!("libmorph: audio capture pause failed: {error}");
            }
            for stream in [AudioStream::Output, AudioStream::Capture] {
                if let Some(recovery) = self.recovery(stream).borrow_mut().as_mut() {
                    recovery.paused();
                }
            }
        }

        /// Start both streams again. A start that fails is a loss: its
        /// recovery logs it and the first reopen follows at once.
        pub(crate) fn resume(&self) {
            self.paused.set(false);
            let now = Instant::now();
            for stream in [AudioStream::Output, AudioStream::Capture] {
                let result = match stream {
                    AudioStream::Output => self.output.as_ref().map(Output::play),
                    AudioStream::Capture => self.input.as_ref().map(Input::play),
                };
                let (Some(result), Some(blocks)) = (result, self.blocks(stream)) else {
                    continue;
                };
                let act = match self.recovery(stream).borrow_mut().as_mut() {
                    Some(recovery) => recovery.resumed(now, blocks, result),
                    None => continue,
                };
                if act == Act::Reopen {
                    self.reopen(stream, now);
                }
            }
        }
    }

    fn state(recovery: &RefCell<Option<Recovery>>) -> AudioState {
        recovery
            .borrow()
            .as_ref()
            .map_or(AudioState::Closed, Recovery::state)
    }

    #[cfg(target_os = "android")]
    fn start_capture(mode: CaptureMode) -> Result<Input, String> {
        let (commands, receiver) = mpsc::channel();
        let (returns, removed) = mpsc::channel();
        let blocks = Arc::new(AtomicU64::new(0));
        let stream = super::aaudio::AaudioInput::open(
            mode,
            Capture::new(1, returns),
            receiver,
            blocks.clone(),
        )?;
        Ok(Input {
            format: stream.format(),
            stream,
            blocks,
            lost: Arc::default(),
            commands,
            removed,
        })
    }

    #[cfg(not(target_os = "android"))]
    fn start_capture(_mode: CaptureMode) -> Result<Input, String> {
        let device = cpal::default_host()
            .default_input_device()
            .ok_or("no input device")?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let sample_format = supported.sample_format();
        let config = supported.config();
        let (commands, receiver) = mpsc::channel();
        let (returns, removed) = mpsc::channel();
        let capture = Capture::new(usize::from(config.channels), returns);
        let shared = Arc::new(Mutex::new(CaptureState::new(capture, receiver)));
        let blocks = Arc::new(AtomicU64::new(0));
        let lost = Arc::new(AtomicU8::new(LOST_NONE));
        let stream = open_input(&device, config, sample_format, &shared, &blocks, &lost)?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Input {
            stream: RefCell::new(Some(stream)),
            shared,
            blocks,
            lost,
            commands,
            removed,
            format: CaptureFormat {
                sample_rate: config.sample_rate,
                channels: config.channels,
            },
            device,
            config,
            sample_format,
        })
    }

    /// An input stream at `config` and `sample_format` feeding `shared`.
    #[cfg(not(target_os = "android"))]
    fn open_input(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        sample_format: SampleFormat,
        shared: &Arc<Mutex<CaptureState>>,
        blocks: &Arc<AtomicU64>,
        lost: &Arc<AtomicU8>,
    ) -> Result<cpal::Stream, String> {
        let (shared, blocks, lost) = (shared.clone(), blocks.clone(), lost.clone());
        match sample_format {
            SampleFormat::F32 => build_input::<f32>(device, config, shared, blocks, lost),
            SampleFormat::I16 => build_input::<i16>(device, config, shared, blocks, lost),
            SampleFormat::U16 => build_input::<u16>(device, config, shared, blocks, lost),
            SampleFormat::I32 => build_input::<i32>(device, config, shared, blocks, lost),
            other => Err(format!("unsupported sample format {other:?}")),
        }
    }

    #[cfg(not(target_os = "android"))]
    fn build_input<T>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        shared: Arc<Mutex<CaptureState>>,
        blocks: Arc<AtomicU64>,
        lost: Arc<AtomicU8>,
    ) -> Result<cpal::Stream, String>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let mut scratch = Vec::<f32>::with_capacity(4096);
        device
            .build_input_stream(
                config,
                move |data: &[T], info: &cpal::InputCallbackInfo| {
                    blocks.fetch_add(1, Ordering::Relaxed);
                    super::capture::take(&shared, &mut scratch, data, info.timestamp().capture);
                },
                on_error(lost, "audio capture stream"),
                None,
            )
            .map_err(|e| e.to_string())
    }

    fn start() -> Result<Output, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("no output device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let sample_format = supported.sample_format();
        let config = supported.config();
        let (commands, receiver) = mpsc::channel();
        let (returns, removed) = mpsc::channel();
        let mixer = Mixer::new(usize::from(config.channels), config.sample_rate, returns);
        let shared = Arc::new(Mutex::new(MixerState::new(mixer, receiver)));
        let blocks = Arc::new(AtomicU64::new(0));
        let lost = Arc::new(AtomicU8::new(LOST_NONE));
        let stream = open_output(&device, config, sample_format, &shared, &blocks, &lost)?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Output {
            stream: RefCell::new(Some(stream)),
            shared,
            blocks,
            lost,
            commands,
            removed,
            device,
            config,
            sample_format,
        })
    }

    /// An output stream at `config` and `sample_format` playing `shared`.
    fn open_output(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        sample_format: SampleFormat,
        shared: &Arc<Mutex<MixerState>>,
        blocks: &Arc<AtomicU64>,
        lost: &Arc<AtomicU8>,
    ) -> Result<cpal::Stream, String> {
        let (shared, blocks, lost) = (shared.clone(), blocks.clone(), lost.clone());
        match sample_format {
            SampleFormat::F32 => build::<f32>(device, config, shared, blocks, lost),
            SampleFormat::I16 => build::<i16>(device, config, shared, blocks, lost),
            SampleFormat::U16 => build::<u16>(device, config, shared, blocks, lost),
            SampleFormat::I32 => build::<i32>(device, config, shared, blocks, lost),
            other => Err(format!("unsupported sample format {other:?}")),
        }
    }

    fn build<T>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        shared: Arc<Mutex<MixerState>>,
        blocks: Arc<AtomicU64>,
        lost: Arc<AtomicU8>,
    ) -> Result<cpal::Stream, String>
    where
        T: SizedSample + FromSample<f32>,
    {
        let mut scratch = Vec::<f32>::with_capacity(4096);
        device
            .build_output_stream(
                config,
                move |out: &mut [T], _| {
                    blocks.fetch_add(1, Ordering::Relaxed);
                    super::mixer::fill(&shared, &mut scratch, out);
                },
                on_error(lost, "audio stream"),
                None,
            )
            .map_err(|e| e.to_string())
    }
}

#[cfg(all(test, feature = "audio"))]
mod tests {
    use crate::{Audio, AudioSetup};

    #[test]
    fn sinks_wait_for_the_capture_stream() {
        // Opens nothing, so no device is touched.
        let audio = Audio::open(AudioSetup {
            output: false,
            capture: false,
            ..AudioSetup::default()
        });
        let kept = audio.add_sink(Box::new(|_| {}));
        let removed = audio.add_sink(Box::new(|_| {}));
        audio.remove_sink(removed);
        let waiting: Vec<u64> = audio.waiting.borrow().iter().map(|(id, _)| *id).collect();
        assert_eq!(waiting, [kept.0]);
        assert_eq!(audio.capture_format(), None);
    }

    #[test]
    fn no_streams_are_closed_with_nothing_to_restart() {
        use crate::{AudioState, AudioStream};
        let audio = Audio::open(AudioSetup {
            output: false,
            capture: false,
            ..AudioSetup::default()
        });
        assert_eq!(audio.output_state(), AudioState::Closed);
        assert_eq!(audio.capture_state(), AudioState::Closed);
        assert!(!audio.restart(AudioStream::Output));
        assert!(!audio.restart(AudioStream::Capture));
        audio.pause();
        audio.resume();
        audio.poll(web_time::Instant::now());
        assert!(audio.take_events().is_empty());
    }
}

/// The Android capture stream: libmobile's `CaptureStream` (AAudio, with
/// the input preset cpal cannot set), feeding the same `Capture` state as
/// cpal does elsewhere.
#[cfg(all(feature = "audio", target_os = "android"))]
mod aaudio {
    use std::cell::Cell;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::mpsc;

    use libmobile::InputPreset;
    use libmobile::android::CaptureStream;

    use super::capture::{Capture, CaptureCommand};
    use super::cpal::StreamInstant;
    use super::{CaptureFormat, CaptureMode};

    pub(crate) struct AaudioInput {
        stream: CaptureStream,
        format: CaptureFormat,
        /// The mode the app last asked for (at open and in `set_mode`),
        /// which a reopen asks for again.
        asked: Cell<CaptureMode>,
    }

    impl AaudioInput {
        /// Open with `mode` and start. The callback owns `capture` and its
        /// commands; libmobile keeps it across `set_mode`.
        pub fn open(
            mode: CaptureMode,
            mut capture: Capture,
            commands: mpsc::Receiver<CaptureCommand>,
            blocks: Arc<AtomicU64>,
        ) -> Result<Self, String> {
            // Known once the stream is open, before it starts.
            let channels = Arc::new(AtomicUsize::new(1));
            let callback_channels = channels.clone();
            let stream = CaptureStream::open(preset(mode), move |samples, time| {
                blocks.fetch_add(1, Ordering::Relaxed);
                while let Ok(command) = commands.try_recv() {
                    capture.apply(command);
                }
                capture.set_channels(callback_channels.load(Ordering::Relaxed));
                let at =
                    StreamInstant::from_nanos(u64::try_from(time.as_nanos()).unwrap_or(u64::MAX));
                capture.deliver(samples, at);
            })
            .map_err(|e| e.to_string())?;
            let format = stream.format();
            channels.store(usize::from(format.channels), Ordering::Relaxed);
            stream.start().map_err(|e| e.to_string())?;
            log::info!(
                "libmorph: audio capture: asked {mode:?}, got {:?}",
                mode_of(stream.preset())
            );
            Ok(Self {
                format: CaptureFormat {
                    sample_rate: format.sample_rate,
                    channels: format.channels,
                },
                stream,
                asked: Cell::new(mode),
            })
        }

        pub fn format(&self) -> CaptureFormat {
            self.format
        }

        pub fn mode(&self) -> CaptureMode {
            mode_of(self.stream.preset())
        }

        pub fn set_mode(&self, mode: CaptureMode) {
            self.asked.set(mode);
            match self.stream.set_preset(preset(mode)) {
                Ok(granted) => log::info!(
                    "libmorph: audio capture: asked {mode:?}, got {:?}",
                    mode_of(granted)
                ),
                Err(error) => log::warn!("libmorph: audio capture unavailable: {error}"),
            }
        }

        /// Close the stream, whatever its state, and open a new one with the
        /// mode last asked for, at the same format, with the same callback
        /// (so the sinks and the frame count are kept). libmobile starts it,
        /// since the failed start before this marked it started; it must not
        /// be started again. Ok carries the modes for the log line.
        pub fn reopen(&self) -> Result<String, String> {
            let asked = self.asked.get();
            let granted = self
                .stream
                .set_preset(preset(asked))
                .map_err(|e| e.to_string())?;
            Ok(format!(", asked {asked:?}, got {:?}", mode_of(granted)))
        }

        pub fn pause(&self) -> Result<(), String> {
            self.stream.stop().map_err(|e| e.to_string())
        }

        pub fn play(&self) -> Result<(), String> {
            self.stream.start().map_err(|e| e.to_string())
        }
    }

    /// libmobile's preset for `mode`; `None` leaves the choice to AAudio.
    fn preset(mode: CaptureMode) -> Option<InputPreset> {
        match mode {
            CaptureMode::Default => None,
            CaptureMode::Generic => Some(InputPreset::Generic),
            CaptureMode::Camcorder => Some(InputPreset::Camcorder),
            CaptureMode::VoiceRecognition => Some(InputPreset::VoiceRecognition),
            CaptureMode::VoiceCommunication => Some(InputPreset::VoiceCommunication),
            CaptureMode::Unprocessed => Some(InputPreset::Unprocessed),
            CaptureMode::VoicePerformance => Some(InputPreset::VoicePerformance),
        }
    }

    /// The mode for a granted preset; one libmorph does not know reads as
    /// `Default`.
    fn mode_of(preset: InputPreset) -> CaptureMode {
        match preset {
            InputPreset::Generic => CaptureMode::Generic,
            InputPreset::Camcorder => CaptureMode::Camcorder,
            InputPreset::VoiceRecognition => CaptureMode::VoiceRecognition,
            InputPreset::VoiceCommunication => CaptureMode::VoiceCommunication,
            InputPreset::Unprocessed => CaptureMode::Unprocessed,
            InputPreset::VoicePerformance => CaptureMode::VoicePerformance,
            _ => CaptureMode::Default,
        }
    }
}

#[cfg(feature = "audio")]
mod capture {
    use std::sync::mpsc;
    #[cfg(not(target_os = "android"))]
    use std::sync::{Mutex, TryLockError};

    use super::cpal::StreamInstant;
    use super::{Captured, Sink};

    pub(crate) enum CaptureCommand {
        AddSink(u64, Sink),
        RemoveSink(u64),
    }

    /// The capture state and its commands, shared (desktop and web) by every
    /// input stream libmorph opens in a run, so a reopen keeps the sinks.
    #[cfg(not(target_os = "android"))]
    pub(crate) struct CaptureState {
        capture: Capture,
        commands: mpsc::Receiver<CaptureCommand>,
    }

    #[cfg(not(target_os = "android"))]
    impl CaptureState {
        pub fn new(capture: Capture, commands: mpsc::Receiver<CaptureCommand>) -> Self {
            Self { capture, commands }
        }
    }

    /// One input callback: apply the waiting commands and hand `data` to the
    /// sinks. If a reopen holds the state (the old and new streams overlap
    /// for a callback), the block is dropped.
    #[cfg(not(target_os = "android"))]
    pub(crate) fn take<T>(
        shared: &Mutex<CaptureState>,
        scratch: &mut Vec<f32>,
        data: &[T],
        at: StreamInstant,
    ) where
        T: super::cpal::SizedSample,
        f32: super::cpal::FromSample<T>,
    {
        let mut state = match shared.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        let state = &mut *state;
        while let Ok(command) = state.commands.try_recv() {
            state.capture.apply(command);
        }
        scratch.clear();
        scratch.extend(data.iter().map(|&sample| sample.to_sample::<f32>()));
        state.capture.deliver(scratch, at);
    }

    /// The input callback's state: the sinks, and where the stream is.
    pub(crate) struct Capture {
        channels: usize,
        sinks: Vec<(u64, Sink)>,
        /// Frames delivered so far.
        frame: u64,
        /// The first block's capture instant, which `time` counts from.
        first: Option<StreamInstant>,
        /// Removed sinks go back to the main thread to be dropped.
        removed: mpsc::Sender<Sink>,
    }

    impl Capture {
        pub fn new(channels: usize, removed: mpsc::Sender<Sink>) -> Self {
            Self {
                channels: channels.max(1),
                sinks: Vec::with_capacity(16),
                frame: 0,
                first: None,
                removed,
            }
        }

        /// The stream's channel count, once it is known (AAudio chooses it
        /// when the stream opens).
        #[cfg_attr(not(target_os = "android"), allow(dead_code))]
        pub fn set_channels(&mut self, channels: usize) {
            self.channels = channels.max(1);
        }

        pub fn apply(&mut self, command: CaptureCommand) {
            match command {
                CaptureCommand::AddSink(id, sink) => self.sinks.push((id, sink)),
                CaptureCommand::RemoveSink(id) => {
                    if let Some(index) = self.sinks.iter().position(|s| s.0 == id) {
                        let (_, sink) = self.sinks.remove(index);
                        // If the main thread is gone, it drops here.
                        let _ = self.removed.send(sink);
                    }
                }
            }
        }

        /// Hand one block (interleaved) to every sink.
        pub fn deliver(&mut self, samples: &[f32], at: StreamInstant) {
            let first = *self.first.get_or_insert(at);
            let captured = Captured {
                samples,
                frame: self.frame,
                time: at.duration_since(first),
            };
            for (_, sink) in &mut self.sinks {
                sink(&captured);
            }
            self.frame += (samples.len() / self.channels) as u64;
        }
    }

    #[cfg(test)]
    mod tests {
        use std::sync::mpsc;
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        use super::{Capture, CaptureCommand, StreamInstant};
        use crate::{AudioSetup, Sink};

        type Log = Arc<Mutex<Vec<(u64, Duration, Vec<f32>)>>>;

        /// A sink that records what it is handed.
        fn recorder() -> (Sink, Log) {
            let log = Log::default();
            let sink_log = log.clone();
            let sink: Sink = Box::new(move |captured| {
                sink_log.lock().unwrap().push((
                    captured.frame,
                    captured.time,
                    captured.samples.to_vec(),
                ));
            });
            (sink, log)
        }

        #[test]
        fn setup_defaults_to_output_only() {
            let setup = AudioSetup::default();
            assert!(setup.output);
            assert!(!setup.capture);
        }

        #[test]
        fn every_sink_gets_each_block_with_frames_and_time() {
            let mut capture = Capture::new(2, mpsc::channel().0);
            let (a, log_a) = recorder();
            let (b, log_b) = recorder();
            capture.apply(CaptureCommand::AddSink(1, a));
            capture.apply(CaptureCommand::AddSink(2, b));
            capture.deliver(&[0.5, -0.5, 0.25, -0.25], StreamInstant::from_millis(1000));
            capture.deliver(&[0.125, -0.125], StreamInstant::from_millis(1010));
            let expected = vec![
                (0, Duration::ZERO, vec![0.5, -0.5, 0.25, -0.25]),
                (2, Duration::from_millis(10), vec![0.125, -0.125]),
            ];
            assert_eq!(*log_a.lock().unwrap(), expected);
            assert_eq!(*log_b.lock().unwrap(), expected);
        }

        #[test]
        fn frames_count_before_a_sink_is_added() {
            let mut capture = Capture::new(1, mpsc::channel().0);
            capture.deliver(&[0.0; 3], StreamInstant::from_millis(5));
            let (sink, log) = recorder();
            capture.apply(CaptureCommand::AddSink(1, sink));
            capture.deliver(&[1.0], StreamInstant::from_millis(8));
            assert_eq!(
                *log.lock().unwrap(),
                [(3, Duration::from_millis(3), vec![1.0])]
            );
        }

        #[test]
        fn removed_sink_goes_back_undropped() {
            let (sender, removed) = mpsc::channel();
            let mut capture = Capture::new(1, sender);
            let (sink, log) = recorder();
            capture.apply(CaptureCommand::AddSink(1, sink));
            capture.apply(CaptureCommand::RemoveSink(7));
            assert!(removed.try_recv().is_err());
            capture.apply(CaptureCommand::RemoveSink(1));
            capture.deliver(&[0.5], StreamInstant::from_millis(0));
            assert!(log.lock().unwrap().is_empty());
            let mut sink = removed.try_recv().expect("returned to the main thread");
            sink(&crate::Captured {
                samples: &[0.25],
                frame: 9,
                time: Duration::ZERO,
            });
            assert_eq!(*log.lock().unwrap(), [(9, Duration::ZERO, vec![0.25])]);
        }
    }
}

#[cfg(feature = "audio")]
mod mixer {
    use std::sync::{Mutex, TryLockError, mpsc};

    use super::cpal::{FromSample, SizedSample};
    use super::{Sound, Source};

    pub(crate) enum Command {
        Play {
            voice: u64,
            sound: Sound,
            volume: f32,
            looping: bool,
        },
        SetVolume(u64, f32),
        Stop(u64),
        StopAll,
        AddSource(u64, Source),
        RemoveSource(u64),
        SetMasterVolume(f32),
    }

    struct Playing {
        voice: u64,
        sound: Sound,
        /// Position in source frames.
        position: f64,
        /// Source frames per output frame.
        step: f64,
        volume: f32,
        looping: bool,
    }

    impl Playing {
        /// Add this voice into `out`; false once it has finished.
        fn render(&mut self, out: &mut [f32], channels: usize) -> bool {
            let frames = self.sound.frames();
            if frames == 0 {
                return false;
            }
            let end = frames as f64;
            let source_channels = usize::from(self.sound.channels);
            let samples = &self.sound.samples;
            for frame in out.chunks_exact_mut(channels) {
                if self.position >= end {
                    if !self.looping {
                        return false;
                    }
                    self.position %= end;
                }
                let index = self.position as usize;
                let t = (self.position - index as f64) as f32;
                let next = match index + 1 {
                    next if next < frames => next,
                    _ if self.looping => 0,
                    _ => index,
                };
                for (channel, sample) in frame.iter_mut().enumerate() {
                    // Mono feeds every channel; surplus output channels
                    // repeat the source's last one.
                    let source = channel.min(source_channels - 1);
                    let a = samples[index * source_channels + source];
                    let b = samples[next * source_channels + source];
                    *sample += (a + (b - a) * t) * self.volume;
                }
                self.position += self.step;
            }
            self.looping || self.position < end
        }
    }

    /// The mixer and its commands, shared by every output stream libmorph
    /// opens in a run, so a reopen keeps the voices and sources.
    pub(crate) struct MixerState {
        mixer: Mixer,
        commands: mpsc::Receiver<Command>,
    }

    impl MixerState {
        pub fn new(mixer: Mixer, commands: mpsc::Receiver<Command>) -> Self {
            Self { mixer, commands }
        }
    }

    /// One output callback: apply the waiting commands and mix into `out`.
    /// If a reopen holds the state (the old and new streams overlap for a
    /// callback), `out` is silence.
    pub(crate) fn fill<T>(shared: &Mutex<MixerState>, scratch: &mut Vec<f32>, out: &mut [T])
    where
        T: SizedSample + FromSample<f32>,
    {
        let mut state = match shared.try_lock() {
            Ok(state) => state,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => {
                out.fill(T::EQUILIBRIUM);
                return;
            }
        };
        let state = &mut *state;
        while let Ok(command) = state.commands.try_recv() {
            state.mixer.apply(command);
        }
        scratch.resize(out.len(), 0.0);
        state.mixer.mix(scratch);
        for (out, sample) in out.iter_mut().zip(scratch.iter()) {
            *out = T::from_sample(*sample);
        }
    }

    pub(crate) struct Mixer {
        channels: usize,
        sample_rate: u32,
        voices: Vec<Playing>,
        sources: Vec<(u64, Source)>,
        /// One source's output, before it is added to the mix.
        scratch: Vec<f32>,
        master: f32,
        /// Removed sources go back to the main thread to be dropped.
        removed: mpsc::Sender<Source>,
    }

    impl Mixer {
        pub fn new(channels: usize, sample_rate: u32, removed: mpsc::Sender<Source>) -> Self {
            Self {
                channels: channels.max(1),
                sample_rate,
                // Headroom so starting a voice or a source rarely allocates
                // on the audio thread.
                voices: Vec::with_capacity(64),
                sources: Vec::with_capacity(16),
                scratch: Vec::with_capacity(4096),
                master: 1.0,
                removed,
            }
        }

        pub fn apply(&mut self, command: Command) {
            match command {
                Command::Play {
                    voice,
                    sound,
                    volume,
                    looping,
                } => self.voices.push(Playing {
                    voice,
                    step: f64::from(sound.sample_rate) / f64::from(self.sample_rate),
                    sound,
                    position: 0.0,
                    volume,
                    looping,
                }),
                Command::SetVolume(voice, volume) => {
                    for playing in self.voices.iter_mut().filter(|p| p.voice == voice) {
                        playing.volume = volume;
                    }
                }
                Command::Stop(voice) => self.voices.retain(|p| p.voice != voice),
                Command::StopAll => self.voices.clear(),
                Command::AddSource(id, source) => self.sources.push((id, source)),
                Command::RemoveSource(id) => {
                    if let Some(index) = self.sources.iter().position(|s| s.0 == id) {
                        let (_, source) = self.sources.remove(index);
                        // If the main thread is gone, it drops here.
                        let _ = self.removed.send(source);
                    }
                }
                Command::SetMasterVolume(volume) => self.master = volume,
            }
        }

        /// Overwrite `out` (interleaved) with the mix of every voice and
        /// source.
        pub fn mix(&mut self, out: &mut [f32]) {
            out.fill(0.0);
            let channels = self.channels;
            self.voices.retain_mut(|voice| voice.render(out, channels));
            for (_, source) in &mut self.sources {
                // Each source gets its own zeroed buffer, so one that writes
                // instead of adding cannot overwrite the rest of the mix.
                self.scratch.clear();
                self.scratch.resize(out.len(), 0.0);
                source(&mut self.scratch);
                for (out, sample) in out.iter_mut().zip(&self.scratch) {
                    *out += sample;
                }
            }
            for sample in out {
                *sample = (*sample * self.master).clamp(-1.0, 1.0);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::sync::mpsc;

        use super::{Command, Mixer};
        use crate::{Sound, Source};

        fn mixer(channels: usize, sample_rate: u32) -> Mixer {
            Mixer::new(channels, sample_rate, mpsc::channel().0)
        }

        /// A source that writes `value` into every sample.
        fn constant(value: f32) -> Source {
            Box::new(move |out: &mut [f32]| out.fill(value))
        }

        fn play(mixer: &mut Mixer, voice: u64, sound: &Sound, looping: bool) {
            mixer.apply(Command::Play {
                voice,
                sound: sound.clone(),
                volume: 1.0,
                looping,
            });
        }

        #[test]
        fn mono_feeds_both_channels_then_ends() {
            let mut mixer = mixer(2, 4);
            play(&mut mixer, 1, &Sound::new(vec![0.25, 0.5], 1, 4), false);
            let mut out = [9.0; 6];
            mixer.mix(&mut out);
            assert_eq!(out, [0.25, 0.25, 0.5, 0.5, 0.0, 0.0]);
            assert!(mixer.voices.is_empty());
        }

        #[test]
        fn voices_sum_and_clip() {
            let mut mixer = mixer(1, 1);
            let loud = Sound::new(vec![0.75], 1, 1);
            play(&mut mixer, 1, &loud, true);
            play(&mut mixer, 2, &loud, true);
            let mut out = [0.0; 2];
            mixer.mix(&mut out);
            assert_eq!(out, [1.0, 1.0]);
        }

        #[test]
        fn stop_interrupts_one_voice() {
            let mut mixer = mixer(1, 1);
            play(&mut mixer, 1, &Sound::new(vec![0.25], 1, 1), true);
            play(&mut mixer, 2, &Sound::new(vec![0.5], 1, 1), true);
            mixer.apply(Command::Stop(2));
            let mut out = [0.0; 1];
            mixer.mix(&mut out);
            assert_eq!(out, [0.25]);
            mixer.apply(Command::StopAll);
            mixer.mix(&mut out);
            assert_eq!(out, [0.0]);
        }

        #[test]
        fn resamples_by_interpolation() {
            // A 1 Hz source at 2 Hz output: every other sample is a midpoint.
            let mut mixer = mixer(1, 2);
            play(&mut mixer, 1, &Sound::new(vec![0.0, 1.0], 1, 1), false);
            let mut out = [0.0; 4];
            mixer.mix(&mut out);
            assert_eq!(out, [0.0, 0.5, 1.0, 1.0]);
            assert!(mixer.voices.is_empty());
        }

        #[test]
        fn volume_scales() {
            let mut mixer = mixer(1, 1);
            play(&mut mixer, 7, &Sound::new(vec![0.5], 1, 1), true);
            mixer.apply(Command::SetVolume(7, 0.5));
            let mut out = [0.0; 1];
            mixer.mix(&mut out);
            assert_eq!(out, [0.25]);
        }

        #[test]
        fn sources_add_to_voices_and_each_other() {
            let mut mixer = mixer(2, 1);
            play(&mut mixer, 1, &Sound::new(vec![0.25], 1, 1), true);
            // Both write rather than add; neither overwrites the mix.
            mixer.apply(Command::AddSource(2, constant(0.125)));
            mixer.apply(Command::AddSource(3, constant(0.5)));
            let mut out = [0.0; 4];
            mixer.mix(&mut out);
            assert_eq!(out, [0.875; 4]);
            mixer.apply(Command::AddSource(4, constant(0.5)));
            mixer.mix(&mut out);
            assert_eq!(out, [1.0; 4]);
        }

        #[test]
        fn removed_source_goes_back_undropped() {
            let (sender, removed) = mpsc::channel();
            let mut mixer = Mixer::new(1, 1, sender);
            mixer.apply(Command::AddSource(1, constant(0.5)));
            mixer.apply(Command::RemoveSource(7));
            assert!(removed.try_recv().is_err());
            mixer.apply(Command::RemoveSource(1));
            let mut out = [9.0; 2];
            mixer.mix(&mut out);
            assert_eq!(out, [0.0; 2]);
            let mut source = removed.try_recv().expect("returned to the main thread");
            source(&mut out);
            assert_eq!(out, [0.5; 2]);
        }

        #[test]
        fn stop_all_keeps_sources() {
            let mut mixer = mixer(1, 1);
            play(&mut mixer, 1, &Sound::new(vec![0.25], 1, 1), true);
            mixer.apply(Command::AddSource(2, constant(0.5)));
            mixer.apply(Command::StopAll);
            let mut out = [0.0; 1];
            mixer.mix(&mut out);
            assert_eq!(out, [0.5]);
        }

        #[test]
        fn a_voice_plays_on_across_two_streams() {
            use std::sync::{Arc, Mutex};

            use super::{MixerState, fill};

            let (commands, receiver) = mpsc::channel();
            let shared = Arc::new(Mutex::new(MixerState::new(mixer(1, 1), receiver)));
            // One output callback, as `build` makes it for each stream.
            let stream = |shared: Arc<Mutex<MixerState>>| {
                let mut scratch = Vec::new();
                move |out: &mut [f32]| fill(&shared, &mut scratch, out)
            };
            commands
                .send(Command::Play {
                    voice: 1,
                    sound: Sound::new(vec![0.125, 0.25, 0.375, 0.5, 0.625], 1, 1),
                    volume: 1.0,
                    looping: false,
                })
                .unwrap();
            let mut first = stream(shared.clone());
            let mut out = [0.0; 2];
            first(&mut out);
            assert_eq!(out, [0.125, 0.25]);
            // The reopen: the first stream goes, a second plays on.
            drop(first);
            let mut second = stream(shared.clone());
            second(&mut out);
            assert_eq!(out, [0.375, 0.5]);
            // While the state is held (streams overlapping), silence.
            let held = shared.lock().unwrap();
            second(&mut out);
            assert_eq!(out, [0.0; 2]);
            drop(held);
            second(&mut out);
            assert_eq!(out, [0.625, 0.0]);
        }

        #[test]
        fn master_volume_scales_before_clipping() {
            let mut mixer = mixer(1, 1);
            play(&mut mixer, 1, &Sound::new(vec![0.75], 1, 1), true);
            mixer.apply(Command::AddSource(2, constant(0.75)));
            mixer.apply(Command::SetMasterVolume(0.5));
            let mut out = [0.0; 1];
            mixer.mix(&mut out);
            assert_eq!(out, [0.75]);
        }
    }
}
