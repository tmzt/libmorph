//! `geometry <width> <height>`: the window reports a new size, as it does
//! when the device turns. Checks that the app sees it and that the capture
//! follows, both while running and across a suspend.
//!
//! What this does *not* check is the surface lifecycle - `surface.configure`
//! at the new size, `wgpu_config` at the following attach, the format kept
//! across the change, or a swapchain image held over it. A scripted run's
//! target is an offscreen texture and has no swapchain, so none of that is
//! reachable here; it wants a target that simulates one.
//!
//! Needs a GPU adapter; without one the test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{Frame, GpuError, MorphApp, MorphContext, MorphError, RenderStatus, wgpu};

/// The size the app was told to draw at, once per frame.
static SEEN: Mutex<Vec<(u32, u32)>> = Mutex::new(Vec::new());

const PORTRAIT: (u32, u32) = (64, 128);
const LANDSCAPE: (u32, u32) = (128, 64);

/// Clears, and records the size it was handed.
struct Probe;

impl MorphApp for Probe {
    const SRGB_SURFACE: bool = false;

    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) -> RenderStatus {
        // Both the frame's own size and the context's, so a disagreement
        // between them would fail rather than pass quietly.
        assert_eq!(
            frame.size,
            ctx.surface_size(),
            "the frame and the context disagree about the size"
        );
        SEEN.lock().unwrap().push(frame.size);

        let Some(gpu) = ctx.gpu() else {
            return RenderStatus::Skipped;
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLUE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        gpu.queue.submit([encoder.finish()]);
        RenderStatus::Drawn
    }
}

const SCRIPT: &str = "\
frames 1
screenshot portrait
geometry 128 64
frames 1
screenshot landscape
suspend
geometry 64 128
resume
frames 1
screenshot back
";

/// The PNG's own dimensions, which is what a reader of the capture sees.
fn dimensions(path: &std::path::Path) -> (u32, u32) {
    let file = std::fs::read(path).unwrap();
    let reader = png::Decoder::new(std::io::Cursor::new(file))
        .read_info()
        .unwrap();
    let info = reader.info();
    (info.width, info.height)
}

#[test]
fn geometry_changes_the_size_the_app_draws_at() {
    let dir = std::env::temp_dir().join(format!("libmorph-geometry-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    let script = Script::parse(SCRIPT).unwrap();
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = PORTRAIT;
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec![],
    };

    match libmorph::script::run::<Probe>(&script, run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            return;
        }
        result => result.unwrap(),
    };

    // The capture follows the window.
    assert_eq!(dimensions(&dir.join("portrait.png")), PORTRAIT);
    assert_eq!(
        dimensions(&dir.join("landscape.png")),
        LANDSCAPE,
        "`geometry` while running did not reach the target"
    );
    assert_eq!(
        dimensions(&dir.join("back.png")),
        PORTRAIT,
        "`geometry` while suspended did not reach the target the resume attached"
    );

    // And so does the app, which is the part that matters: a capture could
    // change size while the app still drew at the old one.
    assert_eq!(
        *SEEN.lock().unwrap(),
        [PORTRAIT, LANDSCAPE, PORTRAIT],
        "the app was not told the new size"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
