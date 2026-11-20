//! Disk pressure in a scripted run (plan `20261119_02`, step 1): with
//! `DISK_WATCH`, `on_disk_pressure` hears the starting grade after `init`
//! and then exactly one call per change, as `disk-space` crosses `Low`,
//! `ExtremelyLow` and `Critical` and back, also while suspended; `check_disk_now` grades the
//! scripted numbers. With no current reading (`disk-space unreadable`, or
//! none for twice the interval) the grade is `Unknown`, told once, never
//! `Low` or `Critical`. Without it the hook is never called. Needs a GPU
//! adapter; without one the test says so and passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use libmorph::disk::{DiskPressure, DiskPressureReport, DiskWatch};
use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{Frame, GpuError, MorphApp, MorphContext, MorphError, RenderStatus};

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// Each hook's grade, its free bytes, and `disk_pressure()` during it.
static WATCHED: Mutex<Vec<(DiskPressure, Option<u64>, DiskPressure)>> = Mutex::new(Vec::new());
static UPDATES: Mutex<u32> = Mutex::new(0);
static CHECKED: Mutex<Vec<DiskPressure>> = Mutex::new(Vec::new());

struct Watched;

impl MorphApp for Watched {
    const DISK_WATCH: Option<DiskWatch> = Some(DiskWatch::DEFAULT);

    fn init(ctx: &mut MorphContext) -> Self {
        // The starting grade comes after `init`.
        assert!(WATCHED.lock().unwrap().is_empty());
        let space = ctx.disk_space(Path::new("/anywhere")).unwrap();
        assert_eq!((space.free, space.total), (64 * GIB, 128 * GIB));
        Self
    }

    fn update(&mut self, ctx: &mut MorphContext, _dt: Duration) {
        // The first three frames only: a check is a reading, and the end of
        // the script lets the last one go stale.
        let mut updates = UPDATES.lock().unwrap();
        *updates += 1;
        if *updates > 3 {
            return;
        }
        let report = ctx.check_disk_now().unwrap();
        assert_eq!(report.pressure, ctx.disk_pressure());
        CHECKED.lock().unwrap().push(report.pressure);
    }

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) -> RenderStatus {
        RenderStatus::Skipped
    }

    fn on_disk_pressure(&mut self, ctx: &mut MorphContext, report: &DiskPressureReport) {
        WATCHED.lock().unwrap().push((
            report.pressure,
            report.space.map(|space| space.free),
            ctx.disk_pressure(),
        ));
    }
}

// On 64 GiB, Low is under 3.2 GiB (3276.8 MiB), ExtremelyLow under
// 1.28 GiB (1310.72 MiB), Critical under 512 MiB, and each clears 256 MiB
// above its threshold. The watch's 60 s interval makes a reading stale
// after 120 s: 7200 frames at 60 fps.
const SCRIPT: &str = "\
frames 1
disk-space 10GiB 64GiB
disk-space 3GiB 64GiB
disk-space 2900MiB 64GiB
frames 1
disk-space 200MiB 64GiB
disk-space 700MiB 64GiB
disk-space 1000MiB 64GiB
disk-space 3000MiB 64GiB
suspend
disk-space 3300MiB 64GiB
disk-space 4GiB 64GiB
resume
frames 1
disk-space unreadable
disk-space unreadable
disk-space 3GiB 64GiB
frames 7300
";

fn run<A: MorphApp>(name: &str) -> bool {
    let dir = std::env::temp_dir().join(format!("libmorph-disk-{name}-{}", std::process::id()));
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (32, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec![name.into()],
    };
    let ran = match libmorph::script::run::<A>(&Script::parse(SCRIPT).unwrap(), run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            false
        }
        result => {
            result.unwrap();
            true
        }
    };
    let _ = std::fs::remove_dir_all(&dir);
    ran
}

#[test]
fn one_hook_per_change() {
    if !run::<Watched>("watched") {
        return;
    }
    use DiskPressure::{Critical, ExtremelyLow, Low, None, Unknown};
    assert_eq!(
        *WATCHED.lock().unwrap(),
        [
            (None, Some(64 * GIB), None),
            (Low, Some(3 * GIB), Low),
            (Critical, Some(200 * MIB), Critical),
            // 700 MiB is short of 512 + 256 MiB: still Critical.
            (ExtremelyLow, Some(1000 * MIB), ExtremelyLow),
            // 3000 MiB clears ExtremelyLow (1566.72 MiB), not Low.
            (Low, Some(3000 * MIB), Low),
            (None, Some(4 * GIB), None),
            // The read failed: told once, with no reading.
            (Unknown, Option::None, Unknown),
            // Low again only from a reading.
            (Low, Some(3 * GIB), Low),
            // No reading for 120 s.
            (Unknown, Option::None, Unknown),
        ]
    );
    assert_eq!(*CHECKED.lock().unwrap(), [None, Low, None]);
}

static UNWATCHED: Mutex<u32> = Mutex::new(0);

struct Unwatched;

impl MorphApp for Unwatched {
    fn init(_ctx: &mut MorphContext) -> Self {
        Self
    }

    fn update(&mut self, ctx: &mut MorphContext, _dt: Duration) {
        assert_eq!(ctx.disk_pressure(), DiskPressure::None);
        // Graded with the default thresholds, changing nothing.
        let report = ctx.check_disk_now().unwrap();
        assert!([64 * GIB, 128 * GIB].contains(&report.space.unwrap().total));
        assert_eq!(ctx.disk_pressure(), DiskPressure::None);
    }

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) -> RenderStatus {
        RenderStatus::Skipped
    }

    fn on_disk_pressure(&mut self, _ctx: &mut MorphContext, _report: &DiskPressureReport) {
        *UNWATCHED.lock().unwrap() += 1;
    }
}

#[test]
fn no_watch_no_hook() {
    if !run::<Unwatched>("unwatched") {
        return;
    }
    assert_eq!(*UNWATCHED.lock().unwrap(), 0);
}
