//! In-memory sample playback. With the `audio` feature, sounds are mixed
//! into a `cpal` output stream; without it [`Audio`] is a zero-sized no-op,
//! so code calling it compiles either way.

use std::sync::Arc;

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

#[cfg(feature = "audio")]
pub use output::Audio;

#[cfg(not(feature = "audio"))]
pub use stub::Audio;

#[cfg(not(feature = "audio"))]
mod stub {
    use super::{Sound, Voice};

    /// The `audio` feature is off: every call is a no-op.
    #[derive(Debug, Default)]
    pub struct Audio;

    impl Audio {
        pub(crate) fn open() -> Self {
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

        #[cfg_attr(not(target_os = "android"), allow(dead_code))]
        pub(crate) fn pause(&self) {}

        #[cfg_attr(not(target_os = "android"), allow(dead_code))]
        pub(crate) fn resume(&self) {}
    }
}

#[cfg(feature = "audio")]
mod output {
    use std::cell::Cell;
    use std::fmt;
    use std::sync::mpsc;

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample};

    use super::mixer::{Command, Mixer};
    use super::{Sound, Voice};

    /// The output stream and a channel into its mixer. The audio callback
    /// only ever `try_recv`s, so the main thread never blocks it.
    pub struct Audio {
        output: Option<(cpal::Stream, mpsc::Sender<Command>)>,
        next: Cell<u64>,
    }

    impl fmt::Debug for Audio {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("Audio")
                .field("available", &self.is_available())
                .finish()
        }
    }

    impl Audio {
        /// Open the default output. Without one, the app still runs and
        /// every call is a no-op.
        pub(crate) fn open() -> Self {
            let output = start()
                .map_err(|error| log::warn!("libmorph: audio unavailable: {error}"))
                .ok();
            Self {
                output,
                next: Cell::new(1),
            }
        }

        #[must_use]
        pub fn is_available(&self) -> bool {
            self.output.is_some()
        }

        pub fn play(&self, sound: &Sound) -> Voice {
            self.play_with(sound, 1.0, false)
        }

        pub fn play_with(&self, sound: &Sound, volume: f32, looping: bool) -> Voice {
            let voice = self.next.get();
            self.next.set(voice + 1);
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

        pub fn stop_all(&self) {
            self.send(Command::StopAll);
        }

        fn send(&self, command: Command) {
            if let Some((_, commands)) = &self.output {
                let _ = commands.send(command);
            }
        }

        #[cfg_attr(not(target_os = "android"), allow(dead_code))]
        pub(crate) fn pause(&self) {
            if let Some((stream, _)) = &self.output
                && let Err(error) = stream.pause()
            {
                log::warn!("libmorph: audio pause failed: {error}");
            }
        }

        #[cfg_attr(not(target_os = "android"), allow(dead_code))]
        pub(crate) fn resume(&self) {
            if let Some((stream, _)) = &self.output
                && let Err(error) = stream.play()
            {
                log::warn!("libmorph: audio resume failed: {error}");
            }
        }
    }

    fn start() -> Result<(cpal::Stream, mpsc::Sender<Command>), String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("no output device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let config = supported.config();
        let (commands, receiver) = mpsc::channel();
        let mixer = Mixer::new(usize::from(config.channels), config.sample_rate);
        let stream = match supported.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, config, mixer, receiver),
            SampleFormat::I16 => build::<i16>(&device, config, mixer, receiver),
            SampleFormat::U16 => build::<u16>(&device, config, mixer, receiver),
            SampleFormat::I32 => build::<i32>(&device, config, mixer, receiver),
            other => return Err(format!("unsupported sample format {other:?}")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        Ok((stream, commands))
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
mod mixer {
    use super::Sound;

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
    }

    impl Mixer {
        pub fn new(channels: usize, sample_rate: u32) -> Self {
            Self {
                channels: channels.max(1),
                sample_rate,
                // Headroom so starting a voice rarely allocates on the
                // audio thread.
                voices: Vec::with_capacity(64),
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
            }
        }

        /// Overwrite `out` (interleaved) with the mix of every voice.
        pub fn mix(&mut self, out: &mut [f32]) {
            out.fill(0.0);
            let channels = self.channels;
            self.voices.retain_mut(|voice| voice.render(out, channels));
            for sample in out {
                *sample = sample.clamp(-1.0, 1.0);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{Command, Mixer};
        use crate::Sound;

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
            let mut mixer = Mixer::new(2, 4);
            play(&mut mixer, 1, &Sound::new(vec![0.25, 0.5], 1, 4), false);
            let mut out = [9.0; 6];
            mixer.mix(&mut out);
            assert_eq!(out, [0.25, 0.25, 0.5, 0.5, 0.0, 0.0]);
            assert!(mixer.voices.is_empty());
        }

        #[test]
        fn voices_sum_and_clip() {
            let mut mixer = Mixer::new(1, 1);
            let loud = Sound::new(vec![0.75], 1, 1);
            play(&mut mixer, 1, &loud, true);
            play(&mut mixer, 2, &loud, true);
            let mut out = [0.0; 2];
            mixer.mix(&mut out);
            assert_eq!(out, [1.0, 1.0]);
        }

        #[test]
        fn stop_interrupts_one_voice() {
            let mut mixer = Mixer::new(1, 1);
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
            let mut mixer = Mixer::new(1, 2);
            play(&mut mixer, 1, &Sound::new(vec![0.0, 1.0], 1, 1), false);
            let mut out = [0.0; 4];
            mixer.mix(&mut out);
            assert_eq!(out, [0.0, 0.5, 1.0, 1.0]);
            assert!(mixer.voices.is_empty());
        }

        #[test]
        fn volume_scales() {
            let mut mixer = Mixer::new(1, 1);
            play(&mut mixer, 7, &Sound::new(vec![0.5], 1, 1), true);
            mixer.apply(Command::SetVolume(7, 0.5));
            let mut out = [0.0; 1];
            mixer.mix(&mut out);
            assert_eq!(out, [0.25]);
        }
    }
}
