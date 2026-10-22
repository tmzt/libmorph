---
title: Secure store on every host
date: 2026-10-22
status: Draft
covers:
  - A `secure-store` Cargo feature and `libmorph::SecureStore`: named
    byte blobs, encrypted at rest, with one API on every host
  - Android through libmobile's `secure_store` (Keystore envelope
    encryption), the macOS Keychain on the desktop, memory in scripted
    runs, and none on the web
  - `MorphContext::secure_store()`
sources:
  - handlr (owner-approved request, 2026-10-22, from the handlr session):
    the companion app in ~/src/handlr apps/companion, a libmorph app, keeps
    a 32-byte X25519 Noise key and a server record across launches. On
    Android it must be protected at rest with no file-based fallback. On
    macOS, the Keychain, or a 0600 file as handlr's `FileKeyStore` does
    today.
  - handlr docs/design/20261022_04_COMPANION_APP_PLAN.md (its `KeyStore`
    trait, `Send`, and `ServerRecord`).
  - AGENTS.md, "Platform Layering (libmobile first)": the Android part is
    built and released in libmobile first.
  - macOS Security framework: `SecItemAdd` / `SecItemCopyMatching` /
    `SecItemDelete` on `kSecClassGenericPassword`, through the
    `security-framework` crate's `passwords` module.
related:
  - libmobile docs/design/20261022_03_SECURE_STORE_PLAN.md (must be
    implemented and released first)
---

# Secure store on every host

## 1. Goal

A libmorph app keeps small secrets with one API on every host, and can
move the store into its own types or threads. libmorph adds what a
wrapper adds (AGENTS.md, Platform Layering): the same signatures with or
without `mobile`, a desktop backend, and a deterministic one for scripted
runs. The Android encryption is entirely libmobile's.

## 2. API

`src/secure_store.rs`, re-exported at the crate root:

```rust
/// Named byte blobs, encrypted at rest. Cheap to clone, `Send + Sync`.
#[derive(Clone)]
pub struct SecureStore { /* backend */ }

impl SecureStore {
    pub fn get(&self, name: &str) -> Result<Option<Vec<u8>>, SecureStoreError>;
    pub fn put(&self, name: &str, value: &[u8]) -> Result<(), SecureStoreError>;
    pub fn delete(&self, name: &str) -> Result<bool, SecureStoreError>;
    /// Where the secrets are kept on this host.
    pub fn protection(&self) -> Protection;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Protection {
    /// Android Keystore, by libmobile's `SecurityLevel`.
    AndroidSoftware, AndroidTee, AndroidStrongBox,
    /// The user's login Keychain.
    MacKeychain,
    /// Scripted runs: memory only, gone when the run ends.
    Memory,
}

/// libmorph's own type, so the signature does not change with `mobile`.
/// Mirrors libmobile's `SecureStoreError`, with `From` for it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SecureStoreError {
    InvalidName, TooLarge, Keystore(String), Io(String),
    Corrupt(String), Platform(String),
}
```

`MorphContext::secure_store() -> Option<&SecureStore>`: `None` without
the feature, on the web, and on desktop hosts other than macOS. Names and
sizes follow libmobile's rules (1 to 64 of `[A-Za-z0-9._-]`, not starting
with `.`; at most 64 KiB), checked on every host so an app that works on
the desktop works on the phone.

## 3. Backends

- **Android:** holds a clone of the `AndroidApp` and calls
  `libmobile::android::secure_store`. `protection()` comes from its
  `security_level`, read once when the store is made.
- **macOS:** a generic-password Keychain item per blob. Service
  `libmorph.<A::SECURE_STORE_ID>`, account the blob's name, data the
  blob. `MorphApp` gains `const SECURE_STORE_ID: &'static str`, defaulting
  to the executable's file stem when empty (`""`), so two apps do not
  share secrets by accident. Caveat: the item's access list names the
  binary's code signature, and a cargo build is ad-hoc signed with a new
  hash each time, so after a rebuild macOS asks once to allow access.
- **Scripted runs:** an in-memory map, empty at the start of each run, so
  runs stay deterministic and never touch the Keychain.
- **Web, and desktop hosts other than macOS:** none.

## 4. Features

```toml
# Named secrets, encrypted at rest: libmobile's Keystore store on
# Android, the Keychain on macOS.
secure-store = ["libmobile?/secure-store", "dep:security-framework"]
```

`security-framework` is a `cfg(target_os = "macos")` dependency. Not a
`mobile-*` passthrough, because it does something on the desktop too, like
`audio`.

## 5. Steps

1. **Types and the scripted backend.** `SecureStore`, `Protection`,
   `SecureStoreError`, the name and size checks, the memory backend,
   `ctx.secure_store()` and the stub without the feature. Host unit tests,
   and a scripted-run test that writes and reads a blob.
   **Done when:** `cargo test -p libmorph --features secure-store,script`
   passes, and clippy is clean with and without the feature.
2. **macOS Keychain.** The Keychain backend and `SECURE_STORE_ID`.
   **Done when:** a `securestore` desktop example counts launches as in
   libmobile's step 4, the count survives a relaunch, and `security
   find-generic-password -s libmorph.securestore` finds the item.
3. **Android.** Take the libmobile release with `secure-store`; the
   Android backend.
   **Done when:** the same example, built for Android, counts launches
   across a force-stop on the owner's Galaxy S23 Ultra and logs
   `AndroidStrongBox`.

## 6. Release

Additive (a new feature, a new type, a defaulted associated const on
`MorphApp`, a new `MorphContext` method). Released as libmorph 0.3.1
with libmobile 0.2.1, a compatible bump.

## 7. Open questions for the owner

1. **Desktop backend.** The Keychain (this plan) or a 0600 file like
   handlr's `FileKeyStore`. The Keychain protects the secrets at rest, but
   asks again after each rebuild of an unsigned binary. A file does not
   ask, but is plaintext on disk. handlr could also keep its own
   `FileKeyStore` on the desktop and use this only on Android.
2. **Where the desktop code lives.** libmobile is Android-only, so the
   Keychain backend is libmorph's own. The alternative is a separate
   small crate both could use.

## Amendment — 2026-10-22 18:28 EDT: the Keychain on the Mac, and the StrongBox feature

Agreed with the owner (2026-10-22). Settles section 7 and adds to
section 4 and step 3, following libmobile's amendment "secure preferences
in core, StrongBox as a default feature".

- **Desktop backend: the Keychain** (section 3, unchanged). Owner:
  "companion-tty rebuild requiring approval is fine, it shouldn't be that
  often after implemented and stable, Keychain is the right place on
  Mac." The Keychain backend stays in libmorph, not in a separate crate.
- **Features.** `secure-store` stays off by default, so apps that do not
  use it never get `security-framework` on macOS. Its Android half no
  longer needs a libmobile feature, since libmobile's store is core:
  `secure-store = ["dep:security-framework"]`. libmorph adds
  `secure-store-strongbox = ["libmobile?/secure-store-strongbox"]` to its
  `default` list, and depends on libmobile with
  `default-features = false` (on both the normal and the build
  dependency), so an app can turn StrongBox off by turning off libmorph's
  defaults. Today libmobile has no default features, so the change is not
  visible to apps.
- **Step 3** logs `AndroidStrongBox` by default, and `AndroidTee` when
  built without `secure-store-strongbox` and with the app's data cleared.

## Amendment — 2026-10-22 18:32 EDT: the StrongBox feature is negative

Owner (2026-10-22): "we should use a negative feature to disable
strongbox if we don't want it enabled, so that leaving out audio (or
camera) doesn't disable a security feature". Follows libmobile's
amendment "StrongBox by default, turned off by a negative feature", and
replaces the `secure-store-strongbox` default feature and the
`default-features = false` change in the amendment "the Keychain on the
Mac, and the StrongBox feature".

- libmorph's `default` stays `["audio"]`, and its libmobile
  dependencies are unchanged.
- A passthrough, like the other `mobile-*` ones:
  `mobile-secure-store-no-strongbox = ["mobile", "libmobile/secure-store-no-strongbox"]`.
- **Step 3** logs `AndroidStrongBox` by default, and `AndroidTee` when
  built with `mobile-secure-store-no-strongbox` and with the app's data
  cleared.

## Amendment — 2026-10-22 18:35 EDT: the unlock-required flag

Follows libmobile's amendment "an unlock-required flag per blob", on the
owner's question (2026-10-22). Adds to section 2.

- `SecureStore::put_with(name, value, PutOptions)`, with libmorph's own
  `PutOptions` (`#[non_exhaustive]`, `Default`, `unlocked_only(bool)`),
  and `SecureStoreError::Locked`.
- **Android:** passed to libmobile.
- **macOS:** accepted, but not enforced. Per-item accessibility
  (`kSecAttrAccessibleWhenUnlocked`) applies only in the data-protection
  Keychain, which needs a signed app with a keychain-access-groups
  entitlement. A cargo-built binary uses the login Keychain, whose items
  are readable whenever the user is logged in. The item is stored the same
  way, and the rustdoc says so.
- **Scripted runs:** accepted, with nothing to enforce.
