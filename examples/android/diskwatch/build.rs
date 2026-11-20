fn main() {
    // The example reads it at compile time (`option_env!`).
    println!("cargo:rerun-if-env-changed=DISKWATCH_CRITICAL_GIB");
    // Writes target/<triple>/<profile>/gradle/libmorph-example-diskwatch-android/
    // (the whole Gradle project; examples/android/build.sh packages it).
    libmorph_build::gradle_project("dev.highbay.libmorph.example.diskwatch")
        .label("libmorph diskwatch")
        .lib_name("diskwatch_android")
        .generate()
        .expect("libmorph-build: generating the Gradle project");
}
