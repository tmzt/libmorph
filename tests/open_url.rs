//! `open <url>` in a scripted run reaches `on_open_url` after `init`, in
//! order, and links opened while suspended come after `on_resume`. Needs a
//! GPU adapter; without one the test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;
use std::time::Duration;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{Frame, GpuError, MorphApp, MorphContext, MorphError};

static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record(event: impl Into<String>) {
    EVENTS.lock().unwrap().push(event.into());
}

struct Links;

impl MorphApp for Links {
    fn init(_ctx: &mut MorphContext) -> Self {
        record("init");
        Self
    }

    fn update(&mut self, _ctx: &mut MorphContext, _dt: Duration) {}

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) {}

    fn on_open_url(&mut self, _ctx: &mut MorphContext, url: &str) {
        record(format!("open {url}"));
    }

    fn on_suspend(&mut self, _ctx: &mut MorphContext) {
        record("suspend");
    }

    fn on_resume(&mut self, _ctx: &mut MorphContext) {
        record("resume");
    }
}

const SCRIPT: &str = "\
open app://first/pair#secret
frames 1
open app://second?k=v
suspend
open app://third
open app://fourth
resume
frames 1
";

#[test]
fn links_arrive_after_init_in_order() {
    let dir = std::env::temp_dir().join(format!("libmorph-open-url-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["links".into()],
    };
    match libmorph::script::run::<Links>(&Script::parse(SCRIPT).unwrap(), run) {
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
            "open app://first/pair#secret",
            "open app://second?k=v",
            "suspend",
            "resume",
            "open app://third",
            "open app://fourth",
            "suspend",
        ]
    );
}
