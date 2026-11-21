//! The clipboard palette's actions panel: what can be done with the selected copy, the keys
//! that do it without the panel, and which actions a search in the panel leaves.

/// Something to do with a copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Paste,
    Copy,
    Pin,
    Unpin,
    Delete,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Paste => "Paste",
            Action::Copy => "Copy to Clipboard",
            Action::Pin => "Pin Entry",
            Action::Unpin => "Unpin Entry",
            Action::Delete => "Delete Entry",
        }
    }

    /// The keys that do it from the list, as the panel shows them.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Action::Paste => &["\u{21b5}"],
            Action::Copy => &["Ctrl", "\u{21b5}"],
            Action::Pin | Action::Unpin => &["Ctrl", "P"],
            Action::Delete => &["Del"],
        }
    }

    /// Whether it can't be undone, so the panel sets it apart.
    pub fn destructive(self) -> bool {
        match self {
            Action::Delete => true,
            Action::Paste | Action::Copy | Action::Pin | Action::Unpin => false,
        }
    }
}

/// The actions for a copy, `pinned` or not, that every word of `query` finds in the label,
/// ignoring case.
pub fn actions(pinned: bool, query: &str) -> Vec<Action> {
    let pin = if pinned { Action::Unpin } else { Action::Pin };
    let query = query.to_lowercase();
    [Action::Paste, Action::Copy, pin, Action::Delete]
        .into_iter()
        .filter(|action| {
            let label = action.label().to_lowercase();
            query.split_whitespace().all(|word| label.contains(word))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pinned_copy_offers_unpin_and_a_search_narrows_the_list() {
        assert_eq!(
            actions(false, ""),
            [Action::Paste, Action::Copy, Action::Pin, Action::Delete]
        );
        assert_eq!(actions(true, "")[2], Action::Unpin);
        assert_eq!(actions(true, "PIN"), [Action::Unpin]);
        assert_eq!(actions(false, "entry"), [Action::Pin, Action::Delete]);
        assert_eq!(actions(false, "clip copy"), [Action::Copy]);
        assert!(actions(false, "rename").is_empty());
    }
}
