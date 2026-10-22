//! Records the default microphone (`AudioSetup::capture`) into Sun `.au`
//! files of 16-bit PCM, one per chunk, under `out/audiocapture/` (on
//! Android, `audiocapture/` in the app's data directory; see
//! `examples/android/`). Each file
//! is named by the Unix time in milliseconds of its first frame, and its
//! name is logged once it is written. The first argument is the chunk
//! length in milliseconds (default 1000). The screen brightens with the
//! last chunk's peak level, and is dark red without a microphone.
//!
//! ```text
//! cargo run --example audiocapture -- 500
//! afplay out/audiocapture/<unix ms>.au
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, io, mem, thread};

use libmorph::{AudioSetup, CaptureFormat, Captured, Frame, MorphApp, MorphContext, wgpu};

/// Relative to where the example is run, like the scripted runs' `out/`,
/// when there is no data directory.
const DIR: &str = "out/audiocapture";

/// An `f32` shared with the writer thread without locking.
#[derive(Clone, Default)]
struct Shared(Arc<AtomicU32>);

impl Shared {
    fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// A full chunk on its way from the audio thread to the writer.
struct Chunk {
    /// Unix time of the first frame.
    unix_ms: u128,
    samples: Vec<f32>,
}

pub struct AudioCapture {
    /// The last chunk's peak level; `None` without a microphone.
    peak: Option<Shared>,
}

impl MorphApp for AudioCapture {
    fn audio_setup(setup: &mut AudioSetup) {
        setup.capture = true;
        setup.output = false;
    }

    fn init(ctx: &mut MorphContext) -> Self {
        let Some(format) = ctx.audio().capture_format() else {
            log::warn!("no audio capture");
            return Self { peak: None };
        };
        let chunk_ms = ctx
            .args()
            .get(1)
            .and_then(|arg| arg.parse::<u64>().ok())
            .filter(|&ms| ms > 0)
            .unwrap_or(1000);
        let dir = ctx
            .data_dir()
            .map_or_else(|| PathBuf::from(DIR), |data| data.join("audiocapture"));
        log::info!(
            "capture: {format:?}, {chunk_ms} ms per file in {}",
            dir.display()
        );
        let peak = Shared::default();
        match start(ctx, &dir, format, chunk_ms, peak.clone()) {
            Ok(()) => Self { peak: Some(peak) },
            Err(error) => {
                log::error!("{}: {error}", dir.display());
                Self { peak: None }
            }
        }
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        let color = match &self.peak {
            Some(peak) => {
                let level = f64::from(peak.get()).sqrt();
                wgpu::Color {
                    r: 0.05 + 0.3 * level,
                    g: 0.05 + 0.6 * level,
                    b: 0.1 + 0.4 * level,
                    a: 1.0,
                }
            }
            None => wgpu::Color {
                r: 0.3,
                g: 0.02,
                b: 0.02,
                a: 1.0,
            },
        };
        let mut encoder = ctx.device().create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        ctx.queue().submit([encoder.finish()]);
    }
}

/// Add a sink that cuts the capture into chunks, and a thread that writes
/// them. The audio thread only copies samples and swaps buffers.
fn start(
    ctx: &MorphContext,
    dir: &Path,
    format: CaptureFormat,
    chunk_ms: u64,
    peak: Shared,
) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let dir = dir.to_path_buf();
    let channels = usize::from(format.channels);
    let frames = (u64::from(format.sample_rate) * chunk_ms / 1000).max(1) as usize;
    let len = frames * channels;
    let (full, chunks) = mpsc::channel::<Chunk>();
    let (empty, buffers) = mpsc::channel::<Vec<f32>>();

    thread::spawn(move || {
        // Ends when the sink, and with it `full`, is dropped at exit.
        for mut chunk in chunks {
            let path = dir.join(format!("{}.au", chunk.unix_ms));
            match fs::write(&path, au(&chunk.samples, format)) {
                Ok(()) => log::info!(
                    "wrote {} ({} frames, {} ch, {} Hz)",
                    path.display(),
                    chunk.samples.len() / channels,
                    format.channels,
                    format.sample_rate,
                ),
                Err(error) => log::error!("{}: {error}", path.display()),
            }
            peak.set(chunk.samples.iter().fold(0.0, |peak, s| s.abs().max(peak)));
            chunk.samples.clear();
            let _ = empty.send(chunk.samples);
        }
    });

    let rate = f64::from(format.sample_rate);
    let mut buffer = Vec::with_capacity(len);
    // Unix time of the stream's first block, and of the chunk being filled.
    let mut origin_ms = None;
    let mut chunk_unix_ms = 0;
    ctx.audio()
        .add_sink(Box::new(move |captured: &Captured<'_>| {
            let origin = *origin_ms.get_or_insert_with(|| unix_ms() - captured.time.as_millis());
            let mut samples = captured.samples;
            let mut time = captured.time;
            while !samples.is_empty() {
                if buffer.is_empty() {
                    chunk_unix_ms = origin + time.as_millis();
                }
                let take = (len - buffer.len()).min(samples.len());
                buffer.extend_from_slice(&samples[..take]);
                samples = &samples[take..];
                time += Duration::from_secs_f64((take / channels) as f64 / rate);
                if buffer.len() == len {
                    // Allocates only until the writer has sent a buffer back.
                    let next = buffers
                        .try_recv()
                        .unwrap_or_else(|_| Vec::with_capacity(len));
                    let samples = mem::replace(&mut buffer, next);
                    let _ = full.send(Chunk {
                        unix_ms: chunk_unix_ms,
                        samples,
                    });
                }
            }
        }));
    Ok(())
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// A Sun `.au` file: the 24-byte big-endian header, then 16-bit linear PCM
/// (encoding 3), big-endian, clamped to -1..1.
fn au(samples: &[f32], format: CaptureFormat) -> Vec<u8> {
    const HEADER: u32 = 24;
    let data = u32::try_from(samples.len() * 2).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(HEADER as usize + samples.len() * 2);
    for word in [
        0x2e73_6e64, // ".snd"
        HEADER,
        data,
        3,
        format.sample_rate,
        u32::from(format.channels),
    ] {
        bytes.extend_from_slice(&word.to_be_bytes());
    }
    for sample in samples {
        let pcm = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
        bytes.extend_from_slice(&pcm.to_be_bytes());
    }
    bytes
}

#[cfg(not(target_os = "android"))]
fn main() -> Result<(), libmorph::MorphError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    libmorph::run::<AudioCapture>()
}

#[cfg(test)]
mod tests {
    use super::{CaptureFormat, au};

    #[test]
    fn au_header_and_big_endian_pcm() {
        let format = CaptureFormat {
            sample_rate: 48_000,
            channels: 2,
        };
        let bytes = au(&[0.0, 1.0, -1.0, 2.0], format);
        assert_eq!(&bytes[..4], b".snd");
        let word = |i: usize| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap());
        assert_eq!(
            [word(4), word(8), word(12), word(16), word(20)],
            [24, 8, 3, 48_000, 2]
        );
        assert_eq!(
            &bytes[24..],
            &[0x00, 0x00, 0x7f, 0xff, 0x80, 0x01, 0x7f, 0xff]
        );
    }
}
