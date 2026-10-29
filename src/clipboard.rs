//! The clipboard's text, read and written (plans
//! `20261028_03_CLIPBOARD_PLAN.md` and `20261029_01_CLIPBOARD_WRITE_PLAN.md`):
//! [`MorphContext::clipboard_text`](crate::MorphContext::clipboard_text),
//! [`clipboard_has_text`](crate::MorphContext::clipboard_has_text),
//! [`set_clipboard_text`](crate::MorphContext::set_clipboard_text) and
//! [`clipboard_can_write`](crate::MorphContext::clipboard_can_write) wrap
//! libmobile's `clipboard` on Android (feature `mobile-clipboard`) and
//! libdevice's on macOS. Other desktops and the web have none. Scripted
//! runs never touch the host's clipboard: a read gives nothing and a write
//! is only logged. libmorph never logs the text or its length.

use std::fmt;

use crate::MorphContext;

/// What happened after a copy the system accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipboardWrite {
    /// The system showed its own confirmation (Android 13 and later). The
    /// app shows none, so the user is not told twice.
    SystemNotice,
    /// Copied, and the system said nothing (macOS, Android 10 to 12L,
    /// scripted runs). The app may show its own short "Copied" notice.
    Silent,
}

/// Why the text was not copied.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipboardError {
    /// No clipboard write on this host: desktops other than macOS, the web,
    /// and Android built without `mobile-clipboard`.
    Unsupported,
    /// Anything the platform reported (never containing the text).
    Platform(String),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("writing the clipboard is not supported on this host"),
            Self::Platform(message) => write!(f, "writing the clipboard failed: {message}"),
        }
    }
}

impl std::error::Error for ClipboardError {}

/// The clipboard's text on this host; an empty text is `None`.
pub(crate) fn text(ctx: &MorphContext) -> Option<String> {
    if ctx.scripted {
        return None;
    }
    non_empty(platform::text(ctx))
}

/// Whether the clipboard holds text on this host, without Android's
/// "pasted" toast.
pub(crate) fn has_text(ctx: &MorphContext) -> bool {
    !ctx.scripted && platform::has_text(ctx)
}

/// Put `text` on the clipboard; scripted runs only log that they did.
pub(crate) fn write(ctx: &MorphContext, text: &str) -> Result<ClipboardWrite, ClipboardError> {
    if ctx.scripted {
        // Never the text or its length.
        log::info!("libmorph: script: clipboard written");
        return Ok(ClipboardWrite::Silent);
    }
    platform::write(ctx, text).inspect_err(|error| {
        // The reason only: never the text.
        log::debug!("libmorph: clipboard: {error}");
    })
}

/// Whether [`write`] can work on this host, decided at build time.
pub(crate) fn can_write(ctx: &MorphContext) -> bool {
    ctx.scripted || platform::CAN_WRITE
}

/// libmobile's `Copied { system_notice }` as libmorph's answer.
#[cfg_attr(
    not(all(target_os = "android", feature = "mobile-clipboard")),
    allow(dead_code)
)]
fn from_copied(system_notice: bool) -> ClipboardWrite {
    if system_notice {
        ClipboardWrite::SystemNotice
    } else {
        ClipboardWrite::Silent
    }
}

/// An empty or missing text is `None`, on every host.
fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.is_empty())
}

#[cfg(all(target_os = "android", feature = "mobile-clipboard"))]
mod platform {
    use super::{ClipboardError, ClipboardWrite};
    use crate::MorphContext;
    use libmobile::android::clipboard;

    pub(super) const CAN_WRITE: bool = true;

    /// libmobile's error, whatever its variant, as its `Display`.
    pub(super) fn write(ctx: &MorphContext, text: &str) -> Result<ClipboardWrite, ClipboardError> {
        let app = ctx
            .android
            .as_ref()
            .ok_or_else(|| ClipboardError::Platform("no activity".into()))?;
        clipboard::write_text(app, text)
            .map(|copied| super::from_copied(copied.system_notice))
            .map_err(|error| ClipboardError::Platform(error.to_string()))
    }

    pub(super) fn text(ctx: &MorphContext) -> Option<String> {
        let app = ctx.android.as_ref()?;
        clipboard::read_text(app).unwrap_or_else(|error| {
            // The reason only: never the text.
            log::debug!("libmorph: clipboard: {error}");
            None
        })
    }

    pub(super) fn has_text(ctx: &MorphContext) -> bool {
        let Some(app) = ctx.android.as_ref() else {
            return false;
        };
        clipboard::has_text(app).unwrap_or_else(|error| {
            log::debug!("libmorph: clipboard: {error}");
            false
        })
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{ClipboardError, ClipboardWrite};
    use crate::MorphContext;
    use libdevice::clipboard;

    pub(super) const CAN_WRITE: bool = true;

    /// macOS says nothing on a copy, so a write is always `Silent`.
    pub(super) fn write(_ctx: &MorphContext, text: &str) -> Result<ClipboardWrite, ClipboardError> {
        clipboard::write_text(text)
            .map(|()| ClipboardWrite::Silent)
            .map_err(|error| match error {
                clipboard::ClipboardError::Unsupported => ClipboardError::Unsupported,
                clipboard::ClipboardError::Platform(message) => ClipboardError::Platform(message),
                other => ClipboardError::Platform(other.to_string()),
            })
    }

    pub(super) fn text(_ctx: &MorphContext) -> Option<String> {
        clipboard::read_text()
    }

    /// macOS shows no toast, so the text itself is read.
    pub(super) fn has_text(ctx: &MorphContext) -> bool {
        super::non_empty(text(ctx)).is_some()
    }
}

#[cfg(not(any(
    target_os = "macos",
    all(target_os = "android", feature = "mobile-clipboard")
)))]
mod platform {
    use super::{ClipboardError, ClipboardWrite};
    use crate::MorphContext;

    pub(super) const CAN_WRITE: bool = false;

    pub(super) fn write(
        _ctx: &MorphContext,
        _text: &str,
    ) -> Result<ClipboardWrite, ClipboardError> {
        unsupported()
    }

    /// The answer on every host without a write, testable without a
    /// context (one needs a GPU).
    pub(super) fn unsupported() -> Result<ClipboardWrite, ClipboardError> {
        Err(ClipboardError::Unsupported)
    }

    pub(super) fn text(_ctx: &MorphContext) -> Option<String> {
        None
    }

    pub(super) fn has_text(_ctx: &MorphContext) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{ClipboardError, ClipboardWrite, from_copied, non_empty};

    #[test]
    fn empty_text_is_none() {
        assert_eq!(non_empty(None), None);
        assert_eq!(non_empty(Some(String::new())), None);
        assert_eq!(non_empty(Some("a".into())), Some("a".into()));
        assert_eq!(non_empty(Some("\n".into())), Some("\n".into()));
    }

    #[test]
    fn copied_maps_to_the_write() {
        assert_eq!(from_copied(true), ClipboardWrite::SystemNotice);
        assert_eq!(from_copied(false), ClipboardWrite::Silent);
    }

    #[test]
    fn errors_display_without_text() {
        assert_eq!(
            ClipboardError::Unsupported.to_string(),
            "writing the clipboard is not supported on this host"
        );
        assert_eq!(
            ClipboardError::Platform("no activity".into()).to_string(),
            "writing the clipboard failed: no activity"
        );
    }

    /// Desktops other than macOS, the web, and Android without
    /// `mobile-clipboard`.
    #[cfg(not(any(
        target_os = "macos",
        all(target_os = "android", feature = "mobile-clipboard")
    )))]
    #[test]
    fn other_hosts_cannot_write() {
        const { assert!(!super::platform::CAN_WRITE) };
        assert_eq!(
            super::platform::unsupported(),
            Err(ClipboardError::Unsupported)
        );
    }
}
