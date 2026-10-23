---
title: Key store for keying material
date: 2026-10-22
status: In implementation
covers:
  - One small key store engine (`src/key_store.rs`), the same file in
    libmobile and libdevice, as free functions over a per-platform `Vault`
  - "`SecretKey` adapters: the store generates, seals and validates the
    adapter's private key `K`, returns only `K::Public`, and lends `&K` for
    the length of one call (`with_key`)"
  - A flags byte (device unlock required; app unlock reserved), enforced by
    the platform and refused, never ignored, by software
  - The blob store losing its `&AndroidApp` argument, with the Application
    context from `ndk-context`
  - libmorph re-exporting the active platform's engine, and `ctx.key_store()`
supersedes:
  - docs/design/20261022_05_KEY_STORE_PLAN.md (its body and three
    amendments, consolidated here)
sources:
  - "Owner (2026-10-22): the engine must stay small and manageable; this is
    the important part of secure storage, protecting actual keying material"
  - "Owner (2026-10-22): no `KeyStore` struct; `Vault` is a thin handle, not
    the secret store itself; drop `&AndroidApp` everywhere; `Vault::memory()`
    is for testing only; the rename comes first"
  - "Owner (2026-10-22): on read, stored flags this build does not enforce
    are refused with `UnsupportedFlags`, never ignored"
  - android-activity 0.6.1 `src/init.rs` (`APP_ONCE` registers the JavaVM
    and the Application with `ndk-context`)
  - AGENTS.md, "Platform Layering (libmobile first)"
related:
  - docs/design/20261022_03_SECURE_STORE_PLAN.md (libmorph's blob store;
    its `SecureStore` stops holding an app clone)
  - libmobile docs/design/20261022_03_SECURE_STORE_PLAN.md (the Android
    blob store: its plugin, file header and wrapping keys carry the key
    store's second space)
  - libdevice docs/design/20261022_01_SECURE_STORE_AND_KEY_STORE_PLAN.md
    (its blob store half stays; its key store half is replaced by this plan)
  - handlr docs/design/20261022_04_COMPANION_APP_PLAN.md
---

# Key store for keying material

## 1. Goal

An app keeps private keys on the device without its code reading them.
The key's algorithm lives in an adapter (a black box such as handlr's Noise
adapter). The store generates the key through the adapter, keeps it sealed
by the platform, and gives back its public value. It lends the private key
to the adapter's code only for the length of one call, then drops and
clears it.

libmorph only wraps. libmobile (Android) and libdevice (macOS) each
implement the store with the same API. libmorph re-exports whichever
applies. There is no bridge type and no libmorph `SecretKey`.

## 2. Files

| Path (both crates) | Contents |
|---|---|
| `src/key_store.rs` | The engine. Byte-identical in libmobile and libdevice. Declares `mod vault; pub use vault::Vault;` |
| `src/key_store/vault.rs` | The platform half. Differs per crate. |

Each engine file ends with `#[doc(hidden)] pub const ENGINE_SOURCE: &str =
include_str!("key_store.rs");`. A libmorph test, built with
`--features mobile` on the host (libmobile's platform-free modules build
there), asserts `libmobile::key_store::ENGINE_SOURCE ==
libdevice::key_store::ENGINE_SOURCE`. A change is made in libmobile first
and copied to libdevice in the same release cycle. The engine's unit tests
live in the engine file, under `#[cfg(all(test, feature = "test-vault"))]`,
so they are identical too.

In libmobile the module is in core (no feature), like the blob store. In
libdevice and libmorph it is under `secure-store`. `zeroize` (with the
`derive` feature) and `log` are dependencies of libmobile, and of libdevice
and libmorph under `secure-store`.

## 3. The engine (`src/key_store.rs`)

```rust
use std::sync::{Mutex, PoisonError};
use zeroize::{ZeroizeOnDrop, Zeroizing};

/// Proof that the store is the caller. Not Clone, Copy or Default; made only
/// inside this crate; lent by reference, so it cannot outlive a call.
pub struct SealToken(());

/// The platform keeps the key usable only while the device is unlocked.
pub const FLAG_DEVICE_UNLOCK_REQUIRED: u8 = 1 << 0;
/// Reserved for keys the app must unlock before use (a later plan).
/// Refused on write and on read until then.
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
    /// Reserved: the key must be unlocked by the app first. Never returned yet.
    Locked,
    /// Flags this build does not enforce, or unknown bits; the flags byte.
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

/// A new key called `name`, replacing any earlier one. Returns its public
/// value; the private key is dropped (and cleared) before return.
pub fn generate<K: SecretKey>(vault: &Vault, name: &str, flags: u8) -> Result<K::Public, KeyStoreError>;
/// The public value of the key called `name`.
pub fn public<K: SecretKey>(vault: &Vault, name: &str) -> Result<K::Public, KeyStoreError>;
/// Lend the private key to `f` for this call only. `f` cannot return the
/// borrow. The lock is not held while `f` runs, so `f` may use the store.
pub fn with_key<K: SecretKey, R>(vault: &Vault, name: &str, f: impl FnOnce(&K) -> R) -> Result<R, KeyStoreError>;
pub fn delete(vault: &Vault, name: &str) -> Result<bool, KeyStoreError>;

static LOCK: Mutex<()> = Mutex::new(());
```

The engine uses these private operations of `Vault` (section 4):
`supported_flags() -> u8`, `get(name) -> Result<Option<(u8, Zeroizing<Vec<u8>>)>>`
(flags and envelope together), `put(name, flags, envelope)` and
`delete(name) -> Result<bool>`.

### 3.1 Envelope

`[kind length: u8][kind: UTF-8][adapter's sealed bytes]`, at most 64 KiB,
held in `Zeroizing`. The flags byte is not in it: the vault keeps the flags
outside the envelope and authenticates them with it (section 4).

### 3.2 Behaviour

- **Names:** 1 to 64 of `[A-Za-z0-9._-]`, not starting with `.`;
  otherwise `InvalidName`, before any platform call.
- **`generate`:**
  1. Check the name, and that `K::KIND` is 1 to 255 bytes (a `KIND`
     outside that is a programming error: `Platform("kind length")`).
  2. Refuse `flags & !vault.supported_flags()` with `UnsupportedFlags(flags)`.
     That covers `FLAG_APP_UNLOCK_REQUIRED` everywhere until its plan, and
     unknown bits.
  3. `K::generate(&token)`, then `seal`, then build the envelope. Take
     `LOCK`, `vault.put`, release.
  4. Return `key.public()`; `key` drops here.
- **`with_key`** (and `public`, which does the same and calls `public()`):
  1. Take `LOCK`, `vault.get`, release. `None` is `NotFound`.
  2. **Fail safe on flags.** If `stored_flags & !vault.supported_flags() != 0`,
     return `UnsupportedFlags(stored_flags)`. A build that does not enforce
     a bit never hands out the key. (Once the app-unlock plan exists, such
     keys return `Locked` until unlocked.) Software refuses on a flags
     byte; it never grants on one.
  3. Parse the envelope (`Corrupt` if malformed or the kind is not UTF-8).
     `WrongKind(kind)` unless `K::accepts(kind)`.
  4. `K::unseal(kind, bytes, &token)`, mapping `Err(s)` to `Corrupt(s)`.
  5. If `kind != K::KIND`, migrate: `seal` again and build the new
     envelope. Take `LOCK`, `vault.get` again, and `vault.put` with the
     **same stored flags** only if the envelope read now equals the one
     read in step 1, so a concurrent `generate` is never overwritten.
     Release. A failed rewrite is `log::warn!`ed, not returned.
  6. Call `f(&key)`, drop `key`, return `Ok(result)`.
- **`delete`:** check the name; under `LOCK`, `vault.delete`.
- **Lock:** one process-wide `static LOCK: Mutex<()>`. It is held only
  around vault calls, never while adapter code (`generate`, `seal`,
  `unseal`, `f`) runs, so adapter code may use the store. A poisoned lock
  is recovered with `PoisonError::into_inner`, since it guards no data.
- **Zeroization:** `K` is cleared when it drops at the end of `with_key`
  or `generate`; `ZeroizeOnDrop` is only a marker, so this depends on the
  adapter's derive. Rust moves are copies that leave the old bytes behind,
  so adapters keep the secret behind a heap pointer (`Box<[u8; 32]>`). The
  store's own buffers are `Zeroizing`. Platform copies, swap and core
  dumps are out of scope.

## 4. `Vault` (`src/key_store/vault.rs`, per crate)

`Vault` is a thin, `Clone`, `'static` handle over the OS and not a security
boundary. The OS protects the keys (Keystore's non-exportable wrapping keys
and their unlock rule; the Keychain's access list); the engine adds the
token, the kind check and `with_key`'s scope.

```rust
#[derive(Clone)]
pub struct Vault { /* Named(String) | Memory(Arc<MemoryState>) under test-vault */ }
impl Vault {
    /// Namespace rule: 1 to 64 of [A-Za-z0-9._-], not starting with '.'.
    pub fn named(namespace: &str) -> Result<Vault, KeyStoreError>;   // InvalidName
    #[cfg(feature = "test-vault")] pub fn memory() -> Vault;
    /// Simulates a locked device for FLAG_DEVICE_UNLOCK_REQUIRED. No effect
    /// on a named vault.
    #[cfg(feature = "test-vault")] pub fn set_device_locked(&self, locked: bool);
}
```

| Platform | `supported_flags` | Storage |
|---|---|---|
| Android (libmobile) | `FLAG_DEVICE_UNLOCK_REQUIRED` | `SecureStorePlugin` space 1: directory `<noBackupFilesDir>/libmobile-key-store/<namespace>/`, file `<name>.blob`, the blob store's file header (`LMSS`, version, flags, IV length, IV). Shared wrapping keys `libmobile.key-store.v1` and `libmobile.key-store.unlocked.v1` (`setUnlockedDeviceRequired(true)`), StrongBox unless `secure-store-no-strongbox`. Header flags bit 0 picks the wrapping key. The GCM associated data is the 7 header bytes, the namespace and the name, so a changed flag or an item moved between namespaces or names fails to decrypt. Unlike the blob store, the key store space stores and authenticates any flags byte; the Rust side refuses bits it does not enforce (3.2). Reading an item wrapped by the unlocked key while `KeyguardManager.isDeviceLocked`, or a `UserNotAuthenticatedException`, is `"device-locked: ..."` in Kotlin and `DeviceLocked` in Rust. |
| macOS (libdevice) | none | Login Keychain generic password, service `<namespace>.keys`, account = name, data `[flags: u8][envelope]`. `FLAG_DEVICE_UNLOCK_REQUIRED` is refused as `UnsupportedFlags`: per-item lock state needs the data-protection Keychain, which an unsigned binary cannot use. `errSecItemNotFound` (-25300) is `NotFound`/`Ok(None)`; `errSecInteractionNotAllowed` (-25308, a Keychain that cannot prompt, e.g. ssh or a non-GUI shell) is `Platform`. |
| Memory (both, `test-vault`) | `FLAG_DEVICE_UNLOCK_REQUIRED`, simulated | A per-instance `Mutex<HashMap<String, (u8, Zeroizing<Vec<u8>>)>>` and an `AtomicBool`; no globals; empty when made. `get` of an item with bit 0 set while locked is `DeviceLocked`. |

The engine and the memory vault compile on every host. Only
`Vault::named` is platform-specific: libmobile on Android, libdevice on
macOS. Linux, Windows and the web have no named vault, so `Vault::named`
does not exist there, but libdevice's tests and libmorph's scripted runs
still get `Vault::memory()` under `test-vault`.

### 4.1 Android context

libmobile depends on `ndk-context = "0.1"` directly and reads
`ndk_context::android_context()` for the JavaVM and the **Application**
object, which android-activity 0.6.1 registers once per process before
`android_main`. There is no slot of libmobile's own and no lifecycle
bookkeeping; it is valid across `android_main` runs and from any thread.
`SecureStorePlugin` takes a `Context` (the Application), not an `Activity`,
finds its class through the Application's class loader, and reaches
Keystore, `noBackupFilesDir` and `KeyguardManager` from it. On Android,
`Vault::named(namespace)` holds only the namespace.

### 4.2 macOS context

None. `SecItem` functions are process-global, scoped by the login session
and the code signature, so `Vault::named` holds everything. A cargo-built
binary is ad-hoc signed anew on each build, so the Keychain prompts again
for items an earlier build created. Documented in libdevice's example: an
optional stable local identity (`security create-keypair`-style self-signed
code-signing certificate in the login Keychain, then
`codesign -s <identity> target/debug/<exe>`) avoids the prompts.

## 5. The blob store without `&AndroidApp`

libmobile's blob store uses the same Application context:
`secure_store::{get, put, put_with, delete, security_level}(name, ..)`,
plus `android::SecureStore`, a unit struct with the same methods, so it
has libdevice's shape. libmorph's `SecureStore` stops holding an app clone.
It is part of this plan because the two stores share `SecureStorePlugin`.
The key store needs the plugin to take a `Context` (the Application) and a
space argument, and that changes the blob store's JNI calls too. So the
blob store moves off `&AndroidApp` at the same time. This departs from the
frozen libmobile and libmorph secure-store plans and is recorded in each
repository's CHANGES.md when it is made (Rule 1.7). The macOS blob store's
handling of `unlocked_only` is unchanged by this plan.

## 6. libmorph

- Re-exports the active crate's `key_store` module as `libmorph::key_store`
  (`SecretKey`, `SealToken`, `Vault`, `KeyStoreError`, the flags and the
  four functions): libmobile's on Android, libdevice's on macOS. Nothing on
  other hosts.
- `MorphContext::key_store() -> Option<&Vault>`, under `secure-store`:
  - Android: `Vault::named("libmorph.<SECURE_STORE_ID>")`, the id
    defaulting to the package name;
  - macOS: the same namespace, the id defaulting to the executable's file
    stem;
  - scripted runs (`script`, which enables libdevice's `test-vault`):
    `Vault::memory()`, new for each run;
  - the web, Linux, Windows: `None`.

  `SECURE_STORE_ID` is the one libmorph's secure store plan defines. Apps
  may also make their own vaults with `Vault::named`, one per paired
  server for example.
- A normal app build has no memory vault: `test-vault` is on only in
  libmobile's and libdevice's own tests and through libmorph's `script`.

## 7. How handlr uses it

handlr's Noise adapter defines `NoiseStatic(Box<[u8; 32]>)` with
`#[derive(ZeroizeOnDrop)]`, kind `"handlr.noise.x25519.v1"`, `Public` the
32-byte public key, generated with no flags so background reconnects work
while the phone is locked. Its snow `CryptoResolver` returns, for the
static key, a `Dh` that owns a `Vault` clone: `pubkey()` is the stored
public value and `dh(peer, out)` is
`key_store::with_key::<NoiseStatic, _>(&vault, "noise.static", |k| x25519(k, peer))`.
`set()` receives only a placeholder and `privkey()` is never called, so
snow never holds the static private key; only each DH's output enters its
state. Ephemeral keys stay snow's. `Corrupt`, `WrongKind` and `NotFound`
mean "re-pair needed". The server record stays a blob.

## 8. Steps

1. **Rename first**, in its own commit in each repository before anything
   else: `SecureStoreError::Locked` becomes `DeviceLocked` in libmobile,
   libdevice and libmorph; the Kotlin error kind `locked:` becomes
   `device-locked:`; libmobile's `Jni` variant becomes `Platform`
   (unreleased).
   **Done when:** each repository's `cargo test` and clippy pass, and the
   `securestore` example on the S23 Ultra reports `DeviceLocked` for an
   `unlocked_only` blob read with the screen locked.
2. **The Application context and the blob store without `&app`**
   (libmobile, then libmorph): `ndk-context` dependency, `SecureStorePlugin`
   over a `Context`, the free functions and `android::SecureStore`
   (section 5), CHANGES.md addenda.
   **Done when:** the blob store tests pass; on the S23 Ultra the example's
   blobs survive a force-stop, and a blob written from a spawned thread
   with no `AndroidApp` in reach reads back on the main thread.
3. **The engine, in libmobile.** `src/key_store.rs` (section 3) and the
   memory variant of `src/key_store/vault.rs` behind `test-vault`. Unit
   tests on `Vault::memory()`, run with `--features test-vault`:
   - `generate` then `public` round-trip; `generate` again replaces;
   - `with_key` lends the key, and calling the store from inside `f`
     neither deadlocks nor errors;
   - another kind gives `WrongKind(stored)`;
   - a v2 adapter accepting v1 migrates, the stored kind is v2 afterwards,
     and the stored flags are unchanged;
   - a concurrent `generate` during a migration is not overwritten: a test
     adapter's `unseal` (which runs between the first read and the
     migration's write) calls `generate` on the same vault and name, and
     afterwards the stored key is the newly generated one, not the
     migrated one;
   - `UnsupportedFlags(0b10)` from `generate` with the app-unlock bit, and
     for an unknown bit;
   - an item put directly through the vault with flags `0b10` gives
     `UnsupportedFlags(0b10)` from `with_key` and from `public`;
   - `DeviceLocked` with `set_device_locked(true)` for a bit-0 key, and a
     no-flags key still readable;
   - a malformed envelope (empty; kind length past the end) gives
     `Corrupt`; `NotFound` for an absent name; `InvalidName` for `.x`;
   - `compile_fail` doc tests: building a `SealToken`, storing one in a
     `static`, returning `&K` from `f`, and `Vault::memory()` without
     `test-vault`.

   **Done when:** `cargo test --features test-vault` passes on the host
   and clippy is clean.
4. **`Vault::named` on Android.** Space 1 in `SecureStorePlugin`
   (section 4). The `securestore` example generates a test key in namespace
   `libmobile.example` on first launch and logs its public value after.
   **Done when:** on the S23 Ultra: the public value is the same across a
   force-stop; a key with `FLAG_DEVICE_UNLOCK_REQUIRED` gives
   `DeviceLocked` with the screen locked and works unlocked; `pm clear`
   gives a new key; a `.blob` copied to another name or namespace with
   `run-as` gives `Corrupt`.
5. **libmobile 0.2.1** released with the blob store, on the owner's word.
   **Done when:** merged, tagged `v0.2.1`, pushed, VERSIONS.md entry.
6. **libdevice.** Its blob store (its plan's blob half, with the rename),
   the engine file copied byte for byte, `src/key_store/vault.rs` with the
   Keychain and memory variants, `test-vault`, and the command-line
   example generating and reporting a test key.
   **Done when:** `cargo test --features secure-store,test-vault` passes;
   from a macOS Terminal two runs print the same public value,
   `FLAG_DEVICE_UNLOCK_REQUIRED` gives `UnsupportedFlags(1)`, and
   `security find-generic-password -s libdevice.example.keys` shows the
   item. Then **libdevice 0.1.1** is released on the owner's word, with the
   first VERSIONS.md entry.
7. **libmorph.** The re-exports, `ctx.key_store()`, `SecureStore`'s desktop
   backend replaced by libdevice's, the `ENGINE_SOURCE` test, `script`
   enabling `test-vault`, and the `securestore` example's key on both
   hosts; CHANGES.md addendum for the blob store change.
   **Done when:** tests and clippy pass with and without `mobile`, the
   example keeps its public value across relaunches on the S23 Ultra and
   from a macOS Terminal, and a scripted run gets a fresh memory vault.
8. **Release.** libmorph 0.3.1 and libmorph-build 0.2.1, taking libmobile
   0.2.1 and libdevice 0.1.1, on the owner's word. Then the handlr session
   gets the revision to pin.
   **Done when:** tagged `v0.3.1` and `libmorph-build-v0.2.1`, pushed.

## 9. Release notes

Everything here is unreleased, so the key store is additive in all three
crates: patch bumps. `zeroize`, `log` and (libmobile) `ndk-context` are
new dependencies as in section 2.

## Amendment — 2026-10-22 20:32 EDT: implementation has started; the document is now immutable

The owner said it is ready to build (2026-10-22). The work is on the
`secure-store` branches of libmobile and libmorph, in their worktrees.
Step 1 is done in both: libmobile `b02c823` and this commit. libdevice has
no store yet, so its types start with `DeviceLocked` (step 6).
