//! Counts launches in the secure store (`ctx.secure_store()`, feature
//! `secure-store`): on macOS in the login Keychain (service
//! `libmorph.securestore`), on Android in libmobile's Keystore-sealed
//! blobs (see `examples/android/`).
//!
//! * At start it logs where the secrets are kept (`protection()`), reads
//!   blob `counter` (a little-endian `u64`), logs it, and writes it back plus
//!   one.
//! * It writes blob `unlocked-only` with `PutOptions::unlocked_only` if there
//!   is none, and reads it at start and on every resume: on Android it gives
//!   `DeviceLocked` while the device is locked; macOS does not enforce it.
//! * A tap deletes `counter`, so the next launch counts from 1 again.
//! * The key store (`ctx.key_store()`): key `device` is generated on the
//!   first launch and its fingerprint logged on later ones. Its adapter,
//!   `ExampleKey`, is 32 random bytes with a `u64` fingerprint as its public
//!   value (enough for an example).
//!
//! The screen is green with the key in StrongBox, blue elsewhere, and grey
//! with no store.
//!
//! ```text
//! cargo run --example securestore --features secure-store
//! security find-generic-password -s libmorph.securestore -a counter
//! ```

use std::time::Duration;

use libmorph::{
    Frame, KeyStoreError, MorphApp, MorphContext, Protection, PutOptions, SealToken, SecretKey,
    SecureStore, Touch, TouchPhase, key_store, wgpu,
};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const COUNTER: &str = "counter";
const DEVICE_KEY: &str = "device";

/// 32 bytes behind a pointer, so moves copy only the pointer.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct ExampleKey(Box<[u8; 32]>);

impl SecretKey for ExampleKey {
    type Public = u64;
    const KIND: &'static str = "libmorph.example.test.v1";

    fn generate(_: &SealToken) -> Self {
        let mut key = Self(Box::new([0; 32]));
        if let Err(error) = std::fs::File::open("/dev/urandom")
            .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut key.0[..]))
        {
            panic!("example key: no randomness: {error}");
        }
        key
    }

    fn seal(&self, _: &SealToken) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(self.0.to_vec())
    }

    fn unseal(_: &str, bytes: &[u8], _: &SealToken) -> Result<Self, String> {
        if bytes.len() != 32 {
            return Err(format!("{} bytes, not 32", bytes.len()));
        }
        let mut key = Self(Box::new([0; 32]));
        key.0.copy_from_slice(bytes);
        Ok(key)
    }

    fn public(&self) -> u64 {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        hasher.finish()
    }
}
const UNLOCKED_ONLY: &str = "unlocked-only";

pub struct SecureStoreApp {
    store: Option<SecureStore>,
    protection: Option<Protection>,
}

/// The stored count, or 0 for none. A blob of the wrong length is 0 too.
fn decode(blob: Option<&[u8]>) -> u64 {
    blob.and_then(|b| b.try_into().ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0)
}

fn read_unlocked_only(store: &SecureStore, when: &str) {
    match store.get(UNLOCKED_ONLY) {
        Ok(Some(text)) => log::info!(
            "securestore: {UNLOCKED_ONLY} ({when}): {:?}",
            String::from_utf8_lossy(&text)
        ),
        Ok(None) => log::info!("securestore: {UNLOCKED_ONLY} ({when}): none"),
        Err(error) => log::info!("securestore: {UNLOCKED_ONLY} ({when}): {error:?}"),
    }
}

impl MorphApp for SecureStoreApp {
    const SECURE_STORE_ID: &'static str = "securestore";

    fn init(ctx: &mut MorphContext) -> Self {
        match ctx.key_store() {
            Some(vault) => match key_store::public::<ExampleKey>(vault, DEVICE_KEY) {
                Ok(public) => log::info!("securestore: key {DEVICE_KEY}: {public:016x} (loaded)"),
                Err(KeyStoreError::NotFound) => {
                    match key_store::generate::<ExampleKey>(vault, DEVICE_KEY, 0) {
                        Ok(public) => {
                            log::info!("securestore: key {DEVICE_KEY}: {public:016x} (generated)")
                        }
                        Err(error) => log::error!("securestore: key {DEVICE_KEY}: {error}"),
                    }
                }
                Err(error) => log::error!("securestore: key {DEVICE_KEY}: {error:?}"),
            },
            None => log::warn!("securestore: no key store on this host"),
        }
        let Some(store) = ctx.secure_store().cloned() else {
            log::warn!("securestore: no secure store on this host");
            return Self {
                store: None,
                protection: None,
            };
        };
        let protection = match store.protection() {
            Ok(protection) => {
                log::info!("securestore: kept in {protection:?}");
                Some(protection)
            }
            Err(error) => {
                log::error!("securestore: protection: {error}");
                None
            }
        };
        match store.get(COUNTER) {
            Ok(blob) => {
                let count = decode(blob.as_deref()) + 1;
                log::info!("securestore: launch {count}");
                if let Err(error) = store.put(COUNTER, &count.to_le_bytes()) {
                    log::error!("securestore: put {COUNTER}: {error}");
                }
            }
            Err(error) => log::error!("securestore: get {COUNTER}: {error}"),
        }
        if let Ok(None) = store.get(UNLOCKED_ONLY) {
            let options = PutOptions::default().unlocked_only(true);
            match store.put_with(UNLOCKED_ONLY, b"only while unlocked", options) {
                Ok(()) => log::info!("securestore: wrote {UNLOCKED_ONLY}"),
                Err(error) => log::error!("securestore: put {UNLOCKED_ONLY}: {error}"),
            }
        }
        read_unlocked_only(&store, "start");
        Self {
            store: Some(store),
            protection,
        }
    }

    fn update(&mut self, _ctx: &mut MorphContext, _dt: Duration) {}

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        let color = match self.protection {
            Some(Protection::AndroidStrongBox) => wgpu::Color {
                r: 0.05,
                g: 0.35,
                b: 0.12,
                a: 1.0,
            },
            Some(_) => wgpu::Color {
                r: 0.05,
                g: 0.12,
                b: 0.35,
                a: 1.0,
            },
            None => wgpu::Color {
                r: 0.15,
                g: 0.15,
                b: 0.15,
                a: 1.0,
            },
        };
        let mut encoder = ctx.device().create_command_encoder(&Default::default());
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        ctx.queue().submit([encoder.finish()]);
    }

    fn on_touch(&mut self, _ctx: &mut MorphContext, touch: Touch) {
        if touch.phase != TouchPhase::Started {
            return;
        }
        if let Some(store) = &self.store {
            match store.delete(COUNTER) {
                Ok(removed) => log::info!("securestore: deleted {COUNTER}: {removed}"),
                Err(error) => log::error!("securestore: delete {COUNTER}: {error}"),
            }
        }
    }

    fn on_resume(&mut self, _ctx: &mut MorphContext) {
        if let Some(store) = &self.store {
            read_unlocked_only(store, "resume");
        }
    }
}

#[cfg(not(target_os = "android"))]
fn main() -> Result<(), libmorph::MorphError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    libmorph::run::<SecureStoreApp>()
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn counter_round_trip() {
        assert_eq!(decode(None), 0);
        assert_eq!(decode(Some(&[1, 2])), 0);
        assert_eq!(decode(Some(&41u64.to_le_bytes())), 41);
    }
}
