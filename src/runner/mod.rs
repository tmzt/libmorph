//! Host event loops. Each one owns the surface and drives a shared [`Shell`].

#[cfg(all(target_arch = "wasm32", not(feature = "web")))]
compile_error!("The 'web' feature must be enabled when compiling for the wasm32 target.");

#[cfg(all(target_os = "android", not(feature = "mobile")))]
compile_error!("The 'mobile' feature must be enabled when compiling for Android.");

#[cfg(all(target_os = "android", feature = "mobile"))]
mod android;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod overlay;
mod shell;
#[cfg(not(target_os = "android"))]
mod winit;

#[cfg(all(target_os = "android", feature = "mobile"))]
pub use android::run_android;
#[cfg(not(target_os = "android"))]
pub use winit::run;
