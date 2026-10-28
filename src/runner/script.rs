//! Scripted runs: the app driven by a [`Script`] into an offscreen texture,
//! with no window or event loop.

use std::path::Path;
use std::time::Duration;

use libdevice::InsetType;

use super::overlay::Overlay;
use super::shell::Shell;
use crate::camera::ScriptFrame;
use crate::gpu::{self, Gpu};
use crate::screenshot::{self, Entry, ExitBy, Manifest};
use crate::script::{Action, Report, Screenshot, Script, ScriptError, ScriptRun};
use crate::{Audio, MorphApp, MorphContext, MorphError, SafeArea};

/// See [`crate::script::run`].
pub(crate) fn run<A: MorphApp>(script: &Script, run: ScriptRun) -> Result<Report, MorphError> {
    let dir = run.out.create().map_err(ScriptError::Io)?;
    println!("libmorph: output {}", dir.display());

    let gpu = pollster::block_on(async {
        let instance = gpu::instance(None).await;
        Gpu::new(instance, None, run.fallback_adapter, A::limits, false).await
    })?;
    let adapter = gpu.adapter.get_info();
    let spec = run.spec;
    let size = (spec.geometry.width, spec.geometry.height);
    // Asked as on every host, but scripted runs stay silent.
    let _ = super::audio_setup::<A>();
    let mut ctx = MorphContext::new(gpu, spec, run.args, Audio::silent());
    ctx.safe_area = SafeArea::from(spec.insets(InsetType::SafeDrawing));
    ctx.scripted = true;
    ctx.camera.scripted();
    // The app's directories, inside the output and empty for each run.
    ctx.data_dir = crate::storage::fresh(&dir.join("storage"));
    ctx.cache_dir = crate::storage::fresh(&dir.join("cache"));
    #[cfg(feature = "secure-store")]
    {
        ctx.secure_store = Some(crate::SecureStore::memory());
        ctx.key_store = Some(crate::Vault::memory());
    }

    let fps = script.fps();
    let mut shell = Shell::<A>::start_offscreen(ctx, size, Duration::from_secs(1) / fps);
    if run.overlay {
        shell.overlay = Some(Overlay::new(&shell.ctx));
    }
    let mut manifest = Manifest {
        script: script.path().map(|path| path.display().to_string()),
        device: spec.id.to_owned(),
        size: shell.ctx.surface_size,
        format: format!("{:?}", shell.ctx.surface_format),
        fps,
        adapter_name: adapter.name,
        adapter_backend: format!("{:?}", adapter.backend),
        exit_by: ExitBy::End,
        exit_line: script.last_line(),
        screenshots: Vec::new(),
    };

    // Whatever happens, the app is shut down (one `on_suspend`) before the
    // result is reported.
    let played = play(&mut shell, script, &dir, &mut manifest);
    shell.shutdown();
    played?;

    std::fs::write(dir.join("run.json"), manifest.to_json()).map_err(ScriptError::Io)?;
    let screenshots = manifest
        .screenshots
        .into_iter()
        .map(|entry| Screenshot {
            path: dir.join(&entry.file),
            frame: entry.frame,
            line: entry.line,
            pixels_crc32: entry.pixels_crc32,
        })
        .collect();
    Ok(Report { dir, screenshots })
}

/// Run the script's actions until it ends, says `exit`, or the app asks to.
fn play<A: MorphApp>(
    shell: &mut Shell<A>,
    script: &Script,
    dir: &Path,
    manifest: &mut Manifest,
) -> Result<(), ScriptError> {
    for (line, action) in script.actions() {
        match action {
            Action::Frames(n) => {
                for _ in 0..n {
                    shell.frame();
                    if shell.exit_requested() {
                        break;
                    }
                }
            }
            Action::Touch(touch) => shell.touch(touch),
            Action::Hover(x, y) => shell.hover(x, y),
            Action::Key(key) => {
                let status = shell.key(key);
                log::debug!("libmorph: script line {line}: {key:?} -> {status:?}");
            }
            Action::Screenshot { name, frame } => {
                let rgb = read_rgb(shell)?;
                let file = format!("{name}.png");
                let png = screenshot::encode_png(&rgb, shell.ctx.surface_size)
                    .map_err(|error| ScriptError::Encode(Box::new(error)))?;
                std::fs::write(dir.join(&file), png)?;
                manifest.screenshots.push(Entry {
                    file,
                    frame,
                    line,
                    pixels_crc32: screenshot::pixels_crc32(&rgb),
                });
            }
            Action::Suspend => shell.detach(),
            Action::Resume => shell.attach_offscreen(),
            Action::Open(url) => shell.open_url(url),
            Action::LowMemory => shell.low_memory(),
            Action::TrimMemory(level) => shell.trim_memory(level),
            Action::CameraQr(text) => {
                // The payload may be a secret: its length only.
                log::info!(
                    "libmorph: script line {line}: camera-qr ({} bytes)",
                    text.len()
                );
                if !shell.ctx.camera.script_qr(&text) {
                    log::warn!("libmorph: script line {line}: camera-qr with no QR scan running");
                }
            }
            Action::CameraFrame(path) => {
                let path = match script.path().and_then(Path::parent) {
                    Some(dir) => dir.join(path),
                    None => path,
                };
                let file = std::fs::read(&path).map_err(|error| {
                    ScriptError::Io(std::io::Error::new(
                        error.kind(),
                        format!("camera-frame {}: {error}", path.display()),
                    ))
                })?;
                let (width, height, rgba) =
                    screenshot::decode_png_rgba(&file).map_err(|error| {
                        ScriptError::Io(std::io::Error::other(format!(
                            "camera-frame {}: {error}",
                            path.display()
                        )))
                    })?;
                shell.ctx.camera.script_frame(ScriptFrame {
                    width,
                    height,
                    rgba,
                });
            }
            Action::CameraDeny { settings_only } => shell.ctx.camera.script_deny(settings_only),
            Action::Exit => {
                manifest.exit_by = ExitBy::Script;
                manifest.exit_line = Some(line);
                return Ok(());
            }
        }
        // Camera answers to this command (and to calls the app made in
        // its hooks) arrive now, before the next frame.
        shell.camera_events();
        if shell.exit_requested() {
            manifest.exit_by = ExitBy::App;
            manifest.exit_line = Some(line);
            return Ok(());
        }
    }
    Ok(())
}

/// Copy the offscreen texture back as tight RGB rows. Blocks until the GPU
/// has finished everything submitted so far.
fn read_rgb<A: MorphApp>(shell: &Shell<A>) -> Result<Vec<u8>, ScriptError> {
    let texture = shell
        .offscreen()
        .expect("the script checker rejects screenshots while suspended");
    let (width, height) = shell.ctx.surface_size;
    let bytes_per_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let device = &shell.gpu().device;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("libmorph screenshot"),
        size: u64::from(bytes_per_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("libmorph screenshot"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    shell.gpu().queue.submit([encoder.finish()]);
    let (sender, mapped) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |result| {
        let _ = sender.send(result);
    });
    // The wait runs the map callback before it returns.
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(readback)?;
    mapped
        .try_recv()
        .map_err(|_| readback("the buffer was not mapped"))?
        .map_err(readback)?;
    let rgb = {
        let view = buffer.get_mapped_range(..).map_err(readback)?;
        screenshot::unpad_rgb(&view, (width, height), bytes_per_row)
    };
    buffer.unmap();
    Ok(rgb)
}

fn readback(error: impl std::fmt::Display) -> ScriptError {
    ScriptError::Io(std::io::Error::other(format!(
        "screenshot readback: {error}"
    )))
}
