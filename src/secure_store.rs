//! Named secrets, encrypted at rest, with one API on every host (plans
//! `20261022_03_SECURE_STORE_PLAN.md` and `20261022_06_KEY_STORE_PLAN.md`).
//! libmorph only wraps the platform crates:
//!
//! * Android: libmobile's `secure_store`, blobs sealed with an AES-256-GCM
//!   key Android Keystore keeps (StrongBox where the device has one);
//! * macOS: libdevice's `secure_store`, one generic-password item per blob
//!   in the login Keychain;
//! * scripted runs: libdevice's memory store, empty at the start of each run;
//! * the web and other desktops: none ([`MorphContext::secure_store`] is
//!   `None`).
//!
//! Keys whose algorithm an adapter defines are the key store's
//! (`libmorph::key_store`), not blobs.
//!
//! [`MorphContext::secure_store`]: crate::MorphContext::secure_store

use std::fmt;

/// Longest blob name, in bytes.
pub const MAX_NAME_LEN: usize = 64;

/// Largest blob, in bytes.
pub const MAX_VALUE_LEN: usize = 64 * 1024;

/// Named byte blobs, encrypted at rest. Cheap to clone, `Send + Sync`, so
/// an app can keep one in its own types or move it to another thread.
///
/// Names are 1 to 64 of `[A-Za-z0-9._-]`, not starting with `.`; blobs are
/// at most 64 KiB, on every host. Each call is synchronous and may take
/// milliseconds (a Keystore or Keychain operation): don't call it per
/// frame.
#[derive(Clone)]
pub struct SecureStore {
    backend: Backend,
}

#[derive(Clone)]
enum Backend {
    #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
    Android,
    #[cfg(all(any(target_os = "macos", script_runner), feature = "secure-store"))]
    Desktop(libdevice::secure_store::SecureStore),
}

impl fmt::Debug for SecureStore {
    // Where there is no backend, nothing is reachable.
    #[cfg_attr(
        not(all(
            feature = "secure-store",
            any(target_os = "android", target_os = "macos", script_runner)
        )),
        allow(unused_variables, unreachable_code)
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let backend = match self.backend {
            #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
            Backend::Android => "Android",
            #[cfg(all(any(target_os = "macos", script_runner), feature = "secure-store"))]
            Backend::Desktop(ref store) => match store.security_level() {
                libdevice::secure_store::SecurityLevel::Keychain => "Keychain",
                _ => "Memory",
            },
        };
        f.debug_struct("SecureStore")
            .field("backend", &backend)
            .finish()
    }
}

/// Where a [`SecureStore`] keeps its secrets.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Protection {
    /// Android Keystore without secure hardware.
    AndroidSoftware,
    /// Android Keystore in the trusted execution environment.
    AndroidTee,
    /// Android Keystore in a separate secure element.
    AndroidStrongBox,
    /// The user's login Keychain.
    MacKeychain,
    /// Memory only, gone when the run ends (scripted runs).
    Memory,
}

/// How a blob is stored.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct PutOptions {
    /// Keep the blob unreadable while the device is locked. Enforced on
    /// Android only: on macOS the login Keychain cannot hold per-item
    /// accessibility for an unsigned binary, so the item is readable
    /// whenever the user is logged in.
    pub unlocked_only: bool,
}

impl PutOptions {
    #[must_use]
    pub fn unlocked_only(mut self, unlocked_only: bool) -> Self {
        self.unlocked_only = unlocked_only;
        self
    }
}

/// Why a secure store call failed. The same variants as libmobile's and
/// libdevice's.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum SecureStoreError {
    /// Not 1 to 64 of `[A-Za-z0-9._-]`, or starts with `.`.
    InvalidName,
    /// Over [`MAX_VALUE_LEN`].
    TooLarge,
    /// Android Keystore refused to create or use the key.
    Keystore(String),
    /// The blob was stored with [`PutOptions::unlocked_only`] and the
    /// device is locked. Try again once it is unlocked.
    DeviceLocked,
    /// Reading or writing the blob failed.
    Io(String),
    /// The blob is malformed or fails to decrypt. Not treated as absent:
    /// the app decides whether to delete it and start over.
    Corrupt(String),
    /// Any other failure of the platform (JNI, the Keychain).
    Platform(String),
}

impl fmt::Display for SecureStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => write!(
                f,
                "secure store: names are 1 to {MAX_NAME_LEN} of [A-Za-z0-9._-], not starting with '.'"
            ),
            Self::TooLarge => write!(f, "secure store: blobs are at most {MAX_VALUE_LEN} bytes"),
            Self::Keystore(detail) => write!(f, "secure store: keystore: {detail}"),
            Self::DeviceLocked => write!(f, "secure store: the device is locked"),
            Self::Io(detail) => write!(f, "secure store: io: {detail}"),
            Self::Corrupt(detail) => write!(f, "secure store: corrupt: {detail}"),
            Self::Platform(detail) => write!(f, "secure store: {detail}"),
        }
    }
}

impl std::error::Error for SecureStoreError {}

#[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
impl From<libmobile::SecureStoreError> for SecureStoreError {
    fn from(error: libmobile::SecureStoreError) -> Self {
        use libmobile::SecureStoreError as E;
        match error {
            E::InvalidName => Self::InvalidName,
            E::TooLarge => Self::TooLarge,
            E::Keystore(detail) => Self::Keystore(detail),
            E::DeviceLocked => Self::DeviceLocked,
            E::Io(detail) => Self::Io(detail),
            E::Corrupt(detail) => Self::Corrupt(detail),
            E::Platform(detail) => Self::Platform(detail),
            other => Self::Platform(other.to_string()),
        }
    }
}

#[cfg(all(any(target_os = "macos", script_runner), feature = "secure-store"))]
impl From<libdevice::secure_store::SecureStoreError> for SecureStoreError {
    fn from(error: libdevice::secure_store::SecureStoreError) -> Self {
        use libdevice::secure_store::SecureStoreError as E;
        match error {
            E::InvalidName => Self::InvalidName,
            E::TooLarge => Self::TooLarge,
            E::Keystore(detail) => Self::Keystore(detail),
            E::DeviceLocked => Self::DeviceLocked,
            E::Io(detail) => Self::Io(detail),
            E::Corrupt(detail) => Self::Corrupt(detail),
            E::Platform(detail) => Self::Platform(detail),
            other => Self::Platform(other.to_string()),
        }
    }
}

impl SecureStore {
    #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
    pub(crate) fn android() -> Self {
        Self {
            backend: Backend::Android,
        }
    }

    /// The login Keychain, service `namespace`.
    #[cfg(all(target_os = "macos", feature = "secure-store"))]
    pub(crate) fn keychain(namespace: &str) -> Self {
        Self {
            backend: Backend::Desktop(libdevice::secure_store::SecureStore::keychain(namespace)),
        }
    }

    #[cfg(all(script_runner, feature = "secure-store"))]
    pub(crate) fn memory() -> Self {
        Self {
            backend: Backend::Desktop(libdevice::secure_store::SecureStore::memory()),
        }
    }

    /// The blob called `name`, or `None` if there is none.
    pub fn get(&self, name: &str) -> Result<Option<Vec<u8>>, SecureStoreError> {
        let _ = name;
        match self.backend {
            #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
            Backend::Android => Ok(libmobile::android::secure_store::get(name)?),
            #[cfg(all(any(target_os = "macos", script_runner), feature = "secure-store"))]
            Backend::Desktop(ref store) => Ok(store.get(name)?),
        }
    }

    /// Store `value` as the blob called `name`, replacing any earlier one.
    pub fn put(&self, name: &str, value: &[u8]) -> Result<(), SecureStoreError> {
        self.put_with(name, value, PutOptions::default())
    }

    /// [`put`](Self::put) with options; see [`PutOptions`].
    pub fn put_with(
        &self,
        name: &str,
        value: &[u8],
        options: PutOptions,
    ) -> Result<(), SecureStoreError> {
        let _ = (name, value, options);
        match self.backend {
            #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
            Backend::Android => {
                let options = libmobile::PutOptions::default().unlocked_only(options.unlocked_only);
                Ok(libmobile::android::secure_store::put_with(
                    name, value, options,
                )?)
            }
            #[cfg(all(any(target_os = "macos", script_runner), feature = "secure-store"))]
            Backend::Desktop(ref store) => {
                let options = libdevice::secure_store::PutOptions::default()
                    .unlocked_only(options.unlocked_only);
                Ok(store.put_with(name, value, options)?)
            }
        }
    }

    /// Remove the blob called `name`. `true` if there was one.
    pub fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        let _ = name;
        match self.backend {
            #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
            Backend::Android => Ok(libmobile::android::secure_store::delete(name)?),
            #[cfg(all(any(target_os = "macos", script_runner), feature = "secure-store"))]
            Backend::Desktop(ref store) => Ok(store.delete(name)?),
        }
    }

    /// Where the secrets are kept. On Android this asks Keystore, creating
    /// the key if there is none yet, so it can fail.
    pub fn protection(&self) -> Result<Protection, SecureStoreError> {
        match self.backend {
            #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
            Backend::Android => {
                use libmobile::SecurityLevel;
                Ok(match libmobile::android::secure_store::security_level()? {
                    SecurityLevel::StrongBox => Protection::AndroidStrongBox,
                    SecurityLevel::TrustedEnvironment => Protection::AndroidTee,
                    _ => Protection::AndroidSoftware,
                })
            }
            #[cfg(all(any(target_os = "macos", script_runner), feature = "secure-store"))]
            Backend::Desktop(ref store) => Ok(match store.security_level() {
                libdevice::secure_store::SecurityLevel::Keychain => Protection::MacKeychain,
                _ => Protection::Memory,
            }),
        }
    }
}

/// `libmorph.<id>`: the namespace of the app's secure store and key store.
/// An empty `id` defaults to the app's own name (the executable's file stem
/// on the desktop, the package name on Android), with characters outside
/// `[A-Za-z0-9._-]` replaced by `_` and cut to fit the 64-byte name rule.
#[cfg_attr(
    not(all(
        feature = "secure-store",
        any(target_os = "android", target_os = "macos")
    )),
    allow(dead_code)
)]
pub(crate) fn namespace(id: &str) -> String {
    let id = if id.is_empty() {
        default_id()
    } else {
        id.to_owned()
    };
    let id: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(MAX_NAME_LEN - "libmorph.".len())
        .collect();
    format!("libmorph.{id}")
}

#[cfg(target_os = "android")]
#[cfg_attr(
    not(all(
        feature = "secure-store",
        any(target_os = "android", target_os = "macos")
    )),
    allow(dead_code)
)]
fn default_id() -> String {
    // An Android app's process is named after its package.
    std::fs::read("/proc/self/cmdline")
        .ok()
        .and_then(|cmdline| {
            let name = cmdline.split(|&b| b == 0).next()?;
            let name = String::from_utf8_lossy(name);
            let package = name.split(':').next()?.to_owned();
            (!package.is_empty()).then_some(package)
        })
        .unwrap_or_else(|| "app".to_owned())
}

#[cfg(not(target_os = "android"))]
#[cfg_attr(
    not(all(
        feature = "secure-store",
        any(target_os = "android", target_os = "macos")
    )),
    allow(dead_code)
)]
fn default_id() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "app".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_follow_the_name_rule() {
        assert_eq!(namespace("handlr"), "libmorph.handlr");
        assert_eq!(namespace("a b/c"), "libmorph.a_b_c");
        assert_eq!(namespace(&"x".repeat(100)).len(), MAX_NAME_LEN);
        assert!(namespace("").starts_with("libmorph."));
    }

    #[cfg(all(script_runner, feature = "secure-store"))]
    #[test]
    fn memory_store_round_trip() {
        let store = SecureStore::memory();
        let other = store.clone();
        assert_eq!(store.get("k"), Ok(None));
        store.put("k", b"v").unwrap();
        assert_eq!(other.get("k"), Ok(Some(b"v".to_vec())));
        assert_eq!(store.delete("k"), Ok(true));
        assert_eq!(store.get("../k"), Err(SecureStoreError::InvalidName));
        assert_eq!(store.protection(), Ok(Protection::Memory));
    }

    #[test]
    fn store_is_send_and_sync() {
        fn check<T: Send + Sync + Clone>() {}
        check::<SecureStore>();
    }
}
