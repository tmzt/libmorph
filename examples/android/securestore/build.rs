fn main() {
    // Writes target/<triple>/<profile>/gradle/libmorph-example-securestore-android/
    // (the whole Gradle project; examples/android/build.sh packages it).
    libmorph_build::gradle_project("dev.highbay.libmorph.example.securestore")
        .label("libmorph securestore")
        .lib_name("securestore_android")
        .generate()
        .expect("libmorph-build: generating the Gradle project");
}
