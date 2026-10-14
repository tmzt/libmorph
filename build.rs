//! Republish libmobile's Gradle handoff (`links` metadata) under libmorph's
//! own `links` key, so an app that depends on libmorph alone can generate
//! its Gradle project. See `libmobile::gradle::forward_metadata`.
//!
//! Also sets `cfg(script_runner)`: the `script` feature on a native desktop
//! target, where scripted runs exist.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "mobile")]
    libmobile::gradle::forward_metadata();

    println!("cargo::rustc-check-cfg=cfg(script_runner)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if std::env::var_os("CARGO_FEATURE_SCRIPT").is_some() && os != "android" && arch != "wasm32" {
        println!("cargo::rustc-cfg=script_runner");
    }
}
