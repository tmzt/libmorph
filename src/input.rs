//! Uniform touch and key input. Positions are physical pixels of the surface.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    /// The contact ended without a normal release: the host took it away (a
    /// system gesture, or on desktop the held mouse leaving the window).
    /// libmorph attaches no further meaning; whether that aborts a drag or
    /// anything else is the app's call.
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Touch {
    /// Stable for one finger from `Started` to `Ended`/`Cancelled`.
    pub id: u64,
    pub phase: TouchPhase,
    pub x: f32,
    pub y: f32,
    /// 0.0..=1.0 where the host reports it: Android, and macOS Force Touch
    /// while the left button is held. Otherwise `None`.
    pub pressure: Option<f32>,
}

/// A key the app can act on, delivered on press by
/// [`MorphApp::on_key`](crate::MorphApp::on_key).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum Key {
    Character(char),
    Escape,
    /// The system Back action (Android Back, a browser-back key on desktop).
    Back,
}

/// Whether the app consumed a key. On Android, `Unhandled` lets the system
/// act on it (Back closes or backgrounds the activity); desktop ignores it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KeyStatus {
    Handled,
    Unhandled,
}

/// What the mouse turned into.
#[cfg(not(target_os = "android"))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MouseOutput {
    Touch(Touch),
    Hover(f32, f32),
}

/// Turns a mouse into a single finger: the left button is the contact, and
/// the cursor with no button held is a hover. A held contact that leaves the
/// window is cancelled once; nothing more is reported until the next press.
#[cfg(not(target_os = "android"))]
#[derive(Debug, Default)]
pub(crate) struct MouseTouch {
    /// The left button is down and its touch is live.
    held: bool,
    /// The left button is down but its touch was cancelled.
    cancelled: bool,
    x: f32,
    y: f32,
    /// The latest Force Touch reading since the left press.
    pressure: Option<f32>,
}

#[cfg(not(target_os = "android"))]
impl MouseTouch {
    /// Kept apart from real touch ids, which count up from zero.
    pub const ID: u64 = u64::MAX;

    fn touch(&self, phase: TouchPhase) -> MouseOutput {
        MouseOutput::Touch(Touch {
            id: Self::ID,
            phase,
            x: self.x,
            y: self.y,
            pressure: self.pressure,
        })
    }

    fn cancel(&mut self) -> MouseOutput {
        self.held = false;
        self.cancelled = true;
        self.touch(TouchPhase::Cancelled)
    }

    /// The cursor moved to `(x, y)` on a surface of `(width, height)`.
    pub fn moved(&mut self, x: f32, y: f32, (width, height): (u32, u32)) -> Option<MouseOutput> {
        let inside = (0.0..width as f32).contains(&x) && (0.0..height as f32).contains(&y);
        if self.held && !inside {
            // Cancelled at the last position inside the window.
            return Some(self.cancel());
        }
        (self.x, self.y) = (x, y);
        if self.held {
            Some(self.touch(TouchPhase::Moved))
        } else if self.cancelled || !inside {
            None
        } else {
            Some(MouseOutput::Hover(x, y))
        }
    }

    /// The left button changed.
    pub fn button(&mut self, pressed: bool) -> Option<MouseOutput> {
        if pressed {
            if self.held || self.cancelled {
                return None;
            }
            self.held = true;
            self.pressure = None;
            Some(self.touch(TouchPhase::Started))
        } else if self.held {
            self.held = false;
            Some(self.touch(TouchPhase::Ended))
        } else {
            self.cancelled = false;
            None
        }
    }

    /// A Force Touch reading (macOS `TouchpadPressure`).
    pub fn pressure(&mut self, pressure: f32) -> Option<MouseOutput> {
        self.pressure = Some(pressure);
        self.held.then(|| self.touch(TouchPhase::Moved))
    }

    /// The platform reports the cursor left the window.
    pub fn left(&mut self) -> Option<MouseOutput> {
        self.held.then(|| self.cancel())
    }

    /// Trackpad fingers took over: a held touch is cancelled, as when it
    /// leaves the window.
    #[cfg(any(test, target_os = "macos"))]
    pub fn interrupt(&mut self) -> Option<MouseOutput> {
        self.held.then(|| self.cancel())
    }
}

/// One finger on a trackpad: the host's identity for it turned into a key,
/// and its position, 0..1 from the pad's top left.
#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PadFinger {
    pub key: u64,
    pub x: f32,
    pub y: f32,
}

/// Every finger on the pad after one host event.
#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PadFrame {
    /// Touching, not resting.
    pub fingers: Vec<PadFinger>,
    /// Fingers the host cancelled in this event.
    pub cancelled: Vec<u64>,
    /// The pad's size in points (1/72 inch).
    pub pad_size: (f32, f32),
}

/// How pad positions land on the surface.
#[cfg(any(test, target_os = "macos"))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PadMapping {
    /// Pad distances equal distances on the emulated device, centred on the
    /// cursor. `px_per_mm` is in device pixels.
    Physical { px_per_mm: f32 },
    /// The whole pad covers the whole surface.
    Fit,
}

#[cfg(any(test, target_os = "macos"))]
#[derive(Debug)]
struct PadSession {
    /// The cursor when the session started, in surface pixels.
    anchor: (f32, f32),
    /// The starting fingers' centroid, in pad units.
    origin: (f32, f32),
    /// Fingers down: key, touch id and last surface position.
    fingers: Vec<(u64, u64, f32, f32)>,
}

/// Turns trackpad fingers into touches. A session starts when two or more
/// fingers are on the pad with the cursor over the surface, reports every
/// finger (including ones that land later), and ends when the last one
/// lifts. One finger alone is left to the mouse.
#[cfg(any(test, target_os = "macos"))]
#[derive(Debug)]
pub(crate) struct PadTouch {
    mapping: PadMapping,
    session: Option<PadSession>,
    next_id: u64,
}

#[cfg(any(test, target_os = "macos"))]
impl PadTouch {
    /// Kept apart from [`MouseTouch::ID`] and from touchscreen ids, which
    /// count up from zero.
    pub const ID_BASE: u64 = 1 << 32;

    pub fn new(mapping: PadMapping) -> Self {
        Self {
            mapping,
            session: None,
            next_id: Self::ID_BASE,
        }
    }

    /// A session is active: the mouse is held off.
    pub fn active(&self) -> bool {
        self.session.is_some()
    }

    /// The touches for one pad frame. `cursor` is the cursor position if it
    /// is over the surface; `scale` is surface size / device size.
    pub fn frame(
        &mut self,
        frame: &PadFrame,
        cursor: Option<(f32, f32)>,
        surface: (u32, u32),
        scale: (f32, f32),
    ) -> Vec<Touch> {
        let mut touches = Vec::new();
        if self.session.is_none() {
            let (Some(anchor), true) = (cursor, frame.fingers.len() >= 2) else {
                return touches;
            };
            let n = frame.fingers.len() as f32;
            let origin = frame
                .fingers
                .iter()
                .fold((0.0, 0.0), |(x, y), f| (x + f.x / n, y + f.y / n));
            self.session = Some(PadSession {
                anchor,
                origin,
                fingers: Vec::new(),
            });
        }
        let Some(session) = self.session.as_mut() else {
            return touches;
        };
        let touch = |id, phase, x, y| Touch {
            id,
            phase,
            x,
            y,
            pressure: None,
        };

        session.fingers.retain(|&(key, id, x, y)| {
            let phase = if frame.cancelled.contains(&key) {
                TouchPhase::Cancelled
            } else if frame.fingers.iter().any(|f| f.key == key) {
                return true;
            } else {
                TouchPhase::Ended
            };
            touches.push(touch(id, phase, x, y));
            false
        });
        for finger in &frame.fingers {
            if frame.cancelled.contains(&finger.key) {
                continue;
            }
            let (x, y) = place(
                self.mapping,
                session,
                finger,
                frame.pad_size,
                surface,
                scale,
            );
            match session.fingers.iter_mut().find(|f| f.0 == finger.key) {
                Some(known) if (known.2, known.3) != (x, y) => {
                    (known.2, known.3) = (x, y);
                    touches.push(touch(known.1, TouchPhase::Moved, x, y));
                }
                Some(_) => {}
                None => {
                    let id = self.next_id;
                    self.next_id += 1;
                    session.fingers.push((finger.key, id, x, y));
                    touches.push(touch(id, TouchPhase::Started, x, y));
                }
            }
        }
        if session.fingers.is_empty() {
            self.session = None;
        }
        touches
    }

    /// Cancels every finger still down (focus lost, suspended).
    pub fn cancel(&mut self) -> Vec<Touch> {
        self.session.take().map_or_else(Vec::new, |session| {
            session
                .fingers
                .into_iter()
                .map(|(_, id, x, y)| Touch {
                    id,
                    phase: TouchPhase::Cancelled,
                    x,
                    y,
                    pressure: None,
                })
                .collect()
        })
    }
}

/// Where `finger` lands on the surface, clamped to it.
#[cfg(any(test, target_os = "macos"))]
fn place(
    mapping: PadMapping,
    session: &PadSession,
    finger: &PadFinger,
    (pad_width, pad_height): (f32, f32),
    (width, height): (u32, u32),
    (scale_x, scale_y): (f32, f32),
) -> (f32, f32) {
    let (x, y) = match mapping {
        PadMapping::Fit => (finger.x * width as f32, finger.y * height as f32),
        PadMapping::Physical { px_per_mm } => {
            // Points are 1/72 inch.
            let px_per_point = px_per_mm * 25.4 / 72.0;
            (
                session.anchor.0
                    + (finger.x - session.origin.0) * pad_width * px_per_point * scale_x,
                session.anchor.1
                    + (finger.y - session.origin.1) * pad_height * px_per_point * scale_y,
            )
        }
    };
    (
        x.clamp(0.0, width.saturating_sub(1) as f32),
        y.clamp(0.0, height.saturating_sub(1) as f32),
    )
}

/// Android acts on a key such as Back when it comes *up*, so the status the
/// app gave the press must also answer the repeats and the release.
#[cfg(any(test, target_os = "android"))]
#[derive(Debug)]
pub(crate) struct KeyRouting {
    back: KeyStatus,
    escape: KeyStatus,
}

#[cfg(any(test, target_os = "android"))]
impl Default for KeyRouting {
    fn default() -> Self {
        Self {
            back: KeyStatus::Unhandled,
            escape: KeyStatus::Unhandled,
        }
    }
}

#[cfg(any(test, target_os = "android"))]
impl KeyRouting {
    fn slot(&mut self, key: Key) -> Option<&mut KeyStatus> {
        match key {
            Key::Back => Some(&mut self.back),
            Key::Escape => Some(&mut self.escape),
            Key::Character(_) => None,
        }
    }

    /// The app answered the first press of `key` with `status`.
    pub fn pressed(&mut self, key: Key, status: KeyStatus) -> KeyStatus {
        if let Some(slot) = self.slot(key) {
            *slot = status;
        }
        status
    }

    /// A repeat or the release of `key`: the press's answer.
    pub fn followup(&mut self, key: Key) -> KeyStatus {
        self.slot(key).map_or(KeyStatus::Unhandled, |slot| *slot)
    }
}

#[cfg(test)]
mod routing_tests {
    use super::{Key, KeyRouting, KeyStatus};

    #[test]
    fn release_follows_the_press() {
        let mut routing = KeyRouting::default();
        assert_eq!(
            routing.followup(Key::Back),
            KeyStatus::Unhandled,
            "no press yet"
        );
        assert_eq!(
            routing.pressed(Key::Back, KeyStatus::Handled),
            KeyStatus::Handled
        );
        assert_eq!(routing.followup(Key::Back), KeyStatus::Handled, "repeat");
        assert_eq!(routing.followup(Key::Back), KeyStatus::Handled, "release");
        routing.pressed(Key::Back, KeyStatus::Unhandled);
        assert_eq!(routing.followup(Key::Back), KeyStatus::Unhandled);
    }

    #[test]
    fn keys_are_tracked_apart() {
        let mut routing = KeyRouting::default();
        routing.pressed(Key::Escape, KeyStatus::Handled);
        assert_eq!(routing.followup(Key::Back), KeyStatus::Unhandled);
        assert_eq!(routing.followup(Key::Escape), KeyStatus::Handled);
        assert_eq!(
            routing.pressed(Key::Character('a'), KeyStatus::Handled),
            KeyStatus::Handled
        );
        assert_eq!(routing.followup(Key::Character('a')), KeyStatus::Unhandled);
    }
}

/// The [`Key`] for a winit logical key, if the app gets one for it.
#[cfg(not(target_os = "android"))]
pub(crate) fn key_from_winit(key: &winit::keyboard::Key) -> Option<Key> {
    use winit::keyboard::{Key as WinitKey, NamedKey};
    match key {
        WinitKey::Character(text) => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Some(Key::Character(c)),
                _ => None,
            }
        }
        WinitKey::Named(NamedKey::Escape) => Some(Key::Escape),
        WinitKey::Named(NamedKey::BrowserBack | NamedKey::GoBack) => Some(Key::Back),
        _ => None,
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::{Key, MouseOutput, MouseTouch, Touch, TouchPhase, key_from_winit};
    use winit::keyboard::{Key as WinitKey, NamedKey};

    const SIZE: (u32, u32) = (100, 200);

    fn touch(output: Option<MouseOutput>) -> Touch {
        match output {
            Some(MouseOutput::Touch(touch)) => touch,
            other => panic!("expected a touch, got {other:?}"),
        }
    }

    fn phase(output: Option<MouseOutput>) -> TouchPhase {
        touch(output).phase
    }

    #[test]
    fn moving_with_the_button_up_hovers() {
        let mut mouse = MouseTouch::default();
        assert_eq!(
            mouse.moved(1.0, 2.0, SIZE),
            Some(MouseOutput::Hover(1.0, 2.0))
        );
        assert_eq!(mouse.button(false), None);
    }

    #[test]
    fn hover_outside_the_window_is_dropped() {
        let mut mouse = MouseTouch::default();
        assert_eq!(mouse.moved(-1.0, 2.0, SIZE), None);
        assert_eq!(mouse.moved(100.0, 2.0, SIZE), None);
    }

    #[test]
    fn press_drag_release() {
        let mut mouse = MouseTouch::default();
        mouse.moved(1.0, 2.0, SIZE);
        let start = touch(mouse.button(true));
        assert_eq!(
            (start.phase, start.x, start.y),
            (TouchPhase::Started, 1.0, 2.0)
        );
        assert_eq!((start.id, start.pressure), (MouseTouch::ID, None));
        assert_eq!(mouse.button(true), None);
        assert_eq!(phase(mouse.moved(3.0, 4.0, SIZE)), TouchPhase::Moved);
        let end = touch(mouse.button(false));
        assert_eq!((end.phase, end.x, end.y), (TouchPhase::Ended, 3.0, 4.0));
        assert!(matches!(
            mouse.moved(5.0, 5.0, SIZE),
            Some(MouseOutput::Hover(..))
        ));
    }

    #[test]
    fn pressure_follows_readings_while_held() {
        let mut mouse = MouseTouch::default();
        assert_eq!(mouse.pressure(0.3), None, "stored, but nothing is held");
        assert_eq!(touch(mouse.button(true)).pressure, None, "cleared on press");
        let pressed = touch(mouse.pressure(0.7));
        assert_eq!(
            (pressed.phase, pressed.pressure),
            (TouchPhase::Moved, Some(0.7))
        );
        assert_eq!(touch(mouse.moved(2.0, 2.0, SIZE)).pressure, Some(0.7));
        assert_eq!(touch(mouse.button(false)).pressure, Some(0.7));
    }

    #[test]
    fn leaving_by_position_cancels_at_the_last_inside_point() {
        let mut mouse = MouseTouch::default();
        mouse.moved(10.0, 10.0, SIZE);
        mouse.button(true);
        mouse.moved(99.0, 50.0, SIZE);
        let cancel = touch(mouse.moved(120.0, 50.0, SIZE));
        assert_eq!(
            (cancel.phase, cancel.x, cancel.y),
            (TouchPhase::Cancelled, 99.0, 50.0)
        );
    }

    #[test]
    fn leaving_by_event_cancels_once() {
        let mut mouse = MouseTouch::default();
        mouse.button(true);
        assert_eq!(phase(mouse.left()), TouchPhase::Cancelled);
        assert_eq!(mouse.left(), None);
    }

    #[test]
    fn nothing_after_a_cancel_until_the_next_press() {
        let mut mouse = MouseTouch::default();
        mouse.button(true);
        mouse.moved(150.0, 10.0, SIZE);
        assert_eq!(
            mouse.moved(50.0, 10.0, SIZE),
            None,
            "back inside: no resume"
        );
        assert_eq!(mouse.pressure(0.5), None);
        assert_eq!(mouse.button(false), None, "release after a cancel");
        assert!(matches!(
            mouse.moved(50.0, 11.0, SIZE),
            Some(MouseOutput::Hover(..))
        ));
        assert_eq!(
            phase(mouse.button(true)),
            TouchPhase::Started,
            "a new press starts again"
        );
    }

    #[test]
    fn winit_keys_map() {
        let named = |key| key_from_winit(&WinitKey::Named(key));
        assert_eq!(
            key_from_winit(&WinitKey::Character("a".into())),
            Some(Key::Character('a'))
        );
        assert_eq!(
            key_from_winit(&WinitKey::Character("é".into())),
            Some(Key::Character('é'))
        );
        assert_eq!(named(NamedKey::Escape), Some(Key::Escape));
        assert_eq!(named(NamedKey::BrowserBack), Some(Key::Back));
        assert_eq!(named(NamedKey::GoBack), Some(Key::Back));
    }

    #[test]
    fn other_winit_keys_are_not_delivered() {
        assert_eq!(key_from_winit(&WinitKey::Character("ab".into())), None);
        assert_eq!(key_from_winit(&WinitKey::Character("".into())), None);
        assert_eq!(key_from_winit(&WinitKey::Named(NamedKey::Enter)), None);
        assert_eq!(key_from_winit(&WinitKey::Named(NamedKey::Backspace)), None);
        assert_eq!(key_from_winit(&WinitKey::Dead(None)), None);
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod pad_tests {
    use super::{MouseOutput, MouseTouch, PadFinger, PadFrame, PadMapping, PadTouch, TouchPhase};

    const SURFACE: (u32, u32) = (1000, 2000);
    const PAD: (f32, f32) = (453.5, 283.5); // 160 × 100 mm in points

    fn frame(fingers: &[(u64, f32, f32)]) -> PadFrame {
        PadFrame {
            fingers: fingers
                .iter()
                .map(|&(key, x, y)| PadFinger { key, x, y })
                .collect(),
            cancelled: Vec::new(),
            pad_size: PAD,
        }
    }

    fn phases(touches: &[super::Touch]) -> Vec<(u64, TouchPhase)> {
        touches.iter().map(|t| (t.id, t.phase)).collect()
    }

    const A: u64 = PadTouch::ID_BASE;
    const B: u64 = PadTouch::ID_BASE + 1;
    const C: u64 = PadTouch::ID_BASE + 2;
    const CURSOR: Option<(f32, f32)> = Some((500.0, 1000.0));

    fn fit() -> PadTouch {
        PadTouch::new(PadMapping::Fit)
    }

    #[test]
    fn one_finger_is_left_to_the_mouse() {
        let mut pad = fit();
        assert!(
            pad.frame(&frame(&[(7, 0.5, 0.5)]), CURSOR, SURFACE, (1.0, 1.0))
                .is_empty()
        );
        assert!(!pad.active());
    }

    #[test]
    fn no_session_without_the_cursor_over_the_surface() {
        let mut pad = fit();
        let two = frame(&[(1, 0.2, 0.5), (2, 0.8, 0.5)]);
        assert!(pad.frame(&two, None, SURFACE, (1.0, 1.0)).is_empty());
        assert!(!pad.active());
    }

    #[test]
    fn session_from_two_fingers_to_the_last_lift() {
        let mut pad = fit();
        let started = pad.frame(
            &frame(&[(1, 0.2, 0.5), (2, 0.8, 0.5)]),
            CURSOR,
            SURFACE,
            (1.0, 1.0),
        );
        assert_eq!(
            phases(&started),
            [(A, TouchPhase::Started), (B, TouchPhase::Started)]
        );
        assert_eq!(
            (started[0].x, started[0].y, started[0].pressure),
            (200.0, 1000.0, None)
        );
        assert!(pad.active());

        let moved = pad.frame(
            &frame(&[(1, 0.2, 0.5), (2, 0.9, 0.5)]),
            CURSOR,
            SURFACE,
            (1.0, 1.0),
        );
        assert_eq!(
            phases(&moved),
            [(B, TouchPhase::Moved)],
            "only the finger that moved"
        );

        let joined = pad.frame(
            &frame(&[(1, 0.2, 0.5), (2, 0.9, 0.5), (3, 0.5, 0.1)]),
            CURSOR,
            SURFACE,
            (1.0, 1.0),
        );
        assert_eq!(phases(&joined), [(C, TouchPhase::Started)]);

        let lifted = pad.frame(&frame(&[(3, 0.5, 0.2)]), None, SURFACE, (1.0, 1.0));
        assert_eq!(
            phases(&lifted),
            [
                (A, TouchPhase::Ended),
                (B, TouchPhase::Ended),
                (C, TouchPhase::Moved)
            ],
            "the last finger keeps reporting"
        );
        assert!(pad.active());

        let last = pad.frame(&frame(&[]), CURSOR, SURFACE, (1.0, 1.0));
        assert_eq!(phases(&last), [(C, TouchPhase::Ended)]);
        assert!(!pad.active());

        let again = pad.frame(
            &frame(&[(4, 0.1, 0.1), (5, 0.2, 0.2)]),
            CURSOR,
            SURFACE,
            (1.0, 1.0),
        );
        assert_eq!(again[0].id, PadTouch::ID_BASE + 3, "ids keep counting");
    }

    #[test]
    fn host_cancels_and_cancel_all() {
        let mut pad = fit();
        pad.frame(
            &frame(&[(1, 0.2, 0.5), (2, 0.8, 0.5)]),
            CURSOR,
            SURFACE,
            (1.0, 1.0),
        );
        let mut cancelled = frame(&[(2, 0.8, 0.5)]);
        cancelled.cancelled.push(1);
        assert_eq!(
            phases(&pad.frame(&cancelled, CURSOR, SURFACE, (1.0, 1.0))),
            [(A, TouchPhase::Cancelled)]
        );
        assert_eq!(phases(&pad.cancel()), [(B, TouchPhase::Cancelled)]);
        assert!(!pad.active());
        assert!(pad.cancel().is_empty());
    }

    #[test]
    fn positions_are_clamped_to_the_surface() {
        let mut pad = fit();
        let touches = pad.frame(
            &frame(&[(1, 0.0, 0.0), (2, 1.0, 1.0)]),
            CURSOR,
            SURFACE,
            (1.0, 1.0),
        );
        assert_eq!((touches[0].x, touches[0].y), (0.0, 0.0));
        assert_eq!((touches[1].x, touches[1].y), (999.0, 1999.0));
    }

    #[test]
    fn physical_keeps_millimetres_and_centres_on_the_cursor() {
        // 416 ppi, as a Galaxy S24.
        let px_per_mm = 416.0 / 25.4;
        let mut pad = PadTouch::new(PadMapping::Physical { px_per_mm });
        // 30 mm apart across a 160 mm pad, centred.
        let (left, right) = (0.5 - 15.0 / 160.0, 0.5 + 15.0 / 160.0);
        let touches = pad.frame(
            &frame(&[(1, left, 0.5), (2, right, 0.5)]),
            CURSOR,
            SURFACE,
            (1.0, 1.0),
        );
        let spread = touches[1].x - touches[0].x;
        assert!((spread - 491.3).abs() < 0.5, "{spread}");
        let centre = (touches[0].x + touches[1].x) / 2.0;
        assert!((centre - 500.0).abs() < 0.01 && touches[0].y == 1000.0);

        // On a surface at half the device size, half the pixels.
        let mut pad = PadTouch::new(PadMapping::Physical { px_per_mm });
        let touches = pad.frame(
            &frame(&[(1, left, 0.5), (2, right, 0.5)]),
            CURSOR,
            SURFACE,
            (0.5, 0.5),
        );
        assert!((touches[1].x - touches[0].x - 245.7).abs() < 0.5);
    }

    #[test]
    fn interrupt_cancels_a_held_mouse_touch_once() {
        let mut mouse = MouseTouch::default();
        assert_eq!(mouse.interrupt(), None);
        mouse.button(true);
        assert!(matches!(
            mouse.interrupt(),
            Some(MouseOutput::Touch(t)) if t.phase == TouchPhase::Cancelled
        ));
        assert_eq!(mouse.interrupt(), None);
        assert_eq!(mouse.button(false), None);
    }
}
