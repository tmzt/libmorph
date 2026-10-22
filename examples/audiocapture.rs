//! Records the default microphone (`AudioSetup::capture`) into Sun `.au`
//! files of 16-bit PCM, one per chunk, under `out/audiocapture/` (on
//! Android, `audiocapture/` in the app's data directory; see
//! `examples/android/`). Each file
//! is named by the Unix time in milliseconds of its first frame, and its
//! name is logged once it is written. The first argument is the chunk
//! length in milliseconds (default 1000).
//!
//! The screen is a scrolling spectrogram of the capture, newest at the
//! right and low frequencies at the bottom (log-spaced from 40 Hz), and is
//! dark red until capture is ready (on Android, until the user allows the
//! microphone). Silence, such as Android gives an app behind Recents, is
//! black.
//!
//! ```text
//! cargo run --example audiocapture -- 500
//! afplay out/audiocapture/<unix ms>.au
//! ```

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, io, mem, thread};

use libmorph::{AudioSetup, CaptureFormat, Captured, Frame, MorphApp, MorphContext, wgpu};

/// Relative to where the example is run, like the scripted runs' `out/`,
/// when there is no data directory.
const DIR: &str = "out/audiocapture";

/// FFT length, and the frames between columns.
const FFT: usize = 1024;
const HOP: usize = 512;
/// The spectrogram texture: one column per FFT, one row per frequency band.
const COLUMNS: usize = 512;
const ROWS: usize = 256;
/// The lowest band's lower edge; the highest ends at the Nyquist frequency.
const LOW_HZ: f32 = 40.0;
/// dBFS mapped to black and to white.
const FLOOR_DB: f32 = -100.0;
const CEIL_DB: f32 = -20.0;

/// The spectrogram ring, shared by the analysis sink and the renderer.
/// Row-major `COLUMNS` x `ROWS`, row 0 the lowest band; column
/// `written % COLUMNS` is the next to be written.
struct Columns {
    texels: Vec<u8>,
    written: u64,
}

/// A full chunk on its way from the audio thread to the writer.
struct Chunk {
    /// Unix time of the first frame.
    unix_ms: u128,
    samples: Vec<f32>,
}

pub struct AudioCapture {
    chunk_ms: u64,
    /// `None` until capture is ready.
    columns: Option<Arc<Mutex<Columns>>>,
    view: Spectrogram,
}

impl MorphApp for AudioCapture {
    fn audio_setup(setup: &mut AudioSetup) {
        setup.capture = true;
        setup.output = false;
    }

    fn init(ctx: &mut MorphContext) -> Self {
        let chunk_ms = ctx
            .args()
            .get(1)
            .and_then(|arg| arg.parse::<u64>().ok())
            .filter(|&ms| ms > 0)
            .unwrap_or(1000);
        Self {
            chunk_ms,
            columns: None,
            view: Spectrogram::new(ctx),
        }
    }

    /// Right after `init` on the desktop; on Android, once the user allows
    /// the microphone. Until then the screen stays dark red.
    fn on_capture_ready(&mut self, ctx: &mut MorphContext) {
        let Some(format) = ctx.audio().capture_format() else {
            return;
        };
        let dir = ctx
            .data_dir()
            .map_or_else(|| PathBuf::from(DIR), |data| data.join("audiocapture"));
        log::info!(
            "capture: {format:?}, {} ms per file in {}",
            self.chunk_ms,
            dir.display()
        );
        if let Err(error) = start(ctx, &dir, format, self.chunk_ms) {
            log::error!("{}: {error}", dir.display());
        }
        let columns = Arc::new(Mutex::new(Columns {
            texels: vec![0; COLUMNS * ROWS],
            written: 0,
        }));
        let mut analyzer = Analyzer::new(format, columns.clone());
        ctx.audio()
            .add_sink(Box::new(move |captured: &Captured<'_>| {
                analyzer.push(captured.samples);
            }));
        self.columns = Some(columns);
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        match &self.columns {
            Some(columns) => {
                self.view.upload(ctx, columns);
                self.view.draw(ctx, frame);
            }
            None => clear(ctx, frame),
        }
    }
}

/// The dark red screen before capture is ready.
fn clear(ctx: &MorphContext, frame: &Frame<'_>) {
    let mut encoder = ctx.device().create_command_encoder(&Default::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: frame.view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.3,
                    g: 0.02,
                    b: 0.02,
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    ctx.queue().submit([encoder.finish()]);
}

/// The spectrogram on the GPU: the ring as an `R8Unorm` texture, drawn full
/// screen by `audiocapture.wgsl`.
struct Spectrogram {
    pipeline: wgpu::RenderPipeline,
    texture: wgpu::Texture,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    format: wgpu::TextureFormat,
    /// `Columns::written` at the last upload.
    uploaded: u64,
}

impl Spectrogram {
    fn new(ctx: &MorphContext) -> Self {
        let device = ctx.device();
        let format = ctx.surface_format();
        let shader = device.create_shader_module(wgpu::include_wgsl!("audiocapture.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("spectrogram"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spectrogram"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("spectrogram"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: ctx.pipeline_cache(),
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("spectrogram"),
            size: wgpu::Extent3d {
                width: COLUMNS as u32,
                height: ROWS as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("spectrogram"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("spectrogram"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniforms.as_entire_binding(),
                },
            ],
        });
        Self {
            pipeline,
            texture,
            uniforms,
            bind_group,
            format,
            uploaded: u64::MAX,
        }
    }

    /// Copy the ring to the texture if the analysis has added columns.
    fn upload(&mut self, ctx: &MorphContext, columns: &Mutex<Columns>) {
        // The audio thread only `try_lock`s, so this never makes it wait.
        let columns = columns.lock().unwrap_or_else(|e| e.into_inner());
        if columns.written == self.uploaded {
            return;
        }
        self.uploaded = columns.written;
        ctx.queue().write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &columns.texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(COLUMNS as u32),
                rows_per_image: Some(ROWS as u32),
            },
            self.texture.size(),
        );
        let next = (columns.written % COLUMNS as u64) as u32;
        let bytes: Vec<u8> = [next, COLUMNS as u32, ROWS as u32, 0]
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect();
        ctx.queue().write_buffer(&self.uniforms, 0, &bytes);
    }

    fn draw(&mut self, ctx: &MorphContext, frame: &Frame<'_>) {
        if frame.format != self.format {
            *self = Self::new(ctx);
            return;
        }
        let mut encoder = ctx.device().create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("spectrogram"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: frame.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        ctx.queue().submit([encoder.finish()]);
    }
}

/// The analysis sink's state: a mono history, the FFT, and the band edges.
/// Everything is allocated in [`new`](Self::new), so [`push`](Self::push)
/// on the audio thread never allocates.
struct Analyzer {
    channels: usize,
    /// The last `FFT` mono samples, circular, `pos` the oldest.
    history: Vec<f32>,
    pos: usize,
    /// Samples seen, capped at `FFT` once the history is full.
    filled: usize,
    since_hop: usize,
    window: Vec<f32>,
    fft: Fft,
    re: Vec<f32>,
    im: Vec<f32>,
    /// Each row's FFT bins, `start..end`.
    rows: Vec<(usize, usize)>,
    column: Vec<u8>,
    shared: Arc<Mutex<Columns>>,
}

impl Analyzer {
    fn new(format: CaptureFormat, shared: Arc<Mutex<Columns>>) -> Self {
        Self {
            channels: usize::from(format.channels).max(1),
            history: vec![0.0; FFT],
            pos: 0,
            filled: 0,
            since_hop: 0,
            // Hann.
            window: (0..FFT)
                .map(|i| 0.5 - 0.5 * (TAU * i as f32 / FFT as f32).cos())
                .collect(),
            fft: Fft::new(FFT),
            re: vec![0.0; FFT],
            im: vec![0.0; FFT],
            rows: band_bins(format.sample_rate as f32, FFT, ROWS, LOW_HZ),
            column: vec![0; ROWS],
            shared,
        }
    }

    fn push(&mut self, samples: &[f32]) {
        for frame in samples.chunks_exact(self.channels) {
            self.history[self.pos] = frame.iter().sum::<f32>() / self.channels as f32;
            self.pos = (self.pos + 1) % FFT;
            self.filled = (self.filled + 1).min(FFT);
            self.since_hop += 1;
            if self.since_hop >= HOP && self.filled == FFT {
                self.since_hop = 0;
                self.analyse();
            }
        }
    }

    fn analyse(&mut self) {
        for i in 0..FFT {
            self.re[i] = self.history[(self.pos + i) % FFT] * self.window[i];
            self.im[i] = 0.0;
        }
        self.fft.run(&mut self.re, &mut self.im);
        // A full-scale sine through a Hann window peaks at FFT / 4.
        let scale = 4.0 / FFT as f32;
        for (row, &(start, end)) in self.column.iter_mut().zip(&self.rows) {
            let peak = (start..end)
                .map(|bin| self.re[bin].hypot(self.im[bin]))
                .fold(0.0, f32::max);
            let db = 20.0 * (peak * scale).max(1e-10).log10();
            *row = (((db - FLOOR_DB) / (CEIL_DB - FLOOR_DB)).clamp(0.0, 1.0) * 255.0) as u8;
        }
        // A column that meets the renderer's lock is dropped, never waited for.
        if let Ok(mut shared) = self.shared.try_lock() {
            let x = (shared.written % COLUMNS as u64) as usize;
            for (row, &value) in self.column.iter().enumerate() {
                shared.texels[row * COLUMNS + x] = value;
            }
            shared.written += 1;
        }
    }
}

/// Each of `rows` bands' FFT bins, log-spaced from `low_hz` to the Nyquist
/// frequency. Every band has at least one bin; bin 0 (DC) is never used.
fn band_bins(rate: f32, fft: usize, rows: usize, low_hz: f32) -> Vec<(usize, usize)> {
    let nyquist = rate / 2.0;
    let half = fft / 2;
    let bin = |hz: f32| ((hz * fft as f32 / rate) as usize).clamp(1, half);
    (0..rows)
        .map(|row| {
            let edge = |r: usize| low_hz * (nyquist / low_hz).powf(r as f32 / rows as f32);
            let start = bin(edge(row)).min(half - 1);
            let end = bin(edge(row + 1)).max(start + 1);
            (start, end)
        })
        .collect()
}

/// An in-place iterative radix-2 complex FFT of one power-of-two size.
struct Fft {
    n: usize,
    /// `exp(-2 pi i k / n)` for `k < n / 2`.
    twiddles: Vec<(f32, f32)>,
}

impl Fft {
    fn new(n: usize) -> Self {
        assert!(n.is_power_of_two(), "FFT size must be a power of two");
        let twiddles = (0..n / 2)
            .map(|k| {
                let angle = -TAU * k as f32 / n as f32;
                (angle.cos(), angle.sin())
            })
            .collect();
        Self { n, twiddles }
    }

    fn run(&self, re: &mut [f32], im: &mut [f32]) {
        let n = self.n;
        let bits = n.trailing_zeros();
        for i in 0..n {
            let j = i.reverse_bits() >> (usize::BITS - bits);
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let stride = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let (wr, wi) = self.twiddles[k * stride];
                    let (a, b) = (start + k, start + k + len / 2);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
    }
}

/// Add a sink that cuts the capture into chunks, and a thread that writes
/// them. The audio thread only copies samples and swaps buffers.
fn start(ctx: &MorphContext, dir: &Path, format: CaptureFormat, chunk_ms: u64) -> io::Result<()> {
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
    use std::f32::consts::TAU;

    use super::{CaptureFormat, Fft, au, band_bins};

    #[test]
    fn fft_finds_a_sine() {
        let n = 1024;
        let mut re: Vec<f32> = (0..n)
            .map(|i| (TAU * 64.0 * i as f32 / n as f32).sin())
            .collect();
        let mut im = vec![0.0; n];
        Fft::new(n).run(&mut re, &mut im);
        let magnitude: Vec<f32> = (0..n / 2).map(|bin| re[bin].hypot(im[bin])).collect();
        let peak = (0..n / 2)
            .max_by(|&a, &b| magnitude[a].total_cmp(&magnitude[b]))
            .unwrap();
        assert_eq!(peak, 64);
        // A unit sine over a rectangular window: n / 2 at its bin.
        assert!((magnitude[64] - n as f32 / 2.0).abs() < 0.01 * n as f32);
        assert!(magnitude[10] < 1e-2 * magnitude[64]);
    }

    #[test]
    fn bands_cover_40_hz_to_nyquist_in_order() {
        let rows = band_bins(48_000.0, 1024, 256, 40.0);
        assert_eq!(rows.len(), 256);
        // 40 Hz is bin 0.85 at 46.875 Hz per bin: the first real bin.
        assert_eq!(rows[0].0, 1);
        assert_eq!(rows.last().unwrap().1, 512);
        for pair in rows.windows(2) {
            assert!(pair[0].0 <= pair[1].0);
        }
        assert!(rows.iter().all(|&(start, end)| start < end && end <= 512));
    }

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
