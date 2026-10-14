//! Scripted runs: a plain-text list of English commands that drives a
//! [`MorphApp`](crate::MorphApp) through frames, touches, keys and
//! screenshots with a fixed frame time.
//!
//! ```text
//! # Idle, drag across, release, back out.
//! frames 30
//! screenshot idle
//! drag 1 100 400 900 1800 20
//! screenshot dragged
//! key escape
//! frames 1
//! screenshot after
//! ```
//!
//! One command per line, operands separated by spaces or tabs, `#` to the end
//! of the line is a comment. Coordinates are physical pixels of the surface.
//! The whole script is checked when it is parsed, before any GPU work.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::{Key, Touch, TouchPhase};

pub use crate::out_dir::{DEFAULT_OUT_ROOT, OutDir};

/// How to run a script (`script` feature, desktop only).
#[cfg(script_runner)]
#[derive(Clone, Debug)]
pub struct ScriptRun {
    /// The emulated device: its geometry is the render size.
    pub spec: libdevice::DeviceSpec,
    pub out: OutDir,
    /// Mask the device's cutouts and corners, as in a window.
    pub overlay: bool,
    pub fallback_adapter: bool,
    /// What `ctx.args()` returns, program name first.
    pub args: Vec<String>,
}

/// What a finished run wrote.
#[cfg(script_runner)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub dir: PathBuf,
    pub screenshots: Vec<Screenshot>,
}

/// One screenshot file.
#[cfg(script_runner)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screenshot {
    pub path: PathBuf,
    /// Frames run before it, counted from 1.
    pub frame: u64,
    /// The script line of the `screenshot` command.
    pub line: u32,
    /// CRC-32 of the RGB rows, independent of PNG compression.
    pub pixels_crc32: u32,
}

/// Run `A` headless through `script`: no window or event loop, a fixed
/// frame time, silent audio. Writes the screenshots and `run.json` into
/// `run.out` and returns what it wrote. Callable from any thread, including
/// `cargo test`.
///
/// # Errors
///
/// No GPU adapter, or a failure creating the directory or writing a file.
#[cfg(script_runner)]
pub fn run<A: crate::MorphApp>(
    script: &Script,
    run: ScriptRun,
) -> Result<Report, crate::MorphError> {
    crate::runner::run_script::<A>(script, run)
}

/// The default frame rate.
pub const DEFAULT_FPS: u32 = 60;
const MAX_FPS: u32 = 1000;
const MAX_FRAMES: u32 = 1_000_000;
const MAX_NAME: usize = 64;

/// One script command.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Command {
    /// `fps <n>`: the fixed frame rate, before the first frame.
    Fps(u32),
    /// `frames <n>`: run `n` frames.
    Frames(u32),
    /// `touch <id> <start|move|end|cancel> <x> <y> [<pressure>]`.
    Touch(Touch),
    /// `tap <x> <y>`: touch id 0 down, one frame, up.
    Tap(f32, f32),
    /// `drag <id> <x0> <y0> <x1> <y1> <n>`: down at `from`, `n` moves along
    /// the line with a frame after each, up at `to`.
    Drag {
        id: u64,
        from: (f32, f32),
        to: (f32, f32),
        frames: u32,
    },
    /// `hover <x> <y>`.
    Hover(f32, f32),
    /// `key <escape|back|space|character|U+XXXX>`.
    Key(Key),
    /// `screenshot [<name>]`: the last rendered frame to `<name>.png`.
    Screenshot(Option<String>),
    /// `suspend`: the surface goes away.
    Suspend,
    /// `resume`: a new surface.
    Resume,
    /// `exit`: end the run here.
    Exit,
}

/// Why a script could not be loaded, checked or run.
#[derive(Debug)]
#[non_exhaustive]
pub enum ScriptError {
    /// A malformed or inconsistent command. Lines count from 1.
    Parse {
        line: u32,
        message: String,
    },
    Io(std::io::Error),
    /// Writing a PNG failed.
    Encode(Box<dyn std::error::Error + Send + Sync>),
    /// `--morph-script` in a build without the `script` feature.
    Disabled,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse { line, message } => write!(f, "script line {line}: {message}"),
            Self::Io(error) => write!(f, "script: {error}"),
            Self::Encode(error) => write!(f, "screenshot: {error}"),
            Self::Disabled => f.write_str("libmorph was built without the `script` feature"),
        }
    }
}

impl std::error::Error for ScriptError {}

impl From<std::io::Error> for ScriptError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// A parsed, checked script.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    /// Each command with its line number.
    steps: Vec<(u32, Command)>,
    /// The file it was loaded from, for the manifest.
    path: Option<PathBuf>,
}

impl Script {
    /// Parse and check a script.
    pub fn parse(source: &str) -> Result<Self, ScriptError> {
        let mut steps = Vec::new();
        for (index, text) in source.lines().enumerate() {
            let line = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let text = text.split_once('#').map_or(text, |(code, _)| code);
            let words: Vec<&str> = text.split_whitespace().collect();
            if words.is_empty() {
                continue;
            }
            let command =
                parse_command(&words).map_err(|message| ScriptError::Parse { line, message })?;
            steps.push((line, command));
        }
        Self::checked(steps)
    }

    /// Read and parse a script file.
    pub fn load(path: &Path) -> Result<Self, ScriptError> {
        let mut script = Self::parse(&std::fs::read_to_string(path)?)?;
        script.path = Some(path.to_owned());
        Ok(script)
    }

    /// The same checks as [`parse`](Self::parse); lines are numbered from 1
    /// in order.
    pub fn from_commands(commands: Vec<Command>) -> Result<Self, ScriptError> {
        let steps = (1..).zip(commands).collect();
        Self::checked(steps)
    }

    fn checked(steps: Vec<(u32, Command)>) -> Result<Self, ScriptError> {
        let script = Self { steps, path: None };
        script.plan()?;
        Ok(script)
    }

    /// The file the script was loaded from, if any.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The line of the last command; `None` for an empty script.
    #[cfg_attr(not(script_runner), allow(dead_code))]
    pub(crate) fn last_line(&self) -> Option<u32> {
        self.steps.last().map(|(line, _)| *line)
    }

    /// The fixed frame rate: the script's `fps`, else [`DEFAULT_FPS`].
    #[must_use]
    pub fn fps(&self) -> u32 {
        self.steps
            .iter()
            .find_map(|(_, command)| match command {
                Command::Fps(fps) => Some(*fps),
                _ => None,
            })
            .unwrap_or(DEFAULT_FPS)
    }

    /// The commands expanded into what the runner does, in order, each with
    /// its line. Already checked, so this cannot fail.
    #[cfg_attr(not(script_runner), allow(dead_code))]
    pub(crate) fn actions(&self) -> Vec<(u32, Action)> {
        self.plan().expect("checked when the script was built")
    }

    /// Check the commands in order and expand them.
    fn plan(&self) -> Result<Vec<(u32, Action)>, ScriptError> {
        let mut planner = Planner::default();
        for (line, command) in &self.steps {
            planner
                .step(*line, command)
                .map_err(|message| ScriptError::Parse {
                    line: *line,
                    message,
                })?;
        }
        Ok(planner.actions)
    }
}

/// What a runner does for one expanded command.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    /// Run this many frames.
    Frames(u32),
    Touch(Touch),
    Hover(f32, f32),
    Key(Key),
    /// Write the last rendered frame as `<name>.png`. `frame` counts the
    /// frames run so far, from 1.
    Screenshot {
        name: String,
        frame: u64,
    },
    Suspend,
    Resume,
    Exit,
}

/// The static checks of the plan, section 3, with the expansion.
#[derive(Default)]
struct Planner {
    actions: Vec<(u32, Action)>,
    frames: u64,
    down: HashSet<u64>,
    names: HashSet<String>,
    suspended: bool,
}

impl Planner {
    fn step(&mut self, line: u32, command: &Command) -> Result<(), String> {
        let running = |what: &str, suspended: bool| {
            if suspended {
                Err(format!("`{what}` while suspended"))
            } else {
                Ok(())
            }
        };
        match command {
            Command::Fps(fps) => {
                range("fps", *fps, 1, MAX_FPS)?;
                if self.frames > 0 {
                    return Err("`fps` after the first frame".into());
                }
            }
            Command::Frames(n) => {
                running("frames", self.suspended)?;
                range("frames", *n, 1, MAX_FRAMES)?;
                self.frames(line, *n);
            }
            Command::Touch(touch) => {
                running("touch", self.suspended)?;
                finite(&[touch.x, touch.y])?;
                if let Some(pressure) = touch.pressure
                    && !(0.0..=1.0).contains(&pressure)
                {
                    return Err(format!("pressure {pressure} is outside 0.0–1.0"));
                }
                match touch.phase {
                    TouchPhase::Started => self.press(touch.id)?,
                    TouchPhase::Moved => self.held(touch.id)?,
                    TouchPhase::Ended | TouchPhase::Cancelled => self.release(touch.id)?,
                }
                self.push(line, Action::Touch(*touch));
            }
            Command::Tap(x, y) => {
                running("tap", self.suspended)?;
                finite(&[*x, *y])?;
                self.press(0)?;
                self.down.remove(&0);
                self.push(line, Action::Touch(touch(0, TouchPhase::Started, *x, *y)));
                self.frames(line, 1);
                self.push(line, Action::Touch(touch(0, TouchPhase::Ended, *x, *y)));
            }
            &Command::Drag {
                id,
                from,
                to,
                frames,
            } => {
                running("drag", self.suspended)?;
                finite(&[from.0, from.1, to.0, to.1])?;
                range("drag frames", frames, 1, MAX_FRAMES)?;
                self.press(id)?;
                self.down.remove(&id);
                self.push(
                    line,
                    Action::Touch(touch(id, TouchPhase::Started, from.0, from.1)),
                );
                for i in 1..=frames {
                    // In f64, so the f32 points land on round values where
                    // the operands are round; the last one is `to` itself.
                    let t = f64::from(i) / f64::from(frames);
                    let lerp =
                        |a: f32, b: f32| (f64::from(a) * (1.0 - t) + f64::from(b) * t) as f32;
                    let (x, y) = if i == frames {
                        to
                    } else {
                        (lerp(from.0, to.0), lerp(from.1, to.1))
                    };
                    self.push(line, Action::Touch(touch(id, TouchPhase::Moved, x, y)));
                    self.frames(line, 1);
                }
                self.push(
                    line,
                    Action::Touch(touch(id, TouchPhase::Ended, to.0, to.1)),
                );
            }
            Command::Hover(x, y) => {
                running("hover", self.suspended)?;
                finite(&[*x, *y])?;
                self.push(line, Action::Hover(*x, *y));
            }
            Command::Key(key) => {
                running("key", self.suspended)?;
                self.push(line, Action::Key(*key));
            }
            Command::Screenshot(name) => {
                running("screenshot", self.suspended)?;
                if self.frames == 0 {
                    return Err("`screenshot` before any frame has run".into());
                }
                let name = match name {
                    Some(name) => {
                        check_name(name)?;
                        name.clone()
                    }
                    None => format!("screenshot-{:06}", self.frames),
                };
                if !self.names.insert(name.clone()) {
                    return Err(format!("screenshot `{name}` is already taken"));
                }
                let frame = self.frames;
                self.push(line, Action::Screenshot { name, frame });
            }
            Command::Suspend => {
                if self.suspended {
                    return Err("`suspend` while already suspended".into());
                }
                self.suspended = true;
                self.push(line, Action::Suspend);
            }
            Command::Resume => {
                if !self.suspended {
                    return Err("`resume` while running".into());
                }
                self.suspended = false;
                self.push(line, Action::Resume);
            }
            Command::Exit => self.push(line, Action::Exit),
        }
        Ok(())
    }

    fn push(&mut self, line: u32, action: Action) {
        self.actions.push((line, action));
    }

    fn frames(&mut self, line: u32, n: u32) {
        self.frames += u64::from(n);
        // Consecutive frames from one command merge into one action.
        if let Some((last, Action::Frames(count))) = self.actions.last_mut()
            && *last == line
        {
            *count += n;
            return;
        }
        self.push(line, Action::Frames(n));
    }

    fn press(&mut self, id: u64) -> Result<(), String> {
        if self.down.insert(id) {
            Ok(())
        } else {
            Err(format!("touch {id} is already down"))
        }
    }

    fn held(&self, id: u64) -> Result<(), String> {
        if self.down.contains(&id) {
            Ok(())
        } else {
            Err(format!("touch {id} is not down"))
        }
    }

    fn release(&mut self, id: u64) -> Result<(), String> {
        if self.down.remove(&id) {
            Ok(())
        } else {
            Err(format!("touch {id} is not down"))
        }
    }
}

fn touch(id: u64, phase: TouchPhase, x: f32, y: f32) -> Touch {
    Touch {
        id,
        phase,
        x,
        y,
        pressure: None,
    }
}

fn range(what: &str, value: u32, min: u32, max: u32) -> Result<(), String> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(format!("{what} {value} is outside {min}–{max}"))
    }
}

fn finite(values: &[f32]) -> Result<(), String> {
    match values.iter().find(|value| !value.is_finite()) {
        Some(value) => Err(format!("{value} is not a finite number")),
        None => Ok(()),
    }
}

fn check_name(name: &str) -> Result<(), String> {
    let valid = (1..=MAX_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "screenshot name `{name}` must be 1–{MAX_NAME} of A–Z, a–z, 0–9, `_`, `-`"
        ))
    }
}

/// One line's words into a command, without the cross-line checks.
fn parse_command(words: &[&str]) -> Result<Command, String> {
    let (&name, args) = words.split_first().expect("a non-empty line");
    let arity = |counts: &[usize], usage: &str| {
        if counts.contains(&args.len()) {
            Ok(())
        } else {
            Err(format!("usage: {usage}"))
        }
    };
    let command = match name {
        "fps" => {
            arity(&[1], "fps <n>")?;
            Command::Fps(number(args[0])?)
        }
        "frames" => {
            arity(&[1], "frames <n>")?;
            Command::Frames(number(args[0])?)
        }
        "touch" => {
            arity(
                &[4, 5],
                "touch <id> <start|move|end|cancel> <x> <y> [<pressure>]",
            )?;
            let phase = match args[1] {
                "start" => TouchPhase::Started,
                "move" => TouchPhase::Moved,
                "end" => TouchPhase::Ended,
                "cancel" => TouchPhase::Cancelled,
                other => return Err(format!("unknown touch phase `{other}`")),
            };
            Command::Touch(Touch {
                id: number(args[0])?,
                phase,
                x: coordinate(args[2])?,
                y: coordinate(args[3])?,
                pressure: args.get(4).map(|p| coordinate(p)).transpose()?,
            })
        }
        "tap" => {
            arity(&[2], "tap <x> <y>")?;
            Command::Tap(coordinate(args[0])?, coordinate(args[1])?)
        }
        "drag" => {
            arity(&[6], "drag <id> <x0> <y0> <x1> <y1> <frames>")?;
            Command::Drag {
                id: number(args[0])?,
                from: (coordinate(args[1])?, coordinate(args[2])?),
                to: (coordinate(args[3])?, coordinate(args[4])?),
                frames: number(args[5])?,
            }
        }
        "hover" => {
            arity(&[2], "hover <x> <y>")?;
            Command::Hover(coordinate(args[0])?, coordinate(args[1])?)
        }
        "key" => {
            arity(&[1], "key <escape|back|space|character|U+XXXX>")?;
            Command::Key(key(args[0])?)
        }
        "screenshot" => {
            arity(&[0, 1], "screenshot [<name>]")?;
            Command::Screenshot(args.first().map(|name| (*name).to_owned()))
        }
        "suspend" => {
            arity(&[0], "suspend")?;
            Command::Suspend
        }
        "resume" => {
            arity(&[0], "resume")?;
            Command::Resume
        }
        "exit" => {
            arity(&[0], "exit")?;
            Command::Exit
        }
        other => return Err(format!("unknown command `{other}`")),
    };
    Ok(command)
}

fn number<T: std::str::FromStr>(word: &str) -> Result<T, String> {
    word.parse()
        .map_err(|_| format!("`{word}` is not a whole number in range"))
}

fn coordinate(word: &str) -> Result<f32, String> {
    match word.parse::<f32>() {
        Ok(value) if value.is_finite() => Ok(value),
        _ => Err(format!("`{word}` is not a finite number")),
    }
}

/// A single character, a key name, or `U+XXXX`. Single characters win, so
/// `key e` is the letter.
fn key(word: &str) -> Result<Key, String> {
    let mut chars = word.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Ok(Key::Character(c));
    }
    match word.to_ascii_lowercase().as_str() {
        "escape" => return Ok(Key::Escape),
        "back" => return Ok(Key::Back),
        "space" => return Ok(Key::Character(' ')),
        _ => {}
    }
    if let Some(hex) = word.strip_prefix("U+").or_else(|| word.strip_prefix("u+"))
        && (1..=6).contains(&hex.len())
        && let Some(c) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
    {
        return Ok(Key::Character(c));
    }
    Err(format!("unknown key `{word}`"))
}

#[cfg(test)]
mod tests {
    use super::{Action, Command, DEFAULT_FPS, Script, ScriptError};
    use crate::{Key, Touch, TouchPhase};

    fn actions(source: &str) -> Vec<Action> {
        Script::parse(source)
            .unwrap()
            .actions()
            .into_iter()
            .map(|(_, action)| action)
            .collect()
    }

    /// The line and message of a rejected script.
    fn rejected(source: &str) -> (u32, String) {
        match Script::parse(source) {
            Err(ScriptError::Parse { line, message }) => (line, message),
            other => panic!("expected a parse error, got {other:?}"),
        }
    }

    fn touch(id: u64, phase: TouchPhase, x: f32, y: f32) -> Action {
        Action::Touch(Touch {
            id,
            phase,
            x,
            y,
            pressure: None,
        })
    }

    #[test]
    fn every_command_parses() {
        let script = Script::parse(
            "# comment\n\
             fps 30\n\
             \n\
             frames 2   # trailing comment\n\
             touch 7 start 1 2 0.5\n\
             touch 7 move 3.5 4\n\
             touch 7 end 3.5 4\n\
             touch 8 start 0 0\n\
             touch 8 cancel 0 0\n\
             tap 10 20\n\
             drag 3 0 0 10 10 2\n\
             hover 5 6\n\
             key escape\n\
             screenshot\n\
             screenshot named_shot-1\n\
             suspend\n\
             resume\n\
             exit\n",
        )
        .unwrap();
        assert_eq!(script.fps(), 30);
        let lines: Vec<u32> = script.steps.iter().map(|(line, _)| *line).collect();
        assert_eq!(
            lines,
            [2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18]
        );
        assert_eq!(
            script.steps[2].1,
            Command::Touch(Touch {
                id: 7,
                phase: TouchPhase::Started,
                x: 1.0,
                y: 2.0,
                pressure: Some(0.5),
            })
        );
        assert_eq!(
            script.steps[8].1,
            Command::Drag {
                id: 3,
                from: (0.0, 0.0),
                to: (10.0, 10.0),
                frames: 2,
            }
        );
        assert_eq!(script.steps[9].1, Command::Hover(5.0, 6.0));
        assert_eq!(script.steps[11].1, Command::Screenshot(None));
        assert_eq!(
            script.steps[12].1,
            Command::Screenshot(Some("named_shot-1".into()))
        );
        let actions: Vec<Action> = script.actions().into_iter().map(|(_, a)| a).collect();
        assert_eq!(actions.last(), Some(&Action::Exit));
        assert!(actions.contains(&Action::Screenshot {
            name: "screenshot-000005".into(),
            frame: 5,
        }));
    }

    #[test]
    fn tabs_separate_and_fps_defaults() {
        let script = Script::parse("frames\t3\n").unwrap();
        assert_eq!(script.fps(), DEFAULT_FPS);
        assert_eq!(script.actions(), [(1, Action::Frames(3))]);
    }

    #[test]
    fn tap_is_down_frame_up() {
        assert_eq!(
            actions("tap 10 20"),
            [
                touch(0, TouchPhase::Started, 10.0, 20.0),
                Action::Frames(1),
                touch(0, TouchPhase::Ended, 10.0, 20.0),
            ]
        );
    }

    #[test]
    fn drag_moves_then_frames_and_ends_exactly() {
        assert_eq!(
            actions("drag 4 0 100 30 400 3\nscreenshot"),
            [
                touch(4, TouchPhase::Started, 0.0, 100.0),
                touch(4, TouchPhase::Moved, 10.0, 200.0),
                Action::Frames(1),
                touch(4, TouchPhase::Moved, 20.0, 300.0),
                Action::Frames(1),
                touch(4, TouchPhase::Moved, 30.0, 400.0),
                Action::Frames(1),
                touch(4, TouchPhase::Ended, 30.0, 400.0),
                Action::Screenshot {
                    name: "screenshot-000003".into(),
                    frame: 3,
                },
            ]
        );
        // An awkward fraction still lands exactly on `to`.
        let last_move = actions("drag 1 0.1 0.2 0.7 0.9 7")
            .into_iter()
            .rev()
            .find(|a| matches!(a, Action::Touch(t) if t.phase == TouchPhase::Moved));
        assert_eq!(last_move, Some(touch(1, TouchPhase::Moved, 0.7, 0.9)));
    }

    #[test]
    fn key_names() {
        let key = |word: &str| match &Script::parse(&format!("key {word}")).unwrap().steps[0].1 {
            Command::Key(key) => *key,
            other => panic!("{other:?}"),
        };
        assert_eq!(key("escape"), Key::Escape);
        assert_eq!(key("Escape"), Key::Escape);
        assert_eq!(key("BACK"), Key::Back);
        assert_eq!(key("space"), Key::Character(' '));
        assert_eq!(key("e"), Key::Character('e'));
        assert_eq!(key("é"), Key::Character('é'));
        assert_eq!(key("U+0020"), Key::Character(' '));
        assert_eq!(key("u+1F600"), Key::Character('😀'));
        assert_eq!(key("U+0023"), Key::Character('#'));
        for bad in ["enter", "U+D800", "U+110000", "U+", "ab"] {
            let (line, message) = rejected(&format!("key {bad}"));
            assert_eq!(line, 1);
            assert!(message.contains("unknown key"), "{bad}: {message}");
        }
    }

    #[test]
    fn line_rejections() {
        let cases = [
            ("capture", "unknown command `capture`"),
            ("frame 1", "unknown command"),
            ("frames", "usage: frames"),
            ("frames 1 2", "usage: frames"),
            ("frames -1", "not a whole number"),
            ("frames 0", "outside 1–1000000"),
            ("frames 1000001", "outside"),
            ("fps 0", "outside 1–1000"),
            ("fps 1001", "outside"),
            ("touch 1 press 0 0", "unknown touch phase"),
            ("touch 1 start 0", "usage: touch"),
            ("touch 1 start NaN 0", "not a finite number"),
            ("touch 1 start 0 inf", "not a finite number"),
            ("touch 1 start 0 0 1.5", "pressure 1.5"),
            ("hover 1", "usage: hover"),
            ("tap 1 2 3", "usage: tap"),
            ("drag 1 0 0 1 1", "usage: drag"),
            ("drag 1 0 0 1 1 0", "outside"),
            ("suspend now", "usage: suspend"),
        ];
        for (source, expected) in cases {
            let (line, message) = rejected(&format!("frames 1\n\n{source}"));
            assert_eq!(line, 3, "{source}");
            assert!(message.contains(expected), "{source}: {message}");
        }
    }

    #[test]
    fn order_rejections() {
        let cases = [
            ("frames 1\nfps 30", 2, "`fps` after the first frame"),
            ("touch 1 start 0 0\ntouch 1 start 0 0", 2, "already down"),
            ("touch 1 move 0 0", 1, "not down"),
            (
                "touch 1 start 0 0\ntouch 1 end 0 0\ntouch 1 cancel 0 0",
                3,
                "not down",
            ),
            ("touch 0 start 0 0\ntap 1 1", 2, "touch 0 is already down"),
            (
                "touch 5 start 0 0\ndrag 5 0 0 1 1 1",
                2,
                "touch 5 is already down",
            ),
            ("screenshot", 1, "before any frame"),
            ("frames 1\nscreenshot a\nscreenshot a", 3, "already taken"),
            (
                "frames 3\nscreenshot screenshot-000003\nscreenshot",
                3,
                "already taken",
            ),
            ("frames 1\nscreenshot a.png", 2, "must be 1–64"),
            ("suspend\nsuspend", 2, "already suspended"),
            ("resume", 1, "while running"),
            ("suspend\nframes 1", 2, "`frames` while suspended"),
            ("suspend\ntap 0 0", 2, "while suspended"),
            ("suspend\ndrag 0 0 0 1 1 1", 2, "while suspended"),
            ("suspend\ntouch 0 start 0 0", 2, "while suspended"),
            ("suspend\nhover 0 0", 2, "while suspended"),
            ("suspend\nkey a", 2, "while suspended"),
            ("frames 1\nsuspend\nscreenshot", 3, "while suspended"),
        ];
        for (source, line, expected) in cases {
            let (got_line, message) = rejected(source);
            assert_eq!(got_line, line, "{source}");
            assert!(message.contains(expected), "{source}: {message}");
        }
        let long = "x".repeat(65);
        assert!(
            rejected(&format!("frames 1\nscreenshot {long}"))
                .1
                .contains("1–64")
        );
        // Touches left down at the end are fine.
        assert!(Script::parse("touch 1 start 0 0\nframes 1").is_ok());
    }

    #[test]
    fn from_commands_numbers_lines_and_checks() {
        let script =
            Script::from_commands(vec![Command::Frames(2), Command::Screenshot(None)]).unwrap();
        assert_eq!(
            script.actions(),
            [
                (1, Action::Frames(2)),
                (
                    2,
                    Action::Screenshot {
                        name: "screenshot-000002".into(),
                        frame: 2,
                    }
                ),
            ]
        );
        let error = Script::from_commands(vec![Command::Resume]).unwrap_err();
        assert!(matches!(error, ScriptError::Parse { line: 1, .. }));
        assert_eq!(error.to_string(), "script line 1: `resume` while running");
    }
}
