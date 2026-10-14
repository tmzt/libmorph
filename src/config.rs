//! Launch configuration: `--morph-*` options for libmorph itself and
//! libdevice's `--device*` options for the emulated device.

use std::fmt;

use libdevice::DeviceArgsError;
#[cfg(not(target_os = "android"))]
use libdevice::DeviceSpec;

use crate::GpuError;

/// Usage text for libmorph's own options. Device options are listed in
/// [`libdevice::DEVICE_ARGS_HELP`].
pub const MORPH_ARGS_HELP: &str = "\
libmorph options:
  --morph-help                     Print this help and the device options, then exit
  --morph-fallback-adapter         Request a software GPU adapter
  --morph-no-overlay               Don't mask the emulated cutouts and corners
";

/// libmorph's own options.
#[cfg(not(target_os = "android"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MorphOptions {
    pub help: bool,
    pub fallback_adapter: bool,
    pub overlay: bool,
}

#[cfg(not(target_os = "android"))]
impl Default for MorphOptions {
    fn default() -> Self {
        Self {
            help: false,
            fallback_adapter: false,
            overlay: true,
        }
    }
}

/// What the winit runner starts from. Android has no command line and
/// starts from [`libdevice::DEFAULT_DEVICE`].
#[cfg(not(target_os = "android"))]
#[derive(Clone, Debug)]
pub(crate) struct Launch {
    pub spec: DeviceSpec,
    pub options: MorphOptions,
    /// The arguments left for the app, program name first.
    pub args: Vec<String>,
}

#[cfg(not(target_os = "android"))]
impl Launch {
    /// Take the `--morph-*` and `--device*` options out of `args`. Arguments
    /// after `--` belong to the app and are left alone.
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    pub fn from_args<I>(args: I) -> Result<Self, MorphError>
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        let mut options = MorphOptions::default();
        let mut rest = Vec::new();
        let mut args = args.into_iter().map(Into::into);
        while let Some(arg) = args.next() {
            if arg == "--" {
                rest.push(arg);
                rest.extend(args.by_ref());
                break;
            }
            match arg.as_str() {
                "--morph-help" => options.help = true,
                "--morph-fallback-adapter" => options.fallback_adapter = true,
                "--morph-no-overlay" => options.overlay = false,
                other if other.starts_with("--morph-") => {
                    return Err(MorphError::UnknownOption(arg));
                }
                _ => rest.push(arg),
            }
        }
        let (spec, args) = DeviceSpec::extract_args(rest)?;
        Ok(Self {
            spec,
            options,
            args,
        })
    }

    /// No command line on the web: the default device.
    #[cfg(target_arch = "wasm32")]
    pub fn without_args() -> Self {
        Self {
            spec: libdevice::DEFAULT_DEVICE,
            options: MorphOptions::default(),
            args: Vec::new(),
        }
    }

    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    pub fn help() -> String {
        format!("{MORPH_ARGS_HELP}\n{}", libdevice::DEVICE_ARGS_HELP)
    }
}

/// Why a runner could not start or keep running.
#[derive(Debug)]
#[non_exhaustive]
pub enum MorphError {
    /// A malformed `--device*` option.
    DeviceArgs(DeviceArgsError),
    /// A `--morph-*` option libmorph does not know.
    UnknownOption(String),
    Gpu(GpuError),
    /// The surface reports no format the adapter can present.
    NoSurfaceFormat,
    #[cfg(not(target_os = "android"))]
    EventLoop(winit::error::EventLoopError),
    #[cfg(not(target_os = "android"))]
    Window(winit::error::OsError),
}

impl fmt::Display for MorphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceArgs(error) => error.fmt(f),
            Self::UnknownOption(option) => write!(f, "unknown libmorph option `{option}`"),
            Self::Gpu(error) => error.fmt(f),
            Self::NoSurfaceFormat => f.write_str("the surface has no presentable format"),
            #[cfg(not(target_os = "android"))]
            Self::EventLoop(error) => write!(f, "event loop: {error}"),
            #[cfg(not(target_os = "android"))]
            Self::Window(error) => write!(f, "window creation: {error}"),
        }
    }
}

impl std::error::Error for MorphError {}

impl From<DeviceArgsError> for MorphError {
    fn from(error: DeviceArgsError) -> Self {
        Self::DeviceArgs(error)
    }
}

impl From<GpuError> for MorphError {
    fn from(error: GpuError) -> Self {
        Self::Gpu(error)
    }
}

#[cfg(not(target_os = "android"))]
impl From<winit::error::EventLoopError> for MorphError {
    fn from(error: winit::error::EventLoopError) -> Self {
        Self::EventLoop(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{Launch, MorphError};
    use libdevice::{GALAXY_S24, IPHONE_15};

    #[test]
    fn plain_args_pass_through() {
        let launch = Launch::from_args(["app", "scene.toml", "--verbose"]).unwrap();
        assert_eq!(launch.spec, GALAXY_S24);
        assert!(launch.options.overlay && !launch.options.fallback_adapter);
        assert_eq!(launch.args, ["app", "scene.toml", "--verbose"]);
    }

    #[test]
    fn morph_and_device_options_are_taken_out() {
        let launch = Launch::from_args([
            "app",
            "--morph-fallback-adapter",
            "--device",
            "iphone_15",
            "--morph-no-overlay",
            "--device-width=720",
            "input.txt",
        ])
        .unwrap();
        assert_eq!(launch.spec.id, IPHONE_15.id);
        assert_eq!(launch.spec.geometry.width, 720);
        assert!(launch.options.fallback_adapter && !launch.options.overlay);
        assert_eq!(launch.args, ["app", "input.txt"]);
    }

    #[test]
    fn args_after_double_dash_belong_to_the_app() {
        let launch = Launch::from_args(["app", "--", "--morph-help", "--device=x"]).unwrap();
        assert!(!launch.options.help);
        assert_eq!(launch.args, ["app", "--", "--morph-help", "--device=x"]);
    }

    #[test]
    fn unknown_options_are_errors() {
        assert!(matches!(
            Launch::from_args(["app", "--morph-colour"]),
            Err(MorphError::UnknownOption(option)) if option == "--morph-colour"
        ));
        assert!(matches!(
            Launch::from_args(["app", "--device=pixel"]),
            Err(MorphError::DeviceArgs(_))
        ));
    }

    #[test]
    fn help_lists_both_option_sets() {
        let help = Launch::help();
        assert!(help.contains("--morph-help") && help.contains("--device-width"));
    }
}
