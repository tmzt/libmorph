---
title: Key store for keying material
date: 2026-10-22
status: Draft
covers:
  - One small key store engine (`key_store.rs`), the same file in libmobile
    and libdevice, over a per-platform `Vault`
  - "`SecretKey` adapters: the store generates, seals and validates the
    adapter's private key `K`, returns only its public value `P`, and lends
    `&K` for the length of one call (`with_key`)"
  - A flags byte (device unlock required; app unlock reserved), enforced by
    the platform, never by checking a byte in software
  - libmorph re-exporting the platform's key store, and `ctx.key_store()`
supersedes:
  - docs/design/20261022_04_OPAQUE_KEYS_PLAN.md
  - libmobile docs/design/20261022_04_KEY_STORE_PLAN.md
  - the key store half of libdevice
    docs/design/20261022_01_SECURE_STORE_AND_KEY_STORE_PLAN.md
sources:
  - "Owner (2026-10-22): a basic engine implementation to be copied to both
    libraries, \"review it and consider it for a simplified successor plan,
    keeping the code small and managable\", and \"this is the more important
    part of our use of secure preferences/keychain, protecting actual keying
    material, so we need to get it right\"."
  - "The review of that engine in this session, agreed with the owner: the
    lock must not be held across the closure; the app-unlock flag must be
    refused until it is enforced; `unseal` gets the stored kind; `KIND`
    length is checked; poisoned locks are recovered; and the flags belong
    to the platform layer."
  - "Owner: \"sign\" means any use of the keying material, including a DH;
    the protocol's data flow uses the DH output separately."
  - The superseded plans and their agreed amendments (token by reference,
    `ZeroizeOnDrop`, kind migration, `DeviceLocked` and `Locked`, per-instance
    memory stores, the Keychain signing note).
  - AGENTS.md, "Platform Layering (libmobile and libdevice first)".
related:
  - docs/design/20261022_03_SECURE_STORE_PLAN.md (the blob store; unchanged
    by this plan, released alongside it)
  - libmobile docs/design/20261022_03_SECURE_STORE_PLAN.md (its plugin and
    file layout carry the key store's second space)
  - libdevice docs/design/20261022_01_SECURE_STORE_AND_KEY_STORE_PLAN.md
    (its blob store half stays; its key store half moves here)
  - handlr docs/design/20261022_04_COMPANION_APP_PLAN.md
---

# Key store for keying material

## 1. Goal

An app keeps private keys on the device without its code reading them.
The key's algorithm lives in an adapter, a black box such as handlr's Noise
adapter. The store generates the key through the adapter, keeps it sealed
by the platform, and gives back its public value. It lends the private key
to the adapter's code only for the length of one call, then drops and
clears it.

The engine is one small file, the same in libmobile and libdevice. Each
crate adds only a `Vault`, the few lines that read and write sealed bytes
on its platform. libmorph re-exports whichever applies, so apps and
adapters compile against one API on every host without a bridge.

## 2. The engine (`src/key_store.rs`, identical in libmobile and libdevice)

```rust
use std::sync::{Arc, Mutex, PoisonError};
use zeroize::{ZeroizeOnDrop, Zeroizing};

/// Proof that the store is the caller. Not Clone, Copy or Default; made only
/// inside this crate; lent by reference, so it cannot outlive a call.
pub struct SealToken(());

/// The platform keeps the key usable only while the device is unlocked.
pub const FLAG_DEVICE_UNLOCK_REQUIRED: u8 = 1 << 0;
/// Reserved for keys the app must unlock before use (a later plan).
/// Refused by `generate` until then.
pub const FLAG_APP_UNLOCK_REQUIRED: u8 = 1 << 1;

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum KeyStoreError {
    /// Not 1 to 64 of [A-Za-z0-9._-], or starts with '.'.
    InvalidName,
    NotFound,
    /// Stored as a kind the adapter does not accept; the stored kind.
    WrongKind(String),
    /// Malformed, failed to decrypt, or refused by `unseal`.
    Corrupt(String),
    /// The device is locked and the key needs it unlocked.
    DeviceLocked,
    /// Reserved: the key must be unlocked by the app first.
    Locked,
    /// Flags this backend does not enforce (or unknown bits); the flags.
    UnsupportedFlags(u8),
    /// The platform failed.
    Platform(String),
}

pub trait SecretKey: ZeroizeOnDrop + Sized + Send + Sync + 'static {
    type Public: Clone + Send + Sync + 'static;
    /// 1 to 255 bytes, e.g. "handlr.noise.x25519.v1".
    const KIND: &'static str;
    /// Kinds `unseal` can read: `KIND` and older encodings.
    fn accepts(kind: &str) -> bool { kind == Self::KIND }
    fn generate(token: &SealToken) -> Self;
    fn seal(&self, token: &SealToken) -> Zeroizing<Vec<u8>>;
    /// `kind` is the stored kind, which may be an older one `accepts` allows.
    fn unseal(kind: &str, bytes: &[u8], token: &SealToken) -> Result<Self, String>;
    fn public(&self) -> Self::Public;
}

/// Where sealed envelopes live. Implemented once per platform. The backend
/// enforces `flags` itself (a Keystore key, a Keychain attribute) and keeps
/// them outside the envelope, authenticated with it.
pub(crate) trait Vault: Send + Sync {
    /// Which flags this backend enforces.
    fn supported_flags(&self) -> u8;
    /// `Ok(None)` if absent; `Err(DeviceLocked)` if the flags forbid it now.
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, KeyStoreError>;
    fn put(&self, name: &str, flags: u8, envelope: &[u8]) -> Result<(), KeyStoreError>;
    /// The stored flags, to re-seal with the same ones.
    fn flags(&self, name: &str) -> Result<Option<u8>, KeyStoreError>;
    fn delete(&self, name: &str) -> Result<bool, KeyStoreError>;
}

/// Cheap to clone; clones share one vault and one lock.
#[derive(Clone)]
pub struct KeyStore { vault: Arc<dyn Vault>, lock: Arc<Mutex<()>> }

impl KeyStore {
    /// A new key called `name`, replacing any earlier one. Returns its
    /// public value; the private key is dropped (and cleared) before return.
    pub fn generate<K: SecretKey>(&self, name: &str, flags: u8) -> Result<K::Public, KeyStoreError>;
    /// The public value of the key called `name`.
    pub fn public<K: SecretKey>(&self, name: &str) -> Result<K::Public, KeyStoreError>;
    /// Lend the private key to `f` for this call only. `f` cannot return
    /// the borrow. The lock is not held while `f` runs, so `f` may use the
    /// store again.
    pub fn with_key<K: SecretKey, R>(&self, name: &str, f: impl FnOnce(&K) -> R)
        -> Result<R, KeyStoreError>;
    pub fn delete(&self, name: &str) -> Result<bool, KeyStoreError>;
}

/// For tests and scripted runs: a map per instance, no globals, and a
/// switch that simulates a locked device for FLAG_DEVICE_UNLOCK_REQUIRED.
pub struct MemoryVault { /* Mutex<HashMap<String, (u8, Zeroizing<Vec<u8>>)>>, AtomicBool */ }
impl KeyStore { pub fn memory() -> (Self, MemoryHandle) }   // handle: set_device_locked(bool)

#[doc(hidden)]
pub const ENGINE_SOURCE: &str = include_str!("key_store.rs");
```

### 2.1 Behaviour

- **Envelope:** `[kind length: u8][kind: UTF-8][adapter's sealed bytes]`,
  held in `Zeroizing`. The flags are not in it; the vault keeps them.
- **`generate`:**
  1. Check the name, and that `K::KIND` is 1 to 255 bytes.
  2. Refuse `flags & !vault.supported_flags()` with `UnsupportedFlags`.
     That includes `FLAG_APP_UNLOCK_REQUIRED` everywhere until its plan,
     and unknown bits.
  3. `K::generate`, then `seal`, the envelope, `vault.put`, under the
     lock.
  4. Return `public()`.
- **`with_key`:**
  1. Under the lock, `vault.get`, keeping a copy of the envelope, and
     `vault.flags`. Release the lock.
  2. Parse the envelope (`Corrupt` if malformed). `WrongKind` unless
     `K::accepts(kind)`.
  3. `K::unseal(kind, bytes, &token)`, mapping an error to `Corrupt`.
  4. If `kind != K::KIND`, migrate. Seal again; under the lock, re-read
     the envelope, and write the new one with the same flags only if it is
     unchanged. That way a concurrent `generate` is never overwritten. A
     failed rewrite is logged, not returned.
  5. Call `f(&key)`, drop the key, and return `f`'s result.
- **Locks:** one `Mutex<()>` per store, shared by clones. A poisoned lock
  is recovered (`PoisonError::into_inner`), since it guards no data of its
  own. Vault calls are made under it, but adapter code never is.
- **Zeroization:**
  - `K` is cleared when it drops at the end of `with_key` or `generate`.
    `ZeroizeOnDrop` is a marker, so this depends on the adapter deriving it.
  - Rust moves are copies that leave the old bytes behind. Adapters should
    therefore keep the secret behind a heap pointer, for example
    `Box<[u8; 32]>` inside `K`. The test adapters do.
  - The store's buffers are `Zeroizing`. Copies made by the platform, swap
    and core dumps are outside the process's control.
- **Names:** the blob store's rule (1 to 64 of `[A-Za-z0-9._-]`, not
  starting with `.`). Envelopes are at most 64 KiB.

### 2.2 Keeping the two copies identical

The file is identical in both crates, and so is its `ENGINE_SOURCE`
constant. A libmorph test (`--features mobile`, on the host, where
libmobile's platform-free modules build) asserts
`libmobile::key_store::ENGINE_SOURCE == libdevice::key_store::ENGINE_SOURCE`.
A change is made in libmobile first and copied to libdevice in the same
release cycle.

## 3. Vaults

| Platform | Vault | Enforces | Storage |
|---|---|---|---|
| Android (libmobile) | `AndroidVault` | `FLAG_DEVICE_UNLOCK_REQUIRED` | `SecureStorePlugin` space 1: `<noBackupFilesDir>/libmobile-key-store/`, wrapping keys `libmobile.key-store.v1` and `.unlocked.v1` (`setUnlockedDeviceRequired`), StrongBox unless `secure-store-no-strongbox`. The header's flags bit 0 picks the wrapping key and is authenticated as associated data, as in the blob store. |
| macOS (libdevice) | `KeychainVault` | none | Login Keychain, service `<service>.keys`, account the name, data `[flags][envelope]`. Per-item lock state needs the data-protection Keychain, which needs a signed app with an entitlement, so `FLAG_DEVICE_UNLOCK_REQUIRED` is refused rather than accepted without being enforced. |
| Any (both) | `MemoryVault` | `FLAG_DEVICE_UNLOCK_REQUIRED`, simulated | Per instance; empty when made. |

`KeyStore::android(&app)` (libmobile) and `KeyStore::keychain(service)`
(libdevice, macOS) construct the platform stores.

## 4. libmorph

- Re-exports the active crate's `key_store` module as `libmorph::key_store`,
  with `SecretKey`, `SealToken`, `KeyStore`, `KeyStoreError` and the flags
  at the root: libmobile's on Android, libdevice's everywhere else. There
  is no bridge type and no libmorph `SecretKey`. The two crates' engines
  are identical (2.2), so the signatures do not change with the host.
- `MorphContext::key_store() -> Option<&KeyStore>`, under the
  `secure-store` feature:
  - Android: `KeyStore::android`;
  - macOS: `KeyStore::keychain("libmorph.<SECURE_STORE_ID>")`;
  - scripted runs: `KeyStore::memory()`, new for each run;
  - the web and other desktops: `None`.

## 5. How handlr uses it

handlr's Noise adapter defines `NoiseStatic(Box<[u8; 32]>)`, with
`#[derive(ZeroizeOnDrop)]`, kind "handlr.noise.x25519.v1", and `Public` the
32-byte public key. It is generated with no flags, so background reconnects
work while the phone is locked.

The adapter's snow `CryptoResolver` returns, for the static key, a `Dh`
whose `pubkey()` is the stored public value and whose `dh(peer, out)` is
`store.with_key::<NoiseStatic, _>("noise.static", |k| x25519(k, peer))`.
`set()` receives only a placeholder (the key's name), and `privkey()` is
never called by the handshake. So snow never holds the static private
key; only each DH's output enters its state. Ephemeral keys stay snow's.
`Corrupt`, `WrongKind` and `NotFound` mean "re-pair needed". The server
record stays a blob.

## 6. Steps

1. **Rename first.** Before anything else, in its own commit in each
   repository, `SecureStoreError::Locked` becomes `DeviceLocked` (and the
   Kotlin error kind `locked:` becomes `device-locked:`), on the
   `secure-store` branches of libmobile and libmorph. Owner: "do the rename
   when we implement, before anything else".
   **Done when:** each repository's tests and clippy pass.
2. **The engine, in libmobile.** `src/key_store.rs` as in section 2, with
   `zeroize` as a dependency. Host tests on `MemoryVault`:
   - generate and `public` round-trip;
   - `with_key` lends the key, and calling the store from inside `f`
     neither deadlocks nor errors;
   - another kind gives `WrongKind`;
   - a v2 adapter accepting v1 migrates, and the stored kind is v2
     afterwards;
   - a concurrent `generate` during a migration is not overwritten;
   - `UnsupportedFlags` for the app-unlock bit and unknown bits;
   - `DeviceLocked` with the simulated lock;
   - a malformed envelope gives `Corrupt`;
   - `compile_fail` doc tests: building a `SealToken`, storing one in a
     `static`, and returning the `&K` from `f`.

   **Done when:** `cargo test --workspace` passes and clippy is clean.
3. **`AndroidVault`.** Space 1 in `SecureStorePlugin`, as in section 3.
   The `securestore` example generates a test key on first launch and logs
   its public value on later launches.
   **Done when:** on the owner's Galaxy S23 Ultra:
   - the public value is the same across a force-stop;
   - a key with `FLAG_DEVICE_UNLOCK_REQUIRED` gives `DeviceLocked` with the
     screen locked and works unlocked;
   - `pm clear` gives a new key.
4. **libmobile released** with the blob store, as 0.2.1, on the owner's
   word. **Done when:** merged, tagged `v0.2.1`, pushed.
5. **libdevice.** Its blob store (its plan's blob half, unchanged), the
   engine file copied from libmobile, and `KeychainVault`. Its example
   generates and reports a test key. **Done when:** its tests pass, and
   from a Terminal two runs print the same public value and
   `FLAG_DEVICE_UNLOCK_REQUIRED` gives `UnsupportedFlags`. Then it is
   released as 0.1.1, with the first `VERSIONS.md` entry, on the owner's
   word.
6. **libmorph.** The re-exports and `ctx.key_store()`; `SecureStore`'s
   desktop backends replaced by libdevice's; the `ENGINE_SOURCE` test; the
   `securestore` example's key on both hosts.
   **Done when:** tests and clippy pass on every host, and the example
   keeps its public value across relaunches on the S23 and on macOS.
7. **Release.** libmorph 0.3.1 and libmorph-build 0.2.1, taking libmobile
   0.2.1 and libdevice 0.1.1, on the owner's word. Then the handlr session
   gets the revision to pin.

## 7. Release notes

Everything here is new and unreleased, so the key store is additive in
all three crates. `zeroize` becomes a dependency of libmobile, and of
libdevice and libmorph under `secure-store`.
