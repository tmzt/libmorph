//! A build script's `gradle_project(id).plugin(AppPlugin::new(..))`
//! (libmobile 0.5.5, its plan `20261120_01`): the app's own plugin reaches
//! the generated project through libmorph-build's re-export. The test
//! stands in for cargo, setting the variables an Android build script sees,
//! and is the only test in this process, so setting them is safe.

use libmorph_build::gradle::AppPlugin;
use std::path::PathBuf;

#[test]
fn app_plugin_reaches_the_generated_project() {
    let root =
        std::env::temp_dir().join(format!("libmorph-build-app-plugin-{}", std::process::id()));
    let out_dir: PathBuf = root.join("aarch64-linux-android/debug/build/app-0000/out");
    let project = root.join("project");
    let sources = root.join("android/java");
    let plugin_dir = sources.join("dev/example/media");
    std::fs::create_dir_all(&plugin_dir).expect("the plugin's package directory");
    std::fs::write(
        plugin_dir.join("MediaPlugin.kt"),
        "package dev.example.media\n\nobject MediaPlugin : dev.highbay.libmobile.MobileActivityPlugin\n",
    )
    .expect("the plugin's source");
    // SAFETY: this file's one test runs alone in its process (each
    // integration test file is its own binary), so no thread reads the
    // environment while it is written.
    unsafe {
        std::env::set_var("CARGO_CFG_TARGET_OS", "android");
        std::env::set_var("TARGET", "aarch64-linux-android");
        std::env::set_var("OUT_DIR", &out_dir);
        std::env::set_var(
            "DEP_LIBMORPH_GRADLE_SCRIPT",
            root.join("libmobile/libmobile.gradle"),
        );
        std::env::remove_var("LIBMOBILE_GRADLE_PROJECT_DIR");
    }
    let written = libmorph_build::gradle_project("dev.example.media")
        .lib_name("media")
        .plugin(
            AppPlugin::new("dev.example.media.MediaPlugin")
                .sources(&sources)
                .permission("android.permission.READ_MEDIA_IMAGES"),
        )
        .out_dir(&project)
        .generate()
        .expect("generating the Gradle project");
    assert_eq!(written.as_deref(), Some(project.as_path()));
    let read = |rel: &str| std::fs::read_to_string(project.join(rel)).expect(rel);
    let list = read("app/libmobile-app-plugins/dev/highbay/libmobile/GeneratedAppPlugins.kt");
    let app = read("app/build.gradle.kts");
    let manifest = read("app/src/main/AndroidManifest.xml");
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        list.contains("        dev.example.media.MediaPlugin,\n"),
        "{list}"
    );
    assert!(app.contains("LIBMOBILE_APP_PLUGINS_ROOT"), "{app}");
    assert!(
        app.contains(&format!("kotlin.srcDir(\"{}\")", sources.display())),
        "{app}"
    );
    assert!(
        manifest.contains("android.permission.READ_MEDIA_IMAGES"),
        "{manifest}"
    );
}
