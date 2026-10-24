//! `ctx.open_link` in a scripted run checks the link and logs it, cut after
//! its path, without opening it. Needs a GPU adapter; without one the test
//! says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{
    Frame, GpuError, MorphApp, MorphContext, MorphError, OpenLinkError, Touch, TouchPhase,
};

static RESULTS: Mutex<Vec<Result<(), OpenLinkError>>> = Mutex::new(Vec::new());

struct Opener;

impl MorphApp for Opener {
    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) {}

    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        if touch.phase == TouchPhase::Ended {
            let mut results = RESULTS.lock().unwrap();
            results.push(ctx.open_link("https://example.com/a?token=secret#f"));
            results.push(ctx.open_link("javascript:alert(1)"));
            results.push(ctx.open_link("file:///etc/passwd"));
        }
    }
}

#[test]
fn scripted_runs_check_and_do_not_open() {
    let dir = std::env::temp_dir().join(format!("libmorph-open-link-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["opener".into()],
    };
    match libmorph::script::run::<Opener>(&Script::parse("tap 1 1\n").unwrap(), run) {
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
        *RESULTS.lock().unwrap(),
        [
            Ok(()),
            Err(OpenLinkError::BadUrl),
            Err(OpenLinkError::BadUrl)
        ]
    );
}
