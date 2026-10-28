//! The camera's types: libmorph's own, mirroring libmobile's `camera`
//! module field for field (libmobile's `SessionId` is [`ScanId`] here), so
//! signatures are the same with or without `mobile`. The conversions from
//! and to libmobile's are in `camera/android.rs`.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// Default [`CaptureOptions::timeout`].
pub const CAPTURE_TIMEOUT: Duration = Duration::from_millis(5000);

/// Which way the camera faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Facing {
    /// The camera on the back of the phone.
    #[default]
    Back,
    /// The selfie camera.
    Front,
}

/// A running camera session: a preview ([`start_preview`]) or a QR scan
/// ([`start_qr`]). Opaque; ids count from 1 per run.
///
/// [`start_preview`]: super::Camera::start_preview
/// [`start_qr`]: super::Camera::start_qr
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScanId(pub(crate) u32);

impl ScanId {
    /// The id as a number, for logs.
    #[must_use]
    pub fn id(self) -> u32 {
        self.0
    }
}

/// One [`capture_frame`](super::Camera::capture_frame) call. Opaque; ids
/// count from 1 per run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CaptureId(pub(crate) u32);

impl CaptureId {
    /// The id as a number, for logs.
    #[must_use]
    pub fn id(self) -> u32 {
        self.0
    }
}

/// Why a session closed or a capture failed.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum CameraError {
    /// The user refused CAMERA. `settings_only`: asking again shows no
    /// dialog (the user chose "Don't allow" twice, or the device policy
    /// forbids it); the app may offer
    /// [`MorphContext::open_app_settings`](crate::MorphContext::open_app_settings).
    PermissionDenied { settings_only: bool },
    /// No camera with that facing.
    NoCamera,
    /// Another app holds the camera, or a capture asked for the other facing
    /// while a session runs.
    InUse,
    /// Disabled by device policy.
    Disabled,
    /// The capture's timeout passed.
    Timeout,
    /// `cancel_capture` or `stop`.
    Cancelled,
    /// The app was suspended before the capture finished.
    Interrupted,
    /// Not available: no camera on this host or in this build (the reason
    /// names which), or the device cannot do what was asked (format, size).
    Unsupported(&'static str),
    /// Anything else, for logs.
    Failed(String),
}

impl fmt::Display for CameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PermissionDenied {
                settings_only: true,
            } => f.write_str("camera permission refused; only Settings can grant it now"),
            Self::PermissionDenied {
                settings_only: false,
            } => f.write_str("camera permission refused"),
            Self::NoCamera => f.write_str("no camera with that facing"),
            Self::InUse => f.write_str("the camera is in use"),
            Self::Disabled => f.write_str("the camera is disabled by policy"),
            Self::Timeout => f.write_str("the capture timed out"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Interrupted => f.write_str("interrupted by the app suspending"),
            Self::Unsupported(what) => write!(f, "camera not supported: {what}"),
            Self::Failed(message) => write!(f, "camera failed: {message}"),
        }
    }
}

impl std::error::Error for CameraError {}

/// What changed the zoom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomSource {
    /// `set_zoom` or `zoom_by`.
    App,
    /// Automatic zoom (QR scanning).
    Auto,
}

/// What the camera reports, to
/// [`MorphApp::on_camera`](crate::MorphApp::on_camera).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum CameraEvent {
    /// A session's camera is open and streaming (again, after a resume).
    /// `size` is the size of its preview frames; `zoom_range` as
    /// [`zoom_range`](super::Camera::zoom_range).
    Opened {
        scan: ScanId,
        facing: Facing,
        size: (u32, u32),
        zoom_range: (f32, f32),
    },
    /// The session's camera closed: `reason` is `None` for `stop` (or a
    /// newer session replacing it, or a `Once` scan that read its code), and
    /// the error otherwise, such as [`CameraError::PermissionDenied`] or
    /// [`CameraError::Unsupported`].
    Closed {
        scan: ScanId,
        reason: Option<CameraError>,
    },
    /// The app was suspended: the session's camera is closed and reopens on
    /// resume (no event until `Opened`).
    Suspended { scan: ScanId },
    /// The zoom changed.
    Zoom {
        scan: ScanId,
        ratio: f32,
        source: ZoomSource,
    },
    /// A capture finished.
    Frame {
        capture: CaptureId,
        result: Result<Frame, CameraError>,
    },
    /// A QR code was read.
    Qr { scan: ScanId, code: QrCode },
}

/// How a session's preview frames are made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PreviewOptions {
    /// Longest side of the frames [`preview`](super::Camera::preview)
    /// returns. Default 640.
    pub max_side: u32,
    /// Turn the planes upright on the CPU (`quarter_turns` is then 0).
    /// `false` leaves them as the sensor delivers them and reports the turns
    /// still needed, which the [`Viewfinder`](super::Viewfinder) applies in
    /// its shader. Default `true`.
    pub upright: bool,
}

impl Default for PreviewOptions {
    fn default() -> Self {
        Self {
            max_side: 640,
            upright: true,
        }
    }
}

/// The pixels a capture returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameFormat {
    /// The luma plane, one byte per pixel, rows packed.
    Gray8,
    /// RGBA, 4 bytes per pixel, rows packed (BT.601 full range from YUV).
    Rgba8,
    /// The camera's hardware JPEG.
    Jpeg,
}

/// How large a capture is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Resolution {
    /// The largest supported size within 1280 × 720 (fast, small).
    Preview,
    /// The largest size the device offers for the format.
    Max,
    /// The largest supported size within this box (either orientation).
    AtMost { width: u32, height: u32 },
}

/// What [`capture_frame`](super::Camera::capture_frame) takes.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct CaptureOptions {
    /// Default `Back`.
    pub facing: Facing,
    /// Default `Rgba8`.
    pub format: FrameFormat,
    /// Default `Preview`.
    pub resolution: Resolution,
    /// Rotate to the display's orientation. Default `true`.
    pub upright: bool,
    /// From the camera opening to the frame. Default 5 s.
    pub timeout: Duration,
    /// 1..=100, Jpeg only. Default 90.
    pub jpeg_quality: u8,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            facing: Facing::Back,
            format: FrameFormat::Rgba8,
            resolution: Resolution::Preview,
            upright: true,
            timeout: CAPTURE_TIMEOUT,
            jpeg_quality: 90,
        }
    }
}

/// One captured frame.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub format: FrameFormat,
    pub data: Vec<u8>,
    /// Clockwise quarter turns still needed to show it upright: 0 when
    /// `upright` was honoured.
    pub quarter_turns: u8,
    pub facing: Facing,
    /// Sensor timestamp, ns (0 in scripted runs).
    pub timestamp_ns: i64,
}

/// When a QR scan stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QrMode {
    /// Stop after the first code (`Closed { reason: None }` follows `Qr`).
    #[default]
    Once,
    /// Keep scanning; the same payload is reported again only after a pause
    /// without it.
    Continuous,
}

/// Exposure for codes shown on screens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenExposure {
    /// Lower the exposure when a code looks like it is on a bright screen.
    #[default]
    Auto,
    /// Never adjust.
    Off,
    /// Start low, metering the centre: for apps that know the code is on a
    /// screen (a TV pairing code).
    Screen,
}

/// What [`start_qr`](super::Camera::start_qr) takes.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct QrOptions {
    /// Default `Back`.
    pub facing: Facing,
    /// Default `Once`.
    pub mode: QrMode,
    /// Zoom in on a code too small to read. Default `true`.
    pub auto_zoom: bool,
    /// Default `Auto`.
    pub exposure: ScreenExposure,
    /// Preview frames for a viewfinder; `None` for none. Default
    /// `Some(PreviewOptions::default())`.
    pub preview: Option<PreviewOptions>,
}

impl Default for QrOptions {
    fn default() -> Self {
        Self {
            facing: Facing::Back,
            mode: QrMode::Once,
            auto_zoom: true,
            exposure: ScreenExposure::Auto,
            preview: Some(PreviewOptions::default()),
        }
    }
}

/// One QR code read. The payload may be a secret (a pairing code): do not
/// log it past its length.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct QrCode {
    /// The payload bytes as decoded.
    pub bytes: Vec<u8>,
    /// `bytes` as UTF-8, when they are.
    pub text: Option<String>,
    /// Corners (top-left, top-right, bottom-right, bottom-left of the code)
    /// in the upright preview frame, as fractions 0.0..=1.0.
    pub corners: [(f32, f32); 4],
    /// QR version 1..=40.
    pub version: u8,
}

/// One preview frame: YUV 4:2:0 as three packed planes (I420), BT.601 full
/// range, as the [`Viewfinder`](super::Viewfinder) uploads them. Cheap to
/// clone: the planes are shared (on Android, with libmobile's own frame,
/// without a copy).
#[derive(Clone)]
pub struct PreviewFrame {
    planes: Planes,
}

#[derive(Clone)]
enum Planes {
    Owned(Arc<Owned>),
    /// libmobile's frame, shared as it is.
    #[cfg(all(target_os = "android", feature = "mobile-camera"))]
    Mobile(Arc<libmobile::camera::PreviewFrame>),
}

#[derive(Clone, Debug, PartialEq)]
struct Owned {
    width: u32,
    height: u32,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    quarter_turns: u8,
    facing: Facing,
    timestamp_ns: i64,
    sequence: u64,
}

impl PreviewFrame {
    /// A frame from packed I420 planes: `y` is `width × height` bytes, `u`
    /// and `v` `ceil(width / 2) × ceil(height / 2)` each. `None` when a
    /// plane has the wrong length or a side is 0. Upright, `Back`, sequence
    /// 1; change those with the `with_` calls. For tests and for frames
    /// from elsewhere.
    #[must_use]
    pub fn from_i420(width: u32, height: u32, y: Vec<u8>, u: Vec<u8>, v: Vec<u8>) -> Option<Self> {
        let luma = width as usize * height as usize;
        let chroma = width.div_ceil(2) as usize * height.div_ceil(2) as usize;
        if luma == 0 || y.len() != luma || u.len() != chroma || v.len() != chroma {
            return None;
        }
        Some(Self {
            planes: Planes::Owned(Arc::new(Owned {
                width,
                height,
                y,
                u,
                v,
                quarter_turns: 0,
                facing: Facing::Back,
                timestamp_ns: 0,
                sequence: 1,
            })),
        })
    }

    /// A frame from RGBA rows (4 bytes per pixel, packed), converted to I420
    /// with BT.601 full range, each chroma sample the average of its 2 × 2
    /// block. `None` when `rgba` is not `width × height × 4` bytes.
    #[must_use]
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<Self> {
        if rgba.len() != width as usize * height as usize * 4 {
            return None;
        }
        let (y, u, v) = super::convert::rgba_to_i420(width as usize, height as usize, rgba);
        Self::from_i420(width, height, y, u, v)
    }

    #[cfg(all(target_os = "android", feature = "mobile-camera"))]
    pub(crate) fn from_mobile(frame: Arc<libmobile::camera::PreviewFrame>) -> Self {
        Self {
            planes: Planes::Mobile(frame),
        }
    }

    fn owned(self) -> Owned {
        match self.planes {
            Planes::Owned(owned) => Arc::unwrap_or_clone(owned),
            #[cfg(all(target_os = "android", feature = "mobile-camera"))]
            Planes::Mobile(_) => Owned {
                width: self.width(),
                height: self.height(),
                y: self.y().to_vec(),
                u: self.u().to_vec(),
                v: self.v().to_vec(),
                quarter_turns: self.quarter_turns(),
                facing: self.facing(),
                timestamp_ns: self.timestamp_ns(),
                sequence: self.sequence(),
            },
        }
    }

    fn with(self, change: impl FnOnce(&mut Owned)) -> Self {
        let mut owned = self.owned();
        change(&mut owned);
        Self {
            planes: Planes::Owned(Arc::new(owned)),
        }
    }

    /// The same frame, needing `turns` clockwise quarter turns (taken mod 4).
    #[must_use]
    pub fn with_quarter_turns(self, turns: u8) -> Self {
        self.with(|o| o.quarter_turns = turns % 4)
    }

    /// The same frame with another sequence number.
    #[must_use]
    pub fn with_sequence(self, sequence: u64) -> Self {
        self.with(|o| o.sequence = sequence)
    }

    /// The same frame from the other camera.
    #[must_use]
    pub fn with_facing(self, facing: Facing) -> Self {
        self.with(|o| o.facing = facing)
    }

    /// The same frame with another timestamp.
    #[must_use]
    pub fn with_timestamp_ns(self, timestamp_ns: i64) -> Self {
        self.with(|o| o.timestamp_ns = timestamp_ns)
    }

    /// Luma width, in pixels of the frame as stored (before `quarter_turns`).
    #[must_use]
    pub fn width(&self) -> u32 {
        match &self.planes {
            Planes::Owned(o) => o.width,
            #[cfg(all(target_os = "android", feature = "mobile-camera"))]
            Planes::Mobile(m) => m.width,
        }
    }

    /// Luma height, as stored.
    #[must_use]
    pub fn height(&self) -> u32 {
        match &self.planes {
            Planes::Owned(o) => o.height,
            #[cfg(all(target_os = "android", feature = "mobile-camera"))]
            Planes::Mobile(m) => m.height,
        }
    }

    /// The luma plane, `width × height` bytes: a greyscale image.
    #[must_use]
    pub fn y(&self) -> &[u8] {
        match &self.planes {
            Planes::Owned(o) => &o.y,
            #[cfg(all(target_os = "android", feature = "mobile-camera"))]
            Planes::Mobile(m) => &m.y,
        }
    }

    /// The U (Cb) plane, [`chroma_size`](Self::chroma_size) bytes.
    #[must_use]
    pub fn u(&self) -> &[u8] {
        match &self.planes {
            Planes::Owned(o) => &o.u,
            #[cfg(all(target_os = "android", feature = "mobile-camera"))]
            Planes::Mobile(m) => &m.u,
        }
    }

    /// The V (Cr) plane, [`chroma_size`](Self::chroma_size) bytes.
    #[must_use]
    pub fn v(&self) -> &[u8] {
        match &self.planes {
            Planes::Owned(o) => &o.v,
            #[cfg(all(target_os = "android", feature = "mobile-camera"))]
            Planes::Mobile(m) => &m.v,
        }
    }

    /// Clockwise quarter turns still needed to show it upright (0..=3).
    #[must_use]
    pub fn quarter_turns(&self) -> u8 {
        match &self.planes {
            Planes::Owned(o) => o.quarter_turns,
            #[cfg(all(target_os = "android", feature = "mobile-camera"))]
            Planes::Mobile(m) => m.quarter_turns % 4,
        }
    }

    #[must_use]
    pub fn facing(&self) -> Facing {
        match &self.planes {
            Planes::Owned(o) => o.facing,
            #[cfg(all(target_os = "android", feature = "mobile-camera"))]
            Planes::Mobile(m) => m.facing.into(),
        }
    }

    /// Sensor timestamp, ns.
    #[must_use]
    pub fn timestamp_ns(&self) -> i64 {
        match &self.planes {
            Planes::Owned(o) => o.timestamp_ns,
            #[cfg(all(target_os = "android", feature = "mobile-camera"))]
            Planes::Mobile(m) => m.timestamp_ns,
        }
    }

    /// Counts up per frame of a session, from 1: the viewfinder skips the
    /// upload when it has not changed.
    #[must_use]
    pub fn sequence(&self) -> u64 {
        match &self.planes {
            Planes::Owned(o) => o.sequence,
            #[cfg(all(target_os = "android", feature = "mobile-camera"))]
            Planes::Mobile(m) => m.sequence,
        }
    }

    /// The size of the `u` and `v` planes: half the luma's, rounded up.
    #[must_use]
    pub fn chroma_size(&self) -> (u32, u32) {
        (self.width().div_ceil(2), self.height().div_ceil(2))
    }

    /// The size shown once turned upright.
    #[must_use]
    pub fn upright_size(&self) -> (u32, u32) {
        if self.quarter_turns() % 2 == 1 {
            (self.height(), self.width())
        } else {
            (self.width(), self.height())
        }
    }

    /// The frame as RGBA, 4 bytes per pixel, rows packed, as stored (not
    /// turned): the CPU path, the same conversion as libmobile's.
    #[must_use]
    pub fn to_rgba(&self) -> Vec<u8> {
        super::convert::i420_to_rgba(
            self.width() as usize,
            self.height() as usize,
            self.y(),
            self.u(),
            self.v(),
        )
    }
}

impl PartialEq for PreviewFrame {
    fn eq(&self, other: &Self) -> bool {
        self.width() == other.width()
            && self.height() == other.height()
            && self.quarter_turns() == other.quarter_turns()
            && self.facing() == other.facing()
            && self.timestamp_ns() == other.timestamp_ns()
            && self.sequence() == other.sequence()
            && self.y() == other.y()
            && self.u() == other.u()
            && self.v() == other.v()
    }
}

impl fmt::Debug for PreviewFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewFrame")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("quarter_turns", &self.quarter_turns())
            .field("facing", &self.facing())
            .field("timestamp_ns", &self.timestamp_ns())
            .field("sequence", &self.sequence())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_libmobiles() {
        let p = PreviewOptions::default();
        assert_eq!((p.max_side, p.upright), (640, true));
        let c = CaptureOptions::default();
        assert_eq!(c.facing, Facing::Back);
        assert_eq!(c.format, FrameFormat::Rgba8);
        assert_eq!(c.resolution, Resolution::Preview);
        assert!(c.upright);
        assert_eq!(c.timeout, Duration::from_secs(5));
        assert_eq!(c.jpeg_quality, 90);
        let q = QrOptions::default();
        assert_eq!(q.mode, QrMode::Once);
        assert!(q.auto_zoom);
        assert_eq!(q.exposure, ScreenExposure::Auto);
        assert_eq!(q.preview, Some(PreviewOptions::default()));
    }

    #[test]
    fn i420_planes_are_checked() {
        assert!(PreviewFrame::from_i420(3, 1, vec![0; 3], vec![128; 2], vec![128; 2]).is_some());
        assert!(PreviewFrame::from_i420(3, 1, vec![0; 3], vec![128; 1], vec![128; 2]).is_none());
        assert!(PreviewFrame::from_i420(0, 0, vec![], vec![], vec![]).is_none());
        assert!(PreviewFrame::from_rgba(2, 2, &[0; 15]).is_none());
    }

    #[test]
    fn turns_swap_the_upright_size() {
        let frame = PreviewFrame::from_i420(4, 2, vec![0; 8], vec![128; 2], vec![128; 2]).unwrap();
        assert_eq!(frame.upright_size(), (4, 2));
        let frame = frame.with_quarter_turns(5).with_sequence(9);
        assert_eq!(frame.quarter_turns(), 1);
        assert_eq!(frame.upright_size(), (2, 4));
        assert_eq!(frame.sequence(), 9);
    }

    #[test]
    fn errors_name_the_reason() {
        assert_eq!(
            CameraError::Unsupported("no camera on this host").to_string(),
            "camera not supported: no camera on this host"
        );
        assert_eq!(
            CameraError::PermissionDenied {
                settings_only: true
            }
            .to_string(),
            "camera permission refused; only Settings can grant it now"
        );
    }
}
