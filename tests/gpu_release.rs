//! Giving the device back off screen (plan 20261111_03, step 4), in
//! scripted runs: a `Release` app's device goes after `after` without a
//! surface and not before, or at once under a `Background` trim or low
//! memory while suspended, never while resumed; no frame runs while it is
//! released; `ctx.gpu()` is `None` in `on_intent` then and `Some` otherwise;
//! the restore asks for the same limits and `on_gpu(Restored)` comes before
//! `on_resume`; a `Keep` app is never released. Needs a GPU adapter; without
//! one the test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;
use std::time::Duration;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{
    Frame, GpuError, GpuEvent, GpuInBackground, Intent, MorphApp, MorphContext, MorphError,
    RenderStatus, TrimLevel,
};

static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn record(event: impl Into<String>) {
    EVENTS.lock().unwrap().push(event.into());
}

/// What both apps record: every hook, and whether each callback that can
/// run without a device found one.
macro_rules! recording_app {
    ($name:ident, $policy:expr) => {
        struct $name {
            limits: libmorph::wgpu::Limits,
        }

        impl MorphApp for $name {
            const GPU_IN_BACKGROUND: GpuInBackground = $policy;

            fn init(ctx: &mut MorphContext) -> Self {
                record("init");
                Self {
                    limits: ctx.limits().clone(),
                }
            }

            fn update(&mut self, ctx: &mut MorphContext, _dt: Duration) {
                assert!(ctx.gpu().is_some(), "update always has a device");
                record("update");
            }

            fn render(&mut self, ctx: &mut MorphContext, _frame: &Frame<'_>) -> RenderStatus {
                assert!(ctx.gpu().is_some(), "render always has a device");
                RenderStatus::Skipped
            }

            fn on_intent(&mut self, ctx: &mut MorphContext, intent: &Intent) {
                let gpu = if ctx.gpu().is_some() { "gpu" } else { "no gpu" };
                if ctx.gpu().is_none() {
                    assert!(ctx.device().is_none() && ctx.queue().is_none());
                    assert!(ctx.adapter_info().is_none());
                    assert!(ctx.pipeline_cache().is_none(), "the cache goes too");
                    assert_eq!(ctx.limits(), &self.limits, "the limits stay");
                }
                record(format!(
                    "intent {} {gpu}",
                    intent.extra("at").unwrap_or("?")
                ));
            }

            fn on_low_memory(&mut self, _ctx: &mut MorphContext) {
                record("low memory");
            }

            fn on_trim_memory(&mut self, _ctx: &mut MorphContext, level: TrimLevel) {
                record(match level {
                    TrimLevel::UiHidden => "trim ui-hidden",
                    TrimLevel::Background => "trim background",
                    _ => "trim other",
                });
            }

            fn on_gpu(&mut self, ctx: &mut MorphContext, event: &GpuEvent) {
                let gpu = ctx.gpu().expect("on_gpu always has a device");
                match event {
                    GpuEvent::Release => record("release"),
                    GpuEvent::Restored => {
                        assert_eq!(ctx.limits(), &self.limits, "the same limits");
                        assert_eq!(&gpu.device.limits(), ctx.limits(), "granted again");
                        record("restored");
                    }
                    _ => record("gpu other"),
                }
            }

            fn on_suspend(&mut self, ctx: &mut MorphContext) {
                assert!(ctx.gpu().is_some(), "on_suspend comes before a release");
                record("suspend");
            }

            fn on_resume(&mut self, ctx: &mut MorphContext) {
                assert!(ctx.gpu().is_some(), "on_resume comes after a restore");
                record("resume");
            }
        }
    };
}

recording_app!(
    Releasing,
    GpuInBackground::Release {
        after: Duration::from_secs(1),
    }
);
recording_app!(Keeping, GpuInBackground::Keep);

/// Run `script` with `A` and return what it recorded, or `None` without an
/// adapter.
fn run<A: MorphApp>(name: &str, script: &str) -> Option<Vec<String>> {
    EVENTS.lock().unwrap().clear();
    let dir = std::env::temp_dir().join(format!(
        "libmorph-gpu-release-{name}-{}",
        std::process::id()
    ));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec![name.into()],
    };
    match libmorph::script::run::<A>(&Script::parse(script).unwrap(), run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            return None;
        }
        result => {
            result.unwrap();
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    Some(EVENTS.lock().unwrap().clone())
}

/// At 10 fps a frame is 100 ms: nine frames suspended is 0.9 s, short of
/// `after`; the tenth reaches it.
const AFTER: &str = "\
fps 10
frames 1
suspend
frames 9
intent at=0.9s
frames 1
intent at=1.0s
frames 3
resume
intent at=resumed
frames 1
";

/// Memory pressure: nothing while resumed; while suspended, a trim below
/// `Background` keeps the device and `Background` releases it; low memory
/// releases it at once.
const PRESSURE: &str = "\
fps 10
frames 1
low-memory
trim-memory background
suspend
trim-memory ui-hidden
intent at=ui-hidden
trim-memory background
intent at=background
resume
frames 1
suspend
low-memory
intent at=low-memory
resume
frames 1
";

// One test: the runs share the recording, so they must not overlap.
#[test]
fn the_device_goes_off_screen_and_comes_back() {
    let Some(events) = run::<Releasing>("after", AFTER) else {
        return;
    };
    assert_eq!(
        events,
        [
            "init",
            "update",
            "suspend",
            "intent 0.9s gpu",
            "release",
            "intent 1.0s no gpu",
            "restored",
            "resume",
            "intent resumed gpu",
            "update",
            "suspend",
        ],
        "released after `after` and not before, restored before on_resume"
    );

    let events = run::<Releasing>("pressure", PRESSURE).unwrap();
    assert_eq!(
        events,
        [
            "init",
            "update",
            "low memory",
            "trim background",
            "suspend",
            "trim ui-hidden",
            "intent ui-hidden gpu",
            "trim background",
            "release",
            "intent background no gpu",
            "restored",
            "resume",
            "update",
            "suspend",
            "low memory",
            "release",
            "intent low-memory no gpu",
            "restored",
            "resume",
            "update",
            "suspend",
        ],
        "released at once under pressure while suspended, never while resumed"
    );

    let mut keep = run::<Keeping>("keep-after", AFTER).unwrap();
    keep.extend(run::<Keeping>("keep-pressure", PRESSURE).unwrap());
    assert!(
        !keep
            .iter()
            .any(|e| e == "release" || e == "restored" || e.ends_with("no gpu")),
        "a Keep app is never released: {keep:?}"
    );
}
