fn main() {
    // Writes target/<triple>/<profile>/gradle/libmorph-example-urlopen-android/
    // (the whole Gradle project; examples/android/build.sh packages it).
    libmorph_build::gradle_project("dev.highbay.libmorph.example.urlopen")
        .label("libmorph urlopen")
        .lib_name("urlopen_android")
        .url_scheme("libmorph-urlopen")
        .generate()
        .expect("libmorph-build: generating the Gradle project");
}
