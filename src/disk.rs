//! **Disk space and disk pressure** (plan
//! `20261119_02_DISK_PRESSURE_PLAN.md`): the space of a volume, and a
//! grade of how short of it the app's volume is, told to the app when it
//! changes.
//!
//! libmorph owns the grading. The platforms give raw readings only:
//!
//! * **Android** (feature `mobile-disk-space`): libmobile's
//!   `android::disk_space`. With [`DiskWatch`] set, libmobile's Kotlin timer
//!   reads the volume on resume and every interval while resumed, off the
//!   loop thread, and posts each reading; libmorph grades it here.
//!   Without the feature: [`DiskError::Unsupported`], and no watch.
//! * **The desktop:** libdevice's `disk_space::space` (`statvfs`), read by
//!   libmorph's own timer, one `Instant` comparison per loop iteration,
//!   not run while suspended or minimised, and once on resume.
//! * **The web:** [`DiskError::Unsupported`], and no watch.
//! * **Scripted runs:** the `disk-space <free> <total>` verb sets the
//!   numbers (64 GiB free of 128 GiB until it does) and grades them; no
//!   timer.
//!
//! The grade is advisory. libmorph deletes, trims and refuses nothing at
//! any grade; what to drop or defer is the app's decision.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use web_time::Instant;

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// A volume's space, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DiskSpace {
    /// The bytes an unprivileged process may still write (`statvfs`'s
    /// `f_bavail * f_frsize`).
    pub free: u64,
    /// The volume's size (`f_blocks * f_frsize`).
    pub total: u64,
    /// Android: `StorageManager.getAllocatableBytes`, what the system
    /// could give the app if it also cleared other apps' caches. `None`
    /// elsewhere, in the watch's readings, and when Android could not say.
    pub allocatable: Option<u64>,
    /// Android: `StorageManager.getCacheQuotaBytes`, how much cached data
    /// this app may keep before its cache is cleared first. `None` as for
    /// `allocatable`.
    pub cache_quota: Option<u64>,
}

impl DiskSpace {
    /// `free` and `total` alone.
    #[must_use]
    pub(crate) const fn new(free: u64, total: u64) -> Self {
        Self {
            free,
            total,
            allocatable: None,
            cache_quota: None,
        }
    }
}

/// How short of space a volume is, in increasing order. `Unknown`, when
/// there is no current reading, sorts lowest: `pressure >= Low` is never
/// true for it, so an app trims nothing on missing data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum DiskPressure {
    /// No current reading of the watched volume: none since the start, the
    /// last one failed, or the latest is older than twice the watch's
    /// interval (on Android, while the app is paused or the phone locked;
    /// on the desktop, while minimised or suspended). Never graded `Low`
    /// or `Critical` from missing data.
    Unknown,
    /// Enough space.
    None,
    /// Under the low threshold: trim caches, put off large writes.
    Low,
    /// Under the critical threshold: write only what cannot wait.
    Critical,
}

/// Where the grades start and how far a volume must recover to leave one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskThresholds {
    /// `Low` under `max(low_min, total * low_fraction_permille / 1000)`.
    pub low_min: u64,
    /// See `low_min`; 50 is 5 %.
    pub low_fraction_permille: u32,
    /// `Critical` under this many free bytes.
    pub critical: u64,
    /// A grade clears only once free space is this far above its
    /// threshold, so a volume hovering at one does not flap.
    pub hysteresis: u64,
}

impl DiskThresholds {
    /// `Low` under max(1 GiB, 5 % of total), `Critical` under 256 MiB, a
    /// grade clearing 256 MiB above its threshold.
    pub const DEFAULT: Self = Self::new(GIB, 50, 256 * MIB, 256 * MIB);

    #[must_use]
    pub const fn new(
        low_min: u64,
        low_fraction_permille: u32,
        critical: u64,
        hysteresis: u64,
    ) -> Self {
        Self {
            low_min,
            low_fraction_permille,
            critical,
            hysteresis,
        }
    }

    /// The free bytes under which a volume of `total` bytes is `Low`.
    #[must_use]
    pub const fn low_threshold(&self, total: u64) -> u64 {
        let fraction = (total as u128 * self.low_fraction_permille as u128 / 1000) as u64;
        if fraction > self.low_min {
            fraction
        } else {
            self.low_min
        }
    }

    /// The grade for `free` of `total` bytes, after `prev`. A rise is
    /// immediate; a fall only once `free` is at least the threshold of
    /// the grade being left plus [`hysteresis`](Self::hysteresis). After
    /// `Unknown` the reading's grade stands alone. Never `Unknown`.
    #[must_use]
    pub const fn grade(&self, prev: DiskPressure, free: u64, total: u64) -> DiskPressure {
        let low = self.low_threshold(total);
        let raw = if free < self.critical {
            DiskPressure::Critical
        } else if free < low {
            DiskPressure::Low
        } else {
            DiskPressure::None
        };
        let clears_low = free >= low.saturating_add(self.hysteresis);
        let clears_critical = free >= self.critical.saturating_add(self.hysteresis);
        match (prev, raw) {
            // From no reading, the reading's own grade: nothing to clear.
            (_, DiskPressure::Critical) | (DiskPressure::None | DiskPressure::Unknown, _) => raw,
            (DiskPressure::Low, DiskPressure::Low) => DiskPressure::Low,
            (DiskPressure::Low, _) if clears_low => DiskPressure::None,
            (DiskPressure::Low, _) => DiskPressure::Low,
            (DiskPressure::Critical, _) if clears_low => DiskPressure::None,
            (DiskPressure::Critical, _) if clears_critical => DiskPressure::Low,
            (DiskPressure::Critical, _) => DiskPressure::Critical,
        }
    }
}

impl Default for DiskThresholds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A grade and the reading it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DiskPressureReport {
    pub pressure: DiskPressure,
    /// The reading; `None` exactly when `pressure` is `Unknown`.
    pub space: Option<DiskSpace>,
}

/// Why a disk space reading failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiskError {
    /// The path does not exist or cannot be reached.
    Path(String),
    /// No reading on this host or build: the web, Windows, Android without
    /// `mobile-disk-space`.
    Unsupported,
    /// Anything else the platform reported.
    Platform(String),
}

impl fmt::Display for DiskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(detail) => write!(f, "disk space: bad path: {detail}"),
            Self::Unsupported => f.write_str("reading disk space is not supported on this host"),
            Self::Platform(detail) => write!(f, "reading disk space failed: {detail}"),
        }
    }
}

impl std::error::Error for DiskError {}

/// Which of the app's directories names the watched volume.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiskVolume {
    /// [`data_dir`](crate::MorphContext::data_dir)'s volume.
    #[default]
    DataDir,
    /// [`cache_dir`](crate::MorphContext::cache_dir)'s volume.
    CacheDir,
}

/// What [`DISK_WATCH`](crate::MorphApp::DISK_WATCH) asks libmorph to watch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskWatch {
    /// Which of the app's directories names the volume.
    pub volume: DiskVolume,
    pub thresholds: DiskThresholds,
    /// How often the volume is read while the app is resumed: 60 s by
    /// default, clamped to 15 s ..= 600 s
    /// ([`interval_clamped`](Self::interval_clamped)).
    pub interval: Duration,
}

impl DiskWatch {
    /// The shortest interval used.
    pub const MIN_INTERVAL: Duration = Duration::from_secs(15);
    /// The longest interval used.
    pub const MAX_INTERVAL: Duration = Duration::from_secs(600);

    /// The data directory's volume, [`DiskThresholds::DEFAULT`], every 60 s.
    pub const DEFAULT: Self = Self {
        volume: DiskVolume::DataDir,
        thresholds: DiskThresholds::DEFAULT,
        interval: Duration::from_secs(60),
    };

    /// [`interval`](Self::interval) within 15 s ..= 600 s.
    #[must_use]
    pub fn interval_clamped(&self) -> Duration {
        self.interval.clamp(Self::MIN_INTERVAL, Self::MAX_INTERVAL)
    }
}

impl Default for DiskWatch {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The scripted volume before any `disk-space` command: 64 GiB free of
/// 128 GiB, `None`.
#[cfg_attr(not(script_runner), allow(dead_code))]
pub(crate) const SCRIPTED_SPACE: DiskSpace = DiskSpace::new(64 * GIB, 128 * GIB);

/// The context's disk state: the watch, its volume, the last grade, and
/// the reports waiting for `on_disk_pressure`.
#[derive(Debug)]
pub(crate) struct DiskState {
    /// `A::DISK_WATCH`, set before `init`.
    pub(crate) watch: Option<DiskWatch>,
    /// The watched directory, resolved after `init`.
    pub(crate) path: Option<PathBuf>,
    /// `Unknown` until the first successful reading.
    pub(crate) pressure: DiskPressure,
    /// When the last successful reading was taken.
    last_reading: Option<Instant>,
    /// The last grade told to `on_disk_pressure`; `None` until the
    /// starting report. The hook hears an automatic reading's grade only
    /// when it differs from this.
    told: Option<DiskPressure>,
    /// Changes not yet delivered to the hook, oldest first.
    pending: Vec<DiskPressureReport>,
    /// The scripted volume (scripted runs); `None` after `disk-space
    /// unreadable`.
    #[cfg_attr(not(script_runner), allow(dead_code))]
    pub(crate) scripted: Option<DiskSpace>,
    /// Scripted runs' clock (the shell's fixed frame time), so a reading's
    /// age follows the script; `None` elsewhere, where it is the wall clock.
    pub(crate) scripted_clock: Option<Instant>,
    /// The desktop timer: when the volume is next read.
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    pub(crate) next_check: Option<Instant>,
}

impl Default for DiskState {
    fn default() -> Self {
        Self {
            watch: None,
            path: None,
            pressure: DiskPressure::Unknown,
            last_reading: None,
            told: None,
            pending: Vec::new(),
            scripted: Some(SCRIPTED_SPACE),
            scripted_clock: None,
            #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
            next_check: None,
        }
    }
}

impl DiskState {
    /// The time a reading is stamped with.
    pub(crate) fn now(&self) -> Instant {
        self.scripted_clock.unwrap_or_else(Instant::now)
    }

    /// A reading of the watched volume, or its failure, at `now`. A
    /// reading is graded against the last grade; a failure makes the grade
    /// `Unknown`. Either way [`pressure`](Self::pressure) follows at once.
    /// Only an `automatic` reading (the platform's timer, the first one
    /// after `init`, a scripted `disk-space`) is told to the hook, and only
    /// when its grade differs from the last one told; `check_disk_now`'s is
    /// not.
    pub(crate) fn reading(
        &mut self,
        result: Result<DiskSpace, DiskError>,
        now: Instant,
        automatic: bool,
    ) -> Result<DiskPressureReport, DiskError> {
        match result {
            Ok(space) => {
                let thresholds = self.watch.map_or(DiskThresholds::DEFAULT, |w| w.thresholds);
                let pressure = thresholds.grade(self.pressure, space.free, space.total);
                self.last_reading = Some(now);
                let report = DiskPressureReport {
                    pressure,
                    space: Some(space),
                };
                self.set(report, automatic);
                Ok(report)
            }
            Err(error) => {
                log::warn!("libmorph: no disk reading ({error}): the grade is Unknown");
                self.last_reading = None;
                self.set(Self::UNKNOWN, automatic);
                Err(error)
            }
        }
    }

    const UNKNOWN: DiskPressureReport = DiskPressureReport {
        pressure: DiskPressure::Unknown,
        space: None,
    };

    /// When the current grade turns `Unknown` for want of a newer reading:
    /// twice the watch's interval after the last one. `None` while it is
    /// `Unknown` or there is no watch.
    pub(crate) fn stale_at(&self) -> Option<Instant> {
        let watch = self.watch?;
        if self.pressure == DiskPressure::Unknown {
            return None;
        }
        Some(self.last_reading? + watch.interval_clamped() * 2)
    }

    /// The grade turns `Unknown` once the last reading is older than twice
    /// the interval; the hook hears that.
    pub(crate) fn expire(&mut self, now: Instant) {
        if self.stale_at().is_some_and(|at| now >= at) {
            log::debug!("libmorph: the last disk reading is stale: the grade is Unknown");
            self.last_reading = None;
            self.set(Self::UNKNOWN, true);
        }
    }

    fn set(&mut self, report: DiskPressureReport, automatic: bool) {
        if report.pressure != self.pressure {
            match report.space {
                Some(space) => log::debug!(
                    "libmorph: disk pressure {:?} ({} of {} bytes free)",
                    report.pressure,
                    space.free,
                    space.total
                ),
                None => log::debug!("libmorph: disk pressure {:?}", report.pressure),
            }
        }
        self.pressure = report.pressure;
        if automatic && self.told != Some(report.pressure) {
            self.told = Some(report.pressure);
            self.pending.push(report);
        }
    }

    /// The reports waiting for `on_disk_pressure`, oldest first.
    pub(crate) fn take_pending(&mut self) -> Vec<DiskPressureReport> {
        std::mem::take(&mut self.pending)
    }
}

/// What a scripted read answers after `disk-space unreadable`.
#[cfg_attr(not(script_runner), allow(dead_code))]
pub(crate) fn scripted_unreadable() -> DiskError {
    DiskError::Platform("scripted: the volume cannot be read".into())
}

/// `<n><unit>` with the unit `B`, `KiB`, `MiB` or `GiB` (the script verb).
#[cfg_attr(not(script_runner), allow(dead_code))]
pub(crate) fn parse_size(word: &str) -> Option<u64> {
    let split = word.find(|c: char| !c.is_ascii_digit())?;
    let (digits, unit) = word.split_at(split);
    let unit = match unit {
        "B" => 1,
        "KiB" => 1 << 10,
        "MiB" => MIB,
        "GiB" => GIB,
        _ => return None,
    };
    digits.parse::<u64>().ok()?.checked_mul(unit)
}

/// The watch's reading on the loop thread (`check_disk_now`, the starting
/// grade, the desktop timer): `statvfs` only on Android, never
/// `StorageManager`.
pub(crate) use platform::quick_space;
/// The reading for `path` on this host (not scripted runs).
pub(crate) use platform::space;

#[cfg(all(target_os = "android", feature = "mobile-disk-space"))]
mod platform {
    use super::{DiskError, DiskSpace};
    use std::path::Path;

    fn from_mobile_error(error: libmobile::disk_space::DiskSpaceError) -> DiskError {
        use libmobile::disk_space::DiskSpaceError as E;
        match error {
            E::Path(detail) => DiskError::Path(detail),
            E::Platform(detail) => DiskError::Platform(detail),
            other => DiskError::Platform(other.to_string()),
        }
    }

    pub(crate) fn from_mobile(space: libmobile::disk_space::DiskSpace) -> DiskSpace {
        DiskSpace {
            free: space.free,
            total: space.total,
            allocatable: space.allocatable,
            cache_quota: space.cache_quota,
        }
    }

    pub(crate) fn space(path: &Path) -> Result<DiskSpace, DiskError> {
        libmobile::android::disk_space::space(path)
            .map(from_mobile)
            .map_err(from_mobile_error)
    }

    pub(crate) fn quick_space(path: &Path) -> Result<DiskSpace, DiskError> {
        libmobile::disk_space::statvfs_space(path)
            .map(from_mobile)
            .map_err(from_mobile_error)
    }

    /// Start libmobile's timer on the watched volume.
    pub(crate) fn watch(path: &Path, interval: std::time::Duration) {
        match libmobile::android::disk_space::watch(Some(path), interval) {
            Ok(()) => log::debug!(
                "libmorph: watching {} every {} s",
                path.display(),
                interval.as_secs()
            ),
            Err(error) => log::error!("libmorph: the disk watch could not start: {error}"),
        }
    }
}

#[cfg(all(target_os = "android", feature = "mobile-disk-space"))]
pub(crate) use platform::{from_mobile, watch};

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod platform {
    use super::{DiskError, DiskSpace};
    use std::path::Path;

    pub(crate) fn space(path: &Path) -> Result<DiskSpace, DiskError> {
        use libdevice::disk_space::DiskSpaceError as E;
        libdevice::disk_space::space(path)
            .map(|space| DiskSpace::new(space.free, space.total))
            .map_err(|error| match error {
                E::Path(detail) => DiskError::Path(detail),
                E::Unsupported => DiskError::Unsupported,
                E::Platform(detail) => DiskError::Platform(detail),
                other => DiskError::Platform(other.to_string()),
            })
    }

    pub(crate) fn quick_space(path: &Path) -> Result<DiskSpace, DiskError> {
        space(path)
    }
}

/// The web, and Android without `mobile-disk-space`.
#[cfg(any(
    all(target_os = "android", not(feature = "mobile-disk-space")),
    target_arch = "wasm32"
))]
mod platform {
    use super::{DiskError, DiskSpace};
    use std::path::Path;

    pub(crate) fn space(_path: &Path) -> Result<DiskSpace, DiskError> {
        Err(DiskError::Unsupported)
    }

    pub(crate) fn quick_space(_path: &Path) -> Result<DiskSpace, DiskError> {
        Err(DiskError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::{DiskPressure, DiskThresholds, DiskWatch, GIB, MIB, parse_size};
    use std::time::Duration;

    const T: DiskThresholds = DiskThresholds::DEFAULT;
    const HYST: u64 = 256 * MIB;
    const CRITICAL: u64 = 256 * MIB;

    fn grade(prev: DiskPressure, free: u64, total: u64) -> DiskPressure {
        T.grade(prev, free, total)
    }

    #[test]
    fn low_thresholds_of_two_volumes() {
        // 5 % of 64 GiB is 3.2 GiB, above the 1 GiB floor.
        assert_eq!(T.low_threshold(64 * GIB), 64 * GIB / 20);
        // 5 % of 16 GiB is 0.8 GiB: the floor wins.
        assert_eq!(T.low_threshold(16 * GIB), GIB);
        assert_eq!(T.low_threshold(0), GIB);
        assert_eq!(
            T.low_threshold(u64::MAX),
            (u64::MAX as u128 * 50 / 1000) as u64
        );
    }

    #[test]
    fn each_rise_is_immediate() {
        use DiskPressure::{Critical, Low, None};
        for total in [64 * GIB, 16 * GIB] {
            let low = T.low_threshold(total);
            assert_eq!(grade(None, low, total), None, "at the low threshold");
            assert_eq!(grade(None, low - 1, total), Low, "just under it");
            assert_eq!(grade(Low, CRITICAL, total), Low, "at the critical one");
            assert_eq!(grade(Low, CRITICAL - 1, total), Critical, "just under it");
            assert_eq!(
                grade(None, CRITICAL - 1, total),
                Critical,
                "None to Critical"
            );
            assert_eq!(grade(None, 0, total), Critical);
        }
    }

    #[test]
    fn a_fall_waits_for_the_hysteresis() {
        use DiskPressure::{Critical, Low, None};
        for total in [64 * GIB, 16 * GIB] {
            let low = T.low_threshold(total);
            assert_eq!(grade(Low, low + HYST - 1, total), Low, "1 byte short");
            assert_eq!(
                grade(Low, low + HYST, total),
                None,
                "at threshold + hysteresis"
            );
            assert_eq!(grade(Critical, CRITICAL + HYST - 1, total), Critical);
            assert_eq!(grade(Critical, CRITICAL + HYST, total), Low);
            assert_eq!(grade(Critical, low + HYST - 1, total), Low);
            assert_eq!(grade(Critical, low + HYST, total), None, "a large recovery");
            assert_eq!(grade(Critical, total, total), None);
        }
    }

    #[test]
    fn an_oscillation_around_low_changes_once() {
        let total = 64 * GIB;
        let low = T.low_threshold(total);
        let mut pressure = DiskPressure::None;
        let mut changes = 0;
        for step in 0..40 {
            let free = if step % 2 == 0 {
                low - 100 * MIB
            } else {
                low + 100 * MIB
            };
            let next = grade(pressure, free, total);
            if next != pressure {
                changes += 1;
            }
            pressure = next;
        }
        assert_eq!(changes, 1);
        assert_eq!(pressure, DiskPressure::Low);
    }

    #[test]
    fn the_interval_is_clamped() {
        let at = |secs| {
            DiskWatch {
                interval: Duration::from_secs(secs),
                ..DiskWatch::DEFAULT
            }
            .interval_clamped()
        };
        assert_eq!(at(1), Duration::from_secs(15));
        assert_eq!(at(60), Duration::from_secs(60));
        assert_eq!(at(3600), Duration::from_secs(600));
        assert_eq!(DiskWatch::DEFAULT.interval, Duration::from_secs(60));
    }

    /// The desktop's reading (libdevice's `statvfs`), on the host.
    #[cfg(unix)]
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    #[test]
    fn the_desktop_reads_a_volume() {
        let space = super::space(&std::env::temp_dir()).unwrap();
        assert!(space.total > 0 && space.free <= space.total);
        assert_eq!((space.allocatable, space.cache_quota), (None, None));
        assert!(matches!(
            super::space(std::path::Path::new("/no/such/libmorph/dir")),
            Err(super::DiskError::Path(_))
        ));
    }

    #[test]
    fn a_grade_after_unknown_is_the_readings_own() {
        use DiskPressure::{Critical, Low, None, Unknown};
        let total = 64 * GIB;
        let low = T.low_threshold(total);
        assert_eq!(grade(Unknown, low + 1, total), None);
        assert_eq!(grade(Unknown, low - 1, total), Low);
        assert_eq!(grade(Unknown, CRITICAL - 1, total), Critical);
        assert!(Unknown < None && Unknown < Low, "Unknown never reaches Low");
    }

    mod no_reading {
        use super::super::{DiskError, DiskPressure, DiskSpace, DiskState, DiskWatch, GIB, MIB};
        use std::time::Duration;
        use web_time::Instant;

        /// A 15 s watch: a grade goes stale 30 s after its reading.
        fn state() -> DiskState {
            DiskState {
                watch: Some(DiskWatch {
                    interval: Duration::from_secs(15),
                    ..DiskWatch::DEFAULT
                }),
                ..DiskState::default()
            }
        }

        fn grades(state: &mut DiskState) -> Vec<DiskPressure> {
            state.take_pending().iter().map(|r| r.pressure).collect()
        }

        fn critical() -> Result<DiskSpace, DiskError> {
            Ok(DiskSpace::new(100 * MIB, 64 * GIB))
        }

        fn failed() -> Result<DiskSpace, DiskError> {
            Err(DiskError::Platform("no".into()))
        }

        #[test]
        fn none_since_the_start_is_unknown() {
            let state = state();
            assert_eq!(state.pressure, DiskPressure::Unknown);
            assert_eq!(state.stale_at(), None);
        }

        /// The read right after `init` failed: the starting report is
        /// `Unknown`, without a reading, and claims no pressure.
        #[test]
        fn a_failed_starting_read_reports_unknown() {
            let mut state = state();
            let now = Instant::now();
            assert!(state.reading(failed(), now, true).is_err());
            let pending = state.take_pending();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].pressure, DiskPressure::Unknown);
            assert_eq!(pending[0].space, None);
            // A second failure is no change: no hook.
            assert!(state.reading(failed(), now, true).is_err());
            assert!(grades(&mut state).is_empty());
        }

        #[test]
        fn a_read_error_after_a_grade_is_unknown_and_told() {
            let mut state = state();
            let now = Instant::now();
            state.reading(critical(), now, true).unwrap();
            assert!(state.reading(failed(), now, true).is_err());
            assert_eq!(
                grades(&mut state),
                [DiskPressure::Critical, DiskPressure::Unknown]
            );
        }

        /// No reading for twice the interval (Android paused or locked, the
        /// desktop minimised): `Unknown`, told once.
        #[test]
        fn a_stale_reading_is_unknown_and_told_once() {
            let mut state = state();
            let t0 = Instant::now();
            state.reading(critical(), t0, true).unwrap();
            assert_eq!(state.stale_at(), Some(t0 + Duration::from_secs(30)));
            state.expire(t0 + Duration::from_millis(29_999));
            assert_eq!(state.pressure, DiskPressure::Critical);
            state.expire(t0 + Duration::from_secs(30));
            assert_eq!(state.pressure, DiskPressure::Unknown);
            state.expire(t0 + Duration::from_secs(90));
            assert_eq!(
                grades(&mut state),
                [DiskPressure::Critical, DiskPressure::Unknown]
            );
            assert_eq!(state.stale_at(), None);
        }

        /// A fresh reading keeps the grade from going stale; `Critical`
        /// comes back only from a reading.
        #[test]
        fn only_a_reading_grades_again() {
            let mut state = state();
            let t0 = Instant::now();
            state.reading(critical(), t0, true).unwrap();
            state
                .reading(critical(), t0 + Duration::from_secs(20), true)
                .unwrap();
            state.expire(t0 + Duration::from_secs(45));
            assert_eq!(state.pressure, DiskPressure::Critical, "read at 20 s");
            state.expire(t0 + Duration::from_secs(50));
            assert_eq!(state.pressure, DiskPressure::Unknown);
            state
                .reading(critical(), t0 + Duration::from_secs(60), true)
                .unwrap();
            assert_eq!(
                grades(&mut state),
                [
                    DiskPressure::Critical,
                    DiskPressure::Unknown,
                    DiskPressure::Critical
                ]
            );
        }
    }

    mod requested {
        use super::super::{DiskError, DiskPressure, DiskSpace, DiskState, DiskWatch, GIB, MIB};
        use web_time::Instant;

        /// `check_disk_now`'s readings change the grade it reports but are
        /// never told; the next automatic reading is told only if its grade
        /// differs from the last one told.
        #[test]
        fn a_requested_reading_is_never_told() {
            let mut state = DiskState {
                watch: Some(DiskWatch::DEFAULT),
                ..DiskState::default()
            };
            let now = Instant::now();
            let roomy = || Ok::<_, DiskError>(DiskSpace::new(40 * GIB, 64 * GIB));
            let tight = || Ok::<_, DiskError>(DiskSpace::new(100 * MIB, 64 * GIB));
            state.reading(roomy(), now, true).unwrap();
            assert_eq!(state.take_pending().len(), 1, "the starting report");
            state.reading(tight(), now, false).unwrap();
            assert_eq!(state.pressure, DiskPressure::Critical);
            assert!(
                state
                    .reading(Err(DiskError::Unsupported), now, false)
                    .is_err()
            );
            assert_eq!(state.pressure, DiskPressure::Unknown);
            assert!(state.take_pending().is_empty(), "no hook for a request");
            // The timer next sees Critical: the hook last heard None.
            state.reading(tight(), now, true).unwrap();
            let pending = state.take_pending();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].pressure, DiskPressure::Critical);
            // A request finding None, then the timer finding Critical again:
            // nothing new to tell.
            state.reading(roomy(), now, false).unwrap();
            state.reading(tight(), now, true).unwrap();
            assert!(state.take_pending().is_empty());
        }
    }

    #[test]
    fn sizes_take_a_unit() {
        assert_eq!(parse_size("900MiB"), Some(900 * MIB));
        assert_eq!(parse_size("64GiB"), Some(64 * GIB));
        assert_eq!(parse_size("2KiB"), Some(2048));
        assert_eq!(parse_size("7B"), Some(7));
        for bad in ["900", "MiB", "1.5GiB", "9mb", "-1B", "99999999999GiB", ""] {
            assert_eq!(parse_size(bad), None, "{bad}");
        }
    }
}
