//! `intent <key>=<value>` in a scripted run reaches `on_intent` after
//! `init`, in order, with the extras it was given and `on_open_url` left
//! alone; intents that arrive while suspended come after `on_resume`. Needs
//! a GPU adapter; without one the test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;
use std::time::Duration;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{Frame, GpuError, Intent, MorphApp, MorphContext, MorphError, RenderStatus};

static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record(event: impl Into<String>) {
    EVENTS.lock().unwrap().push(event.into());
}

struct Intents;

impl MorphApp for Intents {
    fn init(_ctx: &mut MorphContext) -> Self {
        record("init");
        Self
    }

    fn update(&mut self, _ctx: &mut MorphContext, _dt: Duration) {}

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) -> RenderStatus {
        // Nothing is drawn: this drives hooks, not pixels. Said
        // explicitly rather than left to the default, so the default's
        // log stays a signal that an app forgot to draw.
        RenderStatus::Skipped
    }

    /// What an app does with this: read the extras it knows by key, and
    /// ignore the rest.
    fn on_intent(&mut self, _ctx: &mut MorphContext, intent: &Intent) {
        record(format!(
            "intent scene={:?} n={:?} missing={:?} new={} action={:?} data={:?}",
            intent.extra("scene"),
            intent.extra("n"),
            intent.extra("missing"),
            intent.is_new,
            intent.action,
            intent.data,
        ));
    }

    /// Not called by `intent`: an intent carrying extras is not a link.
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
intent scene=demo
frames 1
intent scene=tavern n=3
suspend
intent scene=third
resume
frames 1
";

#[test]
fn intents_arrive_after_init_in_order() {
    let dir = std::env::temp_dir().join(format!("libmorph-intent-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["intents".into()],
    };
    match libmorph::script::run::<Intents>(&Script::parse(SCRIPT).unwrap(), run) {
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
            r#"intent scene=Some("demo") n=None missing=None new=true action=None data=None"#,
            r#"intent scene=Some("tavern") n=Some("3") missing=None new=true action=None data=None"#,
            "suspend",
            "resume",
            r#"intent scene=Some("third") n=None missing=None new=true action=None data=None"#,
            "suspend",
        ],
        "an intent while suspended waits for the resume, and no link is reported"
    );
}

/// The parse rejects what cannot be an extra, with the line and a usage.
#[test]
fn an_extra_without_a_key_and_a_value_is_rejected() {
    assert!(Script::parse("intent\n").is_err(), "no extras");
    let err = Script::parse("intent scene\n").unwrap_err().to_string();
    assert!(err.contains("<key>=<value>"), "{err}");
    let err = Script::parse("intent =demo\n").unwrap_err().to_string();
    assert!(err.contains("no key"), "{err}");
}

/// An empty value is an extra: `--es k ""` is a key that is present and
/// empty, which is not the same as a key that is absent.
#[test]
fn an_empty_value_is_still_an_extra() {
    Script::parse("intent scene=\n").expect("an empty value parses");
}
