//! An app that implements no `render` draws nothing and presents nothing,
//! and says why once. Its own test binary, so the collecting logger it
//! installs is the only one in the process. Needs a GPU adapter; without
//! one the test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{GpuError, MorphApp, MorphContext, MorphError};

/// An app with `init` and nothing else: the shape an app has when its
/// author meant to take the frame and forgot `OWNS`, or meant to draw and
/// has not written `render` yet.
struct Mute;

impl MorphApp for Mute {
    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }
}

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Collect;

impl log::Log for Collect {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        LINES.lock().unwrap().push(record.args().to_string());
    }

    fn flush(&self) {}
}

/// Three frames and a capture: whatever the default does, it does it more
/// than once.
const SCRIPT: &str = "\
frames 3
screenshot blank
";

#[test]
fn a_missing_render_presents_nothing_and_says_so_once() {
    log::set_logger(&Collect).expect("the only logger in this binary");
    log::set_max_level(log::LevelFilter::Warn);

    let dir = std::env::temp_dir().join(format!("libmorph-render-default-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (16, 32);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["mute".into()],
    };
    let report = match libmorph::script::run::<Mute>(&Script::parse(SCRIPT).unwrap(), run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            return;
        }
        result => result.unwrap(),
    };

    // Nothing was presented, so the offscreen texture is as wgpu left it:
    // zero-initialized, which reads black.
    let png = std::fs::read(&report.screenshots[0].path).unwrap();
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    pixels.truncate(info.buffer_size());
    assert_eq!(info.color_type, png::ColorType::Rgb);
    assert_eq!(
        (info.width, info.height),
        (16, 32),
        "the capture is the device's size"
    );
    assert!(
        pixels.iter().all(|&byte| byte == 0),
        "an unpresented frame leaves the texture black"
    );

    let _ = std::fs::remove_dir_all(&dir);

    let lines = LINES.lock().unwrap();
    let named: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains("MorphApp::render is not implemented"))
        .collect();
    assert_eq!(
        named.len(),
        1,
        "three frames, one line; all lines were {lines:?}"
    );
    assert!(
        named[0].contains("Owned::FRAME"),
        "the line names the other way out: {}",
        named[0]
    );
}
