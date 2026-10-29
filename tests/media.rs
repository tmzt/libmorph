//! Media requests in scripted runs (plan `20261028_02_MEDIA_REQUEST_PLAN.md`,
//! 3.4): a request answered with a PNG whose bytes the app sees, a cancel,
//! and a denial. Needs a GPU adapter; without one each test says so and
//! passes.

#![cfg(all(
    feature = "script",
    not(any(target_os = "android", target_arch = "wasm32"))
))]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use libmorph::media::{
    CameraEntry, Delivery, MediaData, MediaError, MediaRequest, MediaRequestId, MediaResponse,
    MediaSource,
};
use libmorph::script::{OutDir, Report, Script, ScriptRun};
use libmorph::{Frame, GpuError, MorphApp, MorphContext, MorphError, wgpu};

fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("libmorph-media-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `A` through `script` into `dir/out`; `None` without a GPU.
fn run<A: MorphApp>(script: &Script, dir: &Path) -> Option<Report> {
    let mut spec = libmorph::libdevice::GALAXY_S24;
    (spec.geometry.width, spec.geometry.height) = (64, 64);
    let run = ScriptRun {
        spec,
        out: OutDir::Exact(dir.join("out")),
        overlay: false,
        fallback_adapter: false,
        args: vec!["media".into()],
    };
    match libmorph::script::run::<A>(script, run) {
        Err(MorphError::Gpu(GpuError::Adapter(error))) => {
            eprintln!("skipped: no GPU adapter ({error})");
            None
        }
        result => Some(result.unwrap()),
    }
}

fn clear(ctx: &MorphContext, frame: &Frame<'_>) {
    let mut encoder = ctx.device().create_command_encoder(&Default::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: frame.view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    ctx.queue().submit([encoder.finish()]);
}

/// What the apps saw, one test at a time.
static SEEN: Mutex<Vec<(Option<MediaRequestId>, String)>> = Mutex::new(Vec::new());
static LOCK: Mutex<()> = Mutex::new(());

fn describe(response: &MediaResponse) -> String {
    match &response.result {
        Ok(reply) => reply
            .items
            .iter()
            .map(|item| {
                let data = match &item.data {
                    MediaData::Bytes(bytes) => format!("bytes {}", bytes.len()),
                    MediaData::File(path) => {
                        format!("file {}", std::fs::read(path).map_or(0, |b| b.len()))
                    }
                    other => format!("{other:?}"),
                };
                format!(
                    "{:?} {} {:?} {data}",
                    item.source,
                    item.mime.as_deref().unwrap_or("-"),
                    item.size
                )
            })
            .collect::<Vec<_>>()
            .join("; "),
        Err(error) => format!("{error:?}"),
    }
}

/// Asks in `init` for one image (the camera entry asked for too) and again,
/// as files, for up to two.
struct Asker {
    first: MediaRequestId,
    second: Option<MediaRequestId>,
}

impl MorphApp for Asker {
    fn init(ctx: &mut MorphContext) -> Self {
        let mut request = MediaRequest::default();
        request.camera = CameraEntry::Photo;
        Self {
            first: ctx.media_request(&request),
            second: None,
        }
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        clear(ctx, frame);
    }

    fn on_media(&mut self, ctx: &mut MorphContext, response: &MediaResponse) {
        SEEN.lock()
            .unwrap()
            .push((response.request, describe(response)));
        if response.request == Some(self.first) {
            let reply = response.result.as_ref().unwrap();
            let MediaData::Bytes(bytes) = &reply.items[0].data else {
                panic!("bytes expected");
            };
            assert!(bytes.starts_with(b"\x89PNG"), "the PNG's own bytes");
            assert_eq!(reply.items[0].source, MediaSource::Camera);
            let mut request = MediaRequest::default();
            request.max_items = 2;
            request.delivery = Delivery::File;
            self.second = Some(ctx.media_request(&request));
        } else {
            assert_eq!(response.request, self.second);
        }
    }
}

fn write_png(path: &Path) -> usize {
    let mut file = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut file, 2, 1);
        encoder.set_color(png::ColorType::Rgb);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255, 0, 0, 0, 255, 0]).unwrap();
    }
    std::fs::write(path, &file).unwrap();
    file.len()
}

#[test]
fn a_request_answered_with_a_png_then_files() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    SEEN.lock().unwrap().clear();
    let dir = dir("png");
    let size = write_png(&dir.join("red.png"));
    std::fs::write(dir.join("notes.txt"), b"hello").unwrap();
    std::fs::write(
        dir.join("pick.morph"),
        "frames 1\nmedia-camera red.png\nframes 1\nmedia red.png notes.txt red.png\nframes 1\n",
    )
    .unwrap();
    let script = Script::load(&dir.join("pick.morph")).unwrap();
    if run::<Asker>(&script, &dir).is_none() {
        return;
    }
    let seen = SEEN.lock().unwrap().clone();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!(
        seen[0].1,
        format!("Camera image/png Some({size}) bytes {size}")
    );
    assert_eq!(
        seen[1].1,
        format!("Picked image/png Some({size}) file {size}; Picked - Some(5) file 5")
    );
}

/// Asks once in `init`, records the answer.
struct Once;

impl MorphApp for Once {
    fn init(ctx: &mut MorphContext) -> Self {
        ctx.media_request(&MediaRequest::default());
        Self
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) {
        clear(ctx, frame);
    }

    fn on_media(&mut self, _ctx: &mut MorphContext, response: &MediaResponse) {
        SEEN.lock()
            .unwrap()
            .push((response.request, describe(response)));
    }
}

#[test]
fn cancel_and_denials() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for (command, expected) in [
        ("media-cancel", MediaError::Cancelled),
        (
            "media-deny",
            MediaError::PermissionDenied {
                settings_only: false,
            },
        ),
        (
            "media-deny-forever",
            MediaError::PermissionDenied {
                settings_only: true,
            },
        ),
    ] {
        SEEN.lock().unwrap().clear();
        let dir = dir(command);
        let script = Script::parse(&format!("frames 1\n{command}\nframes 1\n")).unwrap();
        if run::<Once>(&script, &dir).is_none() {
            return;
        }
        let seen = SEEN.lock().unwrap().clone();
        assert_eq!(seen.len(), 1, "{command}: {seen:?}");
        assert!(seen[0].0.is_some(), "{command}: answered with its id");
        assert_eq!(seen[0].1, format!("{expected:?}"), "{command}");
    }
}
