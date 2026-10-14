//! Screenshot files and the run manifest: RGB PNGs with fixed encoder
//! settings, and `run.json` written by hand in a fixed key order. Nothing
//! in either depends on when the run happened.

// The scripted runner (plan 04, step 5) is the user outside the tests.
#![allow(dead_code)]

use std::fmt::Write as _;

/// Readback rows (RGBA, each `bytes_per_row` long with padding) to tight
/// RGB rows, top to bottom. Alpha is dropped: windows present opaque.
pub(crate) fn unpad_rgb(padded: &[u8], (width, height): (u32, u32), bytes_per_row: u32) -> Vec<u8> {
    let (width, height, stride) = (width as usize, height as usize, bytes_per_row as usize);
    let mut rgb = Vec::with_capacity(width * height * 3);
    for row in padded.chunks(stride).take(height) {
        for pixel in row[..width * 4].chunks_exact(4) {
            rgb.extend_from_slice(&pixel[..3]);
        }
    }
    rgb
}

/// Encode tight RGB rows as a PNG: 8-bit RGB, flate2 level 6 with adaptive
/// filtering, an `sRGB` chunk, and no time or text chunks.
pub(crate) fn encode_png(
    rgb: &[u8],
    (width, height): (u32, u32),
) -> Result<Vec<u8>, png::EncodingError> {
    let mut file = Vec::new();
    let mut encoder = png::Encoder::new(&mut file, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    // `set_compression` also sets the filter, so the filter comes second.
    encoder.set_compression(png::Compression::Balanced);
    encoder.set_filter(png::Filter::Adaptive);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgb)?;
    writer.finish()?;
    Ok(file)
}

/// CRC-32 of the tight RGB rows: the same whichever encoder wrote the file.
pub(crate) fn pixels_crc32(rgb: &[u8]) -> u32 {
    crc32fast::hash(rgb)
}

/// How a run ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExitBy {
    /// The script ran out of commands.
    End,
    /// An `exit` command.
    Script,
    /// The app called `ctx.exit()`.
    App,
}

impl ExitBy {
    fn as_str(self) -> &'static str {
        match self {
            Self::End => "end",
            Self::Script => "script",
            Self::App => "app",
        }
    }
}

/// One screenshot's manifest entry.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub file: String,
    pub frame: u64,
    pub line: u32,
    pub pixels_crc32: u32,
}

/// `run.json`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Manifest {
    pub script: Option<String>,
    pub device: String,
    pub size: (u32, u32),
    pub format: String,
    pub fps: u32,
    pub adapter_name: String,
    pub adapter_backend: String,
    pub exit_by: ExitBy,
    /// The line the run ended on; `None` for an empty script.
    pub exit_line: Option<u32>,
    pub screenshots: Vec<Entry>,
}

impl Manifest {
    pub fn to_json(&self) -> String {
        let mut json = String::from("{\n");
        let _ = writeln!(
            json,
            "  \"libmorph\": {},",
            string(env!("CARGO_PKG_VERSION"))
        );
        let script = self.script.as_deref().map_or_else(|| "null".into(), string);
        let _ = writeln!(json, "  \"script\": {script},");
        let _ = writeln!(json, "  \"device\": {},", string(&self.device));
        let _ = writeln!(json, "  \"size\": [{}, {}],", self.size.0, self.size.1);
        let _ = writeln!(json, "  \"format\": {},", string(&self.format));
        let _ = writeln!(json, "  \"fps\": {},", self.fps);
        let _ = writeln!(
            json,
            "  \"adapter\": {{ \"name\": {}, \"backend\": {} }},",
            string(&self.adapter_name),
            string(&self.adapter_backend)
        );
        let line = self
            .exit_line
            .map_or_else(|| "null".into(), |line| line.to_string());
        let _ = writeln!(
            json,
            "  \"exit\": {{ \"by\": \"{}\", \"line\": {line} }},",
            self.exit_by.as_str()
        );
        if self.screenshots.is_empty() {
            json.push_str("  \"screenshots\": []\n");
        } else {
            json.push_str("  \"screenshots\": [\n");
            for (i, entry) in self.screenshots.iter().enumerate() {
                let comma = if i + 1 < self.screenshots.len() {
                    ","
                } else {
                    ""
                };
                let _ = writeln!(
                    json,
                    "    {{ \"file\": {}, \"frame\": {}, \"line\": {}, \"pixels_crc32\": \"{:08x}\" }}{comma}",
                    string(&entry.file),
                    entry.frame,
                    entry.line,
                    entry.pixels_crc32
                );
            }
            json.push_str("  ]\n");
        }
        json.push_str("}\n");
        json
    }
}

/// A JSON string literal.
fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{Entry, ExitBy, Manifest, encode_png, pixels_crc32, unpad_rgb};

    const SIZE: (u32, u32) = (3, 2);
    const STRIDE: u32 = 16;

    /// 3×2 RGBA rows padded to 16 bytes, with junk in alpha and padding.
    fn padded() -> Vec<u8> {
        let mut rows = Vec::new();
        for y in 0..2u8 {
            for x in 0..3u8 {
                rows.extend_from_slice(&[x * 80, y * 200, 7, 0x55]);
            }
            rows.extend_from_slice(&[0xEE; 4]);
        }
        rows
    }

    fn rgb() -> Vec<u8> {
        unpad_rgb(&padded(), SIZE, STRIDE)
    }

    /// Chunk types in file order.
    fn chunks(file: &[u8]) -> Vec<String> {
        assert_eq!(&file[..8], b"\x89PNG\r\n\x1a\n");
        let mut types = Vec::new();
        let mut at = 8;
        while at < file.len() {
            let length = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
            let kind = &file[at + 4..at + 8];
            let crc =
                u32::from_be_bytes(file[at + 8 + length..at + 12 + length].try_into().unwrap());
            assert_eq!(
                crc,
                crc32fast::hash(&file[at + 4..at + 8 + length]),
                "chunk CRC"
            );
            types.push(String::from_utf8_lossy(kind).into_owned());
            at += 12 + length;
        }
        types
    }

    #[test]
    fn padding_and_alpha_are_stripped() {
        assert_eq!(
            rgb(),
            [
                0, 0, 7, 80, 0, 7, 160, 0, 7, 0, 200, 7, 80, 200, 7, 160, 200, 7
            ]
        );
    }

    #[test]
    fn png_round_trips() {
        let file = encode_png(&rgb(), SIZE).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(&file));
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), SIZE);
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        assert_eq!(pixels, rgb());
    }

    #[test]
    fn png_is_deterministic_with_srgb_and_no_time() {
        let file = encode_png(&rgb(), SIZE).unwrap();
        assert_eq!(file, encode_png(&rgb(), SIZE).unwrap());
        let types = chunks(&file);
        assert_eq!(types.first().map(String::as_str), Some("IHDR"));
        assert_eq!(types.last().map(String::as_str), Some("IEND"));
        assert!(types.iter().any(|t| t == "sRGB"), "{types:?}");
        assert!(types.iter().any(|t| t == "IDAT"), "{types:?}");
        for absent in ["tIME", "tEXt", "zTXt", "iTXt"] {
            assert!(!types.iter().any(|t| t == absent), "{types:?}");
        }
    }

    #[test]
    fn pixel_crc_is_crc32_of_the_rows() {
        assert_eq!(pixels_crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn manifest_json() {
        let mut manifest = Manifest {
            script: Some("examples/scripts/touch \"one\"\\.morph".into()),
            device: "galaxy_s24".into(),
            size: (1080, 2340),
            format: "Rgba8UnormSrgb".into(),
            fps: 60,
            adapter_name: "GPU\tone\u{1}".into(),
            adapter_backend: "Metal".into(),
            exit_by: ExitBy::App,
            exit_line: Some(9),
            screenshots: vec![
                Entry {
                    file: "idle.png".into(),
                    frame: 30,
                    line: 3,
                    pixels_crc32: 0x1c29_1ca3,
                },
                Entry {
                    file: "after.png".into(),
                    frame: 51,
                    line: 8,
                    pixels_crc32: 0xa,
                },
            ],
        };
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(
            manifest.to_json(),
            format!(
                r#"{{
  "libmorph": "{version}",
  "script": "examples/scripts/touch \"one\"\\.morph",
  "device": "galaxy_s24",
  "size": [1080, 2340],
  "format": "Rgba8UnormSrgb",
  "fps": 60,
  "adapter": {{ "name": "GPU\tone\u0001", "backend": "Metal" }},
  "exit": {{ "by": "app", "line": 9 }},
  "screenshots": [
    {{ "file": "idle.png", "frame": 30, "line": 3, "pixels_crc32": "1c291ca3" }},
    {{ "file": "after.png", "frame": 51, "line": 8, "pixels_crc32": "0000000a" }}
  ]
}}
"#
            )
        );
        manifest.script = None;
        manifest.exit_by = ExitBy::End;
        manifest.exit_line = None;
        manifest.screenshots.clear();
        let json = manifest.to_json();
        assert!(json.contains("  \"script\": null,\n"), "{json}");
        assert!(
            json.contains("\"exit\": { \"by\": \"end\", \"line\": null },"),
            "{json}"
        );
        assert!(json.ends_with("  \"screenshots\": []\n}\n"), "{json}");
    }
}
