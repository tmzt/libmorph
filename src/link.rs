//! Opening a link in another app (plan `20261023_02_OPEN_LINK_PLAN.md`):
//! [`MorphContext::open_link`](crate::MorphContext::open_link) wraps
//! libmobile on Android and libdevice on the Mac; the web and scripted
//! runs are libmorph's own.

use std::fmt;

/// Why a link was not opened.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenLinkError {
    /// Not an absolute http or https URL with a host, or it holds
    /// whitespace or control characters.
    BadUrl,
    /// No app on the system opens it.
    NoHandler,
    /// Android: the app is hidden, and Android does not let it start
    /// another app.
    NotVisible,
    /// No opener on this host (desktops other than macOS).
    Unsupported,
    /// Anything else the platform reported; on the web, `"blocked"` for a
    /// blocked pop-up.
    Platform(String),
}

impl fmt::Display for OpenLinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadUrl => f.write_str("not an http or https link"),
            Self::NoHandler => f.write_str("no app opens this link"),
            Self::NotVisible => f.write_str("a link may only be opened while the app is visible"),
            Self::Unsupported => f.write_str("opening links is not supported on this host"),
            Self::Platform(message) => write!(f, "opening the link failed: {message}"),
        }
    }
}

impl std::error::Error for OpenLinkError {}

/// The check every host makes first, the same as libmobile's and
/// libdevice's `check_link` (a test holds them to it): the scheme is `http`
/// or `https` (any case), followed by `://` and a non-empty host, and the
/// link holds no whitespace or control characters.
pub(crate) fn check_link(url: &str) -> Result<(), OpenLinkError> {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(OpenLinkError::BadUrl);
    }
    let (scheme, rest) = url.split_once("://").ok_or(OpenLinkError::BadUrl)?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(OpenLinkError::BadUrl);
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_and_port = authority.rsplit('@').next().unwrap_or_default();
    let host = match host_and_port.strip_prefix('[') {
        // An IPv6 literal: everything up to the closing bracket.
        Some(literal) => literal.split_once(']').ok_or(OpenLinkError::BadUrl)?.0,
        None => host_and_port.split(':').next().unwrap_or_default(),
    };
    if host.is_empty() {
        return Err(OpenLinkError::BadUrl);
    }
    Ok(())
}

/// `url` cut after its path, for logs: a query or fragment may be a
/// secret.
pub(crate) fn redacted(url: &str) -> String {
    match url.find(['?', '#']) {
        Some(end) => format!("{}<redacted>", &url[..end]),
        None => url.to_owned(),
    }
}

/// Hand a checked link to the host's opener.
pub(crate) fn open(url: &str) -> Result<(), OpenLinkError> {
    check_link(url)?;
    platform::open(url)
}

#[cfg(all(target_os = "android", feature = "mobile"))]
mod platform {
    use super::OpenLinkError;
    use libmobile::android::link::OpenLinkError as E;

    pub(super) fn open(url: &str) -> Result<(), OpenLinkError> {
        libmobile::android::link::open_link(url).map_err(from_mobile)
    }

    /// libmobile's error in libmorph's terms (also for `open_app_settings`).
    pub(crate) fn from_mobile(error: E) -> OpenLinkError {
        match error {
            E::BadUrl => OpenLinkError::BadUrl,
            E::NoHandler => OpenLinkError::NoHandler,
            E::NotVisible => OpenLinkError::NotVisible,
            E::Platform(message) => OpenLinkError::Platform(message),
            other => OpenLinkError::Platform(other.to_string()),
        }
    }
}

#[cfg(all(target_os = "android", feature = "mobile"))]
pub(crate) use platform::from_mobile;

#[cfg(target_os = "macos")]
mod platform {
    use super::OpenLinkError;
    use libdevice::link::OpenLinkError as E;

    pub(super) fn open(url: &str) -> Result<(), OpenLinkError> {
        libdevice::link::open_link(url).map_err(|error| match error {
            E::BadUrl => OpenLinkError::BadUrl,
            E::NoHandler => OpenLinkError::NoHandler,
            E::Unsupported => OpenLinkError::Unsupported,
            E::Platform(message) => OpenLinkError::Platform(message),
            other => OpenLinkError::Platform(other.to_string()),
        })
    }
}

/// A new tab, with the opener cut off. `noopener` in the features string
/// would make `window.open` return null even on success, so a blocked
/// pop-up could not be told apart; the returned window's `opener` is set
/// to null instead.
#[cfg(target_arch = "wasm32")]
mod platform {
    use super::OpenLinkError;

    pub(super) fn open(url: &str) -> Result<(), OpenLinkError> {
        let window =
            web_sys::window().ok_or_else(|| OpenLinkError::Platform("no window".into()))?;
        match window.open_with_url_and_target(url, "_blank") {
            Ok(Some(tab)) => {
                let _ = tab.set_opener(&wasm_bindgen::JsValue::NULL);
                Ok(())
            }
            Ok(None) => Err(OpenLinkError::Platform("blocked".into())),
            Err(error) => Err(OpenLinkError::Platform(format!("{error:?}"))),
        }
    }
}

#[cfg(not(any(
    all(target_os = "android", feature = "mobile"),
    target_os = "macos",
    target_arch = "wasm32"
)))]
mod platform {
    use super::OpenLinkError;

    pub(super) fn open(_url: &str) -> Result<(), OpenLinkError> {
        Err(OpenLinkError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenLinkError, check_link, redacted};

    /// Links every `check_link` must accept.
    const GOOD: &[&str] = &[
        "http://example.com",
        "https://example.com/",
        "https://example.com/a?k=v#f",
        "http://host:8088/#/src--handlr/docs/design/plan.md",
        "http://user:pass@host/",
        "http://[::1]:8080/",
        "https://h?q",
        "https://h#f",
        "HTTP://example.com/",
        "HtTpS://example.com/",
    ];

    /// Links every `check_link` must reject.
    const BAD: &[&str] = &[
        "",
        "http://",
        "https:///path",
        "http://?q",
        "http://#f",
        "http://:8080/",
        "http://user@/",
        "http://[]/",
        "http://[::1/",
        "http:example.com",
        "https:/example.com",
        "example.com",
        "https://example.com/a b",
        " https://example.com/",
        "https://example.com/\t",
        "https://example.com/\n",
        "https://example.com/\u{a0}",
        "https://example.com/\0",
        "https://example.com/\u{7f}",
        "https://exa\u{85}mple.com/",
        "file:///etc/passwd",
        "content://com.example.provider/x",
        "intent://scan/#Intent;scheme=zxing;end",
        "javascript:alert(1)",
        "javascript://example.com/%0aalert(1)",
        "mailto:someone@example.com",
        "tel:+15555550100",
        "ftp://example.com/",
        "httpx://example.com/",
        "://example.com",
    ];

    #[test]
    fn libmorphs_check_accepts_and_rejects_the_shared_list() {
        for url in GOOD {
            assert_eq!(check_link(url), Ok(()), "{url:?}");
        }
        for url in BAD {
            assert_eq!(check_link(url), Err(OpenLinkError::BadUrl), "{url:?}");
        }
    }

    /// libmobile's (Android) and libdevice's (macOS) checks agree with each
    /// other, and with libmorph's, on the shared list.
    #[cfg(all(feature = "mobile", target_os = "macos"))]
    #[test]
    fn libmobile_and_libdevice_agree() {
        for url in GOOD.iter().chain(BAD) {
            let mobile = libmobile::link::check_link(url).is_ok();
            let device = libdevice::link::check_link(url).is_ok();
            assert_eq!(mobile, device, "libmobile and libdevice differ on {url:?}");
            assert_eq!(
                check_link(url).is_ok(),
                mobile,
                "libmorph differs on {url:?}"
            );
        }
    }

    /// Without libmobile, libdevice's check still agrees with libmorph's.
    #[cfg(target_os = "macos")]
    #[test]
    fn libdevice_agrees() {
        for url in GOOD.iter().chain(BAD) {
            assert_eq!(
                libdevice::link::check_link(url).is_ok(),
                check_link(url).is_ok(),
                "{url:?}"
            );
        }
    }

    #[test]
    fn logs_stop_at_the_path() {
        assert_eq!(redacted("https://h/p?token=1"), "https://h/p<redacted>");
        assert_eq!(redacted("https://h/p#secret"), "https://h/p<redacted>");
        assert_eq!(redacted("https://h/p"), "https://h/p");
    }
}
