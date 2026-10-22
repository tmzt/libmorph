fn main() {
    // Writes target/<triple>/<profile>/gradle/libmorph-example-audiocapture-android/
    // (the whole Gradle project; examples/android/build.sh packages it).
    // libmobile has no runtime permission request: build.sh grants
    // RECORD_AUDIO with adb.
    libmorph_build::gradle_project("dev.highbay.libmorph.example.audiocapture")
        .label("libmorph audiocapture")
        .lib_name("audiocapture_android")
        .permission("android.permission.RECORD_AUDIO")
        .generate()
        .expect("libmorph-build: generating the Gradle project");
}
