use libmorph_build::gradle::ServiceType;

fn main() {
    // Writes target/<triple>/<profile>/gradle/libmorph-example-background-android/
    // (the whole Gradle project; examples/android/build.sh packages it).
    libmorph_build::gradle_project("dev.highbay.libmorph.example.background")
        .label("libmorph background")
        .lib_name("background_android")
        // The example's TCP connection.
        .permission("android.permission.INTERNET")
        // libmorph-background://<ip:port> gives the host.
        .url_scheme("libmorph-background")
        .foreground_service(&[ServiceType::SpecialUse(
            "a persistent connection to the user's own server",
        )])
        .generate()
        .expect("libmorph-build: generating the Gradle project");
}
