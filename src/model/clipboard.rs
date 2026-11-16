//! Clipboard history: what was copied, newest first, and how long it is kept. Time is Unix
//! seconds, passed in.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::clip_kind::{Kind, kind_of};

/// Text longer than this isn't kept (a whole log file copied by accident).
pub const MAX_TEXT: usize = 1 << 20;
/// The most entries kept; past it the oldest unpinned go, whatever the retention.
pub const MAX_ENTRIES: usize = 2000;
/// Seconds within which the same thing copied again is one copy announced twice.
const ECHO: u64 = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Content {
    Text(String),
    Files(Vec<PathBuf>),
    /// The pixels are kept beside the history as `<id>.png`; `hash` tells copies apart.
    Image {
        width: u32,
        height: u32,
        hash: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: u64,
    pub content: Content,
    /// The exe of the app that copied it, when Windows could say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<PathBuf>,
    /// When it was (last) copied.
    pub copied: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

impl Content {
    pub fn kind(&self) -> Kind {
        match self {
            Content::Text(text) => kind_of(text),
            Content::Files(_) => Kind::Files,
            Content::Image { .. } => Kind::Image,
        }
    }
}

/// What a change to the history did, for the store to mirror on disk.
#[derive(Debug, PartialEq, Eq)]
pub enum Added {
    /// A new entry, at the top.
    New(u64),
    /// Copied again: the old entry moves to the top with the new time.
    Bumped(u64),
    /// Nothing to keep (empty, too long).
    Skipped,
}

#[derive(Debug, Default)]
pub struct History {
    /// Newest first.
    entries: Vec<Entry>,
    next_id: u64,
}

impl History {
    /// Back from disk, in any order.
    pub fn restore(mut entries: Vec<Entry>) -> Self {
        entries.sort_by(|a, b| b.copied.cmp(&a.copied).then(b.id.cmp(&a.id)));
        let next_id = entries.iter().map(|e| e.id + 1).max().unwrap_or(1);
        Self { entries, next_id }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn add(&mut self, content: Content, app: Option<PathBuf>, now: u64) -> Added {
        let keep = match &content {
            Content::Text(text) => !text.trim().is_empty() && text.len() <= MAX_TEXT,
            Content::Files(files) => !files.is_empty(),
            Content::Image { width, height, .. } => *width > 0 && *height > 0,
        };
        if !keep {
            return Added::Skipped;
        }
        if let Some(at) = self.entries.iter().position(|e| e.content == content) {
            if at == 0 && now.saturating_sub(self.entries[0].copied) < ECHO {
                return Added::Skipped;
            }
            let mut entry = self.entries.remove(at);
            entry.copied = now;
            entry.app = app.or(entry.app);
            let id = entry.id;
            self.entries.insert(0, entry);
            return Added::Bumped(id);
        }
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.entries.insert(
            0,
            Entry {
                id,
                content,
                app,
                copied: now,
                pinned: false,
            },
        );
        Added::New(id)
    }

    /// Drop unpinned entries older than `keep` seconds (`None`: kept forever), and the oldest
    /// unpinned past `MAX_ENTRIES`. The ids dropped.
    pub fn prune(&mut self, now: u64, keep: Option<u64>) -> Vec<u64> {
        let mut unpinned = 0;
        let mut dropped = Vec::new();
        self.entries.retain(|e| {
            if e.pinned {
                return true;
            }
            unpinned += 1;
            let expired = keep.is_some_and(|keep| now.saturating_sub(e.copied) > keep);
            let over = unpinned > MAX_ENTRIES;
            if expired || over {
                dropped.push(e.id);
            }
            !(expired || over)
        });
        dropped
    }

    /// When the next unpinned entry expires, so one timer can be set for it.
    pub fn next_expiry(&self, keep: Option<u64>) -> Option<u64> {
        let keep = keep?;
        let oldest = self
            .entries
            .iter()
            .filter(|e| !e.pinned)
            .map(|e| e.copied)
            .min()?;
        Some(oldest + keep + 1)
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(e) if e.pinned != pinned => {
                e.pinned = pinned;
                true
            }
            _ => false,
        }
    }

    /// Everything but the pinned entries. The ids dropped.
    pub fn clear(&mut self) -> Vec<u64> {
        let dropped = self
            .entries
            .iter()
            .filter(|e| !e.pinned)
            .map(|e| e.id)
            .collect();
        self.entries.retain(|e| e.pinned);
        dropped
    }
}

/// A cheap fingerprint of an image's pixels (FNV-1a), to spot the same image copied again.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Content {
        Content::Text(s.into())
    }

    #[test]
    fn copies_stack_newest_first_and_a_repeat_moves_up() {
        let mut h = History::default();
        assert_eq!(h.add(text("a"), None, 10), Added::New(1));
        assert_eq!(h.add(text("b"), None, 20), Added::New(2));
        assert_eq!(h.add(text("a"), Some("x.exe".into()), 30), Added::Bumped(1));
        let order: Vec<_> = h.entries().iter().map(|e| (e.id, e.copied)).collect();
        assert_eq!(order, [(1, 30), (2, 20)]);
        assert_eq!(h.get(1).unwrap().app, Some("x.exe".into()));
        // Apps often announce one copy twice; the echo is not a new copy.
        assert_eq!(h.add(text("a"), None, 31), Added::Skipped);
        assert_eq!(h.add(text("a"), None, 40), Added::Bumped(1));
    }

    #[test]
    fn blank_and_huge_text_and_empty_copies_are_not_kept() {
        let mut h = History::default();
        assert_eq!(h.add(text("  \n"), None, 1), Added::Skipped);
        assert_eq!(
            h.add(text(&"x".repeat(MAX_TEXT + 1)), None, 1),
            Added::Skipped
        );
        assert_eq!(h.add(Content::Files(vec![]), None, 1), Added::Skipped);
        assert!(h.entries().is_empty());
    }

    #[test]
    fn old_entries_expire_unless_pinned() {
        let day = 86_400;
        let mut h = History::default();
        h.add(text("old"), None, 0);
        h.add(text("pinned"), None, 0);
        h.add(text("new"), None, 6 * day);
        h.set_pinned(2, true);
        assert_eq!(h.next_expiry(Some(7 * day)), Some(7 * day + 1));
        assert_eq!(h.prune(7 * day, Some(7 * day)), Vec::<u64>::new());
        assert_eq!(h.prune(7 * day + 1, Some(7 * day)), vec![1]);
        assert_eq!(h.prune(100 * day, None), Vec::<u64>::new());
        let left: Vec<_> = h.entries().iter().map(|e| e.id).collect();
        assert_eq!(left, [3, 2]);
        assert_eq!(h.next_expiry(None), None);
    }

    #[test]
    fn past_the_cap_the_oldest_unpinned_go() {
        let mut h = History::default();
        for i in 0..MAX_ENTRIES as u64 + 3 {
            h.add(text(&i.to_string()), None, i);
        }
        h.set_pinned(1, true);
        let dropped = h.prune(MAX_ENTRIES as u64 + 3, None);
        assert_eq!(dropped, [3, 2]);
        assert_eq!(h.entries().len(), MAX_ENTRIES + 1);
    }

    #[test]
    fn clear_keeps_pins_and_restore_keeps_order_and_ids() {
        let mut h = History::default();
        h.add(text("a"), None, 1);
        h.add(text("b"), None, 2);
        h.set_pinned(1, true);
        assert_eq!(h.clear(), vec![2]);
        let mut back = History::restore(vec![
            Entry {
                id: 4,
                content: text("x"),
                app: None,
                copied: 5,
                pinned: false,
            },
            Entry {
                id: 9,
                content: text("y"),
                app: None,
                copied: 7,
                pinned: false,
            },
        ]);
        assert_eq!(back.entries()[0].id, 9);
        assert_eq!(back.add(text("z"), None, 8), Added::New(10));
    }

    #[test]
    fn entries_survive_the_save_format() {
        let e = Entry {
            id: 3,
            content: Content::Image {
                width: 4,
                height: 2,
                hash: 77,
            },
            app: Some(r"C:\Windows\explorer.exe".into()),
            copied: 9,
            pinned: true,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Entry>(&json).unwrap(), e);
        let files = Entry {
            content: Content::Files(vec![r"C:\a b\c.txt".into()]),
            app: None,
            pinned: false,
            ..e
        };
        let json = serde_json::to_string(&files).unwrap();
        assert!(!json.contains("pinned") && !json.contains("app"));
        assert_eq!(serde_json::from_str::<Entry>(&json).unwrap(), files);
    }

    #[test]
    fn the_same_pixels_have_the_same_fingerprint() {
        assert_eq!(fingerprint(&[1, 2, 3]), fingerprint(&[1, 2, 3]));
        assert_ne!(fingerprint(&[1, 2, 3]), fingerprint(&[3, 2, 1]));
    }
}
