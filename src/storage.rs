//! The app's own directories (plan `20261023_03_APP_STORAGE_PLAN.md`):
//! [`MorphContext::data_dir`](crate::MorphContext::data_dir) and
//! [`cache_dir`](crate::MorphContext::cache_dir), created by the runner
//! before `init`. libmobile gives Android's, libdevice the desktop's by app
//! id; scripted runs use their own output directory; the web has none.

use std::path::{Path, PathBuf};

/// Longest app id, in bytes (libdevice's rule).
#[cfg_attr(any(target_os = "android", target_arch = "wasm32"), allow(dead_code))]
const MAX_ID_LEN: usize = 64;

/// Whether `id` is a directory name libdevice accepts: 1 to 64 of
/// `[A-Za-z0-9._-]`, not `.` or `..`.
#[cfg_attr(any(target_os = "android", target_arch = "wasm32"), allow(dead_code))]
fn valid_id(id: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&id.len())
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `MorphApp::APP_ID`, or for an empty one `fallback` (the executable's
/// file stem) with every other character replaced by `_` and cut to 64
/// bytes. `None`, logged, for an `APP_ID` that breaks the rule.
#[cfg_attr(any(target_os = "android", target_arch = "wasm32"), allow(dead_code))]
fn app_id(id: &str, fallback: impl FnOnce() -> String) -> Option<String> {
    if !id.is_empty() {
        if valid_id(id) {
            return Some(id.to_owned());
        }
        log::error!("libmorph: storage: APP_ID `{id}` is not 1 to {MAX_ID_LEN} of [A-Za-z0-9._-]");
        return None;
    }
    let id: String = fallback()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(MAX_ID_LEN)
        .collect();
    match id.as_str() {
        "" | "." | ".." => Some("app".to_owned()),
        _ => Some(id),
    }
}

/// `path`, once it exists: created with `create_dir_all`. A failure is
/// logged and gives `None`.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn ensure(path: Option<PathBuf>) -> Option<PathBuf> {
    let path = path?;
    match std::fs::create_dir_all(&path) {
        Ok(()) => Some(path),
        Err(error) => {
            log::error!(
                "libmorph: storage: cannot create {}: {error}",
                path.display()
            );
            None
        }
    }
}

/// `path` emptied and created again, for scripted runs.
#[cfg_attr(not(script_runner), allow(dead_code))]
pub(crate) fn fresh(path: &Path) -> Option<PathBuf> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            log::error!(
                "libmorph: storage: cannot empty {}: {error}",
                path.display()
            );
            return None;
        }
    }
    ensure(Some(path.to_owned()))
}

/// The desktop's data and cache directories for `APP_ID`, created.
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub(crate) fn desktop(id: &str) -> (Option<PathBuf>, Option<PathBuf>) {
    let Some(id) = app_id(id, crate::secure_store::default_id) else {
        return (None, None);
    };
    let data = libdevice::dirs::data_dir(&id);
    let cache = libdevice::dirs::cache_dir(&id);
    if data.is_none() {
        log::error!("libmorph: storage: no data directory for `{id}` on this host");
    }
    (ensure(data), ensure(cache))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids_follow_libdevices_rule() {
        let stem = || "my app/v2".to_owned();
        assert_eq!(
            app_id("dev.tmzt.handlr", stem),
            Some("dev.tmzt.handlr".into())
        );
        assert_eq!(app_id("", stem), Some("my_app_v2".into()));
        assert_eq!(app_id("", || "x".repeat(100)).unwrap().len(), MAX_ID_LEN);
        assert_eq!(app_id("", String::new), Some("app".into()));
        assert_eq!(app_id("", || "..".into()), Some("app".into()));
        for bad in ["a b", "a/b", ".", "..", &"x".repeat(65)] {
            assert_eq!(app_id(bad, stem), None, "{bad}");
        }
        assert!(valid_id("a.b_c-D9"));
    }

    #[test]
    fn a_path_under_a_file_gives_none() {
        let dir = std::env::temp_dir().join(format!("libmorph-storage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("file");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(ensure(Some(file.join("data"))), None);
        assert_eq!(fresh(&file.join("data")), None);
        let made = ensure(Some(dir.join("a/b"))).unwrap();
        assert!(made.is_dir());
        std::fs::write(made.join("f"), b"x").unwrap();
        let emptied = fresh(&made).unwrap();
        assert_eq!(std::fs::read_dir(&emptied).unwrap().count(), 0);
        assert_eq!(ensure(None), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_mac_uses_application_support() {
        let id = app_id("dev.highbay.libmorph.test", String::new).unwrap();
        let data = libdevice::dirs::data_dir(&id).unwrap();
        assert!(data.ends_with("Library/Application Support/dev.highbay.libmorph.test"));
        let cache = libdevice::dirs::cache_dir(&id).unwrap();
        assert!(cache.ends_with("Library/Caches/dev.highbay.libmorph.test"));
    }
}
