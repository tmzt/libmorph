//! A unified windowing, graphics and device-emulation shell for `wgpu`
//! applications.
//!
//! Implement [`MorphApp`] and hand it to the runner for the host:
//!
//! * desktop and web: [`run`], driven by `winit`. On desktop the window takes
//!   the exact physical size of the emulated device (`--device <name>`, see
//!   [`libdevice::DEVICE_ARGS_HELP`]) and mouse input arrives as touches.
//! * Android (`mobile` feature): `run_android`, driven by libmobile's
//!   `NativeActivity` loop.
//!
//! The same `wgpu::Device` and `Queue` live for the whole run; only the
//! surface is dropped and recreated across suspend/resume.

mod app;
mod audio;
mod config;
mod context;
mod gpu;
mod input;
mod out_dir;
mod runner;
pub mod safe_area;
pub mod script;

pub use app::{Frame, MorphApp};
pub use audio::{Audio, Sound, Voice};
pub use config::{MORPH_ARGS_HELP, MorphError};
pub use context::MorphContext;
pub use gpu::GpuError;
pub use input::{Key, KeyStatus, Touch, TouchPhase};
pub use safe_area::SafeArea;

pub use libdevice;
pub use wgpu;

#[cfg(not(target_os = "android"))]
pub use runner::run;

#[cfg(all(target_os = "android", feature = "mobile"))]
pub use libmobile::android::AndroidApp;
#[cfg(all(target_os = "android", feature = "mobile"))]
pub use runner::run_android;
