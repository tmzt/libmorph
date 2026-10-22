//! Host event loops. Each one owns the surface and drives a shared [`Shell`].

#[cfg(all(target_arch = "wasm32", not(feature = "web")))]
compile_error!("The 'web' feature must be enabled when compiling for the wasm32 target.");

#[cfg(all(target_os = "android", not(feature = "mobile")))]
compile_error!("The 'mobile' feature must be enabled when compiling for Android.");

#[cfg(all(target_os = "android", feature = "mobile"))]
mod android;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod overlay;
#[cfg(script_runner)]
mod script;
mod shell;
#[cfg(target_os = "macos")]
mod trackpad;
#[cfg(not(target_os = "android"))]
mod winit;

#[cfg(all(target_os = "android", feature = "mobile"))]
pub use android::run_android;
#[cfg(script_runner)]
pub(crate) use script::run as run_script;
#[cfg(not(target_os = "android"))]
pub use winit::run;

/// The app's answer to [`MorphApp::audio_setup`](crate::MorphApp::audio_setup),
/// asked once per run before audio is opened.
fn audio_setup<A: crate::MorphApp>() -> crate::AudioSetup {
    let mut setup = crate::AudioSetup::default();
    A::audio_setup(&mut setup);
    setup
}
