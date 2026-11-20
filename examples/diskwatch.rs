//! Disk pressure (`libmorph::disk`): the app watches its data directory's
//! volume every 15 s (`MorphApp::DISK_WATCH`) and logs the starting grade
//! and each change (`on_disk_pressure`), with the numbers. The screen is
//! grey at `Unknown` (no current reading: none yet, a failed read, or none
//! for 30 s, as while paused), green at `None`, amber at `Low` and red at
//! `Critical`. A tap checks the
//! volume at once (`ctx.check_disk_now()`) and logs the full reading
//! (`ctx.disk_space()`, which on Android adds `StorageManager`'s numbers).
//!
//! ```sh
//! RUST_LOG=libmorph=debug,diskwatch=info cargo run --example diskwatch
//! df -k "$HOME/Library/Application Support/dev.highbay.libmorph.example.diskwatch"
//! ```
//!
//! The debug log shows one `disk check` per 15 s, and none while the
//! window is minimised. To see `Critical` without filling a disk, build
//! with a raised critical threshold in GiB, read at compile time:
//! `DISKWATCH_CRITICAL_GIB=100000 cargo run --example diskwatch`, or on
//! Android `DISKWATCH_CRITICAL_GIB=100000 examples/android/build.sh diskwatch`.

use std::time::Duration;

use libmorph::disk::{DiskPressure, DiskPressureReport, DiskThresholds, DiskWatch};
use libmorph::{Frame, MorphApp, MorphContext, RenderStatus, Touch, TouchPhase};

const GIB: u64 = 1 << 30;

/// A whole number of GiB from the build's environment, or `default`.
const fn gib_or(text: Option<&str>, default: u64) -> u64 {
    let Some(text) = text else {
        return default;
    };
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return default;
    }
    let mut value: u64 = 0;
    let mut i = 0;
    while i < bytes.len() {
        let digit = bytes[i];
        assert!(
            digit.is_ascii_digit(),
            "DISKWATCH_CRITICAL_GIB is whole GiB"
        );
        value = value * 10 + (digit - b'0') as u64;
        i += 1;
    }
    value * GIB
}

const CRITICAL: u64 = gib_or(
    option_env!("DISKWATCH_CRITICAL_GIB"),
    DiskThresholds::DEFAULT.critical,
);

pub struct Diskwatch {
    pressure: DiskPressure,
}

impl MorphApp for Diskwatch {
    const APP_ID: &'static str = "dev.highbay.libmorph.example.diskwatch";
    const DISK_WATCH: Option<DiskWatch> = Some(DiskWatch {
        thresholds: DiskThresholds {
            critical: CRITICAL,
            ..DiskThresholds::DEFAULT
        },
        interval: Duration::from_secs(15),
        ..DiskWatch::DEFAULT
    });

    fn init(ctx: &mut MorphContext) -> Self {
        log::info!(
            "diskwatch: init, data_dir {:?}, critical under {CRITICAL} bytes",
            ctx.data_dir()
        );
        Self {
            pressure: ctx.disk_pressure(),
        }
    }

    fn on_disk_pressure(&mut self, _ctx: &mut MorphContext, report: &DiskPressureReport) {
        self.pressure = report.pressure;
        match report.space {
            Some(space) => log::info!(
                "diskwatch: on_disk_pressure {:?}: {} of {} bytes free ({} of {} KiB)",
                report.pressure,
                space.free,
                space.total,
                space.free / 1024,
                space.total / 1024,
            ),
            None => log::info!(
                "diskwatch: on_disk_pressure {:?}: no current reading",
                report.pressure
            ),
        }
    }

    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        if touch.phase != TouchPhase::Ended {
            return;
        }
        match ctx.check_disk_now() {
            Ok(report) => log::info!("diskwatch: checked now: {report:?}"),
            Err(error) => log::warn!("diskwatch: check failed: {error}"),
        }
        if let Some(dir) = ctx.data_dir().map(ToOwned::to_owned) {
            match ctx.disk_space(&dir) {
                Ok(space) => log::info!("diskwatch: disk_space {space:?}"),
                Err(error) => log::warn!("diskwatch: disk_space failed: {error}"),
            }
        }
    }

    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) -> RenderStatus {
        let Some(gpu) = ctx.gpu() else {
            return RenderStatus::Skipped;
        };
        let color = match self.pressure {
            DiskPressure::Unknown => libmorph::wgpu::Color {
                r: 0.2,
                g: 0.2,
                b: 0.2,
                a: 1.0,
            },
            DiskPressure::None => libmorph::wgpu::Color {
                r: 0.05,
                g: 0.35,
                b: 0.12,
                a: 1.0,
            },
            DiskPressure::Low => libmorph::wgpu::Color {
                r: 0.6,
                g: 0.4,
                b: 0.05,
                a: 1.0,
            },
            _ => libmorph::wgpu::Color {
                r: 0.6,
                g: 0.05,
                b: 0.05,
                a: 1.0,
            },
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&libmorph::wgpu::CommandEncoderDescriptor::default());
        encoder.begin_render_pass(&libmorph::wgpu::RenderPassDescriptor {
            label: Some("diskwatch"),
            color_attachments: &[Some(libmorph::wgpu::RenderPassColorAttachment {
                view: frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: libmorph::wgpu::Operations {
                    load: libmorph::wgpu::LoadOp::Clear(color),
                    store: libmorph::wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        gpu.queue.submit([encoder.finish()]);
        RenderStatus::Drawn
    }
}

#[cfg(not(target_os = "android"))]
fn main() -> Result<(), libmorph::MorphError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    libmorph::run::<Diskwatch>()
}
