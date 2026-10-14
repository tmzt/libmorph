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
