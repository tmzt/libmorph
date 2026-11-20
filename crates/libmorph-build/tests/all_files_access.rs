//! A build script's `gradle_project(id).all_files_access()` (plan
//! `20261119_01`, step 2): the generated manifest declares
//! `MANAGE_EXTERNAL_STORAGE` once. The test stands in for cargo, setting
//! the variables an Android build script sees, and is the only test in
//! this process, so setting them is safe.

use std::path::PathBuf;

#[test]
fn all_files_access_reaches_the_manifest() {
    let root =
        std::env::temp_dir().join(format!("libmorph-build-all-files-{}", std::process::id()));
    let out_dir: PathBuf = root.join("aarch64-linux-android/debug/build/app-0000/out");
    let project = root.join("project");
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
    let written = libmorph_build::gradle_project("dev.example.files")
        .label("Files")
        .lib_name("files")
        .all_files_access()
        .out_dir(&project)
        .generate()
        .expect("generating the Gradle project");
    assert_eq!(written.as_deref(), Some(project.as_path()));
    let manifest = std::fs::read_to_string(project.join("app/src/main/AndroidManifest.xml"))
        .expect("the generated manifest");
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(
        manifest
            .matches("android.permission.MANAGE_EXTERNAL_STORAGE")
            .count(),
        1,
        "{manifest}"
    );
}
