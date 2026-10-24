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
pub mod battery;
mod config;
mod context;
mod gpu;
mod input;
mod link;
mod out_dir;
mod permission;
mod pipeline_cache;
mod runner;
pub mod safe_area;
#[cfg(script_runner)]
mod screenshot;
pub mod script;
mod secure_store;
pub mod service;
mod storage;
pub mod system_events;

pub use app::{Frame, MorphApp};
pub use audio::{
    Audio, AudioSetup, CaptureFormat, CaptureMode, Captured, OutputFormat, Sink, SinkId, Sound,
    Source, SourceId, Voice,
};
pub use config::{MORPH_ARGS_HELP, MorphError};
pub use context::MorphContext;
pub use gpu::{GpuError, LimitFailure, LimitsRequest};
pub use input::{Key, KeyStatus, Touch, TouchPhase};
#[cfg(feature = "secure-store")]
pub use key_store::{
    FLAG_APP_UNLOCK_REQUIRED, FLAG_DEVICE_UNLOCK_REQUIRED, KeyStoreError, SealToken, SecretKey,
    Vault,
};
#[cfg(all(feature = "secure-store", not(target_os = "android")))]
pub use libdevice::key_store;
/// The key store (plan `20261022_06_KEY_STORE_PLAN.md`): the active
/// platform crate's engine, re-exported as is. libmobile's on Android,
/// libdevice's elsewhere; the two engines are the same file.
#[cfg(all(feature = "secure-store", target_os = "android"))]
pub use libmobile::key_store;
pub use link::OpenLinkError;
pub use permission::{PermissionRequest, Permissions};
pub use safe_area::SafeArea;
pub use secure_store::{
    MAX_NAME_LEN, MAX_VALUE_LEN, Protection, PutOptions, SecureStore, SecureStoreError,
};

#[cfg(feature = "audio-cpal")]
pub use audio::cpal;
pub use libdevice;
pub use wgpu;

#[cfg(not(target_os = "android"))]
pub use runner::run;

#[cfg(all(target_os = "android", feature = "mobile"))]
pub use libmobile::android::AndroidApp;
#[cfg(all(target_os = "android", feature = "mobile"))]
pub use runner::run_android;
