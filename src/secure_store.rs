//! Named secrets, encrypted at rest, with one API on every host (plan
//! `20261022_03_SECURE_STORE_PLAN.md`):
//!
//! * Android: libmobile's `secure_store`, blobs sealed with an AES-256-GCM
//!   key Android Keystore keeps (StrongBox where the device has one);
//! * macOS: one generic-password item per blob in the login Keychain;
//! * scripted runs: memory, empty at the start of each run;
//! * the web and other desktops: none ([`MorphContext::secure_store`] is
//!   `None`).
//!
//! [`MorphContext::secure_store`]: crate::MorphContext::secure_store

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Longest blob name, in bytes.
pub const MAX_NAME_LEN: usize = 64;

/// Largest blob, in bytes.
pub const MAX_VALUE_LEN: usize = 64 * 1024;

/// Named byte blobs, encrypted at rest. Cheap to clone, `Send + Sync`, so
/// an app can keep one in its own types or move it to another thread.
///
/// Names are 1 to 64 of `[A-Za-z0-9._-]`, not starting with `.`; blobs are
/// at most 64 KiB. Both are checked on every host, so what works on the
/// desktop works on the phone. Each call is synchronous and may take
/// milliseconds (a Keystore or Keychain operation): don't call it per
/// frame.
#[derive(Clone)]
pub struct SecureStore {
    backend: Backend,
}

#[derive(Clone)]
enum Backend {
    #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
    Android(libmobile::android::AndroidApp),
    #[cfg(all(target_os = "macos", feature = "secure-store"))]
    Keychain { service: Arc<str> },
    /// Scripted runs.
    #[cfg_attr(not(script_runner), allow(dead_code))]
    Memory(Arc<Mutex<HashMap<String, Vec<u8>>>>),
}

impl fmt::Debug for SecureStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let backend = match &self.backend {
            #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
            Backend::Android(_) => "Android",
            #[cfg(all(target_os = "macos", feature = "secure-store"))]
            Backend::Keychain { .. } => "Keychain",
            Backend::Memory(_) => "Memory",
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

/// Why a secure store call failed.
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

impl SecureStore {
    #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
    pub(crate) fn android(app: libmobile::android::AndroidApp) -> Self {
        Self {
            backend: Backend::Android(app),
        }
    }

    /// The login Keychain, service `libmorph.<id>`; `id` defaults to the
    /// executable's file stem.
    #[cfg(all(target_os = "macos", feature = "secure-store"))]
    pub(crate) fn keychain(id: &str) -> Self {
        let id = if id.is_empty() {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "app".to_owned())
        } else {
            id.to_owned()
        };
        Self {
            backend: Backend::Keychain {
                service: format!("libmorph.{id}").into(),
            },
        }
    }

    #[cfg_attr(not(script_runner), allow(dead_code))] // Scripted runs only.
    pub(crate) fn memory() -> Self {
        Self {
            backend: Backend::Memory(Arc::default()),
        }
    }

    /// The blob called `name`, or `None` if there is none.
    pub fn get(&self, name: &str) -> Result<Option<Vec<u8>>, SecureStoreError> {
        check_name(name)?;
        match &self.backend {
            #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
            Backend::Android(app) => Ok(libmobile::android::secure_store::get(app, name)?),
            #[cfg(all(target_os = "macos", feature = "secure-store"))]
            Backend::Keychain { service } => keychain::get(service, name),
            Backend::Memory(map) => Ok(lock(map).get(name).cloned()),
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
        check_name(name)?;
        check_value(value)?;
        match &self.backend {
            #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
            Backend::Android(app) => {
                let options = libmobile::PutOptions::default().unlocked_only(options.unlocked_only);
                Ok(libmobile::android::secure_store::put_with(
                    app, name, value, options,
                )?)
            }
            #[cfg(all(target_os = "macos", feature = "secure-store"))]
            Backend::Keychain { service } => {
                let _ = options;
                keychain::put(service, name, value)
            }
            Backend::Memory(map) => {
                let _ = options;
                lock(map).insert(name.to_owned(), value.to_vec());
                Ok(())
            }
        }
    }

    /// Remove the blob called `name`. `true` if there was one.
    pub fn delete(&self, name: &str) -> Result<bool, SecureStoreError> {
        check_name(name)?;
        match &self.backend {
            #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
            Backend::Android(app) => Ok(libmobile::android::secure_store::delete(app, name)?),
            #[cfg(all(target_os = "macos", feature = "secure-store"))]
            Backend::Keychain { service } => keychain::delete(service, name),
            Backend::Memory(map) => Ok(lock(map).remove(name).is_some()),
        }
    }

    /// Where the secrets are kept. On Android this asks Keystore, creating
    /// the key if there is none yet, so it can fail.
    pub fn protection(&self) -> Result<Protection, SecureStoreError> {
        match &self.backend {
            #[cfg(all(target_os = "android", feature = "mobile", feature = "secure-store"))]
            Backend::Android(app) => {
                use libmobile::SecurityLevel;
                Ok(
                    match libmobile::android::secure_store::security_level(app)? {
                        SecurityLevel::StrongBox => Protection::AndroidStrongBox,
                        SecurityLevel::TrustedEnvironment => Protection::AndroidTee,
                        _ => Protection::AndroidSoftware,
                    },
                )
            }
            #[cfg(all(target_os = "macos", feature = "secure-store"))]
            Backend::Keychain { .. } => Ok(Protection::MacKeychain),
            Backend::Memory(_) => Ok(Protection::Memory),
        }
    }
}

fn lock(
    map: &Mutex<HashMap<String, Vec<u8>>>,
) -> std::sync::MutexGuard<'_, HashMap<String, Vec<u8>>> {
    map.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 1 to [`MAX_NAME_LEN`] of `[A-Za-z0-9._-]`, not starting with `.`.
fn check_name(name: &str) -> Result<(), SecureStoreError> {
    let valid = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(SecureStoreError::InvalidName)
    }
}

fn check_value(value: &[u8]) -> Result<(), SecureStoreError> {
    if value.len() <= MAX_VALUE_LEN {
        Ok(())
    } else {
        Err(SecureStoreError::TooLarge)
    }
}

/// The login Keychain through `security-framework`: one generic password
/// per blob, service `libmorph.<id>`, account the blob's name.
#[cfg(all(target_os = "macos", feature = "secure-store"))]
mod keychain {
    use super::SecureStoreError;
    use security_framework::base::Error;
    use security_framework::passwords;

    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    fn platform(error: Error) -> SecureStoreError {
        SecureStoreError::Platform(format!("keychain: {error} ({})", error.code()))
    }

    pub(super) fn get(service: &str, name: &str) -> Result<Option<Vec<u8>>, SecureStoreError> {
        match passwords::get_generic_password(service, name) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.code() == NOT_FOUND => Ok(None),
            Err(error) => Err(platform(error)),
        }
    }

    pub(super) fn put(service: &str, name: &str, value: &[u8]) -> Result<(), SecureStoreError> {
        passwords::set_generic_password(service, name, value).map_err(platform)
    }

    pub(super) fn delete(service: &str, name: &str) -> Result<bool, SecureStoreError> {
        match passwords::delete_generic_password(service, name) {
            Ok(()) => Ok(true),
            Err(error) if error.code() == NOT_FOUND => Ok(false),
            Err(error) => Err(platform(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_sizes() {
        for ok in ["a", "noise.static", "server", "A-b_c.9", &"x".repeat(64)] {
            assert_eq!(check_name(ok), Ok(()), "{ok}");
        }
        for bad in ["", ".hidden", "a/b", "a b", "ü", &"x".repeat(65)] {
            assert_eq!(
                check_name(bad),
                Err(SecureStoreError::InvalidName),
                "{bad:?}"
            );
        }
        assert_eq!(check_value(&vec![0; MAX_VALUE_LEN]), Ok(()));
        assert_eq!(
            check_value(&vec![0; MAX_VALUE_LEN + 1]),
            Err(SecureStoreError::TooLarge)
        );
    }

    #[test]
    fn memory_store_round_trip() {
        let store = SecureStore::memory();
        let other = store.clone();
        assert_eq!(store.get("k"), Ok(None));
        store.put("k", b"v").unwrap();
        assert_eq!(other.get("k"), Ok(Some(b"v".to_vec())));
        store
            .put_with("k", b"w", PutOptions::default().unlocked_only(true))
            .unwrap();
        assert_eq!(store.get("k"), Ok(Some(b"w".to_vec())));
        assert_eq!(store.delete("k"), Ok(true));
        assert_eq!(store.delete("k"), Ok(false));
        assert_eq!(store.get("../k"), Err(SecureStoreError::InvalidName));
        assert_eq!(store.protection(), Ok(Protection::Memory));
    }

    /// Writes to the login Keychain (service `libmorph.secure-store-test`),
    /// so it runs only when asked: `cargo test --features secure-store --
    /// --ignored keychain`.
    #[cfg(all(target_os = "macos", feature = "secure-store"))]
    #[test]
    #[ignore = "uses the login Keychain"]
    fn keychain_round_trip() {
        let store = SecureStore::keychain("secure-store-test");
        let _ = store.delete("k");
        assert_eq!(store.get("k"), Ok(None));
        store.put("k", &[1, 2, 3]).unwrap();
        assert_eq!(store.get("k"), Ok(Some(vec![1, 2, 3])));
        store.put("k", &[4]).unwrap();
        assert_eq!(store.get("k"), Ok(Some(vec![4])));
        assert_eq!(store.delete("k"), Ok(true));
        assert_eq!(store.delete("k"), Ok(false));
        assert_eq!(store.protection(), Ok(Protection::MacKeychain));
    }

    #[test]
    fn store_is_send_and_sync() {
        fn check<T: Send + Sync + Clone>() {}
        check::<SecureStore>();
    }
}
