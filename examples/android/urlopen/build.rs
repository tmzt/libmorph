fn main() {
    // Writes target/<triple>/<profile>/gradle/libmorph-example-urlopen-android/
    // (the whole Gradle project; examples/android/build.sh packages it).
    libmorph_build::gradle_project("dev.highbay.libmorph.example.urlopen")
        .label("libmorph urlopen")
        .lib_name("urlopen_android")
        .url_scheme("libmorph-urlopen")
        // Launcher icons from one PNG (libmobile's `icons`).
        .icon(
            libmorph_build::gradle::Icon::png("icon/icon-1024.png")
                .background_color([0x14, 0x18, 0x24]),
        )
        .generate()
        .expect("libmorph-build: generating the Gradle project");
}
