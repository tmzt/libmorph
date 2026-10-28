//! The stand-in camera of scripted runs (plan section 4): sessions open at
//! once with a zoom range of `(1.0, 8.0)`, zoom calls are reported back,
//! `camera-qr` reads a code on the running scan, `camera-frame` answers a
//! capture, and `camera-deny` / `camera-deny-forever` refuse the next call.
//! Pure state: every call returns the events it answers with. Only the
//! script runner makes one, so other builds leave most of it unused.
#![cfg_attr(not(script_runner), allow(dead_code))]

use std::collections::VecDeque;

use super::convert::luma;
use super::types::{
    CameraError, CameraEvent, CaptureId, CaptureOptions, Facing, Frame, FrameFormat, QrCode,
    QrMode, ScanId, ZoomSource,
};

/// The zoom range a scripted session reports.
pub(crate) const ZOOM_RANGE: (f32, f32) = (1.0, 8.0);
/// The preview size a scripted session reports, upright (portrait).
pub(crate) const PREVIEW_SIZE: (u32, u32) = (480, 640);
/// Where a scripted code is: the centred square of half the frame's width.
const QR_CORNERS: [(f32, f32); 4] = [(0.25, 0.25), (0.75, 0.25), (0.75, 0.75), (0.25, 0.75)];

/// A `camera-frame` image, decoded: RGBA rows, packed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScriptFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
struct Session {
    scan: ScanId,
    facing: Facing,
    /// `Some` for a QR scan.
    qr: Option<QrMode>,
    zoom: f32,
    suspended: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Scripted {
    /// `camera-deny` (`false`) or `camera-deny-forever` (`true`): the next
    /// call is refused.
    deny: Option<bool>,
    session: Option<Session>,
    /// Captures waiting for a `camera-frame`, oldest first.
    captures: VecDeque<(CaptureId, CaptureOptions)>,
    /// A `camera-frame` given before any capture asked for it.
    frame: Option<ScriptFrame>,
}

impl Scripted {
    fn refused(&mut self) -> Option<CameraError> {
        self.deny
            .take()
            .map(|settings_only| CameraError::PermissionDenied { settings_only })
    }

    pub fn capture(&mut self, capture: CaptureId, options: &CaptureOptions) -> Vec<CameraEvent> {
        if let Some(error) = self.refused() {
            return vec![CameraEvent::Frame {
                capture,
                result: Err(error),
            }];
        }
        match self.frame.take() {
            Some(frame) => vec![answer(capture, options, &frame)],
            None => {
                self.captures.push_back((capture, options.clone()));
                Vec::new()
            }
        }
    }

    pub fn cancel(&mut self, capture: CaptureId) -> Vec<CameraEvent> {
        let Some(index) = self.captures.iter().position(|(id, _)| *id == capture) else {
            return Vec::new();
        };
        self.captures.remove(index);
        vec![CameraEvent::Frame {
            capture,
            result: Err(CameraError::Cancelled),
        }]
    }

    /// A preview (`qr: None`) or a QR scan: the old session closes first.
    pub fn start(&mut self, scan: ScanId, facing: Facing, qr: Option<QrMode>) -> Vec<CameraEvent> {
        let mut events = self.close();
        if let Some(error) = self.refused() {
            events.push(CameraEvent::Closed {
                scan,
                reason: Some(error),
            });
            return events;
        }
        self.session = Some(Session {
            scan,
            facing,
            qr,
            zoom: ZOOM_RANGE.0,
            suspended: false,
        });
        events.push(opened(scan, facing));
        events
    }

    fn close(&mut self) -> Vec<CameraEvent> {
        self.session
            .take()
            .map(|s| CameraEvent::Closed {
                scan: s.scan,
                reason: None,
            })
            .into_iter()
            .collect()
    }

    fn running(&self, scan: ScanId) -> Option<&Session> {
        self.session.as_ref().filter(|s| s.scan == scan)
    }

    pub fn stop(&mut self, scan: ScanId) -> Vec<CameraEvent> {
        if self.running(scan).is_some() {
            self.close()
        } else {
            Vec::new()
        }
    }

    pub fn zoom_range(&self, scan: ScanId) -> Option<(f32, f32)> {
        self.running(scan).map(|_| ZOOM_RANGE)
    }

    pub fn zoom(&self, scan: ScanId) -> Option<f32> {
        self.running(scan).map(|s| s.zoom)
    }

    /// `set_zoom` and `zoom_by`: the new ratio from the old, clamped; a
    /// `Zoom` event when it changed. Non-finite ratios are ignored.
    pub fn set_zoom(&mut self, scan: ScanId, ratio: impl FnOnce(f32) -> f32) -> Vec<CameraEvent> {
        let Some(session) = self.session.as_mut().filter(|s| s.scan == scan) else {
            return Vec::new();
        };
        let new = ratio(session.zoom);
        if !new.is_finite() {
            return Vec::new();
        }
        let new = new.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        if new == session.zoom {
            return Vec::new();
        }
        session.zoom = new;
        vec![CameraEvent::Zoom {
            scan,
            ratio: new,
            source: ZoomSource::App,
        }]
    }

    /// `camera-deny` / `camera-deny-forever`.
    pub fn deny(&mut self, settings_only: bool) {
        self.deny = Some(settings_only);
    }

    /// `camera-qr <text>`: the running QR scan reads it; `None` when no QR
    /// scan runs.
    pub fn qr(&mut self, text: &str) -> Option<Vec<CameraEvent>> {
        let session = self
            .session
            .as_ref()
            .filter(|s| s.qr.is_some() && !s.suspended)?;
        let scan = session.scan;
        let once = session.qr == Some(QrMode::Once);
        let mut events = vec![CameraEvent::Qr {
            scan,
            code: QrCode {
                bytes: text.as_bytes().to_vec(),
                text: Some(text.to_owned()),
                corners: QR_CORNERS,
                version: 1,
            },
        }];
        if once {
            events.extend(self.close());
        }
        Some(events)
    }

    /// `camera-frame <png>`: the oldest waiting capture, or the next one.
    pub fn frame(&mut self, frame: ScriptFrame) -> Vec<CameraEvent> {
        match self.captures.pop_front() {
            Some((capture, options)) => vec![answer(capture, &options, &frame)],
            None => {
                self.frame = Some(frame);
                Vec::new()
            }
        }
    }

    /// The app suspends: the session reports `Suspended`, waiting captures
    /// end `Interrupted`.
    pub fn suspend(&mut self) -> Vec<CameraEvent> {
        let mut events: Vec<_> = self
            .captures
            .drain(..)
            .map(|(capture, _)| CameraEvent::Frame {
                capture,
                result: Err(CameraError::Interrupted),
            })
            .collect();
        if let Some(session) = self.session.as_mut().filter(|s| !s.suspended) {
            session.suspended = true;
            events.push(CameraEvent::Suspended { scan: session.scan });
        }
        events
    }

    /// The app resumes: a suspended session opens again.
    pub fn resume(&mut self) -> Vec<CameraEvent> {
        match self.session.as_mut().filter(|s| s.suspended) {
            Some(session) => {
                session.suspended = false;
                vec![opened(session.scan, session.facing)]
            }
            None => Vec::new(),
        }
    }
}

fn opened(scan: ScanId, facing: Facing) -> CameraEvent {
    CameraEvent::Opened {
        scan,
        facing,
        size: PREVIEW_SIZE,
        zoom_range: ZOOM_RANGE,
    }
}

/// A capture answered from a `camera-frame` image, in the format asked.
fn answer(capture: CaptureId, options: &CaptureOptions, frame: &ScriptFrame) -> CameraEvent {
    let data = match options.format {
        FrameFormat::Rgba8 => frame.rgba.clone(),
        FrameFormat::Gray8 => frame
            .rgba
            .chunks_exact(4)
            .map(|p| luma(p[0], p[1], p[2]))
            .collect(),
        _ => {
            return CameraEvent::Frame {
                capture,
                result: Err(CameraError::Unsupported("jpeg in scripted runs")),
            };
        }
    };
    CameraEvent::Frame {
        capture,
        result: Ok(Frame {
            width: frame.width,
            height: frame.height,
            format: options.format,
            data,
            quarter_turns: 0,
            facing: options.facing,
            timestamp_ns: 0,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> ScriptFrame {
        ScriptFrame {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 255, 255, 255, 255],
        }
    }

    #[test]
    fn a_once_scan_reads_one_code_and_closes() {
        let mut s = Scripted::default();
        assert!(s.qr("x").is_none(), "no scan running");
        let scan = ScanId(1);
        assert_eq!(
            s.start(scan, Facing::Back, Some(QrMode::Once)),
            [opened(scan, Facing::Back)]
        );
        assert_eq!(s.zoom_range(scan), Some(ZOOM_RANGE));
        let events = s.qr("pair me").unwrap();
        assert!(matches!(&events[0], CameraEvent::Qr { scan: s, code }
            if *s == scan && code.text.as_deref() == Some("pair me") && code.version == 1));
        assert_eq!(events[1], CameraEvent::Closed { scan, reason: None });
        assert!(s.qr("again").is_none());
    }

    #[test]
    fn a_preview_reads_no_codes_and_a_new_session_replaces_it() {
        let mut s = Scripted::default();
        s.start(ScanId(1), Facing::Back, None);
        assert!(s.qr("x").is_none());
        let events = s.start(ScanId(2), Facing::Back, Some(QrMode::Continuous));
        assert_eq!(
            events[0],
            CameraEvent::Closed {
                scan: ScanId(1),
                reason: None
            }
        );
        assert_eq!(s.qr("a").unwrap().len(), 1, "continuous stays open");
        assert_eq!(s.stop(ScanId(2)).len(), 1);
        assert!(s.stop(ScanId(2)).is_empty());
    }

    #[test]
    fn zoom_is_clamped_and_reported_when_it_changes() {
        let mut s = Scripted::default();
        let scan = ScanId(1);
        s.start(scan, Facing::Back, None);
        let zoom = |ratio| CameraEvent::Zoom {
            scan,
            ratio,
            source: ZoomSource::App,
        };
        assert_eq!(s.set_zoom(scan, |z| z * 2.0), [zoom(2.0)]);
        assert_eq!(s.set_zoom(scan, |_| 100.0), [zoom(8.0)]);
        assert!(s.set_zoom(scan, |_| 9.0).is_empty(), "unchanged");
        assert!(s.set_zoom(scan, |_| f32::NAN).is_empty());
        assert!(s.set_zoom(ScanId(7), |_| 2.0).is_empty(), "not running");
        assert_eq!(s.zoom(scan), Some(8.0));
    }

    #[test]
    fn captures_take_the_frame_before_or_after() {
        let mut s = Scripted::default();
        let gray = CaptureOptions {
            format: FrameFormat::Gray8,
            ..CaptureOptions::default()
        };
        // Pending, then answered.
        assert!(s.capture(CaptureId(1), &gray).is_empty());
        let events = s.frame(png());
        let CameraEvent::Frame {
            capture,
            result: Ok(frame),
        } = &events[0]
        else {
            panic!("{events:?}");
        };
        assert_eq!(
            (*capture, frame.format, frame.data.as_slice()),
            (CaptureId(1), FrameFormat::Gray8, &[76u8, 255][..])
        );
        // Given first, then taken by the next capture, once.
        assert!(s.frame(png()).is_empty());
        let events = s.capture(CaptureId(2), &CaptureOptions::default());
        assert!(
            matches!(&events[0], CameraEvent::Frame { result: Ok(f), .. } if f.data == png().rgba)
        );
        assert!(
            s.capture(CaptureId(3), &CaptureOptions::default())
                .is_empty()
        );
        assert_eq!(
            s.cancel(CaptureId(3)),
            [CameraEvent::Frame {
                capture: CaptureId(3),
                result: Err(CameraError::Cancelled)
            }]
        );
        // Jpeg is refused with the reason.
        let jpeg = CaptureOptions {
            format: FrameFormat::Jpeg,
            ..CaptureOptions::default()
        };
        s.capture(CaptureId(4), &jpeg);
        assert!(matches!(
            &s.frame(png())[0],
            CameraEvent::Frame {
                result: Err(CameraError::Unsupported(_)),
                ..
            }
        ));
    }

    #[test]
    fn a_denial_refuses_the_next_call_only() {
        let mut s = Scripted::default();
        s.deny(true);
        assert_eq!(
            s.start(ScanId(1), Facing::Back, None),
            [CameraEvent::Closed {
                scan: ScanId(1),
                reason: Some(CameraError::PermissionDenied {
                    settings_only: true
                }),
            }]
        );
        assert_eq!(s.start(ScanId(2), Facing::Back, None).len(), 1);
        s.deny(false);
        assert_eq!(
            s.capture(CaptureId(1), &CaptureOptions::default()),
            [CameraEvent::Frame {
                capture: CaptureId(1),
                result: Err(CameraError::PermissionDenied {
                    settings_only: false
                }),
            }]
        );
    }

    #[test]
    fn suspend_interrupts_captures_and_resume_reopens() {
        let mut s = Scripted::default();
        s.start(ScanId(1), Facing::Front, None);
        s.capture(CaptureId(1), &CaptureOptions::default());
        assert_eq!(
            s.suspend(),
            [
                CameraEvent::Frame {
                    capture: CaptureId(1),
                    result: Err(CameraError::Interrupted)
                },
                CameraEvent::Suspended { scan: ScanId(1) },
            ]
        );
        assert!(s.suspend().is_empty());
        assert_eq!(s.resume(), [opened(ScanId(1), Facing::Front)]);
        assert!(s.resume().is_empty());
    }
}
