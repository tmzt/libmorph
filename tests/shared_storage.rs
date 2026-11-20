//! All files access in a scripted run (plan `20261119_01`, step 1): it
//! answers `NotRequired` until the `all-files-access` verb sets a state,
//! then each state from the next hook on; `on_resume` reads a grant made
//! while suspended. The settings page answers `Ok`, there is no shared
//! storage root, and `scan_media` takes absolute paths and refuses a
//! relative one. Needs a GPU adapter; without one the test says so and
//! passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{
    AllFilesAccess, Frame, GpuError, MorphApp, MorphContext, MorphError, RenderStatus,
    SharedStorageError,
};

static SEEN: Mutex<Vec<(&'static str, AllFilesAccess)>> = Mutex::new(Vec::new());

fn record(hook: &'static str, ctx: &MorphContext) {
    let state = ctx.all_files_access().expect("scripted runs always answer");
    SEEN.lock().unwrap().push((hook, state));
}

struct Gallery;

impl MorphApp for Gallery {
    fn init(ctx: &mut MorphContext) -> Self {
        record("init", ctx);
        assert_eq!(ctx.open_all_files_access_settings(), Ok(()));
        assert_eq!(ctx.external_storage_dir(), None);
        assert_eq!(
            ctx.scan_media(&[Path::new("/storage/emulated/0/DCIM/a.jpg")]),
            Ok(())
        );
        assert!(matches!(
            ctx.scan_media(&[Path::new("DCIM/a.jpg")]),
            Err(SharedStorageError::BadPaths(_))
        ));
        Self
    }

    fn update(&mut self, ctx: &mut MorphContext, _dt: Duration) {
        record("update", ctx);
    }

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) -> RenderStatus {
        RenderStatus::Skipped
    }

    fn on_resume(&mut self, ctx: &mut MorphContext) {
        record("resume", ctx);
    }
}

const SCRIPT: &str = "\
frames 1
all-files-access missing
frames 1
suspend
all-files-access granted
resume
frames 1
all-files-access not-required
frames 1
";

#[test]
fn scripted_access_follows_the_verb() {
    let dir = std::env::temp_dir().join(format!("libmorph-shared-storage-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["gallery".into()],
    };
    match libmorph::script::run::<Gallery>(&Script::parse(SCRIPT).unwrap(), run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            return;
        }
        result => {
            result.unwrap();
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    use AllFilesAccess::{Granted, Missing, NotRequired};
    assert_eq!(
        *SEEN.lock().unwrap(),
        [
            ("init", NotRequired),
            ("update", NotRequired),
            ("update", Missing),
            ("resume", Granted),
            ("update", Granted),
            ("update", NotRequired),
        ]
    );
}
