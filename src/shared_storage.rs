//! **All files access, the shared storage root and media scans** (plan
//! `20261119_01_ALL_FILES_ACCESS_PLAN.md`), with the same four methods on
//! every host:
//! [`all_files_access`](crate::MorphContext::all_files_access),
//! [`open_all_files_access_settings`](crate::MorphContext::open_all_files_access_settings),
//! [`external_storage_dir`](crate::MorphContext::external_storage_dir) and
//! [`scan_media`](crate::MorphContext::scan_media).
//!
//! * **Android** (features `mobile-all-files-access` and
//!   `mobile-media-scan`): libmobile's `all-files-access` and `media-scan`.
//!   The build script declares the permission with
//!   `libmorph_build::gradle_project(id).all_files_access()`. Without the
//!   features: [`SharedStorageError::Unsupported`] and `None`.
//! * **The desktop and the web:** [`AllFilesAccess::NotRequired`]; there is
//!   no settings page ([`SharedStorageError::Unsupported`]), no shared
//!   storage root (`None`), and no media index to tell (`Ok(())`).
//! * **Scripted runs** (desktop, `script`): the `all-files-access` verb sets
//!   the answer (`NotRequired` until it does); the settings page and each
//!   scanned path are logged.
//!
//! The module is `shared_storage`, not `storage`: the app's own
//! directories (`data_dir`, `cache_dir`) are elsewhere and unchanged.

use std::fmt;
use std::path::Path;

/// The most paths one [`scan_media`](crate::MorphContext::scan_media) takes,
/// libmobile's `MAX_SCAN_PATHS`.
pub const MAX_SCAN_PATHS: usize = 1024;

/// Whether the app may read and write all of shared storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AllFilesAccess {
    /// Android: the user turned All files access on.
    Granted,
    /// Android: it is off. Ask with
    /// [`open_all_files_access_settings`](crate::MorphContext::open_all_files_access_settings).
    Missing,
    /// No such permission on this host: the process reads what its user can
    /// (the desktop), or nothing (the web). It says there is no permission,
    /// not that every path is readable: the OS's own errors still come.
    NotRequired,
}

impl AllFilesAccess {
    /// The script verb's word (`all-files-access <word>`).
    #[cfg_attr(not(script_runner), allow(dead_code))]
    pub(crate) fn from_script(word: &str) -> Option<Self> {
        Some(match word {
            "granted" => Self::Granted,
            "missing" => Self::Missing,
            "not-required" => Self::NotRequired,
            _ => return None,
        })
    }
}

/// Why a shared storage call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SharedStorageError {
    /// The build script did not call `gradle_project(id).all_files_access()`.
    NotDeclared,
    /// Android 10, the app was built without `mobile-all-files-access` (or
    /// `mobile-media-scan` for `scan_media`), or a host with no such page.
    Unsupported,
    /// The settings page was asked for while the app was not visible.
    NotVisible,
    /// No settings page resolved on this device.
    NoSettingsPage,
    /// A path given to `scan_media` was not absolute or not UTF-8, or there
    /// were more than [`MAX_SCAN_PATHS`].
    BadPaths(String),
    /// Anything else the platform reported.
    Platform(String),
}

impl fmt::Display for SharedStorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDeclared => f.write_str("MANAGE_EXTERNAL_STORAGE is not declared"),
            Self::Unsupported => f.write_str("not supported on this host or build"),
            Self::NotVisible => {
                f.write_str("a settings page may only be opened while the app is visible")
            }
            Self::NoSettingsPage => {
                f.write_str("no All files access settings page resolves on this device")
            }
            Self::BadPaths(detail) => write!(f, "bad paths: {detail}"),
            Self::Platform(detail) => write!(f, "platform error: {detail}"),
        }
    }
}

impl std::error::Error for SharedStorageError {}

/// `scan_media`'s check, on every host: at most [`MAX_SCAN_PATHS`], each
/// absolute and UTF-8. libmobile checks again on Android.
pub(crate) fn check_paths(paths: &[&Path]) -> Result<(), SharedStorageError> {
    if paths.len() > MAX_SCAN_PATHS {
        return Err(SharedStorageError::BadPaths(format!(
            "{} paths in one scan; at most {MAX_SCAN_PATHS}",
            paths.len()
        )));
    }
    match paths
        .iter()
        .find(|path| !path.is_absolute() || path.to_str().is_none())
    {
        Some(path) => Err(SharedStorageError::BadPaths(format!(
            "not an absolute UTF-8 path: {}",
            path.display()
        ))),
        None => Ok(()),
    }
}

pub(crate) use platform::{external_storage_dir, open_settings, scan, state};

#[cfg(all(target_os = "android", feature = "mobile-all-files-access"))]
mod access {
    use super::{AllFilesAccess, SharedStorageError};
    use libmobile::android::all_files_access as afa;
    use std::path::PathBuf;

    pub(crate) fn from_mobile(error: afa::AllFilesAccessError) -> SharedStorageError {
        use afa::AllFilesAccessError as E;
        match error {
            E::NotDeclared => SharedStorageError::NotDeclared,
            E::Unsupported => SharedStorageError::Unsupported,
            E::NotVisible => SharedStorageError::NotVisible,
            E::NoSettingsPage => SharedStorageError::NoSettingsPage,
            E::Platform(detail) => SharedStorageError::Platform(detail),
            other => SharedStorageError::Platform(other.to_string()),
        }
    }

    pub(crate) fn state() -> Result<AllFilesAccess, SharedStorageError> {
        match afa::state().map_err(from_mobile)? {
            afa::AllFilesAccess::Granted => Ok(AllFilesAccess::Granted),
            afa::AllFilesAccess::Missing => Ok(AllFilesAccess::Missing),
            other => Err(SharedStorageError::Platform(format!(
                "an unknown access state: {other:?}"
            ))),
        }
    }

    pub(crate) fn open_settings() -> Result<(), SharedStorageError> {
        let page = afa::open_settings().map_err(from_mobile)?;
        log::info!("libmorph: all files access settings opened: {page:?}");
        Ok(())
    }

    pub(crate) fn external_storage_dir() -> Option<PathBuf> {
        afa::external_storage_dir()
            .map_err(|error| log::warn!("libmorph: no external storage directory: {error}"))
            .ok()
    }
}

#[cfg(all(target_os = "android", not(feature = "mobile-all-files-access")))]
mod access {
    use super::{AllFilesAccess, SharedStorageError};
    use std::path::PathBuf;

    pub(crate) fn state() -> Result<AllFilesAccess, SharedStorageError> {
        Err(SharedStorageError::Unsupported)
    }

    pub(crate) fn open_settings() -> Result<(), SharedStorageError> {
        Err(SharedStorageError::Unsupported)
    }

    pub(crate) fn external_storage_dir() -> Option<PathBuf> {
        None
    }
}

#[cfg(target_os = "android")]
mod platform {
    use super::SharedStorageError;
    use std::path::Path;

    pub(crate) use super::access::{external_storage_dir, open_settings, state};

    #[cfg(feature = "mobile-media-scan")]
    pub(crate) fn scan(paths: &[&Path]) -> Result<(), SharedStorageError> {
        use libmobile::android::media_scan::{MediaScanError as E, scan_paths};
        scan_paths(paths).map_err(|error| match error {
            E::BadPath(_) | E::TooMany(_) => SharedStorageError::BadPaths(error.to_string()),
            E::Platform(detail) => SharedStorageError::Platform(detail),
            other => SharedStorageError::Platform(other.to_string()),
        })
    }

    #[cfg(not(feature = "mobile-media-scan"))]
    pub(crate) fn scan(_paths: &[&Path]) -> Result<(), SharedStorageError> {
        Err(SharedStorageError::Unsupported)
    }
}

/// The desktop and the web: no permission, no page, no root, no index.
#[cfg(not(target_os = "android"))]
mod platform {
    use super::{AllFilesAccess, SharedStorageError};
    use std::path::{Path, PathBuf};

    #[allow(clippy::unnecessary_wraps)] // The Android signature.
    pub(crate) fn state() -> Result<AllFilesAccess, SharedStorageError> {
        Ok(AllFilesAccess::NotRequired)
    }

    pub(crate) fn open_settings() -> Result<(), SharedStorageError> {
        Err(SharedStorageError::Unsupported)
    }

    pub(crate) fn external_storage_dir() -> Option<PathBuf> {
        None
    }

    #[allow(clippy::unnecessary_wraps)] // The Android signature.
    pub(crate) fn scan(_paths: &[&Path]) -> Result<(), SharedStorageError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AllFilesAccess, MAX_SCAN_PATHS, SharedStorageError, check_paths};
    use std::path::Path;

    #[test]
    fn script_words() {
        assert_eq!(
            AllFilesAccess::from_script("granted"),
            Some(AllFilesAccess::Granted)
        );
        assert_eq!(
            AllFilesAccess::from_script("missing"),
            Some(AllFilesAccess::Missing)
        );
        assert_eq!(
            AllFilesAccess::from_script("not-required"),
            Some(AllFilesAccess::NotRequired)
        );
        assert_eq!(AllFilesAccess::from_script("Granted"), None);
    }

    #[test]
    fn paths_must_be_absolute_utf8_and_few() {
        assert_eq!(check_paths(&[]), Ok(()));
        assert_eq!(check_paths(&[Path::new("/sdcard/DCIM/a.jpg")]), Ok(()));
        assert!(matches!(
            check_paths(&[Path::new("/a"), Path::new("DCIM/a.jpg")]),
            Err(SharedStorageError::BadPaths(detail)) if detail.contains("DCIM/a.jpg")
        ));
        assert!(matches!(
            check_paths(&[Path::new("")]),
            Err(SharedStorageError::BadPaths(_))
        ));
        let path = Path::new("/a");
        assert_eq!(check_paths(&vec![path; MAX_SCAN_PATHS]), Ok(()));
        assert!(matches!(
            check_paths(&vec![path; MAX_SCAN_PATHS + 1]),
            Err(SharedStorageError::BadPaths(detail)) if detail.contains("1025")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_fail() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let bad = Path::new(OsStr::from_bytes(b"/storage/\xff.jpg"));
        assert!(matches!(
            check_paths(&[bad]),
            Err(SharedStorageError::BadPaths(_))
        ));
    }

    /// The desktop's fixed answers (the test runs on the host).
    #[cfg(not(target_os = "android"))]
    #[test]
    fn the_desktop_answers() {
        assert_eq!(super::state(), Ok(AllFilesAccess::NotRequired));
        assert_eq!(super::open_settings(), Err(SharedStorageError::Unsupported));
        assert_eq!(super::external_storage_dir(), None);
        assert_eq!(super::scan(&[Path::new("/a")]), Ok(()));
    }
}
