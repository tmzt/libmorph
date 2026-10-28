//! The camera in scripted runs (plan `20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md`,
//! section 4): a scan that reads an injected code, a capture answered from a
//! PNG, a denial, and the viewfinder drawing a known frame. Needs a GPU
//! adapter; without one each test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use libmorph::camera::{
    CameraError, CameraEvent, CaptureOptions, Facing, Fit, FrameFormat, PreviewFrame,
    PreviewOptions, QrMode, QrOptions, Viewfinder, ViewfinderOptions,
};
use libmorph::script::{OutDir, Report, Script, ScriptRun};
use libmorph::{
    Frame, GpuError, MorphApp, MorphContext, MorphError, OpenLinkError, Touch, TouchPhase, wgpu,
};

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("libmorph-camera-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `A` through `script` at `size` into `dir/out`; `None` without a GPU.
fn run<A: MorphApp>(script: &Script, dir: &Path, size: (u32, u32)) -> Option<Report> {
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = size;
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.join("out")),
        overlay: false,
        fallback_adapter: false,
        args: vec!["camera".into()],
    };
    match libmorph::script::run::<A>(script, run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            None
        }
        result => Some(result.unwrap()),
    }
}

fn clear(ctx: &MorphContext, frame: &Frame<'_>) {
    let mut encoder = ctx.device().create_command_encoder(&Default::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
    ctx.queue().submit([encoder.finish()]);
}

fn png_rgb(path: &Path) -> (u32, u32, Vec<u8>) {
    let file = std::fs::read(path).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(file))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgb);
    (info.width, info.height, pixels)
}

// --- A QR scan reads an injected code -------------------------------------

static SCAN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Starts a `Once` QR scan in `init`, zooms on the first touch.
struct Scanner {
    scan: libmorph::camera::ScanId,
}

impl MorphApp for Scanner {
    fn init(ctx: &mut MorphContext) -> Self {
        let options = QrOptions::default();
        assert_eq!(options.mode, QrMode::Once);
        Self {
            scan: ctx.camera().start_qr(&options),
        }
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        clear(ctx, frame);
    }

    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        if touch.phase == TouchPhase::Started {
            ctx.camera().zoom_by(self.scan, 2.5);
        }
    }

    fn on_camera(&mut self, ctx: &mut MorphContext, event: &CameraEvent) {
        let line = match event {
            CameraEvent::Opened {
                scan, zoom_range, ..
            } => {
                assert_eq!(*scan, self.scan);
                assert_eq!(ctx.camera().zoom(*scan), Some(1.0));
                format!("opened {zoom_range:?}")
            }
            CameraEvent::Zoom { ratio, .. } => format!("zoom {ratio}"),
            CameraEvent::Qr { scan, code } => {
                assert_eq!(*scan, self.scan);
                assert_eq!(code.version, 1);
                assert_eq!(
                    code.text.as_deref().map(str::as_bytes),
                    Some(&code.bytes[..])
                );
                format!("qr {}", code.text.as_deref().unwrap_or("?"))
            }
            CameraEvent::Closed { reason, .. } => format!("closed {reason:?}"),
            other => format!("{other:?}"),
        };
        SCAN.lock().unwrap().push(line);
    }
}

#[test]
fn a_scan_reads_an_injected_code() {
    let dir = dir("qr");
    let script = Script::parse(
        "frames 1\n\
         tap 5 5\n\
         camera-qr flankr://pair?k=a b #1\n\
         frames 1\n\
         camera-qr ignored: the scan closed\n\
         frames 1\n",
    )
    .unwrap();
    if run::<Scanner>(&script, &dir, (32, 64)).is_none() {
        return;
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        *SCAN.lock().unwrap(),
        [
            "opened (1.0, 8.0)",
            "zoom 2.5",
            "qr flankr://pair?k=a b #1",
            "closed None",
        ]
    );
}

// --- A capture answered from a PNG ----------------------------------------

/// Width, height, format and data of each frame captured.
type Captured = (u32, u32, FrameFormat, Vec<u8>);

static CAPTURES: Mutex<Vec<Captured>> = Mutex::new(Vec::new());

/// Captures in `init` (Gray8) and on each touch (Rgba8).
struct Snapper;

impl MorphApp for Snapper {
    fn init(ctx: &mut MorphContext) -> Self {
        let mut options = CaptureOptions::default();
        options.format = FrameFormat::Gray8;
        ctx.camera().capture_frame(&options);
        Self
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        clear(ctx, frame);
    }

    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        if touch.phase == TouchPhase::Started {
            ctx.camera().capture_frame(&CaptureOptions::default());
        }
    }

    fn on_camera(&mut self, _ctx: &mut MorphContext, event: &CameraEvent) {
        if let CameraEvent::Frame {
            result: Ok(frame), ..
        } = event
        {
            CAPTURES.lock().unwrap().push((
                frame.width,
                frame.height,
                frame.format,
                frame.data.clone(),
            ));
        }
    }
}

#[test]
fn a_capture_is_answered_from_a_png() {
    let dir = dir("frame");
    // A 2 × 1 PNG: red, white.
    let mut file = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut file, 2, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[255, 0, 0, 255, 255, 255])
            .unwrap();
    }
    std::fs::write(dir.join("frame.png"), file).unwrap();
    // `camera-frame` answers the pending Gray8 capture; the second one waits
    // for the next frame, given after the tap.
    let source =
        "frames 1\ncamera-frame frame.png\nframes 1\ntap 1 1\ncamera-frame frame.png\nframes 1\n";
    std::fs::write(dir.join("snap.morph"), source).unwrap();
    let script = Script::load(&dir.join("snap.morph")).unwrap();
    if run::<Snapper>(&script, &dir, (32, 64)).is_none() {
        return;
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        *CAPTURES.lock().unwrap(),
        [
            (2, 1, FrameFormat::Gray8, vec![76, 255]),
            (
                2,
                1,
                FrameFormat::Rgba8,
                vec![255, 0, 0, 255, 255, 255, 255, 255]
            ),
        ]
    );
}

// --- A denial, and Settings off Android -----------------------------------

static DENIALS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Starts a preview on each touch.
struct Denied;

impl MorphApp for Denied {
    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        clear(ctx, frame);
    }

    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        if touch.phase == TouchPhase::Started {
            ctx.camera()
                .start_preview(Facing::Back, PreviewOptions::default());
        }
    }

    fn on_camera(&mut self, ctx: &mut MorphContext, event: &CameraEvent) {
        let line = match event {
            CameraEvent::Closed {
                reason: Some(CameraError::PermissionDenied { settings_only }),
                ..
            } => {
                if *settings_only {
                    assert_eq!(ctx.open_app_settings(), Err(OpenLinkError::Unsupported));
                }
                format!("denied settings_only={settings_only}")
            }
            CameraEvent::Opened { .. } => "opened".into(),
            CameraEvent::Closed { reason: None, .. } => "closed".into(),
            other => format!("{other:?}"),
        };
        DENIALS.lock().unwrap().push(line);
    }
}

#[test]
fn a_denial_fails_the_next_call_with_the_reason() {
    let dir = dir("deny");
    let script = Script::parse(
        "camera-deny-forever\n\
         tap 1 1\n\
         camera-deny\n\
         tap 1 1\n\
         tap 1 1\n\
         frames 1\n",
    )
    .unwrap();
    if run::<Denied>(&script, &dir, (32, 64)).is_none() {
        return;
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        *DENIALS.lock().unwrap(),
        [
            "denied settings_only=true",
            "denied settings_only=false",
            "opened",
        ]
    );
}

// --- The viewfinder draws a known frame -----------------------------------

const QUADRANTS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];

/// An 8 × 8 frame stored on its side: quadrants red (top left), green (top
/// right), blue (bottom left), white, needing one clockwise turn. Each
/// quadrant is 2 × 2 chroma samples, so filtering inside it mixes nothing.
fn sideways_frame() -> PreviewFrame {
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..8 {
            let [r, g, b] = QUADRANTS[(y / 4) * 2 + x / 4];
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    PreviewFrame::from_rgba(8, 8, &rgba)
        .unwrap()
        .with_quarter_turns(1)
        .with_sequence(7)
}

/// Draws [`sideways_frame`] over the whole target.
struct Finder {
    viewfinder: Viewfinder,
}

impl MorphApp for Finder {
    fn init(ctx: &mut MorphContext) -> Self {
        let mut viewfinder = Viewfinder::new(ctx);
        assert!(!viewfinder.has_frame());
        let frame = sideways_frame();
        assert!(viewfinder.upload(ctx, &frame));
        assert!(!viewfinder.upload(ctx, &frame), "same sequence: no upload");
        assert!(viewfinder.upload(ctx, &frame.clone().with_sequence(8)));
        assert_eq!(viewfinder.image_size(), Some((8, 8)));
        Self { viewfinder }
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        let mut encoder = ctx.device().create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
            let mut options = ViewfinderOptions::default();
            options.fit = Fit::Stretch;
            let rect = [0.0, 0.0, frame.size.0 as f32, frame.size.1 as f32];
            self.viewfinder.draw(ctx, &mut pass, rect, &options);
        }
        ctx.queue().submit([encoder.finish()]);
    }
}

/// The same into a surface that is not sRGB: the shader writes the camera's
/// values as they are.
struct LinearFinder(Finder);

impl MorphApp for LinearFinder {
    const SRGB_SURFACE: bool = false;

    fn init(ctx: &mut MorphContext) -> Self {
        assert!(!ctx.surface_format().is_srgb());
        Self(Finder::init(ctx))
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        self.0.render(ctx, frame);
    }
}

#[test]
fn the_viewfinder_turns_and_converts_a_frame() {
    check_viewfinder::<Finder>("viewfinder");
}

#[test]
fn the_viewfinder_writes_plain_values_to_a_linear_target() {
    check_viewfinder::<LinearFinder>("viewfinder-linear");
}

fn check_viewfinder<A: MorphApp>(name: &str) {
    let dir = dir(name);
    let script = Script::parse("frames 1\nscreenshot shot\n").unwrap();
    let Some(report) = run::<A>(&script, &dir, (64, 64)) else {
        return;
    };
    let (w, h, rgb) = png_rgb(&report.screenshots[0].path);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!((w, h), (64, 64));
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 3) as usize;
        [rgb[i], rgb[i + 1], rgb[i + 2]]
    };
    // Turned clockwise: the stored top left (red) is now top right, the
    // stored bottom left (blue) top left, and so on. Expected colours are the
    // CPU path's (BT.601 through I420), sampled at quadrant centres.
    let reference = sideways_frame().to_rgba();
    let stored = |x: usize, y: usize| {
        let i = (y * 8 + x) * 4;
        [reference[i], reference[i + 1], reference[i + 2]]
    };
    let cases = [
        ((48, 16), stored(0, 0)), // red
        ((16, 16), stored(0, 7)), // blue
        ((48, 48), stored(7, 0)), // green
        ((16, 48), stored(7, 7)), // white
    ];
    for ((x, y), want) in cases {
        let got = at(x, y);
        for c in 0..3 {
            assert!(
                got[c].abs_diff(want[c]) <= 3,
                "pixel ({x}, {y}): {got:?}, want {want:?}"
            );
        }
    }
}
