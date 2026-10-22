---
title: Opaque keys in the secure store
date: 2026-10-22
status: Draft
covers:
  - A `SecretKey` trait that an adapter (a "black box" such as handlr's
    Noise adapter) implements for its own private key type `K`, with a
    public opaque type `P`
  - `SecureStore::generate_key::<K>`, `key::<K>` and `delete_key`, which keep
    `K` sealed in the store and hand out a `KeyHandle<K>`
  - A `SealToken` that only libmorph can make, so only the store can turn
    a `K` into bytes or back
sources:
  - Owner (2026-10-22), on handlr's Noise key: "can we use a trait that
    allows generation/validation but not reading?", then "the adapter
    implements noise and appears to the session store as an opaque value,
    we probably need a plan for this", and "putting in libmorph just moves
    the issue down a level, and there isn't a common crate behind both
    libmobile and libdevice, the idea is the key management logic stays in
    the black box passed into the secure store, so we still have a wrapping
    trait, but the actual interface still stays private, and the generic
    aspect is a type (K), and (P) (public opaque key/handle)".
  - snow 0.10 (`src/builder.rs`, `src/types.rs`): the static key reaches
    snow through `Builder::local_private_key(&[u8])` and its
    `CryptoResolver`/`Dh`, both inside the adapter.
related:
  - docs/design/20261022_03_SECURE_STORE_PLAN.md (the store this builds on;
    must be implemented first)
  - libmobile docs/design/20261022_03_SECURE_STORE_PLAN.md (unchanged: it
    already keeps opaque bytes)
  - handlr docs/design/20261022_04_COMPANION_APP_PLAN.md (its `KeyStore`
    trait; its Noise adapter would implement `SecretKey`)
---

# Opaque keys in the secure store

## 1. Goal

An app keeps a private key in the secure store without any of its code,
other than the key's own adapter, being able to read it. The store knows
nothing about the key's algorithm: Noise, signing or anything else stays
in the adapter. The store generates the key through the adapter, keeps it
sealed, checks it when it is loaded, and hands out a handle. The handle
gives the adapter's public opaque value `P` to anyone, and the private key
`K` only as a type whose contents only the adapter can see.

Nothing is enforced by convention. Two Rust rules do the work:

- `K` is the adapter's own type with private fields, so only the
  adapter's crate can look inside it.
- Turning a `K` into bytes, or bytes into a `K`, needs a `SealToken`, which
  only libmorph can make. So only the store can do it, even though the
  adapter writes those functions.

libmobile does not change. Its store already keeps opaque bytes; the
sealing and the namespace are libmorph's, the same on every host.

## 2. API

In `src/secure_store.rs`, re-exported at the crate root, under the
`secure-store` feature like the rest of the store.

```rust
/// Proof that the store is the caller. Only libmorph can make one, so
/// `SecretKey::seal` and `unseal` can only be called by the store.
pub struct SealToken(());

/// A private key type, implemented by its adapter. Keep the fields
/// private: whoever can see them can read the key.
pub trait SecretKey: Sized + Send + Sync + 'static {
    /// The public opaque value: a public key, or any handle the adapter
    /// wants to expose.
    type Public: Clone + Send + Sync + 'static;

    /// Names the key type and its encoding in the sealed bytes, for example
    /// "handlr.noise.x25519.v1". A key stored under one kind does not load
    /// as another.
    const KIND: &'static str;

    /// A new key. Called only by the store.
    fn generate(token: SealToken) -> Self;

    /// The key's encoding. Called only by the store, which writes it to the
    /// platform store and clears its copy.
    fn seal(&self, token: SealToken) -> Vec<u8>;

    /// Decode and validate. An error means the bytes are not a valid key of
    /// this kind; the store reports it as `SecureStoreError::Corrupt`.
    fn unseal(bytes: &[u8], token: SealToken) -> Result<Self, String>;

    fn public(&self) -> Self::Public;
}

/// A key in the store. Cheap to clone.
pub struct KeyHandle<K: SecretKey> {
    key: Arc<K>,
    public: K::Public,
}

impl<K: SecretKey> KeyHandle<K> {
    /// The public opaque value `P`.
    pub fn public(&self) -> &K::Public;
    /// The key, for the adapter's own operations (a DH, a signature). The
    /// app can pass it around but cannot look inside it.
    pub fn key(&self) -> &K;
}

impl SecureStore {
    /// Generate a new key called `name`, replacing any earlier one, and
    /// store it sealed.
    pub fn generate_key<K: SecretKey>(&self, name: &str) -> Result<KeyHandle<K>, SecureStoreError>;
    /// Load the key called `name`. `None` if there is none; `Corrupt` if
    /// it is of another kind or fails `unseal`.
    pub fn key<K: SecretKey>(&self, name: &str) -> Result<Option<KeyHandle<K>>, SecureStoreError>;
    /// Remove it. `true` if there was one.
    pub fn delete_key(&self, name: &str) -> Result<bool, SecureStoreError>;
}
```

`generate_key` and `key` accept the same `PutOptions` through
`generate_key_with`. For handlr's Noise key, the defaults are right, so
background reconnects keep working.

### 2.1 How keys are kept

- Key names follow the blob rules, but are at most 60 bytes. A key is
  stored as the blob `key.<name>`.
- The public `get`, `put`, `put_with` and `delete` refuse any name that
  starts with `key.`, returning `InvalidName`. So no code outside the store
  can read or replace a sealed key through the blob API.
- Sealed blob layout: one byte with the length of `KIND`, then `KIND` in
  UTF-8, then `seal`'s bytes. `key::<K>` checks `KIND` before calling
  `unseal`.
- The store clears its copies of the sealed bytes after writing or
  reading them. The adapter should clear `K` when it is dropped, for
  example with zeroize, which is the adapter's own dependency.
- On Android the blob is sealed again underneath by libmobile's Keystore
  key, as every blob is. On macOS it is a Keychain item. In scripted runs
  it is in memory.

### 2.2 What stays out

- No algorithm in the store, and no snow dependency in libmorph or
  libmobile.
- No hardware-held private keys. `K` is in the app's memory while it is
  in use. Keystore X25519 would keep it in the TEE, but Noise would then be
  in the store's interface.
- No libmobile change. A libmobile-only app gets this only if a later plan
  adds the same trait there.

## 3. How handlr uses it

This section is not part of libmorph, and is shown to check that the
interface fits.

- handlr's Noise adapter defines `NoiseStatic` with private
  `[u8; 32]` X25519 bytes, cleared on drop. Its `Public` is the 32-byte
  public key, and `KIND` is "handlr.noise.x25519.v1".
- `unseal` checks the length, and that the public key derived from the
  bytes is not all zeros.
- The adapter owns a snow `CryptoResolver` and `Dh`. It gives snow the
  static key from `handle.key()` inside its own crate, so only the adapter
  ever holds the raw bytes.
- The session code only sees `KeyHandle<NoiseStatic>` and its public key.
- The server record stays an ordinary blob (`server`), since it is not
  secret in this sense.

## 4. Steps

1. **Trait, token and handle.** `SealToken`, `SecretKey`, `KeyHandle`,
   `generate_key`, `generate_key_with`, `key`, `delete_key`, the reserved
   `key.` prefix, and the kind header, on every backend.
   **Done when:** unit tests with a test key type pass on the memory
   backend. They check that a stored key loads back with the same
   `public()`; that loading it as another kind is `Corrupt`; that
   `get("key.x")` is `InvalidName`; and that a doc test fails to compile
   when calling `seal` without a token.
2. **On the devices.** The `securestore` example also generates a test key
   at first launch, then loads it and logs its public value on later
   launches.
   **Done when:** on the owner's Galaxy S23 Ultra the public value is the
   same across a force-stop, and on macOS across a relaunch.

## 5. Release

Additive: new types, a new trait and new methods. The reserved `key.`
prefix does refuse names that `get`/`put` accepted before. That is not
strictly additive, but no released version has the store, so it ships in
the same release as the secure store (libmorph 0.3.1).

## 6. Open questions for the owner

1. **libmobile.** Should libmobile get the same trait for apps that use it
   without libmorph, or is libmorph the only place?
2. **Generation.** `generate` gets no RNG from the store; the adapter uses
   its own (`OsRng`, or snow's). Should the store pass one in, so a
   scripted run could make repeatable keys?

## Amendment — 2026-10-22 19:08 EDT: a key store of its own, built in libmobile and libdevice first

Agreed with the owner (2026-10-22): "libmobile and libdevice will both
need wrapped implementations with the same API", then "keep the plan in
libmorph's plan for now, part of it will be writing and implementing the
individual plans, this is also a little different than just a protected
symmetric secure store. I also don't think `key.` should be a particular
meaning, this should be a specific API for this type of operation".

This replaces the `SecureStore` methods in section 2, all of section
2.1, the libmobile items in 2.2 and the `related` note on libmobile,
sections 4 and 5, and open question 1, which is settled: both crates get
it.

### What changes

- **A key store, not blob names.** Keys made by an adapter (asymmetric or
  any other kind the adapter defines) are not blobs. No blob name has a
  special meaning, so the reserved `key.` prefix and the 60-byte limit
  are dropped, and `get`/`put` are unchanged. Keys get their own API,
  their own storage and their own error type. Names follow the same
  1-to-64-character rule. A key and a blob can share a name without
  meeting.
- **libmorph wraps; libmobile and libdevice implement.** Each of the two
  crates defines the same API: `SecretKey`, a `SealToken` only it can
  make, `KeyHandle`, `KeyStore`, `KeyOptions` and `KeyStoreError`.
  libmobile's runs on Android and libdevice's on the desktop. libmorph has
  its own `SecretKey` and `SealToken`, the only ones apps implement and
  see, and bridges them to the backend's trait through a private wrapper
  type. The wrapper implements the backend's `SecretKey` and calls the
  adapter's methods with libmorph's own token. So the sealing interface
  stays private at every level, and no shared crate is needed.
- **The same API in both crates.** Both plans state the same names,
  signatures, error variants, name rule, sealed layout (kind length, kind,
  sealed bytes) and options, and each crate's tests check them.

### The API (each crate; libmorph's wrapper has the same shape)

```rust
pub struct SealToken(());                  // crate-private constructor
pub trait SecretKey: Sized + Send + Sync + 'static {
    type Public: Clone + Send + Sync + 'static;
    const KIND: &'static str;
    fn generate(token: SealToken) -> Self;
    fn seal(&self, token: SealToken) -> Vec<u8>;
    fn unseal(bytes: &[u8], token: SealToken) -> Result<Self, String>;
    fn public(&self) -> Self::Public;
}
pub struct KeyHandle<K: SecretKey>;        // public() -> &K::Public, key() -> &K; Clone
#[non_exhaustive] #[derive(Default)]
pub struct KeyOptions { pub unlocked_only: bool }

#[derive(Clone)]                           // Send + Sync
pub struct KeyStore;
impl KeyStore {
    pub fn generate<K: SecretKey>(&self, name: &str) -> Result<KeyHandle<K>, KeyStoreError>;
    pub fn generate_with<K: SecretKey>(&self, name: &str, options: KeyOptions)
        -> Result<KeyHandle<K>, KeyStoreError>;
    pub fn load<K: SecretKey>(&self, name: &str) -> Result<Option<KeyHandle<K>>, KeyStoreError>;
    pub fn delete(&self, name: &str) -> Result<bool, KeyStoreError>;
}

#[non_exhaustive]
pub enum KeyStoreError {
    InvalidName,
    /// Stored as another kind; the stored kind is given.
    WrongKind(String),
    /// `unseal` refused the bytes, or they fail to decrypt.
    Corrupt(String),
    Locked,
    Platform(String),
}
```

In libmorph: `MorphContext::key_store() -> Option<&KeyStore>`, under
the `secure-store` feature, `None` where `secure_store()` is.

### Storage, separate from the blob store

- **libmobile:** directory `<noBackupFilesDir>/libmobile-key-store/`,
  sealed under its own Keystore wrapping keys
  (`libmobile.key-store.v1`, and `libmobile.key-store.unlocked.v1` for
  `unlocked_only`). These follow the blob store's StrongBox rule and
  `secure-store-no-strongbox`. The file layout is the blob store's, with
  the kind header inside the encrypted part.
- **libdevice:** the macOS login Keychain, service `<app>.keys` (blobs
  use `<app>`, where libmorph passes `libmorph.<SECURE_STORE_ID>`), and a
  memory backend for emulated or scripted runs.

### Moving the desktop store to libdevice

The owner agreed (2026-10-22) that the desktop backends belong in
libdevice. So libdevice's plan also takes the blob store's Keychain and
memory backends from libmorph's secure-store plan, and libmorph's
`SecureStore` wraps libdevice on the desktop as it wraps libmobile on
Android. That departs from `20261022_03_SECURE_STORE_PLAN.md` (frozen),
and is recorded in CHANGES.md.

### Steps (replace section 4)

1. **libmobile plan.** Write libmobile's key store plan with the API
   above, the storage, an example and an S23 check; discuss it with the
   owner. **Done when:** the owner has agreed to it.
2. **libdevice plan.** Write libdevice's plan for a `secure-store`
   feature: the blob store (Keychain and memory, moved from libmorph) and
   the key store, with the same API as libmobile; discuss it with the
   owner. **Done when:** the owner has agreed to it.
3. **libmobile implemented and released**, with the blob store, as 0.2.1.
   **Done when:** its plan's steps are done and the release is pushed and
   tagged.
4. **libdevice implemented and released.** **Done when:** its plan's steps
   are done and the release is pushed and tagged.
5. **libmorph wraps both.** `SecretKey`, `SealToken`, `KeyHandle`,
   `KeyStore` and `ctx.key_store()` bridge to the backends, and
   `SecureStore`'s desktop backends are replaced by libdevice's. Unit
   tests with a test key type run on the memory backend: a stored key
   loads with the same `public()`, loading as another kind gives
   `WrongKind`, and a doc test fails to compile when calling `seal`
   without a token. **Done when:** they pass, clippy is clean on every
   host, and the `securestore` example generates a test key on first
   launch and logs the same public value after a relaunch, on the S23 and
   on macOS.

### Release (replaces section 5)

Additive in libmobile (0.2.1, with the blob store). libdevice gets a minor
bump, since it gains platform code and an optional dependency. libmorph
takes both and checks Rule 5.5 for the libdevice bump before choosing its
version.

## Amendment — 2026-10-22 19:16 EDT: the plan as it now stands

On the owner's instruction (2026-10-22, "write the updated plan"). This
states the whole plan in one place. Where it disagrees with the text or
amendments above, this wins. It includes what was settled after the
amendment "a key store of its own".

### Layering

libmorph only wraps; it does not implement platform services (AGENTS.md,
"Platform Layering (libmobile and libdevice first)"). libmobile provides
both stores on Android and libdevice on the desktop, with the same API.
libmorph keeps only what a wrapper adds: one API on every host, its own
`SecretKey` and `SealToken` bridged to each backend, `ctx.secure_store()`
and `ctx.key_store()`, and memory stores for scripted runs (taken from
libdevice's memory backend).

### The two stores (same API in libmobile and libdevice)

- **Blob store `SecureStore`**: `get`, `put`, `put_with(PutOptions {
  unlocked_only })`, `delete`, `security_level`. Names are 1 to 64 of
  `[A-Za-z0-9._-]`, not starting with `.`; values are at most 64 KiB.
  libmobile gains an `android::SecureStore` struct
  (`SecureStore::new(&app)`) next to its functions, so its shape matches
  libdevice's (`SecureStore::keychain(service)` on macOS,
  `SecureStore::memory()`). `SecurityLevel`'s variants are per platform.
- **Key store `KeyStore`**: `generate`, `generate_with(KeyOptions {
  unlocked_only })`, `load`, `delete`, over an adapter's `K:
  SecretKey` (`Public` is `P`, `KIND`, `generate`, `seal` and
  `unseal` taking the crate's `SealToken`, and `public`). It hands out
  `KeyHandle<K>` with `public()` and `key()`. Its storage is its own:
  - libmobile: `libmobile-key-store/` and its own Keystore wrapping keys;
  - libdevice: Keychain service `<service>.keys`, or memory.

  The sealed form is `[kind length][kind][sealed bytes]`, encrypted as a
  whole, with `WrongKind` for another kind. `generate` takes no RNG.
- **Errors.**
  - `SecureStoreError`: `InvalidName`, `TooLarge`, `Keystore`,
    `DeviceLocked`, `Io`, `Corrupt`, `Platform`.
  - `KeyStoreError`: `InvalidName`, `WrongKind`, `Corrupt`,
    `DeviceLocked`, `Locked`, `Platform`.
  - `DeviceLocked` means the device's screen is locked and the item was
    stored `unlocked_only`.
  - `Locked` is reserved for keys that must be unlocked before use, as
    ConnectBot does. Its unlock flow (passphrase or platform
    authentication, how long a key stays unlocked) is a later plan, and
    nothing returns it yet.
  - libmobile's `Jni` becomes `Platform`. Nothing is released, so no
    existing error is renamed.
- **StrongBox** stays the default on Android, with the negative
  `secure-store-no-strongbox` (libmorph: `mobile-secure-store-no-strongbox`).
  `unlocked_only` is enforced on Android only. On macOS the login Keychain
  accepts it without enforcing it.

### Steps (replace every earlier list)

1. **Rename first.** In each repository, before anything else in its
   implementation, the error variant `Locked` becomes `DeviceLocked` (and
   the Kotlin error kind `locked:` becomes `device-locked:`), in its own
   commit. In libmobile and libmorph this is on the `secure-store`
   branches. In libdevice it is in the plan's types from the start.
   **Done when:** each repository's tests and clippy pass after the rename.
2. **libmobile key store plan** (`20261022_04_KEY_STORE_PLAN.md`) agreed
   with the owner. **Done when:** agreed.
3. **libdevice plan** (`20261022_01_SECURE_STORE_AND_KEY_STORE_PLAN.md`)
   agreed with the owner. **Done when:** agreed.
4. **libmobile implemented**: `Jni` to `Platform`, the
   `android::SecureStore` struct, then the key store plan's steps. It is
   released with the blob store as 0.2.1, merged, tagged and pushed on the
   owner's word. **Done when:** that plan's steps are done and the release
   is pushed.
5. **libdevice implemented**: that plan's steps. It is released as 0.1.1
   (its first release under the critical rules, with the first
   `VERSIONS.md` entry), tagged `v0.1.1`, and pushed on the owner's word.
   **Done when:** that plan's steps are done and the release is pushed.
6. **libmorph wraps both**, on its `secure-store` branch:
   - `SecureStore` wraps libmobile on Android and libdevice on the
     desktop, and libmorph's own Keychain code is removed;
   - `KeyStore`, `ctx.key_store()`, libmorph's `SecretKey` and
     `SealToken`, and the private bridge type.

   Unit tests with a test key on the memory store check the public value
   round-trip and `WrongKind`, and a `compile_fail` doc test checks
   `seal` without a token. The `securestore` example generates a key on
   first launch and logs the same public value after a relaunch.
   **Done when:** tests and clippy pass on every host, and the example
   works on the owner's S23 and from a Terminal on macOS.
7. **Release.** libmorph and libmorph-build take libmobile 0.2.1 and
   libdevice 0.1.1, both semver-compatible. libmorph's public changes are
   additive (nothing in the store has been released), so it is 0.3.1, and
   libmorph-build 0.2.1. **Done when:** merged, tagged and pushed on the
   owner's word, and the handlr session has the revision to pin.

### For handlr

handlr's Noise adapter implements libmorph's `SecretKey` for its
X25519 static key (kind "handlr.noise.x25519.v1", `Public` the 32-byte
public key). It is generated and loaded through `ctx.key_store()` without
`unlocked_only`, so background reconnects work while the phone is locked.
The adapter gives snow the raw bytes inside its own crate, through its own
`CryptoResolver`. The server record stays a blob. `Corrupt` and
`WrongKind` mean "re-pair needed".

## Amendment — 2026-10-22 19:28 EDT: token by reference, zeroization, kind migration

Agreed with the owner (2026-10-22) after a review of the plan the owner
relayed. Replaces the `SecretKey` and `SealToken` definitions in every
earlier section and amendment. Adds to "the plan as it now stands":
these apply to libmobile, libdevice and libmorph alike, and the tests join
steps 4 to 6.

- **`SealToken` is passed by reference** and implements neither `Clone`,
  `Copy` nor `Default`. The trait's methods take `token: &SealToken`, so a
  token cannot outlive the call: no adapter can keep one or hand it to
  other code. Non-`Clone` alone would still let a token be moved into a
  static.
- **Zeroization.** `SecretKey` requires `zeroize::ZeroizeOnDrop`, which
  adapters get with `#[derive(ZeroizeOnDrop)]` on a type whose fields are
  zeroizable. It is a marker trait: it states the requirement in the type
  system, but a hand-written impl could still skip clearing. The store
  holds its own copies of sealed and decrypted bytes in
  `Zeroizing<Vec<u8>>`. `zeroize` becomes a dependency of libmobile,
  libdevice and libmorph.
- **Migrating a kind.** The trait becomes:

  ```rust
  pub trait SecretKey: zeroize::ZeroizeOnDrop + Sized + Send + Sync + 'static {
      type Public: Clone + Send + Sync + 'static;
      const KIND: &'static str;
      /// Kinds `unseal` can read: `KIND` and any older encodings.
      fn accepts(kind: &str) -> bool { kind == Self::KIND }
      fn generate(token: &SealToken) -> Self;
      fn seal(&self, token: &SealToken) -> Vec<u8>;
      /// `kind` is the stored kind, which may be an older one `accepts` allows.
      fn unseal(kind: &str, bytes: &[u8], token: &SealToken) -> Result<Self, String>;
      fn public(&self) -> Self::Public;
  }
  ```

  `load` gives `WrongKind` for a kind `accepts` refuses. When it loads an
  older accepted kind, it seals the key again under `KIND` with the same
  options and atomic write, before returning it. If that rewrite fails,
  the key is still returned, and the failure is logged, so a read never
  fails because of an upgrade.
- **Memory stores** are per instance, empty when made, with no globals.
  This restates the plans: libmorph makes a new one per scripted run, and
  parallel tests cannot share state. libdevice adds a test that two
  memory stores do not see each other's names.
- **Keychain prompts after rebuilds.** libdevice's example documents an
  optional developer setup: sign the built binary with a stable local
  identity (`codesign -s "<identity>" <binary>` after each build). A
  stable signature keeps the Keychain's access list valid across
  rebuilds, so it stops asking. The code does not change.
- **Tests added:**
  - a `compile_fail` doc test that a `SealToken` cannot be stored in a
    `static`, or returned from `seal`;
  - a migration test, where a test adapter's v2 accepts v1, loads a v1
    key, and finds it stored as v2 afterwards;
  - `WrongKind` for a kind that is not accepted.
