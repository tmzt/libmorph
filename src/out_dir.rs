//! Where a scripted run writes its files: an exact directory, or the next
//! `YYYYMMDD_NN_<stem>` under a root, named like the plans in `docs/design`.

use std::io;
use std::path::{Path, PathBuf};

/// The default root for sequenced output, relative to the current directory.
pub const DEFAULT_OUT_ROOT: &str = "out/morph";

/// A run's output directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutDir {
    /// `<root>/YYYYMMDD_NN_<stem>/`: the date the run starts (UTC) and the
    /// next free sequence number for that date.
    Sequenced { root: PathBuf, stem: String },
    /// Used as given; created with any missing parents.
    Exact(PathBuf),
}

impl OutDir {
    /// The default for a script file: `out/morph/YYYYMMDD_NN_<file stem>/`.
    #[must_use]
    pub fn for_script(script: &Path) -> Self {
        let stem = script
            .file_stem()
            .map_or_else(|| "script".into(), |stem| stem.to_string_lossy());
        Self::Sequenced {
            root: DEFAULT_OUT_ROOT.into(),
            stem: stem.into_owned(),
        }
    }

    /// Create the directory for a run starting now.
    // The scripted runner (plan 04, step 5) is the user outside the tests.
    #[allow(dead_code)]
    pub(crate) fn create(&self) -> io::Result<PathBuf> {
        self.create_on(utc_today())
    }

    fn create_on(&self, (year, month, day): (i64, u32, u32)) -> io::Result<PathBuf> {
        match self {
            Self::Exact(dir) => {
                std::fs::create_dir_all(dir)?;
                Ok(dir.clone())
            }
            Self::Sequenced { root, stem } => {
                std::fs::create_dir_all(root)?;
                let date = format!("{year:04}{month:02}{day:02}");
                let next = highest_sequence(root, &date)? + 1;
                create_from(root, &date, &clean_stem(stem), next)
            }
        }
    }
}

/// The highest `NN` among entries named `<date>_NN` or `<date>_NN_…`, or 0.
fn highest_sequence(root: &Path, date: &str) -> io::Result<u32> {
    let mut highest = 0;
    for entry in std::fs::read_dir(root)? {
        let name = entry?.file_name();
        let Some(rest) = name
            .to_str()
            .and_then(|name| name.strip_prefix(date))
            .and_then(|rest| rest.strip_prefix('_'))
        else {
            continue;
        };
        let digits = rest.split('_').next().unwrap_or_default();
        if digits.len() >= 2
            && digits.bytes().all(|b| b.is_ascii_digit())
            && let Ok(n) = digits.parse::<u32>()
        {
            highest = highest.max(n);
        }
    }
    Ok(highest)
}

/// Create `<date>_NN_<stem>` from `first` on. `create_dir` fails on an
/// existing directory, so a number another run took meanwhile is skipped.
fn create_from(root: &Path, date: &str, stem: &str, first: u32) -> io::Result<PathBuf> {
    for n in first.. {
        let dir = root.join(format!("{date}_{n:02}_{stem}"));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("no free sequence number"))
}

/// Characters outside `[A-Za-z0-9_-]` become `-`.
fn clean_stem(stem: &str) -> String {
    let stem: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if stem.is_empty() {
        "script".into()
    } else {
        stem
    }
}

/// Today's date in UTC, from the system clock.
fn utc_today() -> (i64, u32, u32) {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    civil_from_days(i64::try_from(seconds / 86_400).unwrap_or(0))
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian
/// calendar (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11], March first
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::{OutDir, civil_from_days, clean_stem, create_from};
    use std::path::{Path, PathBuf};

    const TODAY: (i64, u32, u32) = (2026, 10, 14);

    /// A fresh, empty directory for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("libmorph-out-dir-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sequenced(root: &Path, stem: &str) -> OutDir {
        OutDir::Sequenced {
            root: root.into(),
            stem: stem.into(),
        }
    }

    fn name(dir: &Path) -> String {
        dir.file_name().unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn first_run_of_a_day() {
        let root = scratch("first").join("out/morph");
        let dir = sequenced(&root, "touch").create_on(TODAY).unwrap();
        assert_eq!(dir, root.join("20261014_01_touch"));
        assert!(dir.is_dir());
        let dir = sequenced(&root, "touch").create_on(TODAY).unwrap();
        assert_eq!(name(&dir), "20261014_02_touch");
        let _ = std::fs::remove_dir_all(root.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn next_after_the_highest_existing() {
        let root = scratch("next");
        for existing in [
            "20261014_01_a",
            "20261014_02",
            "20261014_05_b",
            "20261013_09_yesterday",
            "20261014_7_one-digit",
            "20261014_xx_letters",
            "notes.txt",
        ] {
            std::fs::create_dir(root.join(existing)).unwrap();
        }
        let dir = sequenced(&root, "c").create_on(TODAY).unwrap();
        assert_eq!(name(&dir), "20261014_06_c", "after a gap");

        std::fs::create_dir(root.join("20261014_99_d")).unwrap();
        let dir = sequenced(&root, "e").create_on(TODAY).unwrap();
        assert_eq!(name(&dir), "20261014_100_e", "three digits after 99");
        let dir = sequenced(&root, "f").create_on(TODAY).unwrap();
        assert_eq!(name(&dir), "20261014_101_f");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_taken_number_is_skipped() {
        let root = scratch("taken");
        std::fs::create_dir(root.join("20261014_03_x")).unwrap();
        // Another run took 03 between the scan and the create.
        let dir = create_from(&root, "20261014", "x", 3).unwrap();
        assert_eq!(name(&dir), "20261014_04_x");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn exact_is_created_with_parents_and_reused() {
        let root = scratch("exact");
        let target = root.join("a/b/c");
        let out = OutDir::Exact(target.clone());
        assert_eq!(out.create_on(TODAY).unwrap(), target);
        std::fs::write(target.join("keep.txt"), "x").unwrap();
        assert_eq!(out.create_on(TODAY).unwrap(), target);
        assert!(target.join("keep.txt").exists(), "nothing else is touched");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stems() {
        assert_eq!(clean_stem("touch"), "touch");
        assert_eq!(clean_stem("my script.v2"), "my-script-v2");
        assert_eq!(clean_stem("dé_jà-vu"), "d-_j--vu");
        assert_eq!(clean_stem(""), "script");
        assert_eq!(
            OutDir::for_script(Path::new("examples/scripts/touch.morph")),
            OutDir::Sequenced {
                root: "out/morph".into(),
                stem: "touch".into(),
            }
        );
    }

    #[test]
    fn days_to_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(10_956), (1999, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(20_740), (2026, 10, 14));
        assert_eq!(civil_from_days(47_540), (2100, 2, 28));
        assert_eq!(
            civil_from_days(47_541),
            (2100, 3, 1),
            "2100 is not a leap year"
        );
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }
}
