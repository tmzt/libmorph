//! A scripted run's secure store and key store: memory only, empty at the
//! start of each run, never the Keychain. Needs a GPU adapter; without one
//! the test says so and passes. Also: libmobile's and libdevice's key store
//! engines are the same file.

#![cfg(all(
    feature = "script",
    feature = "secure-store",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::sync::Mutex;
use std::time::Duration;

use libmorph::script::{OutDir, Script, ScriptRun};
use libmorph::{
    Frame, GpuError, KeyStoreError, MorphApp, MorphContext, MorphError, Protection, SealToken,
    SecretKey, key_store,
};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// What each run saw: the blob before it wrote one, and the protection.
static SEEN: Mutex<Vec<(Option<Vec<u8>>, Protection)>> = Mutex::new(Vec::new());

/// What each run's key store gave for key `device` before generating it.
static KEYS: Mutex<Vec<Result<u8, KeyStoreError>>> = Mutex::new(Vec::new());

/// A test adapter: one byte, public = the byte.
#[derive(Zeroize, ZeroizeOnDrop)]
struct TestKey(Box<u8>);

impl SecretKey for TestKey {
    type Public = u8;
    const KIND: &'static str = "libmorph.test.v1";
    fn generate(_: &SealToken) -> Self {
        Self(Box::new(42))
    }
    fn seal(&self, _: &SealToken) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(vec![*self.0])
    }
    fn unseal(_: &str, bytes: &[u8], _: &SealToken) -> Result<Self, String> {
        match bytes {
            [byte] => Ok(Self(Box::new(*byte))),
            _ => Err("not one byte".into()),
        }
    }
    fn public(&self) -> u8 {
        *self.0
    }
}

struct Keeper;

impl MorphApp for Keeper {
    fn init(ctx: &mut MorphContext) -> Self {
        let store = ctx
            .secure_store()
            .expect("a store in scripted runs")
            .clone();
        let before = store.get("noise.static").unwrap();
        store.put("noise.static", &[7; 32]).unwrap();
        assert_eq!(store.get("noise.static").unwrap(), Some(vec![7; 32]));
        SEEN.lock()
            .unwrap()
            .push((before, store.protection().unwrap()));

        let vault = ctx
            .key_store()
            .expect("a key store in scripted runs")
            .clone();
        KEYS.lock()
            .unwrap()
            .push(key_store::public::<TestKey>(&vault, "device"));
        assert_eq!(key_store::generate::<TestKey>(&vault, "device", 0), Ok(42));
        assert_eq!(
            key_store::with_key::<TestKey, _>(&vault, "device", |key| key.public()),
            Ok(42)
        );
        Self
    }

    fn update(&mut self, _ctx: &mut MorphContext, _dt: Duration) {}

    fn render(&mut self, _ctx: &mut MorphContext, _frame: &Frame<'_>) {}
}

#[test]
fn scripted_runs_keep_secrets_in_memory() {
    let dir = std::env::temp_dir().join(format!("libmorph-secure-store-{}", std::process::id()));
    let script = Script::parse("frames 1\n").unwrap();
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (16, 32);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.clone()),
        overlay: false,
        fallback_adapter: false,
        args: vec!["keeper".into()],
    };
    for _ in 0..2 {
        match libmorph::script::run::<Keeper>(&script, run.clone()) {
            Err(MorphError::Gpu(GpuError::Adapter(error))) => {
                eprintln!("skipped: no GPU adapter ({error})");
                return;
            }
            result => drop(result.unwrap()),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        *SEEN.lock().unwrap(),
        [(None, Protection::Memory), (None, Protection::Memory)]
    );
    // Each run's key store starts empty, too.
    assert_eq!(
        *KEYS.lock().unwrap(),
        [Err(KeyStoreError::NotFound), Err(KeyStoreError::NotFound)]
    );
}

#[cfg(feature = "mobile")]
#[test]
fn the_key_store_engines_are_the_same_file() {
    assert_eq!(
        libmobile::key_store::ENGINE_SOURCE,
        libdevice::key_store::ENGINE_SOURCE,
        "libmobile's and libdevice's src/key_store.rs differ: change libmobile's, then copy it"
    );
}
