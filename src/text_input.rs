//! Text input and the soft keyboard (plan `20261024_06_TEXT_INPUT_PLAN.md`).

/// Options for text input editing. [`Default`] is one line of text with
/// Done and autocorrect, as libmobile's.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TextInputOptions {
    /// Many lines: Enter is a newline unless `action` is not
    /// [`TextAction::None`].
    pub multiline: bool,
    pub kind: TextKind,
    pub action: TextAction,
    /// Autocorrect and suggestions on Android's keyboard; `false` turns
    /// both off. The desktop has neither.
    pub autocorrect: bool,
}

impl Default for TextInputOptions {
    fn default() -> Self {
        TextInputOptions {
            multiline: false,
            kind: TextKind::Text,
            action: TextAction::Done,
            autocorrect: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextKind {
    #[default]
    Text,
    Url,
    Email,
    Number,
    Password,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextAction {
    #[default]
    Done,
    Send,
    Go,
    Search,
    None,
}

/// libmorph's options as libmobile's, for `set_text_input_with`.
#[cfg(feature = "mobile")]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn to_mobile(options: &TextInputOptions) -> libmobile::TextInputOptions {
    use libmobile::TextKind as K;
    let mut mobile = libmobile::TextInputOptions::default();
    mobile.multiline = options.multiline;
    mobile.kind = match options.kind {
        TextKind::Text => K::Text,
        TextKind::Url => K::Url,
        TextKind::Email => K::Email,
        TextKind::Number => K::Number,
        TextKind::Password => K::Password,
    };
    mobile.action = action_to_mobile(options.action);
    mobile.autocorrect = options.autocorrect;
    mobile
}

#[cfg(feature = "mobile")]
fn action_to_mobile(action: TextAction) -> libmobile::TextAction {
    use libmobile::TextAction as A;
    match action {
        TextAction::Done => A::Done,
        TextAction::Send => A::Send,
        TextAction::Go => A::Go,
        TextAction::Search => A::Search,
        TextAction::None => A::None,
    }
}

/// libmobile's action key as libmorph's; `None` for an action libmobile
/// adds later, which is logged and not passed on.
#[cfg(feature = "mobile")]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn action_from_mobile(action: libmobile::TextAction) -> Option<TextAction> {
    use libmobile::TextAction as A;
    Some(match action {
        A::Done => TextAction::Done,
        A::Send => TextAction::Send,
        A::Go => TextAction::Go,
        A::Search => TextAction::Search,
        A::None => TextAction::None,
        _ => return None,
    })
}

/// What text editing hands the app.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextInputEvent {
    /// The whole text after an edit, composition included.
    Changed(String),
    /// The action key (Done, Send, Go or Search; Enter on a single line).
    Action(TextAction),
    /// The keyboard was closed by the user or the system while editing.
    Dismissed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_input_options_defaults() {
        let opts = TextInputOptions::default();
        assert!(!opts.multiline);
        assert_eq!(opts.kind, TextKind::Text);
        assert_eq!(opts.action, TextAction::Done);
        assert!(opts.autocorrect);
    }

    #[test]
    fn text_input_event_variants() {
        let ev = TextInputEvent::Changed("hello".into());
        assert_eq!(ev, TextInputEvent::Changed("hello".into()));
        let act = TextInputEvent::Action(TextAction::Send);
        assert_eq!(act, TextInputEvent::Action(TextAction::Send));
        let dis = TextInputEvent::Dismissed;
        assert_eq!(dis, TextInputEvent::Dismissed);
    }

    #[cfg(feature = "mobile")]
    #[test]
    fn options_map_to_libmobile_field_by_field() {
        let mut ours = TextInputOptions::default();
        let mobile = to_mobile(&ours);
        assert_eq!(mobile, libmobile::TextInputOptions::default());
        ours.multiline = true;
        ours.kind = TextKind::Url;
        ours.action = TextAction::None;
        ours.autocorrect = false;
        let mobile = to_mobile(&ours);
        assert!(mobile.multiline);
        assert_eq!(mobile.kind, libmobile::TextKind::Url);
        assert_eq!(mobile.action, libmobile::TextAction::None);
        assert!(!mobile.autocorrect);
        for (kind, theirs) in [
            (TextKind::Text, libmobile::TextKind::Text),
            (TextKind::Url, libmobile::TextKind::Url),
            (TextKind::Email, libmobile::TextKind::Email),
            (TextKind::Number, libmobile::TextKind::Number),
            (TextKind::Password, libmobile::TextKind::Password),
        ] {
            ours.kind = kind;
            assert_eq!(to_mobile(&ours).kind, theirs);
        }
    }

    #[cfg(feature = "mobile")]
    #[test]
    fn actions_map_one_to_one_both_ways() {
        for action in [
            TextAction::Done,
            TextAction::Send,
            TextAction::Go,
            TextAction::Search,
            TextAction::None,
        ] {
            assert_eq!(action_from_mobile(action_to_mobile(action)), Some(action));
        }
    }
}
