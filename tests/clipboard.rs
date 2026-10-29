//! `ctx.clipboard_text` and `clipboard_has_text` in a scripted run give
//! `None` and `false`: a run never reads the host's clipboard. Needs a GPU
//! adapter; without one the test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{Frame, GpuError, MorphApp, MorphContext, MorphError, Touch, TouchPhase};

static RESULTS: Mutex<Vec<(Option<String>, bool)>> = Mutex::new(Vec::new());

struct Paster;

impl MorphApp for Paster {
    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) {}

    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        if touch.phase == TouchPhase::Ended {
            let has_text = ctx.clipboard_has_text();
            RESULTS
                .lock()
                .unwrap()
                .push((ctx.clipboard_text(), has_text));
        }
    }
}

#[test]
fn scripted_runs_have_no_clipboard() {
    let dir = std::env::temp_dir().join(format!("libmorph-clipboard-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["paster".into()],
    };
    match libmorph::script::run::<Paster>(&Script::parse("tap 1 1\n").unwrap(), run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            return;
        }
        result => {
            result.unwrap();
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(*RESULTS.lock().unwrap(), [(None, false)]);
}

/// What [`Copier`] saw: the write's answer, `clipboard_can_write`, then
/// the read after the write.
type Copied = (
    Result<libmorph::ClipboardWrite, libmorph::ClipboardError>,
    bool,
    Option<String>,
);

static COPIED: Mutex<Vec<Copied>> = Mutex::new(Vec::new());

struct Copier;

impl MorphApp for Copier {
    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) {}

    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        if touch.phase == TouchPhase::Ended {
            let written = ctx.set_clipboard_text("libmorph-clipboard-test");
            let can_write = ctx.clipboard_can_write();
            COPIED
                .lock()
                .unwrap()
                .push((written, can_write, ctx.clipboard_text()));
        }
    }
}

/// A scripted write answers `Silent` without touching the host's
/// clipboard, and the run still reads nothing.
#[test]
fn scripted_runs_write_nothing() {
    let dir = std::env::temp_dir().join(format!("libmorph-clipcopy-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["copier".into()],
    };
    match libmorph::script::run::<Copier>(&Script::parse("tap 1 1\n").unwrap(), run) {
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
        *COPIED.lock().unwrap(),
        [(Ok(libmorph::ClipboardWrite::Silent), true, None)]
    );
}
