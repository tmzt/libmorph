//! The trait an application implements.

use std::time::Duration;

use crate::log_once::log_once;
use crate::service::ServiceEvent;
use crate::{AudioSetup, Key, KeyStatus, LimitsRequest, MorphContext, Permissions, Touch};

/// The surface texture for one frame. libmorph presents it after
/// [`MorphApp::render`] returns [`RenderStatus::Drawn`].
pub struct Frame<'a> {
    pub view: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    /// Physical pixels.
    pub size: (u32, u32),
}

/// Whether the frame [`MorphApp::render`] was given needs presenting.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RenderStatus {
    /// The app drew into `frame.view`; libmorph presents it.
    Drawn,
    /// Nothing changed. libmorph drops the texture without presenting it,
    /// and the previous frame stays on screen.
    Skipped,
}

/// The subsystems an app takes from libmorph, as [`MorphApp::OWNS`]. A set
/// bit means libmorph does not drive that subsystem: it does not open it,
/// pump it, or call its managed-path hooks.
///
/// Hand-rolled rather than taken from a flags crate, so no foreign type
/// sits in libmorph's public API: under Rule 5.5 that would chain this
/// crate's version to another crate's.
///
/// ```
/// # use libmorph::Owned;
/// const OWNS: Owned = Owned::FRAME;
/// assert!(OWNS.contains(Owned::FRAME));
/// assert!(!Owned::empty().contains(Owned::FRAME));
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Owned(u32);

impl Owned {
    /// The frame: acquiring the surface texture, drawing into it and
    /// presenting it.
    pub const FRAME: Self = Self(1 << 0);

    /// Nothing taken: libmorph drives everything, as it does today.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Whether every subsystem in `other` is in this set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Both sets. A `const fn`, so it composes inside a `const`, which the
    /// [`BitOr`](core::ops::BitOr) operator cannot do on stable Rust.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl core::ops::BitOr for Owned {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

/// A libmorph application. Every hook runs on the main thread with the
/// shared [`MorphContext`].
pub trait MorphApp: Sized + 'static {
    /// The subsystems this app takes from libmorph. [`Owned::empty()`] -
    /// the default - leaves libmorph driving everything, which is every app
    /// written so far.
    ///
    /// Reading it costs nothing: the app's type is monomorphized, so
    /// `A::OWNS.contains(..)` folds at compile time and the branch not
    /// taken is eliminated. The price is that ownership is a compile-time
    /// property - `cfg!` works, a runtime toggle does not.
    ///
    /// ```
    /// # use libmorph::Owned;
    /// const OWNS: Owned = Owned::FRAME;
    /// ```
    ///
    /// Sets compose through [`Owned::union`], which is a `const fn`:
    ///
    /// ```
    /// # use libmorph::Owned;
    /// const OWNS: Owned = Owned::FRAME.union(Owned::FRAME);
    /// ```
    const OWNS: Owned = Owned::empty();

    /// Prefer an sRGB surface format (the GPU encodes gamma on write). Set
    /// `false` for apps whose shaders gamma-encode themselves. Either way, a
    /// format already in use is kept across resume when the new surface
    /// offers it.
    const SRGB_SURFACE: bool = true;

    /// On macOS, the Keychain service of
    /// [`MorphContext::secure_store`](crate::MorphContext::secure_store) is
    /// `libmorph.<SECURE_STORE_ID>`; empty (the default) uses the
    /// executable's file stem. Unused elsewhere.
    const SECURE_STORE_ID: &'static str = "";

    /// The app's directory name on the desktop, for
    /// [`MorphContext::data_dir`] and [`cache_dir`](MorphContext::cache_dir):
    /// `~/Library/Application Support/<APP_ID>` and `~/Library/Caches/<APP_ID>`
    /// on macOS. 1 to 64 of `[A-Za-z0-9._-]`, not `.` or `..`; empty (the
    /// default) uses the executable's file stem, with any other character
    /// replaced by `_`. Android uses the package's own directories instead.
    const APP_ID: &'static str = "";

    /// The device limits to ask for, before [`init`](Self::init). `base` is
    /// what libmorph asks for by default: the downlevel defaults (WebGL2
    /// defaults on a GL adapter) with texture sizes raised to the adapter's.
    /// Limits in `required` that the adapter lacks fail bring-up with
    /// [`GpuError::Limits`](crate::GpuError::Limits); `preferred` is granted
    /// as far as the adapter allows. Read what was granted from
    /// `ctx.limits()`.
    ///
    /// ```
    /// # use libmorph::{wgpu, LimitsRequest};
    /// fn limits(base: &wgpu::Limits) -> LimitsRequest {
    ///     let required = wgpu::Limits { max_storage_buffers_per_shader_stage: 8, ..base.clone() };
    ///     LimitsRequest { preferred: required.clone(), required }
    /// }
    /// ```
    fn limits(base: &wgpu::Limits) -> LimitsRequest {
        LimitsRequest::base(base)
    }

    /// The surface configuration to apply. Called at every attach when
    /// [`OWNS`](Self::OWNS) contains [`Owned::FRAME`]: the first window,
    /// and again after a rotation, a resume or a lost surface. `default` is
    /// what libmorph would have used, with the format already chosen by
    /// [`SRGB_SURFACE`](Self::SRGB_SURFACE) and the format in use kept
    /// across a resume.
    ///
    /// libmorph overrides `width` and `height` after this returns, because
    /// it knows the window's size and a wrong one breaks the surface.
    ///
    /// The baseline arrives by value and goes back by value, as
    /// [`limits`](Self::limits) does, so there is no question of what
    /// libmorph set before or after: it set all of it, and the app
    /// replaced it. Associated rather than a method, because the first
    /// attach precedes [`init`](Self::init) and there is no instance to
    /// call it on.
    ///
    /// **Not called for a managed frame.** A managed app's surface knob is
    /// `SRGB_SURFACE`, and keeping the hook to owning apps is what stops
    /// libmorph mirroring `wgpu::SurfaceConfiguration` field by field and
    /// tracking it upstream. **Not called offscreen** either, where there
    /// is no surface; `SRGB_SURFACE` drives the offscreen format under
    /// both, so a scripted run stays faithful to the device.
    ///
    /// `desired_maximum_frame_latency` is a request, clamped to the range
    /// the surface reports, so asking for 1 gets 1 only if the range
    /// allows; `caps` is in hand to check that and the present modes.
    ///
    /// ```
    /// # use libmorph::wgpu;
    /// fn wgpu_config(
    ///     default: wgpu::SurfaceConfiguration,
    ///     caps: &wgpu::SurfaceCapabilities,
    /// ) -> wgpu::SurfaceConfiguration {
    ///     let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
    ///         wgpu::PresentMode::Mailbox
    ///     } else {
    ///         default.present_mode
    ///     };
    ///     wgpu::SurfaceConfiguration { present_mode, ..default }
    /// }
    /// ```
    fn wgpu_config(
        default: wgpu::SurfaceConfiguration,
        caps: &wgpu::SurfaceCapabilities,
    ) -> wgpu::SurfaceConfiguration {
        let _ = caps;
        default
    }

    /// What audio to open, before [`init`](Self::init). `setup` starts with
    /// output on and capture off; set `setup.capture` to record from the
    /// default input device, and read it through `ctx.audio()`'s
    /// [`add_sink`](crate::Audio::add_sink). On Android, libmorph asks for
    /// `RECORD_AUDIO` after `init` if it is not granted yet (the app's
    /// manifest must declare it); see
    /// [`on_capture_ready`](Self::on_capture_ready).
    ///
    /// ```
    /// # use libmorph::AudioSetup;
    /// fn audio_setup(setup: &mut AudioSetup) {
    ///     setup.capture = true;
    /// }
    /// ```
    fn audio_setup(setup: &mut AudioSetup) {
        let _ = setup;
    }

    /// Build the app once the GPU is up and the first surface is configured,
    /// so `ctx.surface_format()` is already known.
    fn init(ctx: &mut MorphContext) -> Self;

    /// Advance state by `dt`, once per frame before [`render`](Self::render).
    fn update(&mut self, ctx: &mut MorphContext, dt: Duration) {
        let _ = (ctx, dt);
    }

    /// Record and submit this frame's work to `frame.view`, and say whether
    /// anything needs presenting.
    ///
    /// [`RenderStatus::Skipped`] is the app's claim about its own output:
    /// libmorph presents nothing and whatever is on screen stays there.
    /// [`update`](Self::update) still runs on a skipped frame - a skip says
    /// "do not present", not "do not simulate". Two things sit outside the
    /// claim. The desktop overlay is drawn into the same frame, so it is
    /// not presented either and a pressed dpad button does not appear
    /// while frames are skipped. And a freshly attached surface has no
    /// previous frame to keep, so the first frame after a surface change
    /// must be drawn.
    ///
    /// The default draws nothing and returns `Skipped`. That is right for
    /// an app whose [`OWNS`](Self::OWNS) contains [`Owned::FRAME`], where
    /// `render` is never called; for a managed app it means nothing is
    /// drawn at all, so it logs once.
    fn render(&mut self, ctx: &mut MorphContext, frame: &Frame<'_>) -> RenderStatus {
        let _ = (ctx, frame);
        log_once!(
            "libmorph: MorphApp::render is not implemented and the frame is \
             managed, so nothing is drawn. Implement render, or set \
             `const OWNS: Owned = Owned::FRAME;` and construct frames."
        );
        RenderStatus::Skipped
    }

    /// A touch, or on desktop the mouse translated into one.
    fn on_touch(&mut self, ctx: &mut MorphContext, touch: Touch) {
        let _ = (ctx, touch);
    }

    /// The cursor moved with no button held (desktop and web). Physical
    /// pixels, like touches.
    fn on_hover(&mut self, ctx: &mut MorphContext, x: f32, y: f32) {
        let _ = (ctx, x, y);
    }

    /// A key was pressed. On Android, `Handled` keeps Back inside the app and
    /// `Unhandled` lets the system act; desktop ignores the result.
    fn on_key(&mut self, ctx: &mut MorphContext, key: Key) -> KeyStatus {
        let _ = (ctx, key);
        KeyStatus::Unhandled
    }

    /// Text input event: a change in text, an action key press, or keyboard dismissal.
    fn on_text_input(&mut self, ctx: &mut MorphContext, event: &crate::TextInputEvent) {
        let _ = (ctx, event);
    }

    /// The capture stream asked for in [`audio_setup`](Self::audio_setup)
    /// has opened, and `ctx.audio().capture_format()` is `Some` from now on.
    /// Called once per run, after [`init`](Self::init): right after it when
    /// capture opened with the rest of audio, or on Android once the user
    /// allows `RECORD_AUDIO`. Never called if capture did not open.
    fn on_capture_ready(&mut self, ctx: &mut MorphContext) {
        let _ = ctx;
    }

    /// A stream was interrupted, restored or given up on (plan
    /// `20261029_02_AUDIO_STREAM_RECOVERY_PLAN.md`). Delivered on the main
    /// thread before [`update`](Self::update), oldest first, while the app
    /// is resumed. [`on_capture_ready`](Self::on_capture_ready) is not
    /// called again after a reopen: the format and the sinks are kept.
    fn on_audio(&mut self, ctx: &mut MorphContext, event: &crate::AudioEvent) {
        let _ = (ctx, event);
    }

    /// The answer to a
    /// [`ctx.request_permissions`](MorphContext::request_permissions) call,
    /// once per request the app made. Answers to libmorph's own requests
    /// (`RECORD_AUDIO` for capture) are not passed here.
    fn on_permissions(&mut self, ctx: &mut MorphContext, answer: &Permissions) {
        let _ = (ctx, answer);
    }

    /// The app was opened with `url`, a link with one of the URL schemes its
    /// build script declared (`gradle_project(id).url_scheme("myapp")`), at
    /// launch or while running. Called after [`init`](Self::init), in the
    /// order the links arrived; links that arrive while the app is suspended
    /// come after [`on_resume`](Self::on_resume). Android only, and scripted
    /// runs (`open <url>`). The URL is untrusted input: validate it, and do
    /// not log it past its path, since a query or fragment may be a secret.
    fn on_open_url(&mut self, ctx: &mut MorphContext, url: &str) {
        let _ = (ctx, url);
    }

    /// An intent reached the app, whole: a deep link, a launcher tap, or
    /// `am start -n <pkg>/<activity> --es <key> <value>`. Its action, its
    /// data URI, every extra as a key and a string, and whether it arrived
    /// at an already-running instance ([`Intent`](crate::Intent)).
    ///
    /// Called after [`init`](Self::init), in the order the intents arrived;
    /// one that arrives while the app is suspended comes after
    /// [`on_resume`](Self::on_resume). A `VIEW` intent carrying a URI calls
    /// this **first** and then [`on_open_url`](Self::on_open_url), which is
    /// unchanged - the intent is the whole event and the link is one field
    /// of it, so an app may read either or both. Android only, and scripted
    /// runs (`intent <key>=<value>`).
    ///
    /// Every field is untrusted input: validate it, and log a key rather
    /// than its value, since a value may be a secret.
    fn on_intent(&mut self, ctx: &mut MorphContext, intent: &crate::Intent) {
        let _ = (ctx, intent);
    }

    /// The foreground service ([`service`](crate::service)) did something,
    /// or its notification was tapped: an app button
    /// ([`ServiceEvent::Action`]), Exit ([`ServiceEvent::Exited`], after
    /// which the activity closes), a wake-lock change, `Started`, `Stopped`,
    /// `Failed` and the rest. On the main thread, whether or not the app is
    /// suspended, after [`init`](Self::init). Android with
    /// `mobile-foreground-service` only; the app's other threads hear the
    /// same through [`system_events`](crate::system_events).
    fn on_service(&mut self, ctx: &mut MorphContext, event: &ServiceEvent) {
        let _ = (ctx, event);
    }

    /// The camera answered ([`camera`](crate::camera)): a session opened,
    /// closed (with the reason when it failed, such as
    /// [`CameraError::PermissionDenied`](crate::camera::CameraError::PermissionDenied)
    /// or [`Unsupported`](crate::camera::CameraError::Unsupported)), was
    /// suspended or zoomed, a capture finished, or a QR code was read. On
    /// the main thread, before input and [`update`](Self::update), so the
    /// frame after it sees the result; after [`init`](Self::init).
    fn on_camera(&mut self, ctx: &mut MorphContext, event: &crate::camera::CameraEvent) {
        let _ = (ctx, event);
    }

    /// A media request answered ([`media`](crate::media)): the chosen items,
    /// or why there are none (`Cancelled`,
    /// [`PermissionDenied`](crate::media::MediaError::PermissionDenied),
    /// [`Unsupported`](crate::media::MediaError::Unsupported)...). On the
    /// main thread, after [`init`](Self::init): on Android as soon as it
    /// arrives, also while suspended (one that arrived before the app
    /// existed comes right after `init`, with `request: None`); elsewhere
    /// before the next frame's [`update`](Self::update).
    fn on_media(&mut self, ctx: &mut MorphContext, response: &crate::media::MediaResponse) {
        let _ = (ctx, response);
    }

    /// The system is low on memory (Android's `onLowMemory`). Free what can
    /// be rebuilt: caches, offscreen resources, decoded data. Called once
    /// per report after [`init`](Self::init), whether or not the app is
    /// suspended. Never called on the desktop or the web, or in scripted
    /// runs unless the script says `low-memory`.
    fn on_low_memory(&mut self, ctx: &mut MorphContext) {
        let _ = ctx;
    }

    /// Android asked the app to trim memory (`onTrimMemory`). Levels are
    /// ordered, so `level >= TrimLevel::Background` means "free everything
    /// you can". Called once per report after [`init`](Self::init),
    /// whether or not the app is suspended. Never called on the desktop or
    /// the web, or in scripted runs unless the script says
    /// `trim-memory <level>`.
    fn on_trim_memory(&mut self, ctx: &mut MorphContext, level: crate::TrimLevel) {
        let _ = (ctx, level);
    }

    /// The surface is gone (app backgrounded). The device, queue and every
    /// resource built on them stay valid.
    fn on_suspend(&mut self, ctx: &mut MorphContext) {
        let _ = ctx;
    }

    /// A new surface is configured after [`on_suspend`](Self::on_suspend).
    fn on_resume(&mut self, ctx: &mut MorphContext) {
        let _ = ctx;
    }
}

#[cfg(test)]
mod tests {
    use super::{MorphApp, Owned};

    /// `union` is a `const fn`, so a set composes where `OWNS` is
    /// written. The `BitOr` operator cannot: a trait impl is not `const`
    /// on stable Rust (E0015, "cannot call non-const operator in
    /// constants"), which is why `union` exists.
    const COMPOSED: Owned = Owned::FRAME.union(Owned::FRAME).union(Owned::empty());

    #[test]
    fn a_set_composes_in_a_const() {
        assert!(COMPOSED.contains(Owned::FRAME));
        assert_eq!(COMPOSED, Owned::FRAME);
    }

    #[test]
    fn the_operator_is_the_same_set_outside_a_const() {
        assert_eq!(Owned::FRAME | Owned::FRAME, Owned::FRAME);
        assert_eq!(Owned::empty() | Owned::FRAME, Owned::FRAME);
    }

    #[test]
    fn empty_contains_only_itself() {
        let empty = Owned::empty();
        assert_eq!(empty, Owned::default(), "the default is the empty set");
        assert!(empty.contains(empty));
        assert!(!empty.contains(Owned::FRAME));
        assert!(
            Owned::FRAME.contains(empty),
            "every set holds the empty one"
        );
    }

    /// An app that says nothing owns nothing, so every app written before
    /// this const existed stays on the managed path.
    #[test]
    fn an_app_owns_nothing_by_default() {
        struct Quiet;
        impl MorphApp for Quiet {
            fn init(_ctx: &mut crate::MorphContext) -> Self {
                unreachable!("the const is read without an instance")
            }
        }
        assert_eq!(Quiet::OWNS, Owned::empty());
        assert!(!Quiet::OWNS.contains(Owned::FRAME));
    }
}
