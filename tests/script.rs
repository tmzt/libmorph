//! A scripted run end to end: drag, suspend, resume and screenshots, run
//! twice into the same directory to check that the files are identical.
//! Needs a GPU adapter; without one the test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{Frame, GpuError, MorphApp, MorphContext, MorphError, Touch, TouchPhase, wgpu};

static SUSPENDS: AtomicUsize = AtomicUsize::new(0);
static RESUMES: AtomicUsize = AtomicUsize::new(0);
static DT_NANOS: AtomicU64 = AtomicU64::new(0);

const SIZE: (u32, u32) = (64, 128);
const BLUE: [u8; 3] = [0, 0, 255];
const RED: [u8; 3] = [255, 0, 0];

/// Clears to blue until touched, then red. Exact 0 and 1 channels encode
/// the same with or without sRGB.
struct Probe {
    touched: bool,
}

impl MorphApp for Probe {
    fn init(ctx: &mut MorphContext) -> Self {
        assert_eq!(ctx.surface_size(), SIZE);
        assert!(!ctx.audio().is_available(), "scripted runs are silent");
        Self { touched: false }
    }

    fn update(&mut self, _ctx: &mut MorphContext, dt: Duration) {
        DT_NANOS.store(dt.as_nanos() as u64, Ordering::SeqCst);
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        let color = if self.touched {
            wgpu::Color::RED
        } else {
            wgpu::Color::BLUE
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

    fn on_touch(&mut self, _ctx: &mut MorphContext, touch: Touch) {
        if touch.phase == TouchPhase::Started {
            self.touched = true;
        }
    }

    fn on_suspend(&mut self, _ctx: &mut MorphContext) {
        SUSPENDS.fetch_add(1, Ordering::SeqCst);
    }

    fn on_resume(&mut self, _ctx: &mut MorphContext) {
        RESUMES.fetch_add(1, Ordering::SeqCst);
    }
}

const SCRIPT: &str = "\
fps 30
frames 2
screenshot idle
drag 1 10 10 50 100 3
screenshot dragged
suspend
resume
frames 1
screenshot after
";

/// The centre pixel of a screenshot.
fn centre(path: &std::path::Path) -> [u8; 3] {
    let file = std::fs::read(path).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(file))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height), SIZE);
    let at = ((SIZE.1 / 2 * SIZE.0 + SIZE.0 / 2) * 3) as usize;
    pixels[at..at + 3].try_into().unwrap()
}

#[test]
fn scripted_run_is_repeatable() {
    let dir = std::env::temp_dir().join(format!("libmorph-script-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let script = Script::parse(SCRIPT).unwrap();
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = SIZE;
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["probe".into()],
    };

    let report = match libmorph::script::run::<Probe>(&script, run.clone()) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            return;
        }
        result => result.unwrap(),
    };
    assert_eq!(report.dir, dir);
    let shots: Vec<(String, u64, u32)> = report
        .screenshots
        .iter()
        .map(|s| {
            let name = s.path.file_name().unwrap().to_string_lossy().into_owned();
            (name, s.frame, s.line)
        })
        .collect();
    assert_eq!(
        shots,
        [
            ("idle.png".into(), 2, 3),
            ("dragged.png".into(), 5, 5),
            ("after.png".into(), 6, 9),
        ]
    );
    assert_eq!(centre(&dir.join("idle.png")), BLUE);
    assert_eq!(centre(&dir.join("dragged.png")), RED);
    assert_eq!(centre(&dir.join("after.png")), RED);
    assert_eq!(
        SUSPENDS.load(Ordering::SeqCst),
        2,
        "the suspend, then shutdown"
    );
    assert_eq!(RESUMES.load(Ordering::SeqCst), 1);
    assert_eq!(DT_NANOS.load(Ordering::SeqCst), 33_333_333);

    let files = ["idle.png", "dragged.png", "after.png", "run.json"];
    let first: Vec<Vec<u8>> = files
        .iter()
        .map(|f| std::fs::read(dir.join(f)).unwrap())
        .collect();
    let json = String::from_utf8(first[3].clone()).unwrap();
    assert!(json.contains("\"size\": [64, 128],"), "{json}");
    assert!(json.contains("\"fps\": 30,"), "{json}");
    assert!(
        json.contains("\"exit\": { \"by\": \"end\", \"line\": 9 },"),
        "{json}"
    );

    let again = libmorph::script::run::<Probe>(&script, run).unwrap();
    assert_eq!(again, report);
    for (file, bytes) in files.iter().zip(&first) {
        assert_eq!(&std::fs::read(dir.join(file)).unwrap(), bytes, "{file}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
