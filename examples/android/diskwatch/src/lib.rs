//! `examples/diskwatch.rs` on Android, with libmobile's disk readings
//! (`mobile-disk-space`). On a host this is an empty library.

#[cfg(target_os = "android")]
#[path = "../../../diskwatch.rs"]
mod diskwatch;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: libmorph::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("diskwatch"),
    );
    libmorph::run_android::<diskwatch::Diskwatch>(app);
}
