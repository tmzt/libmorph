//! Build-script helpers for libmorph apps.
//!
//! An Android app lists this as its only build-dependency and generates its
//! Gradle project from `build.rs`:
//!
//! ```no_run
//! libmorph_build::gradle_project("com.example.app")
//!     .label("Example")
//!     .generate()
//!     .expect("gradle project");
//! ```
//!
//! An icon is declared from one PNG, and the launcher, round, adaptive and
//! monochrome resources are generated from it:
//!
//! ```no_run
//! use libmorph_build::gradle::Icon;
//!
//! libmorph_build::gradle_project("com.example.app")
//!     .icon(Icon::png("icon/icon-1024.png").background_color([0x10, 0x14, 0x1c]))
//!     .generate()
//!     .expect("gradle project");
//! ```
//!
//! libmorph's own build script republishes libmobile's Gradle handoff under
//! libmorph's `links` key, which Cargo exposes to the app's build script
//! because the app depends on libmorph directly.

pub use libmobile::gradle;

/// libmorph's `links` key.
pub const LINKS: &str = "libmorph";

/// A Gradle project that reads libmorph's forwarded metadata.
pub fn gradle_project(application_id: &str) -> gradle::Project {
    gradle::Project::new(application_id).env_prefix(LINKS)
}

#[cfg(test)]
mod tests {
    use super::LINKS;

    #[test]
    fn links_matches_libmorph_manifest() {
        let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
        let text = std::fs::read_to_string(manifest).expect("libmorph's Cargo.toml");
        let links = text
            .lines()
            .find_map(|line| {
                let value = line
                    .trim()
                    .strip_prefix("links")?
                    .trim()
                    .strip_prefix('=')?;
                Some(value.trim().trim_matches('"').to_owned())
            })
            .expect("libmorph declares `links`");
        assert_eq!(links, LINKS);
    }
}
