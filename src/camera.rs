//! **The camera** (plan `20261028_01_CAMERA_CAPTURE_AND_QR_PLAN.md` and its
//! amendment of 2026-10-28 14:46 EDT): a live preview, one still frame, and
//! QR scanning, with the same API on every host through
//! [`MorphContext::camera`](crate::MorphContext::camera). Answers arrive in
//! [`MorphApp::on_camera`](crate::MorphApp::on_camera).
//!
//! * **Android:** libmobile's in-process NDK camera (no Jetpack):
//!   `start_preview`, `stop`, `preview` and the zoom calls with
//!   `mobile-camera`; `capture_frame` with `mobile-camera-capture`;
//!   `start_qr` with `mobile-camera-qr`.
//! * **Everywhere else, and Android builds without the feature:** every call
//!   answers [`CameraError::Unsupported`] on the next frame, with the reason
//!   (no camera on this host, or the feature it needs).
//! * **Scripted runs** (desktop, `script`): the commands `camera-qr`,
//!   `camera-frame`, `camera-deny` and `camera-deny-forever` stand in for the
//!   camera (see [`script`](crate::script)).
//!
//! CAMERA is asked for by libmobile on the first camera call, never at
//! launch; its answer does not reach `on_permissions`. A refusal fails the
//! call with [`CameraError::PermissionDenied`]`{ settings_only }`; when
//! `settings_only` is set the app may send the user to
//! [`MorphContext::open_app_settings`](crate::MorphContext::open_app_settings).
//!
//! Preview frames are drawn with the [`Viewfinder`]: their YUV planes go to
//! the GPU and are converted there. Pinch zoom stays with the app: track
//! two [`Touch`](crate::Touch)es and call [`Camera::zoom_by`] with the ratio
//! of their distance now to their distance at the last call.

#[cfg(all(target_os = "android", feature = "mobile-camera"))]
mod android;
mod convert;
mod scripted;
mod types;
mod viewfinder;

pub use convert::Fit;
pub use types::{
    CAPTURE_TIMEOUT, CameraError, CameraEvent, CaptureId, CaptureOptions, Facing, Frame,
    FrameFormat, PreviewFrame, PreviewOptions, QrCode, QrMode, QrOptions, Resolution, ScanId,
    ScreenExposure, ZoomSource,
};
pub use viewfinder::{Viewfinder, ViewfinderOptions};

#[cfg(script_runner)]
pub(crate) use scripted::ScriptFrame;

use crate::MorphContext;

/// Why a call answers `Unsupported` in this build, on this host.
#[cfg(not(target_os = "android"))]
const NO_CAMERA: &str = "no camera on this host";
#[cfg(all(target_os = "android", not(feature = "mobile-camera")))]
const NO_CAMERA: &str = "built without the mobile-camera feature";
#[cfg(not(all(target_os = "android", feature = "mobile-camera-capture")))]
const NO_CAPTURE: &str = if cfg!(target_os = "android") {
    "built without the mobile-camera-capture feature"
} else {
    "no camera on this host"
};
#[cfg(not(all(target_os = "android", feature = "mobile-camera-qr")))]
const NO_QR: &str = if cfg!(target_os = "android") {
    "built without the mobile-camera-qr feature"
} else {
    "no camera on this host"
};
/// Before the runner has the activity (a call from `MorphApp::limits`).
#[cfg(all(target_os = "android", feature = "mobile-camera"))]
const NOT_RUNNING: &str = "the app is not running yet";

/// The camera's state in the context: ids, the answers waiting for the next
/// frame, libmobile's ids on Android and the stand-in camera of scripted
/// runs.
#[derive(Debug, Default)]
pub(crate) struct CameraState {
    next_id: u32,
    /// Answered now, delivered to `on_camera` on the next frame.
    events: Vec<CameraEvent>,
    #[cfg(all(target_os = "android", feature = "mobile-camera"))]
    pub(crate) mobile: android::Ids,
    /// Set for scripted runs.
    pub(crate) script: Option<scripted::Scripted>,
}

/// libmobile's camera event for the app, in libmorph's terms; `None` for
/// one libmorph did not start (logged).
#[cfg(all(target_os = "android", feature = "mobile-camera"))]
pub(crate) fn android_event(
    state: &mut CameraState,
    event: libmobile::camera::CameraEvent,
) -> Option<CameraEvent> {
    let converted = android::event(&mut state.mobile, event);
    if converted.is_none() {
        log::debug!("libmorph: a camera event for no session or capture of the app");
    }
    converted
}

impl CameraState {
    fn next(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub(crate) fn push(&mut self, event: CameraEvent) {
        self.events.push(event);
    }

    /// The answers waiting, oldest first.
    pub(crate) fn take_events(&mut self) -> Vec<CameraEvent> {
        std::mem::take(&mut self.events)
    }

    /// A scripted run: the stand-in camera answers.
    #[cfg(script_runner)]
    pub(crate) fn scripted(&mut self) {
        self.script = Some(scripted::Scripted::default());
    }

    /// `camera-qr`: whether a QR scan read it.
    #[cfg(script_runner)]
    pub(crate) fn script_qr(&mut self, text: &str) -> bool {
        let Some(events) = self.script.as_mut().and_then(|s| s.qr(text)) else {
            return false;
        };
        self.events.extend(events);
        true
    }

    /// `camera-frame`.
    #[cfg(script_runner)]
    pub(crate) fn script_frame(&mut self, frame: ScriptFrame) {
        if let Some(script) = &mut self.script {
            let events = script.frame(frame);
            self.events.extend(events);
        }
    }

    /// `camera-deny` and `camera-deny-forever`.
    #[cfg(script_runner)]
    pub(crate) fn script_deny(&mut self, settings_only: bool) {
        if let Some(script) = &mut self.script {
            script.deny(settings_only);
        }
    }

    /// The app is being suspended (scripted runs; libmobile does its own).
    pub(crate) fn suspend(&mut self) {
        if let Some(script) = &mut self.script {
            self.events.extend(script.suspend());
        }
    }

    /// The app resumed (scripted runs).
    pub(crate) fn resume(&mut self) {
        if let Some(script) = &mut self.script {
            self.events.extend(script.resume());
        }
    }
}

/// The camera, from [`MorphContext::camera`]. Calls never block; answers
/// come to [`MorphApp::on_camera`](crate::MorphApp::on_camera).
pub struct Camera<'a> {
    state: &'a mut CameraState,
    /// For libmobile's calls; `None` before the activity is known.
    #[cfg(all(target_os = "android", feature = "mobile"))]
    #[cfg_attr(not(feature = "mobile-camera"), allow(dead_code))]
    app: Option<&'a libmobile::android::AndroidApp>,
}

impl<'a> Camera<'a> {
    pub(crate) fn new(ctx: &'a mut MorphContext) -> Self {
        Self {
            state: &mut ctx.camera,
            #[cfg(all(target_os = "android", feature = "mobile"))]
            app: ctx.android.as_ref(),
        }
    }

    /// Take one still frame. The answer is [`CameraEvent::Frame`] with this
    /// id. Android with `mobile-camera-capture`: asks for CAMERA first if
    /// needed, and is served by a running session on the same facing
    /// without closing it.
    pub fn capture_frame(&mut self, options: &CaptureOptions) -> CaptureId {
        let capture = CaptureId(self.state.next());
        if let Some(script) = &mut self.state.script {
            let events = script.capture(capture, options);
            self.state.events.extend(events);
            return capture;
        }
        #[cfg(all(target_os = "android", feature = "mobile-camera-capture"))]
        if let Some(app) = self.app {
            android::capture_frame(&mut self.state.mobile, app, capture, options);
            return capture;
        }
        #[cfg(not(all(target_os = "android", feature = "mobile-camera-capture")))]
        let reason = NO_CAPTURE;
        #[cfg(all(target_os = "android", feature = "mobile-camera-capture"))]
        let reason = NOT_RUNNING;
        self.state.push(CameraEvent::Frame {
            capture,
            result: Err(CameraError::Unsupported(reason)),
        });
        capture
    }

    /// End a capture early with [`CameraError::Cancelled`] (no-op once
    /// answered).
    pub fn cancel_capture(&mut self, capture: CaptureId) {
        if let Some(script) = &mut self.state.script {
            let events = script.cancel(capture);
            self.state.events.extend(events);
            return;
        }
        #[cfg(all(target_os = "android", feature = "mobile-camera-capture"))]
        android::cancel_capture(&self.state.mobile, capture);
        #[cfg(not(all(target_os = "android", feature = "mobile-camera-capture")))]
        let _ = capture;
    }

    /// Start scanning for QR codes, with auto zoom and exposure for codes on
    /// screens. [`CameraEvent::Opened`], then [`CameraEvent::Qr`] for each
    /// code. A session already running is stopped first. Android with
    /// `mobile-camera-qr`.
    pub fn start_qr(&mut self, options: &QrOptions) -> ScanId {
        let scan = ScanId(self.state.next());
        if let Some(script) = &mut self.state.script {
            let events = script.start(scan, options.facing, Some(options.mode));
            self.state.events.extend(events);
            return scan;
        }
        #[cfg(all(target_os = "android", feature = "mobile-camera-qr"))]
        if let Some(app) = self.app {
            android::start_qr(&mut self.state.mobile, app, scan, options);
            return scan;
        }
        #[cfg(not(all(target_os = "android", feature = "mobile-camera-qr")))]
        let reason = NO_QR;
        #[cfg(all(target_os = "android", feature = "mobile-camera-qr"))]
        let reason = NOT_RUNNING;
        let _ = options;
        self.state.push(CameraEvent::Closed {
            scan,
            reason: Some(CameraError::Unsupported(reason)),
        });
        scan
    }

    /// Open the camera facing `facing` for a live preview (a viewfinder).
    /// [`CameraEvent::Opened`] follows once it streams; poll its frames with
    /// [`preview`](Self::preview). A session already running is stopped
    /// first (`Closed { reason: None }`). Android with `mobile-camera`.
    pub fn start_preview(&mut self, facing: Facing, preview: PreviewOptions) -> ScanId {
        let scan = ScanId(self.state.next());
        if let Some(script) = &mut self.state.script {
            let events = script.start(scan, facing, None);
            self.state.events.extend(events);
            return scan;
        }
        #[cfg(all(target_os = "android", feature = "mobile-camera"))]
        if let Some(app) = self.app {
            android::start_preview(&mut self.state.mobile, app, scan, facing, preview);
            return scan;
        }
        #[cfg(not(all(target_os = "android", feature = "mobile-camera")))]
        let reason = NO_CAMERA;
        #[cfg(all(target_os = "android", feature = "mobile-camera"))]
        let reason = NOT_RUNNING;
        let _ = (facing, preview);
        self.state.push(CameraEvent::Closed {
            scan,
            reason: Some(CameraError::Unsupported(reason)),
        });
        scan
    }

    /// Stop a session. `Closed { reason: None }` follows, unless it had
    /// already closed.
    pub fn stop(&mut self, scan: ScanId) {
        if let Some(script) = &mut self.state.script {
            let events = script.stop(scan);
            self.state.events.extend(events);
            return;
        }
        #[cfg(all(target_os = "android", feature = "mobile-camera"))]
        android::stop(&self.state.mobile, scan);
        #[cfg(not(all(target_os = "android", feature = "mobile-camera")))]
        let _ = scan;
    }

    /// The session's newest preview frame, or `None` before the first (or
    /// once it has stopped, or without a camera). The same frame is returned
    /// until a newer one arrives; [`PreviewFrame::sequence`] tells them
    /// apart. Draw it with the [`Viewfinder`].
    #[must_use]
    pub fn preview(&self, scan: ScanId) -> Option<PreviewFrame> {
        if self.state.script.is_some() {
            return None;
        }
        #[cfg(all(target_os = "android", feature = "mobile-camera"))]
        return android::preview(&self.state.mobile, scan);
        #[cfg(not(all(target_os = "android", feature = "mobile-camera")))]
        {
            let _ = scan;
            None
        }
    }

    /// The session's zoom ratio range, `(1.0, max)`, once `Opened`.
    #[must_use]
    pub fn zoom_range(&self, scan: ScanId) -> Option<(f32, f32)> {
        if let Some(script) = &self.state.script {
            return script.zoom_range(scan);
        }
        #[cfg(all(target_os = "android", feature = "mobile-camera"))]
        return android::zoom_range(&self.state.mobile, scan);
        #[cfg(not(all(target_os = "android", feature = "mobile-camera")))]
        {
            let _ = scan;
            None
        }
    }

    /// The session's current zoom ratio, once `Opened`.
    #[must_use]
    pub fn zoom(&self, scan: ScanId) -> Option<f32> {
        if let Some(script) = &self.state.script {
            return script.zoom(scan);
        }
        #[cfg(all(target_os = "android", feature = "mobile-camera"))]
        return android::zoom(&self.state.mobile, scan);
        #[cfg(not(all(target_os = "android", feature = "mobile-camera")))]
        {
            let _ = scan;
            None
        }
    }

    /// Set the zoom ratio, clamped to [`zoom_range`](Self::zoom_range).
    /// `Zoom { source: App }` follows when it changes.
    pub fn set_zoom(&mut self, scan: ScanId, ratio: f32) {
        if let Some(script) = &mut self.state.script {
            let events = script.set_zoom(scan, |_| ratio);
            self.state.events.extend(events);
            return;
        }
        #[cfg(all(target_os = "android", feature = "mobile-camera"))]
        android::set_zoom(&self.state.mobile, scan, ratio);
        #[cfg(not(all(target_os = "android", feature = "mobile-camera")))]
        let _ = (scan, ratio);
    }

    /// Multiply the zoom ratio by `factor`: for a pinch, the ratio of the
    /// two touches' distance now to their distance at the last call.
    pub fn zoom_by(&mut self, scan: ScanId, factor: f32) {
        if let Some(script) = &mut self.state.script {
            let events = script.set_zoom(scan, |zoom| zoom * factor);
            self.state.events.extend(events);
            return;
        }
        #[cfg(all(target_os = "android", feature = "mobile-camera"))]
        android::zoom_by(&self.state.mobile, scan, factor);
        #[cfg(not(all(target_os = "android", feature = "mobile-camera")))]
        let _ = (scan, factor);
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;

    /// Off Android every call that answers does so with `Unsupported` and
    /// its id, once, on the next frame; the others answer nothing.
    #[test]
    fn every_call_answers_unsupported_once() {
        let mut state = CameraState::default();
        let mut camera = Camera { state: &mut state };
        let capture = camera.capture_frame(&CaptureOptions::default());
        let qr = camera.start_qr(&QrOptions::default());
        let preview = camera.start_preview(Facing::Front, PreviewOptions::default());
        camera.cancel_capture(capture);
        camera.stop(preview);
        camera.set_zoom(qr, 2.0);
        camera.zoom_by(qr, 2.0);
        assert_eq!(camera.preview(preview), None);
        assert_eq!(camera.zoom(qr), None);
        assert_eq!(camera.zoom_range(qr), None);
        assert_ne!(qr, preview);
        let unsupported = || Some(CameraError::Unsupported(NO_CAMERA));
        assert_eq!(
            state.take_events(),
            [
                CameraEvent::Frame {
                    capture,
                    result: Err(CameraError::Unsupported(NO_CAPTURE)),
                },
                CameraEvent::Closed {
                    scan: qr,
                    reason: unsupported(),
                },
                CameraEvent::Closed {
                    scan: preview,
                    reason: unsupported(),
                },
            ]
        );
        assert!(state.take_events().is_empty(), "delivered once");
        assert_eq!(NO_CAPTURE, NO_CAMERA);
        assert_eq!(NO_QR, NO_CAMERA);
    }
}
