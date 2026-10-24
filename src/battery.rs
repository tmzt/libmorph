//! **Battery optimization and app sleeping** (libmobile's battery plan,
//! wrapped under `20261023_01_FOREGROUND_SERVICE_PLAN.md`). The same API on
//! every host: on Android it is libmobile's `android::battery` (feature
//! `mobile-battery`); elsewhere every call fails with
//! [`BatteryError::Unsupported`].
//!
//! An app exempt from battery optimization is left out of some of Android's
//! background limits, such as App Standby and, on some devices, Doze's
//! network limits. On Samsung phones One UI also puts apps to sleep, which
//! is set on Samsung's battery page. What to tell the user is the app's job.
//!
//! Every call is synchronous and may be made from any thread. The calls
//! that open a page need the app to be visible ([`BatteryError::NotVisible`]).
//! Android does not report the dialog's answer; with
//! `mobile-system-events`, [`SystemEvent::BatteryExemption`](crate::system_events::SystemEvent::BatteryExemption)
//! reports a change when the app is visible again.
//!
//! **Google Play** allows [`request_ignore_optimizations`] only for some
//! kinds of app; a build for Play uses [`open_settings`] or
//! [`open_app_details`]. The build script declares the permission it needs
//! with `gradle_project(id).request_ignore_battery_optimizations()`.

use std::fmt;

/// The app's battery settings.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BatteryState {
    /// `PowerManager.isIgnoringBatteryOptimizations(package)`.
    pub ignoring_optimizations: bool,
    /// The "Restricted" battery setting, which stops background work even
    /// with a service (`ActivityManager.isBackgroundRestricted()`).
    pub background_restricted: bool,
    /// `Build.MANUFACTURER`, as reported ("samsung" on Samsung phones).
    pub manufacturer: String,
}

impl BatteryState {
    /// The manufacturer is Samsung, so app sleeping applies.
    #[must_use]
    pub fn is_samsung(&self) -> bool {
        self.manufacturer.trim().eq_ignore_ascii_case("samsung")
    }
}

/// Which settings page opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Page {
    /// Every app's battery optimization setting.
    OptimizationList,
    /// This app's settings page.
    AppDetails,
    /// The vendor's battery page (Samsung: app sleeping).
    Vendor,
}

/// Why a battery call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatteryError {
    /// [`request_ignore_optimizations`] without the build script's
    /// `request_ignore_battery_optimizations()`.
    NotDeclared,
    /// A page was asked for while the app was not visible.
    NotVisible,
    /// No battery settings on this host (anything but Android, or Android
    /// without `mobile-battery`).
    Unsupported,
    /// Anything else the platform reported.
    Platform(String),
}

impl fmt::Display for BatteryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDeclared => {
                f.write_str("REQUEST_IGNORE_BATTERY_OPTIMIZATIONS is not declared")
            }
            Self::NotVisible => {
                f.write_str("a settings page may only be opened while the app is visible")
            }
            Self::Unsupported => f.write_str("no battery settings on this host"),
            Self::Platform(detail) => write!(f, "battery: {detail}"),
        }
    }
}

impl std::error::Error for BatteryError {}

/// The app's battery settings now.
pub fn state() -> Result<BatteryState, BatteryError> {
    imp::state()
}

/// Whether the app is exempt from battery optimization.
pub fn is_ignoring_optimizations() -> Result<bool, BatteryError> {
    imp::is_ignoring_optimizations()
}

/// Show the system's Allow/Deny dialog; nothing (`Ok`) if the app is
/// already exempt. Not for a build for Google Play.
pub fn request_ignore_optimizations() -> Result<(), BatteryError> {
    imp::request_ignore_optimizations()
}

/// Open the list of every app's battery optimization setting.
pub fn open_settings() -> Result<Page, BatteryError> {
    imp::open_settings()
}

/// Open this app's settings page; its Battery entry is one tap away.
pub fn open_app_details() -> Result<Page, BatteryError> {
    imp::open_app_details()
}

/// Open the vendor's battery page if one is known on this device
/// (best-effort), otherwise this app's settings page. Returns which opened.
pub fn open_vendor_battery() -> Result<Page, BatteryError> {
    imp::open_vendor_battery()
}

#[cfg(all(target_os = "android", feature = "mobile-battery"))]
mod imp {
    use super::{BatteryError, BatteryState, Page};
    use libmobile::android::battery as mobile;

    impl From<mobile::BatteryError> for BatteryError {
        fn from(error: mobile::BatteryError) -> Self {
            use mobile::BatteryError as E;
            match error {
                E::NotDeclared => Self::NotDeclared,
                E::NotVisible => Self::NotVisible,
                E::Platform(detail) => Self::Platform(detail),
                other => Self::Platform(other.to_string()),
            }
        }
    }

    impl From<mobile::BatteryState> for BatteryState {
        fn from(state: mobile::BatteryState) -> Self {
            Self {
                ignoring_optimizations: state.ignoring_optimizations,
                background_restricted: state.background_restricted,
                manufacturer: state.manufacturer,
            }
        }
    }

    fn page(page: mobile::Page) -> Page {
        match page {
            mobile::Page::OptimizationList => Page::OptimizationList,
            mobile::Page::Vendor => Page::Vendor,
            // AppDetails, and any page added later: libmobile's fallback.
            _ => Page::AppDetails,
        }
    }

    pub(super) fn state() -> Result<BatteryState, BatteryError> {
        Ok(mobile::state()?.into())
    }

    pub(super) fn is_ignoring_optimizations() -> Result<bool, BatteryError> {
        Ok(mobile::is_ignoring_optimizations()?)
    }

    pub(super) fn request_ignore_optimizations() -> Result<(), BatteryError> {
        Ok(mobile::request_ignore_optimizations()?)
    }

    pub(super) fn open_settings() -> Result<Page, BatteryError> {
        Ok(page(mobile::open_settings()?))
    }

    pub(super) fn open_app_details() -> Result<Page, BatteryError> {
        Ok(page(mobile::open_app_details()?))
    }

    pub(super) fn open_vendor_battery() -> Result<Page, BatteryError> {
        Ok(page(mobile::open_vendor_battery()?))
    }
}

#[cfg(not(all(target_os = "android", feature = "mobile-battery")))]
mod imp {
    use super::{BatteryError, BatteryState, Page};

    pub(super) fn state() -> Result<BatteryState, BatteryError> {
        Err(BatteryError::Unsupported)
    }

    pub(super) fn is_ignoring_optimizations() -> Result<bool, BatteryError> {
        Err(BatteryError::Unsupported)
    }

    pub(super) fn request_ignore_optimizations() -> Result<(), BatteryError> {
        Err(BatteryError::Unsupported)
    }

    pub(super) fn open_settings() -> Result<Page, BatteryError> {
        Err(BatteryError::Unsupported)
    }

    pub(super) fn open_app_details() -> Result<Page, BatteryError> {
        Err(BatteryError::Unsupported)
    }

    pub(super) fn open_vendor_battery() -> Result<Page, BatteryError> {
        Err(BatteryError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samsung_by_manufacturer() {
        let state = |m: &str| BatteryState {
            ignoring_optimizations: false,
            background_restricted: false,
            manufacturer: m.into(),
        };
        assert!(state("samsung").is_samsung());
        assert!(state(" Samsung ").is_samsung());
        assert!(!state("Google").is_samsung());
        assert!(!state("samsungx").is_samsung());
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn unsupported_off_android() {
        assert_eq!(state(), Err(BatteryError::Unsupported));
        assert_eq!(is_ignoring_optimizations(), Err(BatteryError::Unsupported));
        assert_eq!(
            request_ignore_optimizations(),
            Err(BatteryError::Unsupported)
        );
        assert_eq!(open_settings(), Err(BatteryError::Unsupported));
        assert_eq!(open_app_details(), Err(BatteryError::Unsupported));
        assert_eq!(open_vendor_battery(), Err(BatteryError::Unsupported));
    }
}
