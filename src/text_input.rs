//! Text input and the soft keyboard (plan `20261024_06_TEXT_INPUT_PLAN.md`).

/// Options for text input editing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TextInputOptions {
    pub multiline: bool,
    pub kind: TextKind,
    pub action: TextAction,
    pub autocorrect: bool,
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
        assert!(!opts.autocorrect);
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
}
