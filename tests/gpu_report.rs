//! The allocator report (plan 20261111_03, decision 13): a scripted run logs
//! it once at each `suspend` and on each `gpu-report`, with or without a
//! report from the backend. Needs a GPU adapter; without one the test says
//! so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{Frame, GpuError, MorphApp, MorphContext, MorphError, RenderStatus};

/// libmorph's report lines, as logged.
static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        let line = record.args().to_string();
        if line.starts_with("libmorph: gpu memory at ") {
            LINES.lock().unwrap().push(line);
        }
    }

    fn flush(&self) {}
}

static CAPTURE: Capture = Capture;

struct Quiet;

impl MorphApp for Quiet {
    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) -> RenderStatus {
        // Nothing to look at: this checks the log.
        RenderStatus::Skipped
    }
}

const SCRIPT: &str = "\
frames 1
gpu-report
suspend
gpu-report
resume
frames 1
";

#[test]
fn a_report_at_suspend_and_on_gpu_report() {
    log::set_logger(&CAPTURE).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let dir = std::env::temp_dir().join(format!("libmorph-gpu-report-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["quiet".into()],
    };
    match libmorph::script::run::<Quiet>(&Script::parse(SCRIPT).unwrap(), run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            return;
        }
        result => {
            result.unwrap();
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    let lines = LINES.lock().unwrap();
    let at: Vec<&str> = lines
        .iter()
        .map(|line| {
            let rest = line.strip_prefix("libmorph: gpu memory at ").unwrap();
            rest.split(':').next().unwrap()
        })
        .collect();
    // The end of the run suspends the app once more.
    assert_eq!(at, ["gpu-report", "suspend", "gpu-report", "suspend"]);
    for line in lines.iter() {
        assert!(
            line.ends_with("bytes reserved")
                || line.ends_with("the backend gives no allocator report"),
            "{line}"
        );
    }
}
