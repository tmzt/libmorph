---
title: Scripted runs and PNG capture
date: 2026-10-14
status: In implementation
covers:
  - A script format that drives a `MorphApp` through frames, touches, hover,
    keys, suspend and resume, with a fixed frame time
  - A headless desktop runner that renders into an offscreen texture, so a
    script needs no window
  - Capturing frames to PNG with the `png` crate, and a `run.json` manifest
  - Output directories: an exact path, or a dated sequence named like the
    plans (`out/morph/YYYYMMDD_NN_<name>/`)
  - `--morph-script` and `--morph-out` options; a `script` Cargo feature
sources:
  - Owner's request (2026-10-14): "start planning a scripting and png output
    feature with deterministic or defined output directories (such as
    timestamped or using the same convention as plan). also use a real png
    output crate (minimal) not a bad reimplementation that breaks common
    features like IDAT hash, or argue for a deflate2 based implementation"
  - png 0.18.1 (image-rs/image-png): dependencies bitflags, crc32fast,
    fdeflate, flate2, miniz_oxide. `Encoder::set_compression`
    (`Compression::Balanced` maps to `DeflateCompression::Level(6)`, flate2's
    default level, with `Filter::Adaptive`), `set_filter`,
    `set_source_srgb`, `write_image_data`. It writes no `tIME` chunk unless
    asked.
  - wgpu 30.0.1: `COPY_BYTES_PER_ROW_ALIGNMENT` (256),
    `CommandEncoder::copy_texture_to_buffer`, `Buffer::map_async`,
    `RequestAdapterOptions::compatible_surface: Option<&Surface>`
  - libmorph 1a9c510: `src/runner/shell.rs` (`Shell`, `Lifecycle`,
    `FrameStatus`), `src/gpu.rs` (`Gpu::new` takes a surface), `src/config.rs`
    (`Launch`, `MorphOptions`, `MorphError`), `src/audio.rs` (`Audio` with no
    output stream is a silent no-op), `src/runner/overlay.rs`
related:
  - docs/design/20261014_03_SHELL_INPUT_AND_LIFECYCLE_PLAN.md (Implemented):
    `Touch`, `Key`, `on_hover`, `on_suspend`/`on_resume` and `exit()`, which
    scripts drive
  - AGENTS.md "Core API", "Lifecycle", "CLI Overrides" (step 8 updates them)
---

# Scripted runs and PNG capture

## 1. Aim

Run a `MorphApp` from a script instead of a person, with no window, and write
chosen frames to PNG files in a directory whose name is either given or
follows a fixed rule. The same script, app, adapter and `Cargo.lock` produce
the same files, byte for byte. This gives:

- regression checks: render, capture, and compare against earlier runs;
- evidence for `out/` logs (Rule 3.5) without a person at the machine;
- runs in agent sandboxes and in `cargo test`, which cannot open windows on
  macOS.

Desktop only (macOS, Linux, Windows). The web and Android are out of scope:
the web has no command line or file system, and Android owns its own window.
Recording live input into a script is out of scope; section 9 reserves it.

## 2. Determinism

A scripted run removes the run's own sources of variation:

- **Time:** `update` receives a fixed `dt`, `1 s / fps` in whole nanoseconds
  (`Duration::from_secs(1) / fps`; 16 666 666 ns at the default 60). The wall
  clock is never read for frame timing.
- **Input:** only what the script says, in script order, between frames.
- **Size and scale:** the emulated device's `spec.geometry`, `pixel_ratio`
  and safe area, exactly as the windowed desktop runner sets them. No monitor
  or window manager is involved.
- **Audio:** silent. The context's `Audio` has no output stream, so
  `is_available()` is `false` and every call is a no-op, as on a machine with
  no audio device.
- **Output files:** no timestamps or other run-dependent data in the PNGs or
  the manifest (section 6). The directory name may contain a date
  (section 5); the files inside never do.

Not promised: identical pixels on different GPUs, drivers or wgpu versions.
Rasterisation and floating-point precision differ between them. The manifest
records the adapter so runs can be compared like for like. Apps that read the
clock or random numbers themselves are their own concern.

## 3. Script format

A UTF-8 text file, conventionally `*.morph`. One command per line. Words are
separated by spaces or tabs. `#` starts a comment that runs to the end of the
line, and blank lines are ignored. Coordinates are physical pixels of the
surface, like `Touch` and `on_hover`. Numbers are decimal: integers for
counts and ids, and `f32` for coordinates and pressure.

| Command | Effect |
|---|---|
| `fps <n>` | Sets the fixed frame rate, 1–1000. Only allowed before the first `frames`. Default 60. |
| `frames <n>` | Runs `n` frames (1–1 000 000). Each one calls `update(dt)`, then `render`, then draws the overlay. |
| `touch <id> <phase> <x> <y> [<pressure>]` | Calls `on_touch` with `Touch { id, phase, x, y, pressure }`. `phase` is `start`, `move`, `end` or `cancel`. `pressure` is 0.0–1.0, and `None` when absent. |
| `drag <id> <x0> <y0> <x1> <y1> <n>` | `start` at (x0, y0). Then, for i in 1..=n: `move` to the point i/n of the way along the line, then one frame. Then `end` at (x1, y1). This is n frames in total. |
| `hover <x> <y>` | Calls `on_hover`. |
| `key <name>` | Calls `on_key`. `name` is `escape`, `back`, a single non-space character, or `U+XXXX` (hex, any Unicode scalar, e.g. `U+0020` for space). The status the app returns is logged and otherwise ignored, as on desktop. |
| `capture [<name>]` | Writes the last rendered frame to `<name>.png`. `name` matches `[A-Za-z0-9_-]{1,64}`. Without a name the file is `frame-NNNNNN.png`, numbered by frames run so far (six digits, starting at 000001). |
| `suspend` | Drops the target texture and calls `on_suspend`, as when a window goes away. |
| `resume` | Creates a new target in the same format and calls `on_resume`. |
| `exit` | Ends the run. The end of the file does the same. |

Touch ids are `u64`. Several ids may be down at once: these are
script touches, not the mouse, so the rule that mouse buttons never stand in
for extra fingers is unaffected.

The parser checks the whole file before the GPU is brought up and rejects it
with the line number and a message when:

- a command, phase or key name is unknown, or the argument count is wrong;
- a number is out of range or not finite;
- `fps` comes after the first `frames`;
- a `touch` `start` or `drag` uses an id that is already down, or a
  `move`/`end`/`cancel` uses an id that is not down;
- `capture` comes before any frame has run, or a name is used twice;
- `frames`, `drag`, `touch`, `hover`, `key` or `capture` comes while
  suspended, `suspend` while suspended, or `resume` while running.

Touches still down at the end of the script are left alone. libmorph never
invents a release.

If the app calls `ctx.exit()`, the run stops after the current hook and the
remaining commands are skipped. The manifest records this as
`"exit": "app"` with the line number. It is not an error. Every way a run
ends goes through `Shell::shutdown`, so the app hears `on_suspend` exactly
once, unless it is already suspended.

Example (`examples/scripts/touch.morph`, step 7):

```
# Idle, drag across, release, back out.
frames 30
capture idle
drag 1 100 400 900 1800 20
capture dragged
key escape
frames 1
capture after
```

## 4. Headless runner

### 4.1 GPU and target

- `Gpu::new` takes `Option<&wgpu::Surface>`. The headless runner passes
  `None`, so `compatible_surface` is `None`. Backends and limits are the
  desktop ones, and `--morph-fallback-adapter` still applies.
- The render target is a texture of `spec.geometry` size, with usage
  `RENDER_ATTACHMENT | COPY_SRC`. Its format is `Rgba8UnormSrgb` when
  `A::SRGB_SURFACE` is true and `Rgba8Unorm` otherwise. Both are renderable
  and copyable on every adapter, and an RGBA order means readback needs no
  channel swap. Windowed desktop usually gets `Bgra8*`. Apps already build
  pipelines for `ctx.surface_format()`, so this is invisible to them.
- `Shell` holds its target as an internal enum,
  `Target::Surface(surface, config)` or `Target::Offscreen(texture)`.
  `start`, `attach`, `detach`, `shutdown` and `Lifecycle` are unchanged, and
  `frame()` renders into whichever target is attached. For the offscreen
  target it submits and returns `Presented` without presenting anything.
  `suspend` drops the texture (`detach`), and `resume` creates a new one in
  the kept format (`attach`).
- The overlay is drawn as in a window, unless `--morph-no-overlay` is given,
  so captures match what the window shows.

### 4.2 Readback

On `capture`, after the frame's overlay pass:

1. Copy the texture into a buffer with `bytes_per_row` = `width * 4`
   rounded up to 256 (`COPY_BYTES_PER_ROW_ALIGNMENT`). Submit, map the
   buffer for reading, and poll the device until the map completes. This is
   the native runner, so blocking is fine.
2. Strip the row padding and drop alpha: 3 bytes per pixel, rows top to
   bottom. Windowed surfaces are presented opaque, so RGB is what a person
   sees.
3. Hand the rows to the PNG writer (section 6).

Readback happens only on frames being captured. Other frames never wait on
the GPU.

## 5. Output directory

One directory per run, holding only this run's PNGs and `run.json`.

- **`--morph-out <dir>`** (exact): the directory is used as given and created
  with any missing parents. Files with the names this run writes are
  overwritten, and nothing else is touched. `run.json` lists exactly this
  run's files, so leftovers from earlier runs are easy to spot. Use this for
  fixed comparison paths and in tests.
- **Default** (sequenced, the plan convention): `out/morph/YYYYMMDD_NN_<stem>/`
  under the current directory.
  - `YYYYMMDD` is the date when the run starts.
  - `NN` is the next free two-digit sequence number for that date in
    `out/morph/`: one more than the highest existing `YYYYMMDD_NN` prefix,
    starting at `01`. It grows to three digits after `99`.
  - `<stem>` is the script's file stem, with characters outside
    `[A-Za-z0-9_-]` replaced by `-`.

  The last path component is created with `create_dir`, not
  `create_dir_all`. If another run took the number first, the next number is
  tried, so concurrent runs never share a directory.
- The directory is printed on stdout as `libmorph: output <path>` when the
  run starts, and is returned in `Report::dir` (section 7).

`out/` is gitignored in this repository. Apps that use the default should
ignore it too.

## 6. PNG output

**Crate: `png` 0.18.1**, the image-rs encoder, without the `image` crate.

Why not a hand-written writer on flate2: png 0.18 already compresses through
flate2 (miniz_oxide) for the levels used here, and uses crc32fast for chunk
CRCs. A writer of our own would still need the signature, IHDR, per-row
filter bytes, a zlib stream split into IDAT chunks, a CRC on every chunk, sRGB
and IEND. It would save two small crates (bitflags and fdeflate), and we
would own an encoder that nothing else tests. The bugs that break other
tools, such as bad CRCs, wrong chunk lengths or a malformed zlib trailer, are
exactly the ones png's tests and fuzzing already cover. Its decoder also
serves our round-trip tests at no extra cost.

Fixed settings, all set explicitly rather than taken from defaults:

- colour type RGB, bit depth 8;
- `Compression::Balanced` (flate2 level 6) and `Filter::Adaptive`;
- an `sRGB` chunk, perceptual intent (`set_source_srgb`), for both target
  formats. With `SRGB_SURFACE = false` the app gamma-encodes itself, so the
  stored bytes are sRGB-encoded either way, and that is how a display shows
  them;
- no `tIME`, `tEXt` or other metadata chunks.

The same pixels therefore give the same file bytes for as long as
`Cargo.lock` pins png, flate2 and miniz_oxide. If something in a build turns
on flate2's `zlib-rs` or `zlib` backend through feature unification, the
compressed bytes can change. `run.json` therefore also records a CRC-32 of
the raw RGB rows (crc32fast, already one of png's dependencies), which stays
the same across encoders. Tools that hash IDAT or the whole file work on
these files, because they are ordinary, valid PNGs.

**`run.json`**, written by hand (no serde; the values below are
illustrative). Only the adapter name, device id and script path need string
escaping (`"`, `\` and control characters):

```json
{
  "libmorph": "0.1.0",
  "script": "examples/scripts/touch.morph",
  "device": "galaxy_s24",
  "size": [1080, 2340],
  "format": "Rgba8UnormSrgb",
  "fps": 60,
  "adapter": { "name": "Apple M2", "backend": "Metal" },
  "exit": { "by": "end", "line": 9 },
  "captures": [
    { "file": "idle.png", "frame": 30, "line": 3, "pixels_crc32": "1c291ca3" }
  ]
}
```

Key order is fixed, and the file ends with a newline. `exit.by` is `end`,
`script` (an `exit` command) or `app`.

## 7. Public API

The parser builds without the feature: it has no dependencies and can be
used to check scripts anywhere. The runner and PNG writing need the `script`
feature and a native desktop target.

```rust
pub mod script {
    /// A parsed, checked script.
    pub struct Script { /* commands with their line numbers */ }

    impl Script {
        pub fn parse(source: &str) -> Result<Script, ScriptError>;
        pub fn load(path: &Path) -> Result<Script, ScriptError>;
        /// The same checks as `parse`; lines are numbered from 1 in order.
        pub fn from_commands(commands: Vec<Command>) -> Result<Script, ScriptError>;
    }

    #[non_exhaustive]
    pub enum Command {
        Fps(u32),
        Frames(u32),
        Touch(Touch),
        Drag { id: u64, from: (f32, f32), to: (f32, f32), frames: u32 },
        Hover(f32, f32),
        Key(Key),
        Capture(Option<String>),
        Suspend,
        Resume,
        Exit,
    }

    #[non_exhaustive]
    pub enum ScriptError {
        Parse { line: u32, message: String },
        Io(std::io::Error),
        Encode(Box<dyn std::error::Error + Send + Sync>),
        /// `--morph-script` without the `script` feature.
        Disabled,
    }

    pub enum OutDir {
        /// `<root>/YYYYMMDD_NN_<stem>/` (section 5).
        Sequenced { root: PathBuf, stem: String },
        Exact(PathBuf),
    }

    // `script` feature, native desktop only:
    pub struct ScriptRun {
        pub spec: DeviceSpec,
        pub out: OutDir,
        pub overlay: bool,
        pub fallback_adapter: bool,
        /// What `ctx.args()` returns, program name first.
        pub args: Vec<String>,
    }

    pub struct Report {
        pub dir: PathBuf,
        pub captures: Vec<Capture>,
    }

    pub struct Capture {
        pub path: PathBuf,
        pub frame: u64,
        pub line: u32,
        pub pixels_crc32: u32,
    }

    pub fn run<A: MorphApp>(script: &Script, run: ScriptRun) -> Result<Report, MorphError>;
}
```

`MorphError` (already `#[non_exhaustive]`) gains `Script(ScriptError)`. No
existing signature changes. `Gpu::new` and `Shell` are crate-private.

`script::run` brings up the GPU without a window or event loop, so it can be
called from `cargo test` on any thread.

## 8. CLI and Cargo

- New options, parsed by `Launch::from_args` and listed in `MORPH_ARGS_HELP`:
  - `--morph-script <path>`: run the script headless instead of opening a
    window;
  - `--morph-out <dir>`: the exact output directory (section 5). Without
    `--morph-script` it is an error.

  Both also accept the `--opt=value` form, like libdevice.
- `libmorph::run::<A>()` checks for `--morph-script`. If present, it loads
  the script (errors are reported before any GPU work), builds a `ScriptRun`
  from the `Launch`, calls `script::run`, and returns. Without the `script`
  feature it returns `MorphError::Script(ScriptError::Disabled)`.
- Cargo:

  ```toml
  [features]
  script = ["dep:png", "dep:crc32fast"]

  [target.'cfg(not(any(target_os = "android", target_arch = "wasm32")))'.dependencies]
  png = { version = "0.18.1", optional = true }
  crc32fast = { version = "1.5", optional = true }
  ```

  Off by default, so apps that never script don't build png. On the web and
  Android the feature enables nothing and the runner code is compiled out.
  `crc32fast` is named directly, because png doesn't re-export it. It is the
  same crate png already pulls in.

## 9. Out of scope, reserved

- Recording live mouse, touch and key input into a `.morph` file
  (`--morph-record`).
- Captures requested by the app (`ctx.capture(name)`), for example from a
  screenshot key.
- Pixel comparison against reference images, with tolerances. For now the
  manifest's CRCs and the files themselves are what tools compare.
- Scripted runs on the web or Android.
- A windowed playback mode that shows the script running.

## 10. Open decisions (owner)

These are the recommended choices. The plan above assumes them. Any change
is written as one amendment before implementation (Rule 2).

1. **Script language:** the line-based command file of section 3. The
   alternative is an embedded language (Rhai or Lua), which brings a large
   dependency and its own sources of non-determinism, for loops and logic
   that a generated `.morph` file or `Script::from_commands` already covers.
2. **Default directory:** the plan-style sequence `YYYYMMDD_NN_<stem>`. The
   alternative is a timestamp, `YYYYMMDD_HHMMSS_<stem>`, which needs no
   directory scan but gives longer names and isn't the plan convention.
   `--morph-out` covers fixed paths in either case.
3. **Date in the directory name:** UTC, computed from `SystemTime` with no
   new dependency. A run after 20:00 EDT is dated the next day. Local time
   needs a timezone crate (`jiff`) or a platform call.
4. **Feature default:** `script` off by default, as in section 8.
5. **PNG encoder:** the `png` crate, as argued in section 6, rather than a
   hand-written writer on flate2.

## 11. Steps

Each step is one commit and passes `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets` and `cargo test --workspace`, with
default features, with `--no-default-features`, and with `--features script`.
The Android (`--features mobile`) and wasm (`--features web`) clippy checks
must stay clean (Rule 3.4). The first step sets `status: In implementation`
(Rule 1.5).

1. **Parser** (`src/script.rs`: `Script`, `Command`, `ScriptError`; section
   3), always compiled.
   **Done when:** unit tests cover every command in its valid form, the
   `drag` expansion, and every rejection listed in section 3, each reporting
   the right line number.
2. **Output directory** (`OutDir` resolution, the UTC date, sequence
   numbering, stem cleaning; section 5).
   **Done when:** tests in a fresh directory under `std::env::temp_dir()`
   cover the first run of a day (`_01`), the next number after existing
   entries (including a gap and a three-digit `100`), skipping a taken
   number, and an exact path being created with its parents. The
   days-to-date conversion is tested against fixed dates, including leap
   days.
3. **PNG and manifest writing** (feature `script`; section 6).
   **Done when:** a test encodes known padded RGBA rows, decodes the file
   with png's decoder and gets the RGB pixels back; encoding the same pixels
   twice gives identical bytes; the file has an `sRGB` chunk and no `tIME`;
   and `run.json` for a fixed report matches an expected string exactly,
   including escaping.
4. **Headless target in `Shell`** (`Target` enum, `Gpu::new` with
   `Option<&Surface>`; section 4.1). Windowed runs are unchanged.
   **Done when:** the step's checks pass, and the windowed desktop and
   Android code paths compile unchanged apart from the `Target` match.
5. **`script::run`** (section 4, readback, `Report`, silent audio, exit
   handling).
   **Done when:** an integration test runs a small app that clears to a
   colour chosen from the last touch through a script with `drag`,
   `suspend`, `resume` and two captures. It checks the captured pixels, the
   app's counts (one `on_suspend` per suspend plus the final one, one
   `on_resume`), and that a second run into an exact directory gives
   byte-identical PNGs and `run.json`. If no adapter is found, the test
   prints that it was skipped and passes; the owner confirms it ran on the
   Mac.
6. **CLI** (`--morph-script`, `--morph-out`, help text, `run()` dispatch;
   section 8).
   **Done when:** `Launch` tests cover both options in both forms,
   `--morph-out` without a script, and `Disabled` without the feature.
7. **Example script** (`examples/scripts/touch.morph`, section 3) and a
   line in the touch example's doc comment on running it with
   `--morph-script`.
   **Done when:**
   `cargo run --example touch --features script -- --morph-script examples/scripts/touch.morph`
   writes `idle.png`, `dragged.png`, `after.png` and `run.json` into a new
   `out/morph/YYYYMMDD_NN_touch/`, and two runs with the same `--morph-out`
   give identical files.
8. **AGENTS.md and README** (their own commits): AGENTS.md's "Core API" and
   "CLI Overrides" sections, and the README's options table and features
   list, describe scripted runs, `--morph-out` and the `script` feature.
   **Done when:** both describe sections 3, 5 and 8 as built.

## Amendment — 2026-10-14 15:07 EDT: Owner's decisions; screenshot, tap and key names

The owner decided (2026-10-14) on section 10:

1. **Script language:** plain text files, one English command per line,
   with space-separated operands (section 3 as written). No embedded
   language.
2. **Default directory** and 3. **date:** not changed by the owner. Section 5
   stands: `out/morph/YYYYMMDD_NN_<stem>/`, dated in UTC.
4. **Feature:** `script` is optional and off by default (section 8).
5. **PNG:** the `png` crate (section 6).

The owner asked for touch commands, screenshot commands and special keys.
This replaces the following parts of sections 3, 6, 7 and 11:

- **`screenshot` replaces `capture`** everywhere, as
  `screenshot [<name>]`, with the same naming rules and behaviour. Unnamed
  files become `screenshot-NNNNNN.png`. In the API, `Command::Capture`
  becomes `Command::Screenshot`, `Report::captures` becomes
  `Report::screenshots`, and `struct Capture` becomes `struct Screenshot`.
  The `run.json` key `captures` becomes `screenshots`. The parser
  rejects `capture` as an unknown command.
- **New touch command `tap <x> <y>`:** `start` at (x, y) with id 0, one
  frame, then `end` at (x, y). Like `drag`, it is rejected while id 0 is
  down. In the API it is `Command::Tap(f32, f32)`. `touch` and `drag`
  are unchanged.
- **Special keys:** `key <name>` takes the names `escape`, `back` and
  `space` (`Key::Character(' ')`), case-insensitive, as well as a single
  non-space character or `U+XXXX`. A single-letter operand is always a
  character, so `key e` is `Character('e')`. Each `Key` variant has a
  script name. When `Key` gains variants (enter, tab, arrows and so on, in a
  later plan), each one gets a lower-case name here, and `key` accepts no
  name that has no `Key` variant.
- The example in section 3 and step 7 use `screenshot`. Step 7's files are
  `idle.png`, `dragged.png` and `after.png`, as before.
- Step 1's **Done when** also covers `tap`, every key name, and the
  rejection of `capture`.

The owner approved the plan as amended. Implementation is done in a separate
git worktree and branch. The first implementing commit sets
`status: In implementation` (Rule 1.5).

## Amendment — 2026-10-14 15:11 EDT: Implementation started

Step 1 (the parser, `src/script.rs`) is committed with this amendment, on
the `scripted-runs` branch. The status is `In implementation`, and this
document is now immutable (Rule 1.5).
