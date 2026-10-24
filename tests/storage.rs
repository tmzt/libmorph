//! A scripted run's `data_dir()` and `cache_dir()`: `storage/` and `cache/`
//! inside the run's output, existing before `init`, and empty at the start
//! of each run. Needs a GPU adapter; without one the test says so and
//! passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::path::PathBuf;
use std::sync::Mutex;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{Frame, GpuError, MorphApp, MorphContext, MorphError};

/// What `init` saw: each directory and the names in it.
type Seen = (Option<PathBuf>, Vec<String>, Option<PathBuf>, Vec<String>);

static SEEN: Mutex<Vec<Seen>> = Mutex::new(Vec::new());

fn names(dir: Option<&std::path::Path>) -> Vec<String> {
    dir.map(|dir| {
        std::fs::read_dir(dir)
            .expect("the directory exists before init")
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    })
    .unwrap_or_default()
}

struct Keeper;

impl MorphApp for Keeper {
    // Ignored by scripted runs, which never touch the user's directories.
    const APP_ID: &'static str = "dev.highbay.libmorph.test.never-created";

    fn init(ctx: &mut MorphContext) -> Self {
        let (data, cache) = (ctx.data_dir(), ctx.cache_dir());
        SEEN.lock().unwrap().push((
            data.map(ToOwned::to_owned),
            names(data),
            cache.map(ToOwned::to_owned),
            names(cache),
        ));
        for dir in [data, cache].into_iter().flatten() {
            std::fs::write(dir.join("counter"), b"1").unwrap();
        }
        Self
    }

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) {}
}

#[test]
fn each_run_starts_with_empty_directories_in_its_output() {
    let dir = std::env::temp_dir().join(format!("libmorph-storage-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    for _ in 0..2 {
        let run = ScriptRun {
            spec,
            out: OutDir::Exact(dir.clone()),
            overlay: false,
            fallback_adapter: false,
            args: vec!["keeper".into()],
        };
        match libmorph::script::run::<Keeper>(&Script::parse("frames 1\n").unwrap(), run) {
            Err(MorphError::Gpu(GpuError::Adapter(error))) => {
                eprintln!("skipped: no GPU adapter ({error})");
                return;
            }
            result => {
                result.unwrap();
            }
        }
    }
    let seen = SEEN.lock().unwrap().clone();
    let _ = std::fs::remove_dir_all(&dir);
    let empty: Vec<String> = Vec::new();
    let expected = (
        Some(dir.join("storage")),
        empty.clone(),
        Some(dir.join("cache")),
        empty,
    );
    assert_eq!(seen, [expected.clone(), expected]);
}
