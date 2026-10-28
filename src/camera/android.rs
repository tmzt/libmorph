//! Android (`mobile-camera`): the calls forwarded to libmobile's
//! `android::camera`, its types converted to libmorph's, and its ids kept
//! beside libmorph's own so events can be told apart.

use libmobile::android::AndroidApp;
use libmobile::android::camera as mobile;
use libmobile::camera as types;

use super::{CameraError, CameraEvent, Facing, PreviewFrame, PreviewOptions, ScanId, ZoomSource};

/// libmorph's ids and libmobile's for the sessions and captures not yet
/// closed or answered.
#[derive(Debug, Default)]
pub(crate) struct Ids {
    sessions: Vec<(ScanId, types::SessionId)>,
    #[cfg(feature = "mobile-camera-capture")]
    captures: Vec<(super::CaptureId, types::CaptureId)>,
}

impl Ids {
    fn session(&self, scan: ScanId) -> Option<types::SessionId> {
        self.sessions
            .iter()
            .find_map(|(ours, theirs)| (*ours == scan).then_some(*theirs))
    }

    fn scan(&self, session: types::SessionId) -> Option<ScanId> {
        self.sessions
            .iter()
            .find_map(|(ours, theirs)| (*theirs == session).then_some(*ours))
    }
}

impl From<types::Facing> for Facing {
    fn from(facing: types::Facing) -> Self {
        match facing {
            types::Facing::Back => Self::Back,
            types::Facing::Front => Self::Front,
        }
    }
}

impl From<Facing> for types::Facing {
    fn from(facing: Facing) -> Self {
        match facing {
            Facing::Back => Self::Back,
            Facing::Front => Self::Front,
        }
    }
}

impl From<types::ZoomSource> for ZoomSource {
    fn from(source: types::ZoomSource) -> Self {
        match source {
            types::ZoomSource::App => Self::App,
            types::ZoomSource::Auto => Self::Auto,
        }
    }
}

impl From<types::CameraError> for CameraError {
    fn from(error: types::CameraError) -> Self {
        use types::CameraError as E;
        match error {
            E::PermissionDenied { settings_only } => Self::PermissionDenied { settings_only },
            E::NoCamera => Self::NoCamera,
            E::InUse => Self::InUse,
            E::Disabled => Self::Disabled,
            E::Timeout => Self::Timeout,
            E::Cancelled => Self::Cancelled,
            E::Interrupted => Self::Interrupted,
            E::Unsupported(what) => Self::Unsupported(what),
            E::Failed(message) => Self::Failed(message),
            other => Self::Failed(other.to_string()),
        }
    }
}

fn preview_options(preview: PreviewOptions) -> types::PreviewOptions {
    let mut options = types::PreviewOptions::default();
    options.max_side = preview.max_side;
    options.upright = preview.upright;
    options
}

pub(crate) fn start_preview(
    ids: &mut Ids,
    app: &AndroidApp,
    scan: ScanId,
    facing: Facing,
    preview: PreviewOptions,
) {
    let session = mobile::start_preview(app, facing.into(), preview_options(preview));
    ids.sessions.push((scan, session));
}

pub(crate) fn stop(ids: &Ids, scan: ScanId) {
    if let Some(session) = ids.session(scan) {
        mobile::stop(session);
    }
}

pub(crate) fn preview(ids: &Ids, scan: ScanId) -> Option<PreviewFrame> {
    mobile::latest_preview(ids.session(scan)?).map(PreviewFrame::from_mobile)
}

pub(crate) fn zoom_range(ids: &Ids, scan: ScanId) -> Option<(f32, f32)> {
    mobile::zoom_range(ids.session(scan)?)
}

pub(crate) fn zoom(ids: &Ids, scan: ScanId) -> Option<f32> {
    mobile::zoom(ids.session(scan)?)
}

pub(crate) fn set_zoom(ids: &Ids, scan: ScanId, ratio: f32) {
    if let Some(session) = ids.session(scan) {
        mobile::set_zoom(session, ratio);
    }
}

pub(crate) fn zoom_by(ids: &Ids, scan: ScanId, factor: f32) {
    if let Some(session) = ids.session(scan) {
        mobile::zoom_by(session, factor);
    }
}

#[cfg(feature = "mobile-camera-capture")]
mod capture {
    use super::{Ids, mobile, types};
    use crate::camera::{CaptureId, CaptureOptions, Frame, FrameFormat, Resolution};
    use libmobile::android::AndroidApp;

    fn format(format: FrameFormat) -> types::FrameFormat {
        match format {
            FrameFormat::Gray8 => types::FrameFormat::Gray8,
            FrameFormat::Rgba8 => types::FrameFormat::Rgba8,
            FrameFormat::Jpeg => types::FrameFormat::Jpeg,
        }
    }

    fn resolution(resolution: Resolution) -> types::Resolution {
        match resolution {
            Resolution::Preview => types::Resolution::Preview,
            Resolution::Max => types::Resolution::Max,
            Resolution::AtMost { width, height } => types::Resolution::AtMost { width, height },
        }
    }

    fn options(options: &CaptureOptions) -> types::CaptureOptions {
        let mut out = types::CaptureOptions::default();
        out.facing = options.facing.into();
        out.format = format(options.format);
        out.resolution = resolution(options.resolution);
        out.upright = options.upright;
        out.timeout = options.timeout;
        out.jpeg_quality = options.jpeg_quality;
        out
    }

    pub(crate) fn frame(frame: types::Frame) -> Frame {
        Frame {
            width: frame.width,
            height: frame.height,
            format: match frame.format {
                types::FrameFormat::Gray8 => FrameFormat::Gray8,
                types::FrameFormat::Rgba8 => FrameFormat::Rgba8,
                _ => FrameFormat::Jpeg,
            },
            data: frame.data,
            quarter_turns: frame.quarter_turns,
            facing: frame.facing.into(),
            timestamp_ns: frame.timestamp_ns,
        }
    }

    pub(crate) fn capture_frame(
        ids: &mut Ids,
        app: &AndroidApp,
        capture: CaptureId,
        opts: &CaptureOptions,
    ) {
        let theirs = mobile::capture_frame(app, &options(opts));
        ids.captures.push((capture, theirs));
    }

    pub(crate) fn cancel_capture(ids: &Ids, capture: CaptureId) {
        if let Some((_, theirs)) = ids.captures.iter().find(|(ours, _)| *ours == capture) {
            mobile::cancel_capture(*theirs);
        }
    }

    /// The capture's own id, forgotten now that it is answered.
    pub(crate) fn answered(ids: &mut Ids, theirs: types::CaptureId) -> Option<CaptureId> {
        let index = ids.captures.iter().position(|(_, t)| *t == theirs)?;
        Some(ids.captures.remove(index).0)
    }
}

#[cfg(feature = "mobile-camera-capture")]
pub(crate) use capture::{cancel_capture, capture_frame};

#[cfg(feature = "mobile-camera-qr")]
mod qr {
    use super::{Ids, mobile, preview_options, types};
    use crate::camera::{QrCode, QrMode, QrOptions, ScanId, ScreenExposure};
    use libmobile::android::AndroidApp;

    fn options(options: &QrOptions) -> types::QrOptions {
        let mut out = types::QrOptions::default();
        out.facing = options.facing.into();
        out.mode = match options.mode {
            QrMode::Once => types::QrMode::Once,
            QrMode::Continuous => types::QrMode::Continuous,
        };
        out.auto_zoom = options.auto_zoom;
        out.exposure = match options.exposure {
            ScreenExposure::Auto => types::ScreenExposure::Auto,
            ScreenExposure::Off => types::ScreenExposure::Off,
            ScreenExposure::Screen => types::ScreenExposure::Screen,
        };
        out.preview = options.preview.map(preview_options);
        out
    }

    pub(crate) fn code(code: types::QrCode) -> QrCode {
        QrCode {
            bytes: code.bytes,
            text: code.text,
            corners: code.corners,
            version: code.version,
        }
    }

    pub(crate) fn start_qr(ids: &mut Ids, app: &AndroidApp, scan: ScanId, opts: &QrOptions) {
        let session = mobile::start_qr(app, &options(opts));
        ids.sessions.push((scan, session));
    }
}

#[cfg(feature = "mobile-camera-qr")]
pub(crate) use qr::start_qr;

/// libmobile's event in libmorph's terms, with libmorph's ids; `None` for a
/// session or capture libmorph did not start, or an event it does not know.
pub(crate) fn event(ids: &mut Ids, event: types::CameraEvent) -> Option<CameraEvent> {
    use types::CameraEvent as E;
    Some(match event {
        E::Opened {
            session,
            facing,
            size,
            zoom_range,
        } => CameraEvent::Opened {
            scan: ids.scan(session)?,
            facing: facing.into(),
            size,
            zoom_range,
        },
        E::Closed { session, reason } => {
            let scan = ids.scan(session)?;
            ids.sessions.retain(|(ours, _)| *ours != scan);
            CameraEvent::Closed {
                scan,
                reason: reason.map(Into::into),
            }
        }
        E::Suspended { session } => CameraEvent::Suspended {
            scan: ids.scan(session)?,
        },
        E::Zoom {
            session,
            ratio,
            source,
        } => CameraEvent::Zoom {
            scan: ids.scan(session)?,
            ratio,
            source: source.into(),
        },
        #[cfg(feature = "mobile-camera-capture")]
        E::Frame { capture, result } => CameraEvent::Frame {
            capture: capture::answered(ids, capture)?,
            result: result.map(capture::frame).map_err(Into::into),
        },
        #[cfg(feature = "mobile-camera-qr")]
        E::Qr { session, code } => CameraEvent::Qr {
            scan: ids.scan(session)?,
            code: qr::code(code),
        },
        _ => return None,
    })
}
