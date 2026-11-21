//! How the clipboard palette lists and describes a copy: its row's title and age, its section,
//! the order of the list, what a search finds, and the facts under its preview.

use super::clip_kind::{Kind, color, host};
use super::clipboard::{Content, Entry};

/// How long ago: "now", "4 min. ago", "2 hr. ago", "3 days ago".
pub fn ago(secs: u64) -> String {
    match secs {
        0..60 => "now".into(),
        60..3_600 => format!("{} min. ago", secs / 60),
        3_600..86_400 => format!("{} hr. ago", secs / 3_600),
        86_400..172_800 => "1 day ago".into(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// The most of a text's first line a row keeps; the row cuts it to its width anyway.
const TITLE: usize = 200;
const DAY: u64 = 86_400;

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    )
}

/// What a row in the list calls an entry: a text's first line, a link, a colour, an image's
/// size, which files.
pub fn title(content: &Content, kind: Kind) -> String {
    match content {
        Content::Text(text) => match kind {
            Kind::Link | Kind::Color => text.trim().into(),
            Kind::Text | Kind::Code | Kind::Image | Kind::Files => {
                let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                let words: Vec<&str> = line.split_whitespace().collect();
                words.join(" ").chars().take(TITLE).collect()
            }
        },
        Content::Image { width, height, .. } => format!("Image ({width}\u{d7}{height})"),
        Content::Files(files) => match files.as_slice() {
            [] => String::new(),
            [one] => file_name(one),
            [first, rest @ ..] => format!("{} and {} more", file_name(first), rest.len()),
        },
    }
}

/// How long ago, as a row says it beside its title: "now", "4m", "2h", "3d".
pub fn short_ago(secs: u64) -> String {
    match secs {
        0..60 => "now".into(),
        60..3_600 => format!("{}m", secs / 60),
        3_600..DAY => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / DAY),
    }
}

/// What the preview's tag calls a copy: its kind, and one file as a file.
pub fn kind_label(content: &Content, kind: Kind) -> &'static str {
    match content {
        Content::Files(files) if files.len() == 1 => "File",
        Content::Files(_) | Content::Text(_) | Content::Image { .. } => kind.label(),
    }
}

/// The facts under the preview, label and value: how big a copy is and what it is made of.
/// The app and how long ago are in the preview's tag.
pub fn details(content: &Content, kind: Kind) -> Vec<(&'static str, String)> {
    let mut facts = Vec::new();
    match content {
        Content::Text(text) => {
            let t = text.trim();
            let lines = t.lines().count();
            match kind {
                Kind::Link => facts.push(("Site", host(t).to_string())),
                Kind::Color => {
                    if let Some([r, g, b]) = color(t) {
                        facts.push(("Hex", format!("#{r:02X}{g:02X}{b:02X}")));
                        facts.push(("RGB", format!("{r}, {g}, {b}")));
                    }
                }
                Kind::Code => {
                    facts.push(("Lines", lines.to_string()));
                    facts.push(("Characters", text.chars().count().to_string()));
                }
                Kind::Text | Kind::Image | Kind::Files => {
                    facts.push(("Characters", text.chars().count().to_string()));
                    facts.push(("Words", t.split_whitespace().count().to_string()));
                    if lines > 1 {
                        facts.push(("Lines", lines.to_string()));
                    }
                }
            }
        }
        Content::Image { width, height, .. } => {
            facts.push(("Dimensions", format!("{width} \u{d7} {height}")));
        }
        Content::Files(files) => {
            if files.len() > 1 {
                facts.push(("Items", files.len().to_string()));
            }
            if let Some(folder) = files.first().and_then(|f| f.parent()) {
                facts.push(("Folder", folder.display().to_string()));
            }
        }
    }
    facts
}

/// The list's sections, in the order they show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Pinned,
    Today,
    Yesterday,
    Week,
    Older,
}

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Section::Pinned => "Pinned",
            Section::Today => "Today",
            Section::Yesterday => "Yesterday",
            Section::Week => "Last 7 days",
            Section::Older => "Older",
        }
    }
}

/// The section an entry is listed under; `midnight` is when today began, local time, in Unix
/// seconds.
pub fn section(entry: &Entry, midnight: u64) -> Section {
    let day_before = |days: u64| midnight.saturating_sub(days * DAY);
    if entry.pinned {
        Section::Pinned
    } else if entry.copied >= midnight {
        Section::Today
    } else if entry.copied >= day_before(1) {
        Section::Yesterday
    } else if entry.copied >= day_before(6) {
        Section::Week
    } else {
        Section::Older
    }
}

/// The entries the list shows, as indexes into `entries` (newest first, `kinds` beside them):
/// those the search finds, and only pinned ones with `pinned_only`, pinned first, at most `max`.
pub fn listed(
    entries: &[Entry],
    kinds: &[Kind],
    query: &str,
    pinned_only: bool,
    max: usize,
) -> Vec<usize> {
    let found = |pinned: bool| {
        (entries.iter().zip(kinds).enumerate())
            .filter(move |&(_, (e, &kind))| e.pinned == pinned && matches(e, kind, query))
            .map(|(i, _)| i)
    };
    let unpinned = found(false).filter(|_| !pinned_only);
    found(true).chain(unpinned).take(max).collect()
}

/// Whether a search finds the entry: every word of `query`, ignoring case, in its text, its
/// files' names, the app that copied it or its kind.
pub fn matches(entry: &Entry, kind: Kind, query: &str) -> bool {
    let mut hay = match &entry.content {
        Content::Text(text) => text.to_lowercase(),
        Content::Files(files) => files
            .iter()
            .filter_map(|f| f.file_name())
            .map(|n| n.to_string_lossy().to_lowercase())
            .collect::<Vec<_>>()
            .join(" "),
        Content::Image { .. } => String::new(),
    };
    if let Some(app) = entry.app.as_ref().and_then(|a| a.file_stem()) {
        hay.push(' ');
        hay.push_str(&app.to_string_lossy().to_lowercase());
    }
    hay.push(' ');
    hay.push_str(&kind.label().to_lowercase());
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| hay.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_read_like_droppy() {
        assert_eq!(ago(5), "now");
        assert_eq!(ago(14 * 60 + 30), "14 min. ago");
        assert_eq!(ago(3 * 3_600), "3 hr. ago");
        assert_eq!(ago(86_400 + 5), "1 day ago");
        assert_eq!(ago(9 * 86_400), "9 days ago");
        assert_eq!(short_ago(5), "now");
        assert_eq!(short_ago(14 * 60 + 30), "14m");
        assert_eq!(short_ago(3 * 3_600), "3h");
        assert_eq!(short_ago(DAY - 1), "23h");
        assert_eq!(short_ago(9 * DAY), "9d");
    }

    fn entry(id: u64, content: Content, copied: u64, pinned: bool) -> Entry {
        Entry {
            id,
            content,
            app: None,
            copied,
            pinned,
        }
    }

    #[test]
    fn a_row_is_titled_by_its_first_line_or_what_it_is() {
        let title = |c: &Content| title(c, c.kind());
        let text = |s: &str| Content::Text(s.into());
        assert_eq!(
            title(&text("\n\n  Launch   review\tmoved \nSame room")),
            "Launch review moved"
        );
        assert_eq!(
            title(&text(" https://getdroppy.app/blog\n")),
            "https://getdroppy.app/blog"
        );
        assert_eq!(title(&text("x".repeat(5000).as_str())).len(), TITLE);
        let image = Content::Image {
            width: 1400,
            height: 1050,
            hash: 0,
        };
        assert_eq!(title(&image), "Image (1400\u{d7}1050)");
        let one = Content::Files(vec![r"C:\a\notes.txt".into()]);
        assert_eq!(title(&one), "notes.txt");
        let three = Content::Files(vec![r"C:\a\b.txt".into(), "c".into(), "d".into()]);
        assert_eq!(title(&three), "b.txt and 2 more");
    }

    #[test]
    fn each_kind_has_its_own_facts() {
        let details = |c: Content| details(&c, c.kind());
        let text = |s: &str| Content::Text(s.into());
        assert_eq!(
            details(text(" Launch review\nmoved é ")),
            [
                ("Characters", "23".into()),
                ("Words", "4".into()),
                ("Lines", "2".into()),
            ]
        );
        assert_eq!(
            details(text("rgb(110, 123, 242)")),
            [("Hex", "#6E7BF2".into()), ("RGB", "110, 123, 242".into())]
        );
        assert_eq!(
            details(text("https://www.getdroppy.app/blog")),
            [("Site", "getdroppy.app".into())]
        );
        assert_eq!(
            details(text("fn a() {\n}\n")),
            [("Lines", "2".into()), ("Characters", "11".into())]
        );
        let image = Content::Image {
            width: 1400,
            height: 1050,
            hash: 0,
        };
        assert_eq!(details(image), [("Dimensions", "1400 \u{d7} 1050".into())]);
        let one = Content::Files(vec![r"C:\a\notes.txt".into()]);
        assert_eq!(kind_label(&one, one.kind()), "File");
        assert_eq!(details(one), [("Folder", r"C:\a".into())]);
        let two = Content::Files(vec![r"C:\a\b".into(), r"C:\a\c".into()]);
        assert_eq!(kind_label(&two, two.kind()), "Files");
        assert_eq!(details(two)[0], ("Items", "2".into()));
    }

    #[test]
    fn entries_fall_into_sections_by_local_day_and_pins_come_first() {
        let midnight = 10 * DAY;
        let at = |copied, pinned| {
            section(
                &entry(1, Content::Text("a".into()), copied, pinned),
                midnight,
            )
        };
        assert_eq!(at(midnight - 9 * DAY, true), Section::Pinned);
        assert_eq!(at(midnight, false), Section::Today);
        assert_eq!(at(midnight - 1, false), Section::Yesterday);
        assert_eq!(at(midnight - DAY, false), Section::Yesterday);
        assert_eq!(at(midnight - DAY - 1, false), Section::Week);
        assert_eq!(at(midnight - 6 * DAY, false), Section::Week);
        assert_eq!(at(midnight - 6 * DAY - 1, false), Section::Older);
    }

    #[test]
    fn the_list_puts_pinned_matches_first_and_stops_at_its_limit() {
        let text = |s: &str| Content::Text(s.into());
        let entries = [
            entry(5, text("apple pie"), 50, false),
            entry(4, text("apple"), 40, true),
            entry(3, text("banana"), 30, false),
            entry(2, text("apple tart"), 20, true),
            entry(1, text("apple jam"), 10, false),
        ];
        let kinds: Vec<Kind> = entries.iter().map(|e| e.content.kind()).collect();
        assert_eq!(listed(&entries, &kinds, "", false, 100), [1, 3, 0, 2, 4]);
        assert_eq!(listed(&entries, &kinds, "apple", false, 100), [1, 3, 0, 4]);
        assert_eq!(listed(&entries, &kinds, "apple", true, 100), [1, 3]);
        assert_eq!(listed(&entries, &kinds, "", false, 3), [1, 3, 0]);
    }

    #[test]
    fn search_finds_text_file_names_apps_and_kinds() {
        let entry = |content, app: Option<&str>| Entry {
            id: 1,
            content,
            app: app.map(Into::into),
            copied: 0,
            pinned: false,
        };
        let note = entry(
            Content::Text("Launch Review at 10".into()),
            Some(r"C:\x\Notepad.exe"),
        );
        assert!(matches(&note, note.content.kind(), ""));
        assert!(matches(&note, note.content.kind(), "review LAUNCH"));
        assert!(matches(&note, note.content.kind(), "notepad"));
        assert!(matches(&note, note.content.kind(), "text"));
        assert!(!matches(&note, note.content.kind(), "launch dinner"));
        let files = entry(Content::Files(vec![r"C:\work\Budget.xlsx".into()]), None);
        assert!(matches(&files, files.content.kind(), "budget"));
        assert!(!matches(&files, files.content.kind(), "work"));
        let image = Content::Image {
            width: 1,
            height: 1,
            hash: 0,
        };
        let image = entry(image, None);
        assert!(matches(&image, Kind::Image, "image"));
    }
}
