//! `trim-memory <level>` and `low-memory` in a scripted run reach
//! `on_memory_pressure` right after the raw hook, with the report's grade;
//! `ctx.memory_pressure()` holds the highest grade since the last resume
//! and is cleared before `on_resume` (plan `20261119_03`, step 1). Needs a
//! GPU adapter; without one the test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{
    Frame, GpuError, MemoryPressure, MorphApp, MorphContext, MorphError, RenderStatus, TrimLevel,
};

#[derive(Debug, PartialEq)]
enum Event {
    Trim(TrimLevel),
    LowMemory,
    /// The hook's grade, and what the context held during it.
    Pressure(MemoryPressure, Option<MemoryPressure>),
    /// What the context held during `on_resume`.
    Resume(Option<MemoryPressure>),
}

static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());

struct Graded;

impl MorphApp for Graded {
    fn init(ctx: &mut MorphContext) -> Self {
        assert_eq!(ctx.memory_pressure(), None, "no grade before any report");
        Self
    }

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) -> RenderStatus {
        RenderStatus::Skipped
    }

    fn on_trim_memory(&mut self, _ctx: &mut MorphContext, level: TrimLevel) {
        EVENTS.lock().unwrap().push(Event::Trim(level));
    }

    fn on_low_memory(&mut self, _ctx: &mut MorphContext) {
        EVENTS.lock().unwrap().push(Event::LowMemory);
    }

    fn on_memory_pressure(&mut self, ctx: &mut MorphContext, pressure: MemoryPressure) {
        EVENTS
            .lock()
            .unwrap()
            .push(Event::Pressure(pressure, ctx.memory_pressure()));
    }

    fn on_resume(&mut self, ctx: &mut MorphContext) {
        EVENTS
            .lock()
            .unwrap()
            .push(Event::Resume(ctx.memory_pressure()));
    }
}

// `suspend` before `resume`: the script runner accepts `resume` only while
// suspended. It changes no grade.
const SCRIPT: &str = "\
trim-memory running-critical
trim-memory ui-hidden
suspend
resume
low-memory
";

#[test]
fn grades_follow_each_report_and_clear_on_resume() {
    let dir = std::env::temp_dir().join(format!("libmorph-memory-pressure-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["graded".into()],
    };
    match libmorph::script::run::<Graded>(&Script::parse(SCRIPT).unwrap(), run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            return;
        }
        result => {
            result.unwrap();
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    use MemoryPressure::{Low, UiHidden};
    assert_eq!(
        *EVENTS.lock().unwrap(),
        [
            Event::Trim(TrimLevel::RunningCritical),
            Event::Pressure(Low, Some(Low)),
            Event::Trim(TrimLevel::UiHidden),
            // A lower report is passed on; the context keeps the highest.
            Event::Pressure(UiHidden, Some(Low)),
            Event::Resume(None),
            Event::LowMemory,
            Event::Pressure(Low, Some(Low)),
        ]
    );
}
