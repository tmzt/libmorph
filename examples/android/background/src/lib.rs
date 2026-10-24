//! `examples/background.rs` on Android, with the foreground service, system
//! events and battery calls of libmobile. On a host this is an empty
//! library.

#[cfg(target_os = "android")]
#[path = "../../../background.rs"]
mod background;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: libmorph::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("background"),
    );
    libmorph::run_android::<background::Background>(app);
}
