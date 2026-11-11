//! An intent, as plain data (plan `20261104_01_INTENT_EXTRAS_PLAN.md`):
//! what [`MorphApp::on_intent`](crate::MorphApp::on_intent) is handed.
//!
//! Android starts an activity with an intent, and an intent is more than the
//! URI a deep link puts in it: it has an action, and it may carry extras,
//! which are the way to hand a running app a key and a value.
//!
//! ```text
//! adb shell am start -n <pkg>/<activity> --es cmd refresh
//! ```
//!
//! arrives as an [`Intent`] whose `extra("cmd")` is `Some("refresh")`. That
//! intent's action is `MAIN`, not `VIEW`, and it has no data at all - so an
//! app that only hears about links never sees it.
//!
//! This is libmorph's own type rather than libmobile's `IntentData`, which
//! it is otherwise field for field, because a scripted run builds one on a
//! desktop where libmobile is not compiled in.

/// The action of an intent that opens a link.
#[cfg_attr(not(any(test, script_runner, target_os = "android")), allow(dead_code))]
pub(crate) const ACTION_VIEW: &str = "android.intent.action.VIEW";

/// An intent the app was started with, or that arrived while it ran.
///
/// Every field is untrusted input, as a URL is: validate it, and log a key
/// rather than its value, since a value may be a secret.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Intent {
    /// `Intent.getAction()`, such as `"android.intent.action.VIEW"` for a
    /// link or `"android.intent.action.MAIN"` for a launch. `None` for an
    /// intent with no action.
    pub action: Option<String>,
    /// `Intent.getDataString()`: the URI of a deep link, for an intent that
    /// carries one.
    pub data: Option<String>,
    /// Every extra, stringified: `--ei n 3` arrives as `("n", "3")`, the
    /// same as `--es n 3`, because the type is gone by the time the value
    /// crosses JNI. In Android's bundle iteration order, which Android does
    /// not define - read by key rather than by position.
    pub extras: Vec<(String, String)>,
    /// `false` for the intent the app was launched with, `true` for one
    /// delivered to an already-running instance (Android's `onNewIntent`,
    /// which needs `launchMode="singleTop"`; libmobile's generated activity
    /// declares it).
    pub is_new: bool,
}

impl Intent {
    /// The value of extra `key`, if it is there.
    ///
    /// The first, for a key that somehow appears twice: Android's own
    /// `getStringExtra` reads one value, and a bundle cannot hold two under
    /// one key, so a repeat can only come from a sender that built the list
    /// itself.
    #[must_use]
    pub fn extra(&self, key: &str) -> Option<&str> {
        self.extras
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The link this intent opens: its data, for a `VIEW` action.
    ///
    /// What decides whether [`MorphApp::on_open_url`](crate::MorphApp::on_open_url)
    /// is called as well, so the two hooks cannot disagree about which
    /// intents are links.
    #[cfg_attr(not(any(test, script_runner, target_os = "android")), allow(dead_code))]
    pub(crate) fn link(&self) -> Option<&str> {
        if self.action.as_deref() == Some(ACTION_VIEW) {
            self.data.as_deref()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Intent;

    fn with(extras: &[(&str, &str)]) -> Intent {
        Intent {
            extras: extras
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            ..Intent::default()
        }
    }

    #[test]
    fn an_extra_is_read_by_key() {
        let intent = with(&[("cmd", "refresh"), ("n", "3")]);
        assert_eq!(intent.extra("cmd"), Some("refresh"));
        assert_eq!(intent.extra("n"), Some("3"));
        assert_eq!(intent.extra("missing"), None);
    }

    /// A key that appears twice reads as the first, rather than panicking or
    /// picking by chance.
    #[test]
    fn a_repeated_key_reads_as_the_first() {
        assert_eq!(with(&[("k", "one"), ("k", "two")]).extra("k"), Some("one"));
    }

    /// Only a `VIEW` action with data is a link, so a launcher intent
    /// carrying extras does not reach `on_open_url`.
    #[test]
    fn only_a_view_action_with_data_is_a_link() {
        let view = Intent {
            action: Some("android.intent.action.VIEW".to_owned()),
            data: Some("app://x".to_owned()),
            ..Intent::default()
        };
        assert_eq!(view.link(), Some("app://x"));

        let main = Intent {
            action: Some("android.intent.action.MAIN".to_owned()),
            extras: vec![("scene".to_owned(), "demo".to_owned())],
            ..Intent::default()
        };
        assert_eq!(main.link(), None);

        let bare_view = Intent {
            action: Some("android.intent.action.VIEW".to_owned()),
            ..Intent::default()
        };
        assert_eq!(bare_view.link(), None);
    }
}
