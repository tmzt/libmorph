//! `low-memory` in a scripted run reaches `on_low_memory` after `init`, in
//! order with the other hooks, and also while suspended. Needs a GPU
//! adapter; without one the test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{Frame, GpuError, MorphApp, MorphContext, MorphError};

static EVENTS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

fn record(event: &'static str) {
    EVENTS.lock().unwrap().push(event);
}

struct Thrifty;

impl MorphApp for Thrifty {
    fn init(_ctx: &mut MorphContext) -> Self {
        record("init");
        Self
    }

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) {
        record("frame");
    }

    fn on_low_memory(&mut self, _ctx: &mut MorphContext) {
        record("low memory");
    }

    fn on_suspend(&mut self, _ctx: &mut MorphContext) {
        record("suspend");
    }

    fn on_resume(&mut self, _ctx: &mut MorphContext) {
        record("resume");
    }
}

const SCRIPT: &str = "\
low-memory
frames 1
suspend
low-memory
resume
low-memory
";

#[test]
fn low_memory_arrives_in_order_and_while_suspended() {
    let dir = std::env::temp_dir().join(format!("libmorph-low-memory-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["thrifty".into()],
    };
    match libmorph::script::run::<Thrifty>(&Script::parse(SCRIPT).unwrap(), run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            return;
        }
        result => {
            result.unwrap();
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        *EVENTS.lock().unwrap(),
        [
            "init",
            "low memory",
            "frame",
            "suspend",
            "low memory",
            "resume",
            "low memory",
            "suspend",
        ]
    );
}
