//! `examples/urlopen.rs` on Android, declaring the `libmorph-urlopen`
//! scheme in its build script. On a host this is an empty library.

#[cfg(target_os = "android")]
#[path = "../../../urlopen.rs"]
mod urlopen;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: libmorph::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("urlopen"),
    );
    libmorph::run_android::<urlopen::UrlOpen>(app);
}
