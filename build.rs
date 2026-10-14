//! Republish libmobile's Gradle handoff (`links` metadata) under libmorph's
//! own `links` key, so an app that depends on libmorph alone can generate
//! its Gradle project. See `libmobile::gradle::forward_metadata`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "mobile")]
    libmobile::gradle::forward_metadata();
}
