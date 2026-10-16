//! Launch configuration: `--morph-*` options for libmorph itself and
//! libdevice's `--device*` options for the emulated device.

use std::fmt;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
use std::path::PathBuf;

use libdevice::DeviceArgsError;
#[cfg(not(target_os = "android"))]
use libdevice::DeviceSpec;

use crate::GpuError;
use crate::script::ScriptError;

/// Usage text for libmorph's own options. Device options are listed in
/// [`libdevice::DEVICE_ARGS_HELP`].
pub const MORPH_ARGS_HELP: &str = "\
libmorph options:
  --morph-help                     Print this help and the device options, then exit
  --morph-fallback-adapter         Request a software GPU adapter
  --morph-no-overlay               Don't mask the emulated cutouts and corners
  --morph-script <path>            Run the script headless, without a window
                                   (needs the `script` feature)
  --morph-out <dir>                The script's output directory (default
                                   out/morph/YYYYMMDD_NN_<script name>)
  --morph-trackpad <fit|off>       macOS: two or more trackpad fingers are
                                   touches, the whole pad covering the screen
                                   (fit, the default), or not at all (off)
";

/// libmorph's own options.
#[cfg(not(target_os = "android"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MorphOptions {
    pub help: bool,
    pub fallback_adapter: bool,
    pub overlay: bool,
    /// Read on macOS only; accepted everywhere so one command line works.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub trackpad: TrackpadOption,
}

/// `--morph-trackpad`: how trackpad fingers become touches (macOS).
#[cfg(not(target_os = "android"))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) enum TrackpadOption {
    /// The whole pad covers the whole surface.
    #[default]
    Fit,
    /// No trackpad touches.
    Off,
}

#[cfg(not(target_os = "android"))]
impl Default for MorphOptions {
    fn default() -> Self {
        Self {
            help: false,
            fallback_adapter: false,
            overlay: true,
            trackpad: TrackpadOption::default(),
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
    /// `--morph-script`: run this script headless.
    #[cfg(not(target_arch = "wasm32"))]
    pub script: Option<PathBuf>,
    /// `--morph-out`: the script's exact output directory.
    #[cfg(not(target_arch = "wasm32"))]
    pub out: Option<PathBuf>,
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
        let (mut script, mut out) = (None, None);
        let mut rest = Vec::new();
        let mut args = args.into_iter().map(Into::into);
        while let Some(arg) = args.next() {
            if arg == "--" {
                rest.push(arg);
                rest.extend(args.by_ref());
                break;
            }
            let (name, inline) = match arg.split_once('=') {
                Some((name, value)) if name.starts_with("--morph-") => {
                    (name.to_owned(), Some(value.to_owned()))
                }
                _ => (arg.clone(), None),
            };
            let mut value = || {
                inline
                    .clone()
                    .or_else(|| args.next())
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| MorphError::InvalidOption {
                        option: name.clone(),
                        reason: "needs a value",
                    })
            };
            match name.as_str() {
                "--morph-script" => script = Some(PathBuf::from(value()?)),
                "--morph-out" => out = Some(PathBuf::from(value()?)),
                "--morph-trackpad" => {
                    options.trackpad = match value()?.as_str() {
                        "fit" => TrackpadOption::Fit,
                        "off" => TrackpadOption::Off,
                        _ => {
                            return Err(MorphError::InvalidOption {
                                option: name,
                                reason: "must be fit or off",
                            });
                        }
                    }
                }
                _ if inline.is_some() => return Err(MorphError::UnknownOption(arg)),
                "--morph-help" => options.help = true,
                "--morph-fallback-adapter" => options.fallback_adapter = true,
                "--morph-no-overlay" => options.overlay = false,
                other if other.starts_with("--morph-") => {
                    return Err(MorphError::UnknownOption(arg));
                }
                _ => rest.push(arg),
            }
        }
        if out.is_some() && script.is_none() {
            return Err(MorphError::InvalidOption {
                option: "--morph-out".into(),
                reason: "is only used with --morph-script",
            });
        }
        let (spec, args) = DeviceSpec::extract_args(rest)?;
        Ok(Self {
            spec,
            options,
            args,
            script,
            out,
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
    /// A known `--morph-*` option used wrongly.
    InvalidOption {
        option: String,
        reason: &'static str,
    },
    Gpu(GpuError),
    /// The surface reports no format the adapter can present.
    NoSurfaceFormat,
    /// A script that could not be loaded or run (see [`crate::script`]).
    Script(ScriptError),
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
            Self::InvalidOption { option, reason } => write!(f, "`{option}` {reason}"),
            Self::Gpu(error) => error.fmt(f),
            Self::NoSurfaceFormat => f.write_str("the surface has no presentable format"),
            Self::Script(error) => error.fmt(f),
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

impl From<ScriptError> for MorphError {
    fn from(error: ScriptError) -> Self {
        Self::Script(error)
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
    use std::path::Path;

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
        assert!(help.contains("--morph-script") && help.contains("--morph-out"));
    }

    #[test]
    fn script_and_out_take_values_in_both_forms() {
        let launch = Launch::from_args([
            "app",
            "--morph-script",
            "a.morph",
            "--morph-out=out/x",
            "--device-width=720",
            "rest",
        ])
        .unwrap();
        assert_eq!(launch.script.as_deref(), Some(Path::new("a.morph")));
        assert_eq!(launch.out.as_deref(), Some(Path::new("out/x")));
        assert_eq!(launch.spec.geometry.width, 720);
        assert_eq!(launch.args, ["app", "rest"]);

        let launch = Launch::from_args(["app", "--morph-script=b.morph"]).unwrap();
        assert_eq!(launch.script.as_deref(), Some(Path::new("b.morph")));
        assert_eq!(launch.out, None);
        let launch = Launch::from_args(["app"]).unwrap();
        assert_eq!((launch.script, launch.out), (None, None));
    }

    #[test]
    fn script_option_errors() {
        let invalid = |args: &[&str]| match Launch::from_args(args.iter().copied()) {
            Err(MorphError::InvalidOption { option, reason }) => format!("{option} {reason}"),
            other => panic!("{args:?}: {other:?}"),
        };
        assert_eq!(
            invalid(&["app", "--morph-script"]),
            "--morph-script needs a value"
        );
        assert_eq!(
            invalid(&["app", "--morph-script="]),
            "--morph-script needs a value"
        );
        assert_eq!(
            invalid(&["app", "--morph-out", "x"]),
            "--morph-out is only used with --morph-script"
        );
        assert!(matches!(
            Launch::from_args(["app", "--morph-help=yes"]),
            Err(MorphError::UnknownOption(option)) if option == "--morph-help=yes"
        ));
    }
}

#[cfg(test)]
mod trackpad_option_tests {
    use super::{Launch, MorphError, TrackpadOption};

    #[test]
    fn trackpad_option() {
        let trackpad =
            |args: &[&str]| Launch::from_args(args.iter().copied()).map(|l| l.options.trackpad);
        assert_eq!(trackpad(&["app"]).unwrap(), TrackpadOption::Fit);
        assert_eq!(
            trackpad(&["app", "--morph-trackpad", "off"]).unwrap(),
            TrackpadOption::Off
        );
        assert_eq!(
            trackpad(&["app", "--morph-trackpad=fit"]).unwrap(),
            TrackpadOption::Fit
        );
        assert!(matches!(
            trackpad(&["app", "--morph-trackpad=sideways"]),
            Err(MorphError::InvalidOption { option, .. }) if option == "--morph-trackpad"
        ));
        assert!(matches!(
            trackpad(&["app", "--morph-trackpad"]),
            Err(MorphError::InvalidOption {
                reason: "needs a value",
                ..
            })
        ));
        assert!(Launch::help().contains("--morph-trackpad"));
    }
}
