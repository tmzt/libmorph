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

/// Turns a mouse into a single finger: the left button is the contact.
#[cfg(not(target_os = "android"))]
#[derive(Debug, Default)]
pub(crate) struct MouseTouch {
    pressed: bool,
    x: f32,
    y: f32,
}

#[cfg(not(target_os = "android"))]
impl MouseTouch {
    /// Kept apart from real touch ids, which count up from zero.
    pub const ID: u64 = u64::MAX;

    fn touch(&self, phase: TouchPhase) -> Touch {
        Touch {
            id: Self::ID,
            phase,
            x: self.x,
            y: self.y,
            pressure: None,
        }
    }

    pub fn moved(&mut self, x: f32, y: f32) -> Option<Touch> {
        (self.x, self.y) = (x, y);
        self.pressed.then(|| self.touch(TouchPhase::Moved))
    }

    pub fn button(&mut self, pressed: bool) -> Option<Touch> {
        if pressed == self.pressed {
            return None;
        }
        self.pressed = pressed;
        Some(self.touch(if pressed {
            TouchPhase::Started
        } else {
            TouchPhase::Ended
        }))
    }

    /// The cursor left the window: a held contact is cancelled, not ended.
    pub fn left(&mut self) -> Option<Touch> {
        std::mem::take(&mut self.pressed).then(|| self.touch(TouchPhase::Cancelled))
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::{MouseTouch, TouchPhase};

    #[test]
    fn hover_is_not_a_touch() {
        let mut mouse = MouseTouch::default();
        assert_eq!(mouse.moved(1.0, 2.0), None);
        assert_eq!(mouse.button(false), None);
    }

    #[test]
    fn press_drag_release() {
        let mut mouse = MouseTouch::default();
        mouse.moved(1.0, 2.0);
        let start = mouse.button(true).unwrap();
        assert_eq!(
            (start.phase, start.x, start.y),
            (TouchPhase::Started, 1.0, 2.0)
        );
        assert_eq!(mouse.button(true), None);
        assert_eq!(mouse.moved(3.0, 4.0).unwrap().phase, TouchPhase::Moved);
        let end = mouse.button(false).unwrap();
        assert_eq!((end.phase, end.x, end.y), (TouchPhase::Ended, 3.0, 4.0));
        assert_eq!(end.id, MouseTouch::ID);
    }

    #[test]
    fn leaving_cancels_once() {
        let mut mouse = MouseTouch::default();
        mouse.button(true);
        assert_eq!(mouse.left().unwrap().phase, TouchPhase::Cancelled);
        assert_eq!(mouse.left(), None);
        assert_eq!(mouse.moved(1.0, 1.0), None);
    }
}
