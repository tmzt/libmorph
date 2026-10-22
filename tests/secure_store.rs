//! A scripted run's secure store: memory only, empty at the start of each
//! run, never the Keychain. Needs a GPU adapter; without one the test says
//! so and passes.

#![cfg(all(
    feature = "script",
    feature = "secure-store",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;
use std::time::Duration;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{Frame, GpuError, MorphApp, MorphContext, MorphError, Protection};

/// What each run saw: the blob before it wrote one, and the protection.
static SEEN: Mutex<Vec<(Option<Vec<u8>>, Protection)>> = Mutex::new(Vec::new());

struct Keeper;

impl MorphApp for Keeper {
    fn init(ctx: &mut MorphContext) -> Self {
        let store = ctx
            .secure_store()
            .expect("a store in scripted runs")
            .clone();
        let before = store.get("noise.static").unwrap();
        store.put("noise.static", &[7; 32]).unwrap();
        assert_eq!(store.get("noise.static").unwrap(), Some(vec![7; 32]));
        SEEN.lock()
            .unwrap()
            .push((before, store.protection().unwrap()));
        Self
    }

    fn update(&mut self, _ctx: &mut MorphContext, _dt: Duration) {}

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) {}
}

#[test]
fn scripted_runs_keep_secrets_in_memory() {
    let dir = std::env::temp_dir().join(format!("libmorph-secure-store-{}", std::process::id()));
    let script = Script::parse("frames 1\n").unwrap();
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (16, 32);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["keeper".into()],
    };
    for _ in 0..2 {
        match libmorph::script::run::<Keeper>(&script, run.clone()) {
            Err(MorphError::Gpu(GpuError::Adapter(error))) => {
                eprintln!("skipped: no GPU adapter ({error})");
                return;
            }
            result => drop(result.unwrap()),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        *SEEN.lock().unwrap(),
        [(None, Protection::Memory), (None, Protection::Memory)]
    );
}
