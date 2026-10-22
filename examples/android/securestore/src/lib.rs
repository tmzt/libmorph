//! `examples/securestore.rs` on Android: the same app, with its blobs
//! sealed by libmobile under an Android Keystore key. On a host this is an
//! empty library.

#[cfg(target_os = "android")]
#[path = "../../../securestore.rs"]
mod securestore;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: libmorph::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("securestore"),
    );
    libmorph::run_android::<securestore::SecureStoreApp>(app);
}
