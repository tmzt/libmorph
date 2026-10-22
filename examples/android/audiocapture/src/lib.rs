//! `examples/audiocapture.rs` on Android: the same app, writing its `.au`
//! files to `audiocapture/` in the app's data directory. On a host this is
//! an empty library.

#[cfg(target_os = "android")]
#[path = "../../../audiocapture.rs"]
mod audiocapture;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: libmorph::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("audiocapture"),
    );
    libmorph::run_android::<audiocapture::AudioCapture>(app);
}
