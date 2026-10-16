//! macOS: the fingers on the trackpad, read from AppKit's `NSTouch`.
//!
//! winit reports no per-finger trackpad touches, so the window's view is
//! told to accept indirect touches and a local event monitor reads them from
//! each gesture event. The monitor passes every event on unchanged, so winit
//! still sees it. Frames queue up until the runner drains them.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSTouchPhase, NSTouchTypeMask, NSView};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use crate::input::{PadFinger, PadFrame};

/// Reads the trackpad over one window until dropped.
pub(crate) struct Trackpad {
    monitor: Retained<AnyObject>,
    frames: Rc<RefCell<Vec<PadFrame>>>,
}

impl Trackpad {
    /// `None` if the window has no AppKit view or the monitor can't be
    /// installed.
    pub fn new(window: &Window) -> Option<Self> {
        let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: `ns_view` is the live `NSView` of a winit window, and this
        // runs on the main thread, where winit creates windows. It is
        // retained so the monitor's block can keep using it.
        let view: Retained<NSView> = unsafe { Retained::retain(handle.ns_view.as_ptr().cast()) }?;
        // SAFETY: as above; indirect touches are the trackpad's.
        unsafe { view.setAllowedTouchTypes(NSTouchTypeMask::Indirect) };

        let frames = Rc::new(RefCell::new(Vec::new()));
        let keys = RefCell::new(Keys::default());
        let queue = Rc::clone(&frames);
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit hands the monitor a valid event for the call.
            let frame = read(unsafe { event.as_ref() }, &view, &mut keys.borrow_mut());
            if let Some(frame) = frame {
                queue.borrow_mut().push(frame);
            }
            event.as_ptr()
        });
        // Only gesture events carry touches.
        let mask = NSEventMask::Gesture
            | NSEventMask::BeginGesture
            | NSEventMask::EndGesture
            | NSEventMask::Magnify
            | NSEventMask::Rotate
            | NSEventMask::Swipe
            | NSEventMask::SmartMagnify;
        // SAFETY: main thread; the block returns the event it was given.
        let monitor =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) }?;
        Some(Self { monitor, frames })
    }

    /// The frames since the last drain, oldest first.
    pub fn drain(&self) -> Vec<PadFrame> {
        std::mem::take(&mut self.frames.borrow_mut())
    }
}

impl Drop for Trackpad {
    fn drop(&mut self) {
        // SAFETY: `monitor` came from `addLocalMonitorForEventsMatchingMask`
        // and is removed once.
        unsafe { NSEvent::removeMonitor(&self.monitor) };
    }
}

/// Gives each finger AppKit tracks (its `identity`) a key, for as long as
/// it stays on the pad.
#[derive(Default)]
struct Keys {
    known: Vec<(Retained<AnyObject>, u64)>,
    next: u64,
}

impl Keys {
    fn key(&mut self, identity: Retained<AnyObject>) -> u64 {
        for (known, key) in &self.known {
            // SAFETY: identities are `NSObject`s; `isEqual:` is how AppKit
            // says they are compared.
            let equal: bool = unsafe { msg_send![&**known, isEqual: &*identity] };
            if equal {
                return *key;
            }
        }
        let key = self.next;
        self.next += 1;
        self.known.push((identity, key));
        key
    }

    /// Forget fingers no longer on the pad.
    fn keep(&mut self, keys: &[u64]) {
        self.known.retain(|(_, key)| keys.contains(key));
    }
}

/// The fingers in one event, or `None` if it has none and none were down.
fn read(event: &NSEvent, view: &NSView, keys: &mut Keys) -> Option<PadFrame> {
    // SAFETY: the monitor only sees gesture events, which carry touches.
    let touches = unsafe { event.touchesMatchingPhase_inView(NSTouchPhase::Any, Some(view)) };
    let touches = touches.to_vec();
    if touches.is_empty() && keys.known.is_empty() {
        return None;
    }
    let mut frame = PadFrame::default();
    for touch in touches {
        // SAFETY: plain property reads on a touch AppKit just handed us.
        let (identity, phase, resting, position, size) = unsafe {
            (
                touch.identity(),
                touch.phase(),
                touch.isResting(),
                touch.normalizedPosition(),
                touch.deviceSize(),
            )
        };
        let key = keys.key(identity);
        frame.pad_size = (size.width as f32, size.height as f32);
        if phase.contains(NSTouchPhase::Cancelled) {
            frame.cancelled.push(key);
        } else if phase.intersects(NSTouchPhase::Touching) && !resting {
            frame.fingers.push(PadFinger {
                key,
                x: position.x as f32,
                // AppKit's origin is the bottom left.
                y: 1.0 - position.y as f32,
            });
        }
    }
    let down: Vec<u64> = frame.fingers.iter().map(|f| f.key).collect();
    keys.keep(&down);
    Some(frame)
}
