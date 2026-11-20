//! Memory pressure: Android's trim levels, as libmorph's own type, and
//! the three grades of what an app should drop (plan `20261119_03`).

/// How hard Android is asking the app to trim memory (`onTrimMemory`).
/// Levels are ordered, so `level >= TrimLevel::Background` means "free
/// everything you can".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum TrimLevel {
    /// `TRIM_MEMORY_RUNNING_MODERATE`: running, the device is getting low.
    RunningModerate,
    /// `TRIM_MEMORY_RUNNING_LOW`.
    RunningLow,
    /// `TRIM_MEMORY_RUNNING_CRITICAL`.
    RunningCritical,
    /// `TRIM_MEMORY_UI_HIDDEN`: the app's UI is no longer visible.
    UiHidden,
    /// `TRIM_MEMORY_BACKGROUND`: in the background, on the list of
    /// processes to kill.
    Background,
    /// `TRIM_MEMORY_MODERATE`.
    Moderate,
    /// `TRIM_MEMORY_COMPLETE`: among the first to be killed.
    Complete,
}

impl TrimLevel {
    /// The script command's name for the level (`trim-memory <name>`).
    pub(crate) fn from_script(word: &str) -> Option<Self> {
        Some(match word {
            "running-moderate" => Self::RunningModerate,
            "running-low" => Self::RunningLow,
            "running-critical" => Self::RunningCritical,
            "ui-hidden" => Self::UiHidden,
            "background" => Self::Background,
            "moderate" => Self::Moderate,
            "complete" => Self::Complete,
            _ => return None,
        })
    }
}

/// What an app should drop, in increasing order. Graded from Android's
/// trim levels, whose own order is not the order of what they ask:
/// [`TrimLevel::RunningCritical`] sorts below [`TrimLevel::UiHidden`] but
/// asks for more. libmorph frees nothing for the app at any grade; what
/// each grade drops is the app's decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum MemoryPressure {
    /// The UI is hidden, or the device is getting low while running: keep
    /// what is near; drop what is far.
    UiHidden,
    /// In the background, or running low: keep only what is close to the
    /// current view.
    Background,
    /// Low memory, a critical running level or `Complete`: keep only what
    /// is on screen, so the process is not killed.
    Low,
}

impl MemoryPressure {
    /// [`on_low_memory`](crate::MorphApp::on_low_memory)'s grade.
    pub const LOW_MEMORY: Self = Self::Low;

    /// The grade for a trim level; `None` for a level libmorph adds later.
    ///
    /// | `TrimLevel` | `MemoryPressure` |
    /// |---|---|
    /// | `RunningModerate`, `UiHidden` | `UiHidden` |
    /// | `RunningLow`, `Background`, `Moderate` | `Background` |
    /// | `RunningCritical`, `Complete` | `Low` |
    #[must_use]
    pub const fn from_trim(level: TrimLevel) -> Option<Self> {
        // `TrimLevel` is `#[non_exhaustive]` only outside this crate: the
        // match is exhaustive here, so a new level fails to compile until
        // it is graded, and the `Option` keeps that grading free to say
        // `None`.
        Some(match level {
            TrimLevel::RunningModerate | TrimLevel::UiHidden => Self::UiHidden,
            TrimLevel::RunningLow | TrimLevel::Background | TrimLevel::Moderate => Self::Background,
            TrimLevel::RunningCritical | TrimLevel::Complete => Self::Low,
        })
    }
}

/// libmobile's level as libmorph's; `None` for a level libmobile adds
/// later, which is logged and not passed on.
#[cfg(feature = "mobile")]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn from_mobile(level: libmobile::TrimLevel) -> Option<TrimLevel> {
    use libmobile::TrimLevel as M;
    Some(match level {
        M::RunningModerate => TrimLevel::RunningModerate,
        M::RunningLow => TrimLevel::RunningLow,
        M::RunningCritical => TrimLevel::RunningCritical,
        M::UiHidden => TrimLevel::UiHidden,
        M::Background => TrimLevel::Background,
        M::Moderate => TrimLevel::Moderate,
        M::Complete => TrimLevel::Complete,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{MemoryPressure, TrimLevel};

    #[test]
    fn each_level_has_flankrs_grade() {
        let table = [
            (TrimLevel::RunningModerate, MemoryPressure::UiHidden),
            (TrimLevel::UiHidden, MemoryPressure::UiHidden),
            (TrimLevel::RunningLow, MemoryPressure::Background),
            (TrimLevel::Background, MemoryPressure::Background),
            (TrimLevel::Moderate, MemoryPressure::Background),
            (TrimLevel::RunningCritical, MemoryPressure::Low),
            (TrimLevel::Complete, MemoryPressure::Low),
        ];
        for (level, grade) in table {
            assert_eq!(MemoryPressure::from_trim(level), Some(grade), "{level:?}");
        }
        assert_eq!(MemoryPressure::LOW_MEMORY, MemoryPressure::Low);
    }

    /// flankr's test: the running-critical level sorts below `UiHidden` as
    /// a trim level, and grades above it.
    #[test]
    fn running_critical_grades_above_ui_hidden() {
        assert!(TrimLevel::RunningCritical < TrimLevel::UiHidden);
        assert!(
            MemoryPressure::from_trim(TrimLevel::RunningCritical)
                > MemoryPressure::from_trim(TrimLevel::UiHidden)
        );
        assert!(MemoryPressure::UiHidden < MemoryPressure::Background);
        assert!(MemoryPressure::Background < MemoryPressure::Low);
    }

    #[test]
    fn levels_are_ordered_and_named() {
        let names = [
            "running-moderate",
            "running-low",
            "running-critical",
            "ui-hidden",
            "background",
            "moderate",
            "complete",
        ];
        let levels: Vec<TrimLevel> = names
            .iter()
            .map(|name| TrimLevel::from_script(name).unwrap())
            .collect();
        assert!(levels.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(TrimLevel::from_script("UI_HIDDEN"), None);
    }

    #[cfg(feature = "mobile")]
    #[test]
    fn libmobile_levels_map_one_to_one() {
        use libmobile::TrimLevel as M;
        let pairs = [
            (M::RunningModerate, TrimLevel::RunningModerate),
            (M::RunningLow, TrimLevel::RunningLow),
            (M::RunningCritical, TrimLevel::RunningCritical),
            (M::UiHidden, TrimLevel::UiHidden),
            (M::Background, TrimLevel::Background),
            (M::Moderate, TrimLevel::Moderate),
            (M::Complete, TrimLevel::Complete),
        ];
        for (mobile, ours) in pairs {
            assert_eq!(super::from_mobile(mobile), Some(ours));
        }
    }
}
