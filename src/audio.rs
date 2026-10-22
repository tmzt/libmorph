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
}

impl Default for AudioSetup {
    fn default() -> Self {
        Self {
            output: true,
            capture: false,
        }
    }
}

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
        AudioSetup, CaptureFormat, OutputFormat, Sink, SinkId, Sound, Source, SourceId, Voice,
    };

    /// The `audio` feature is off: every call is a no-op.
    #[derive(Debug, Default)]
    pub struct Audio;

    impl Audio {
        pub(crate) fn open(_setup: AudioSetup) -> Self {
            Self
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

        /// Drops the sink: there is no capture to feed it.
        pub fn add_sink(&self, _sink: Sink) -> SinkId {
            SinkId(0)
        }

        pub fn remove_sink(&self, _id: SinkId) {}

        pub(crate) fn pause(&self) {}

        pub(crate) fn resume(&self) {}
    }
}

#[cfg(feature = "audio")]
mod output {
    use std::cell::Cell;
    use std::fmt;
    use std::sync::mpsc;

    use super::cpal;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample};

    use super::capture::{Capture, CaptureCommand};
    use super::mixer::{Command, Mixer};
    use super::{
        AudioSetup, CaptureFormat, OutputFormat, Sink, SinkId, Sound, Source, SourceId, Voice,
    };

    /// The open stream, a channel into its mixer, and one back for the
    /// sources it has removed. The audio callback only ever `try_recv`s and
    /// sends on unbounded channels, so neither side blocks the other.
    struct Output {
        stream: cpal::Stream,
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
    struct Input {
        stream: cpal::Stream,
        commands: mpsc::Sender<CaptureCommand>,
        /// Removed sinks, dropped here rather than on the audio thread.
        removed: mpsc::Receiver<Sink>,
        #[cfg_attr(not(feature = "audio-cpal"), allow(dead_code))]
        device: cpal::Device,
        config: cpal::StreamConfig,
        #[cfg_attr(not(feature = "audio-cpal"), allow(dead_code))]
        sample_format: SampleFormat,
    }

    pub struct Audio {
        output: Option<Output>,
        input: Option<Input>,
        next: Cell<u64>,
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
                    start_capture()
                        .map_err(|error| {
                            log::warn!("libmorph: audio capture unavailable: {error}");
                        })
                        .ok()
                })
                .flatten();
            Self {
                output,
                input,
                next: Cell::new(1),
            }
        }

        /// No streams, as on a machine without an audio device: scripted
        /// runs are silent.
        #[cfg(script_runner)]
        pub(crate) fn silent() -> Self {
            Self {
                output: None,
                input: None,
                next: Cell::new(1),
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

        /// The output device libmorph opened; `None` without one.
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
            self.input.as_ref().map(|input| CaptureFormat {
                sample_rate: input.config.sample_rate,
                channels: input.config.channels,
            })
        }

        /// The input device libmorph opened; `None` without one.
        #[cfg(feature = "audio-cpal")]
        #[must_use]
        pub fn capture_device(&self) -> Option<&impl DeviceTrait> {
            self.input.as_ref().map(|input| &input.device)
        }

        /// The config the capture stream was built with.
        #[cfg(feature = "audio-cpal")]
        #[must_use]
        pub fn capture_stream_config(&self) -> Option<&cpal::StreamConfig> {
            self.input.as_ref().map(|input| &input.config)
        }

        /// The input device's sample type. libmorph converts to `f32`.
        #[cfg(feature = "audio-cpal")]
        #[must_use]
        pub fn capture_sample_format(&self) -> Option<SampleFormat> {
            self.input.as_ref().map(|input| input.sample_format)
        }

        /// Hand every captured block to `sink` until it is removed.
        pub fn add_sink(&self, sink: Sink) -> SinkId {
            let id = self.id();
            self.send_capture(CaptureCommand::AddSink(id, sink));
            SinkId(id)
        }

        /// The sink is dropped on this thread on a later call, not on the
        /// audio thread.
        pub fn remove_sink(&self, id: SinkId) {
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

        pub(crate) fn pause(&self) {
            if let Some(output) = &self.output
                && let Err(error) = output.stream.pause()
            {
                log::warn!("libmorph: audio pause failed: {error}");
            }
            if let Some(input) = &self.input
                && let Err(error) = input.stream.pause()
            {
                log::warn!("libmorph: audio capture pause failed: {error}");
            }
        }

        pub(crate) fn resume(&self) {
            if let Some(output) = &self.output
                && let Err(error) = output.stream.play()
            {
                log::warn!("libmorph: audio resume failed: {error}");
            }
            if let Some(input) = &self.input
                && let Err(error) = input.stream.play()
            {
                log::warn!("libmorph: audio capture resume failed: {error}");
            }
        }
    }

    fn start_capture() -> Result<Input, String> {
        let device = cpal::default_host()
            .default_input_device()
            .ok_or("no input device")?;
        let supported = device.default_input_config().map_err(|e| e.to_string())?;
        let sample_format = supported.sample_format();
        let config = supported.config();
        let (commands, receiver) = mpsc::channel();
        let (returns, removed) = mpsc::channel();
        let capture = Capture::new(usize::from(config.channels), returns);
        let stream = match sample_format {
            SampleFormat::F32 => build_input::<f32>(&device, config, capture, receiver),
            SampleFormat::I16 => build_input::<i16>(&device, config, capture, receiver),
            SampleFormat::U16 => build_input::<u16>(&device, config, capture, receiver),
            SampleFormat::I32 => build_input::<i32>(&device, config, capture, receiver),
            other => return Err(format!("unsupported sample format {other:?}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Input {
            stream,
            commands,
            removed,
            device,
            config,
            sample_format,
        })
    }

    fn build_input<T>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        mut capture: Capture,
        commands: mpsc::Receiver<CaptureCommand>,
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
                    while let Ok(command) = commands.try_recv() {
                        capture.apply(command);
                    }
                    scratch.clear();
                    scratch.extend(data.iter().map(|&sample| sample.to_sample::<f32>()));
                    capture.deliver(&scratch, info.timestamp().capture);
                },
                |error| log::error!("libmorph: audio capture stream: {error}"),
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
        let stream = match sample_format {
            SampleFormat::F32 => build::<f32>(&device, config, mixer, receiver),
            SampleFormat::I16 => build::<i16>(&device, config, mixer, receiver),
            SampleFormat::U16 => build::<u16>(&device, config, mixer, receiver),
            SampleFormat::I32 => build::<i32>(&device, config, mixer, receiver),
            other => return Err(format!("unsupported sample format {other:?}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Output {
            stream,
            commands,
            removed,
            device,
            config,
            sample_format,
        })
    }

    fn build<T>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        mut mixer: Mixer,
        commands: mpsc::Receiver<Command>,
    ) -> Result<cpal::Stream, String>
    where
        T: SizedSample + FromSample<f32>,
    {
        let mut scratch = Vec::<f32>::with_capacity(4096);
        device
            .build_output_stream(
                config,
                move |out: &mut [T], _| {
                    while let Ok(command) = commands.try_recv() {
                        mixer.apply(command);
                    }
                    scratch.resize(out.len(), 0.0);
                    mixer.mix(&mut scratch);
                    for (out, sample) in out.iter_mut().zip(&scratch) {
                        *out = T::from_sample(*sample);
                    }
                },
                |error| log::error!("libmorph: audio stream: {error}"),
                None,
            )
            .map_err(|e| e.to_string())
    }
}

#[cfg(feature = "audio")]
mod capture {
    use std::sync::mpsc;

    use super::cpal::StreamInstant;
    use super::{Captured, Sink};

    pub(crate) enum CaptureCommand {
        AddSink(u64, Sink),
        RemoveSink(u64),
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
    use std::sync::mpsc;

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
