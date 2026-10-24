//! **A foreground service**, which keeps the app's own threads running
//! while it is in the background (plan `20261023_01_FOREGROUND_SERVICE_PLAN.md`).
//! The same API on every host: on Android it is libmobile's
//! `android::service` (feature `mobile-foreground-service`); elsewhere
//! [`start`] fails with [`ServiceError::Unsupported`] and the other calls do
//! nothing.
//!
//! No [`MorphContext`](crate::MorphContext) is needed: every call is
//! synchronous and may be made from any thread, such as the app's network
//! thread. What the service does arrives on the main thread in
//! [`MorphApp::on_service`](crate::MorphApp::on_service), and on any thread
//! through [`system_events`](crate::system_events).
//!
//! On Android, a started foreground service keeps the process from being
//! cached and frozen, so the app's threads keep running with the screen
//! off. It does not keep the process from being killed (memory pressure,
//! Force stop), and Android never restarts it: start it again the next time
//! the app is visible.
//!
//! * Declare it in the build script, with the types it may start with:
//!   `libmorph_build::gradle_project(id).foreground_service(&[ServiceType::SpecialUse("why")])`
//!   (the build script's `ServiceType` is `libmorph_build::gradle::ServiceType`).
//! * [`start`] only while the app is visible, for example in
//!   [`init`](crate::MorphApp::init) or [`on_resume`](crate::MorphApp::on_resume);
//!   Android refuses a start from the background
//!   ([`ServiceError::StartNotAllowed`]). Updating and stopping work at any
//!   time.
//! * On Android 13+, ask for `android.permission.POST_NOTIFICATIONS`
//!   ([`request_permissions`](crate::MorphContext::request_permissions))
//!   before starting. Without it the service runs, but its notification is
//!   hidden ([`ServiceEvent::NotificationHidden`]).
//!
//! The structs are `#[non_exhaustive]`: build them with `new` or `default`,
//! then set fields one by one:
//!
//! ```
//! use libmorph::service::{Controls, NotificationAction, NotificationContent, ServiceOptions, ServiceType};
//!
//! let mut content = NotificationContent::new("handlr", "connected");
//! content.actions.push(NotificationAction::new("ping", "Ping"));
//! let mut options = ServiceOptions::new(vec![ServiceType::SpecialUse("a link")], content);
//! let mut controls = Controls::default();
//! controls.ongoing = true;
//! controls.exit = true;
//! controls.wake_lock_toggle = true;
//! options.controls = controls;
//! // Unsupported off Android.
//! let _ = libmorph::service::start(&options);
//! ```

use std::fmt;

/// Android shows at most this many buttons on a notification, the built-in
/// controls' included.
pub const MAX_ACTIONS: usize = 3;

/// A type the service may be started with. Only the kind matters at start;
/// a [`SpecialUse`](Self::SpecialUse) reason is read from the manifest the
/// build script generated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceType {
    /// `dataSync`: limited to 6 hours in 24 from Android 15.
    DataSync,
    /// `microphone`: needs `RECORD_AUDIO` granted at start.
    Microphone,
    /// `specialUse`, with the reason stated for Play review.
    SpecialUse(&'static str),
    /// `remoteMessaging`.
    RemoteMessaging,
}

/// What the notification shows. Tapping its body opens the app.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct NotificationContent {
    pub title: String,
    pub text: String,
    /// The app's own buttons: with the built-in controls, at most
    /// [`MAX_ACTIONS`] in all ([`ServiceError::TooManyActions`]).
    pub actions: Vec<NotificationAction>,
    /// On the lock screen. Default [`LockScreenVisibility::Private`].
    pub visibility: LockScreenVisibility,
}

impl NotificationContent {
    /// A title and text, with no buttons and the default visibility.
    pub fn new(title: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            text: text.into(),
            ..Self::default()
        }
    }
}

/// One of the app's own notification buttons. A tap arrives as
/// [`ServiceEvent::Action`] with its id.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct NotificationAction {
    /// The app's own id. Not empty, and not starting with `libmobile.`,
    /// which the built-in controls use ([`ServiceError::BadActionId`]).
    pub id: String,
    pub label: String,
    /// Also bring the app forward on a tap.
    pub opens_app: bool,
}

impl NotificationAction {
    /// A button that does not open the app.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            opens_app: false,
        }
    }
}

/// How much of the notification shows on the lock screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum LockScreenVisibility {
    /// All of it.
    Public,
    /// Only that there is one, until the device is unlocked.
    #[default]
    Private,
    /// None of it.
    Secret,
}

/// How the service starts.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ServiceOptions {
    /// A subset of the types the build script declared.
    pub types: Vec<ServiceType>,
    pub content: NotificationContent,
    pub controls: Controls,
    /// Hold a partial wake lock from the start.
    pub wake_lock: bool,
}

impl ServiceOptions {
    /// `types` and `content`, with no built-in controls and no wake lock.
    pub fn new(types: Vec<ServiceType>, content: NotificationContent) -> Self {
        Self {
            types,
            content,
            controls: Controls::default(),
            wake_lock: false,
        }
    }
}

/// The notification's built-in controls, in the manner of Termux. All are
/// off by default.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Controls {
    /// The notification cannot be swiped away: from Android 14 it is posted
    /// again when dismissed, so leaving is only through Exit or [`stop`].
    pub ongoing: bool,
    /// An Exit button: it releases the wake lock, stops the service,
    /// delivers [`ServiceEvent::Exited`] and closes the app's activity. The
    /// process is not killed; the app's threads end themselves.
    pub exit: bool,
    /// A button that acquires or releases the wake lock, delivering
    /// [`ServiceEvent::WakeLock`].
    pub wake_lock_toggle: bool,
    /// The controls' text.
    pub labels: ControlLabels,
}

/// The built-in controls' text.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ControlLabels {
    /// Default "Exit".
    pub exit: String,
    /// The wake-lock button while none is held. Default "Acquire wake lock".
    pub acquire_wake_lock: String,
    /// The wake-lock button while one is held. Default "Release wake lock".
    pub release_wake_lock: String,
    /// After the text while the wake lock is held. Default
    /// " (wake lock held)".
    pub wake_lock_held: String,
}

impl Default for ControlLabels {
    fn default() -> Self {
        Self {
            exit: "Exit".into(),
            acquire_wake_lock: "Acquire wake lock".into(),
            release_wake_lock: "Release wake lock".into(),
            wake_lock_held: " (wake lock held)".into(),
        }
    }
}

/// Why a service call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceError {
    /// The build script did not declare the service, or these types; on
    /// Android also a build without `mobile-foreground-service`.
    NotDeclared,
    /// Started while the app was not visible.
    StartNotAllowed,
    /// A permission the start needs, for example `RECORD_AUDIO` for
    /// [`ServiceType::Microphone`].
    MissingPermission(String),
    /// An update, the wake lock or an action while stopped.
    NotRunning,
    /// More than [`MAX_ACTIONS`] buttons with the built-in controls.
    TooManyActions,
    /// An action id that is empty or starts with `libmobile.`.
    BadActionId,
    /// No foreground service on this host (anything but Android).
    Unsupported,
    /// Anything else the platform reported.
    Platform(String),
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDeclared => f.write_str("the foreground service or this type is not declared"),
            Self::StartNotAllowed => {
                f.write_str("the service may only be started while the app is visible")
            }
            Self::MissingPermission(name) => write!(f, "permission {name} not granted"),
            Self::NotRunning => f.write_str("the service is not running"),
            Self::TooManyActions => write!(f, "more than {MAX_ACTIONS} notification buttons"),
            Self::BadActionId => f.write_str("an action id is empty or starts with `libmobile.`"),
            Self::Unsupported => f.write_str("no foreground service on this host"),
            Self::Platform(detail) => write!(f, "service: {detail}"),
        }
    }
}

impl std::error::Error for ServiceError {}

/// Something the service did, or a tap on its notification.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceEvent {
    /// In the foreground with its notification.
    Started,
    /// Stopped.
    Stopped,
    /// It could not go into the foreground, and has stopped.
    Failed(String),
    /// A type's time limit was reached (`dataSync`); it has stopped.
    TimedOut,
    /// It runs, but notifications are off for the app, so its notification
    /// and buttons show only in the system's running-apps list.
    NotificationHidden,
    /// The app's own button, by id.
    Action(String),
    /// The Exit button: the service has stopped and the app's activity is
    /// closing. The app's threads should end.
    Exited,
    /// The wake lock is now held, or not.
    WakeLock(bool),
    /// The app was swiped away from Recents while the build script asked
    /// the service to keep running; it still runs.
    TaskRemoved,
}

/// Start the service, or restart it with new options if it runs. While the
/// app is visible only. A failure inside the service after this returns
/// arrives as [`ServiceEvent::Failed`]. [`ServiceError::Unsupported`] off
/// Android.
pub fn start(options: &ServiceOptions) -> Result<(), ServiceError> {
    imp::start(options)
}

/// Replace the notification's content. Does nothing off Android.
pub fn update(content: &NotificationContent) -> Result<(), ServiceError> {
    imp::update(content)
}

/// Replace only the notification's text. Does nothing off Android.
pub fn update_text(text: &str) -> Result<(), ServiceError> {
    imp::update_text(text)
}

/// Stop the service, release the wake lock and remove the notification.
/// Nothing happens while stopped, or off Android.
pub fn stop() -> Result<(), ServiceError> {
    imp::stop()
}

/// Whether the service has been started and not stopped since. `false` off
/// Android.
#[must_use]
pub fn is_running() -> bool {
    imp::is_running()
}

/// Acquire or release the service's partial wake lock, as the wake-lock
/// button does. Does nothing off Android.
pub fn set_wake_lock(held: bool) -> Result<(), ServiceError> {
    imp::set_wake_lock(held)
}

/// Whether the service holds its wake lock. `false` off Android.
#[must_use]
pub fn wake_lock_held() -> bool {
    imp::wake_lock_held()
}

#[cfg(all(target_os = "android", feature = "mobile-foreground-service"))]
mod imp {
    use super::{NotificationContent, ServiceError, ServiceOptions};
    use libmobile::android::service as mobile;

    pub(super) fn start(options: &ServiceOptions) -> Result<(), ServiceError> {
        Ok(mobile::start(&options.into())?)
    }

    pub(super) fn update(content: &NotificationContent) -> Result<(), ServiceError> {
        Ok(mobile::update(&content.into())?)
    }

    pub(super) fn update_text(text: &str) -> Result<(), ServiceError> {
        Ok(mobile::update_text(text)?)
    }

    pub(super) fn stop() -> Result<(), ServiceError> {
        Ok(mobile::stop()?)
    }

    pub(super) fn is_running() -> bool {
        mobile::is_running()
    }

    pub(super) fn set_wake_lock(held: bool) -> Result<(), ServiceError> {
        Ok(mobile::set_wake_lock(held)?)
    }

    pub(super) fn wake_lock_held() -> bool {
        mobile::wake_lock_held()
    }
}

/// Anywhere without libmobile's service: no service, and nothing to do.
#[cfg(not(all(target_os = "android", feature = "mobile-foreground-service")))]
mod imp {
    use super::{NotificationContent, ServiceError, ServiceOptions};

    pub(super) fn start(_options: &ServiceOptions) -> Result<(), ServiceError> {
        if cfg!(target_os = "android") {
            Err(ServiceError::NotDeclared)
        } else {
            Err(ServiceError::Unsupported)
        }
    }

    pub(super) fn update(_content: &NotificationContent) -> Result<(), ServiceError> {
        Ok(())
    }

    pub(super) fn update_text(_text: &str) -> Result<(), ServiceError> {
        Ok(())
    }

    pub(super) fn stop() -> Result<(), ServiceError> {
        Ok(())
    }

    pub(super) fn is_running() -> bool {
        false
    }

    pub(super) fn set_wake_lock(_held: bool) -> Result<(), ServiceError> {
        Ok(())
    }

    pub(super) fn wake_lock_held() -> bool {
        false
    }
}

#[cfg(all(target_os = "android", feature = "mobile-foreground-service"))]
mod from_mobile {
    use super::{
        ControlLabels, Controls, LockScreenVisibility, NotificationAction, NotificationContent,
        ServiceError, ServiceOptions, ServiceType,
    };
    use libmobile::android::service as mobile;

    impl From<ServiceType> for mobile::ServiceType {
        fn from(ty: ServiceType) -> Self {
            match ty {
                ServiceType::DataSync => Self::DataSync,
                ServiceType::Microphone => Self::Microphone,
                ServiceType::SpecialUse(reason) => Self::SpecialUse(reason),
                ServiceType::RemoteMessaging => Self::RemoteMessaging,
            }
        }
    }

    impl From<LockScreenVisibility> for mobile::LockScreenVisibility {
        fn from(visibility: LockScreenVisibility) -> Self {
            match visibility {
                LockScreenVisibility::Public => Self::Public,
                LockScreenVisibility::Private => Self::Private,
                LockScreenVisibility::Secret => Self::Secret,
            }
        }
    }

    impl From<&NotificationAction> for mobile::NotificationAction {
        fn from(action: &NotificationAction) -> Self {
            let mut out = Self::new(action.id.clone(), action.label.clone());
            out.opens_app = action.opens_app;
            out
        }
    }

    impl From<&NotificationContent> for mobile::NotificationContent {
        fn from(content: &NotificationContent) -> Self {
            let mut out = Self::new(content.title.clone(), content.text.clone());
            out.actions = content.actions.iter().map(Into::into).collect();
            out.visibility = content.visibility.into();
            out
        }
    }

    impl From<&ControlLabels> for mobile::ControlLabels {
        fn from(labels: &ControlLabels) -> Self {
            let mut out = Self::default();
            out.exit.clone_from(&labels.exit);
            out.acquire_wake_lock.clone_from(&labels.acquire_wake_lock);
            out.release_wake_lock.clone_from(&labels.release_wake_lock);
            out.wake_lock_held.clone_from(&labels.wake_lock_held);
            out
        }
    }

    impl From<&Controls> for mobile::Controls {
        fn from(controls: &Controls) -> Self {
            let mut out = Self::default();
            out.ongoing = controls.ongoing;
            out.exit = controls.exit;
            out.wake_lock_toggle = controls.wake_lock_toggle;
            out.labels = (&controls.labels).into();
            out
        }
    }

    impl From<&ServiceOptions> for mobile::ServiceOptions {
        fn from(options: &ServiceOptions) -> Self {
            let types = options.types.iter().map(|&ty| ty.into()).collect();
            let mut out = Self::new(types, (&options.content).into());
            out.controls = (&options.controls).into();
            out.wake_lock = options.wake_lock;
            out
        }
    }

    impl From<mobile::ServiceError> for ServiceError {
        fn from(error: mobile::ServiceError) -> Self {
            use mobile::ServiceError as E;
            match error {
                E::NotDeclared => Self::NotDeclared,
                E::StartNotAllowed => Self::StartNotAllowed,
                E::MissingPermission(name) => Self::MissingPermission(name),
                E::NotRunning => Self::NotRunning,
                E::TooManyActions => Self::TooManyActions,
                E::BadActionId => Self::BadActionId,
                E::Platform(detail) => Self::Platform(detail),
                other => Self::Platform(other.to_string()),
            }
        }
    }
}

/// libmobile's service event, where libmobile has the type; `None` for a
/// variant libmorph does not know.
#[cfg(all(
    target_os = "android",
    any(
        feature = "mobile-foreground-service",
        feature = "mobile-system-events"
    )
))]
pub(crate) fn from_mobile_event(
    event: libmobile::system_events::ServiceEvent,
) -> Option<ServiceEvent> {
    use libmobile::system_events::ServiceEvent as E;
    Some(match event {
        E::Started => ServiceEvent::Started,
        E::Stopped => ServiceEvent::Stopped,
        E::Failed(detail) => ServiceEvent::Failed(detail),
        E::TimedOut => ServiceEvent::TimedOut,
        E::NotificationHidden => ServiceEvent::NotificationHidden,
        E::Action(id) => ServiceEvent::Action(id),
        E::Exited => ServiceEvent::Exited,
        E::WakeLock(held) => ServiceEvent::WakeLock(held),
        E::TaskRemoved => ServiceEvent::TaskRemoved,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_libmobile() {
        let options = ServiceOptions::new(vec![], NotificationContent::new("t", "x"));
        assert!(!options.wake_lock);
        assert!(!options.controls.ongoing && !options.controls.exit);
        assert!(!options.controls.wake_lock_toggle);
        assert_eq!(options.content.visibility, LockScreenVisibility::Private);
        assert_eq!(options.controls.labels.exit, "Exit");
        assert_eq!(options.controls.labels.wake_lock_held, " (wake lock held)");
        assert!(!NotificationAction::new("ping", "Ping").opens_app);
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn unsupported_off_android() {
        let options = ServiceOptions::new(
            vec![ServiceType::SpecialUse("r")],
            NotificationContent::new("t", "x"),
        );
        assert_eq!(start(&options), Err(ServiceError::Unsupported));
        assert!(!is_running());
        assert!(!wake_lock_held());
        assert_eq!(update(&options.content), Ok(()));
        assert_eq!(update_text("y"), Ok(()));
        assert_eq!(set_wake_lock(true), Ok(()));
        assert_eq!(stop(), Ok(()));
        assert!(!wake_lock_held());
    }
}
