//! Memory pressure: Android's trim levels, as libmorph's own type.

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
    use super::TrimLevel;

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
