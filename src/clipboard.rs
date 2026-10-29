//! The clipboard's text (plan `20261028_03_CLIPBOARD_PLAN.md`):
//! [`MorphContext::clipboard_text`](crate::MorphContext::clipboard_text)
//! and [`clipboard_has_text`](crate::MorphContext::clipboard_has_text) wrap
//! libmobile's `clipboard` on Android (feature `mobile-clipboard`) and
//! libdevice's on macOS. Other desktops, the web and scripted runs have
//! none. Read only; libmorph never logs the text or its length.

use crate::MorphContext;

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

/// An empty or missing text is `None`, on every host.
fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.is_empty())
}

#[cfg(all(target_os = "android", feature = "mobile-clipboard"))]
mod platform {
    use crate::MorphContext;
    use libmobile::android::clipboard;

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
    use crate::MorphContext;

    pub(super) fn text(_ctx: &MorphContext) -> Option<String> {
        libdevice::clipboard::read_text()
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
    use crate::MorphContext;

    pub(super) fn text(_ctx: &MorphContext) -> Option<String> {
        None
    }

    pub(super) fn has_text(_ctx: &MorphContext) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::non_empty;

    #[test]
    fn empty_text_is_none() {
        assert_eq!(non_empty(None), None);
        assert_eq!(non_empty(Some(String::new())), None);
        assert_eq!(non_empty(Some("a".into())), Some("a".into()));
        assert_eq!(non_empty(Some("\n".into())), Some("\n".into()));
    }
}
