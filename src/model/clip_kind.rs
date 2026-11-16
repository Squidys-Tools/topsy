//! What a copied thing is (text, a link, a colour, code, an image, files) and how its card in
//! the clipboard strip describes it.

use super::clipboard::{Content, Entry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Link,
    Color,
    Code,
    Image,
    Files,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Text => "Text",
            Kind::Link => "Link",
            Kind::Color => "Color",
            Kind::Code => "Code",
            Kind::Image => "Image",
            Kind::Files => "Files",
        }
    }
}

/// What kind of text this is, for its card.
pub fn kind_of(text: &str) -> Kind {
    let t = text.trim();
    if is_link(t) {
        Kind::Link
    } else if color(t).is_some() {
        Kind::Color
    } else if is_code(t) {
        Kind::Code
    } else {
        Kind::Text
    }
}

fn is_link(t: &str) -> bool {
    let lower = t.to_ascii_lowercase();
    let rest = ["https://", "http://", "www."]
        .iter()
        .find_map(|scheme| lower.strip_prefix(scheme));
    !t.chars().any(char::is_whitespace)
        && rest.is_some_and(|rest| rest.contains('.') || rest.starts_with("localhost"))
}

/// A link's site, for its card: `https://www.getdroppy.app/blog` is `getdroppy.app`.
pub fn host(link: &str) -> &str {
    let t = link.trim();
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|scheme| {
            t.get(..scheme.len())
                .filter(|s| s.eq_ignore_ascii_case(scheme))
                .map(|s| &t[s.len()..])
        })
        .unwrap_or(t);
    let rest = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    rest.strip_prefix("www.").unwrap_or(rest)
}

/// A colour written as `#rgb`, `#rrggbb`, `#rrggbbaa` or `rgb(r, g, b)` / `rgba(...)`.
pub fn color(t: &str) -> Option<[u8; 3]> {
    if let Some(hex) = t.strip_prefix('#') {
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let digit = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok();
        let pair = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        return match hex.len() {
            3 => Some([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17]),
            6 | 8 => Some([pair(0)?, pair(2)?, pair(4)?]),
            _ => None,
        };
    }
    let lower = t.to_ascii_lowercase();
    let inner = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    let channel = |s: &str| s.parse::<u8>().ok();
    Some([channel(parts[0])?, channel(parts[1])?, channel(parts[2])?])
}

/// Several lines that mostly look like code: ending in a bracket or semicolon, or opening
/// with a keyword most languages share.
fn is_code(t: &str) -> bool {
    const STARTS: [&str; 16] = [
        "fn ",
        "pub ",
        "let ",
        "const ",
        "def ",
        "class ",
        "function ",
        "import ",
        "from ",
        "return ",
        "if (",
        "for (",
        "#include",
        "use ",
        "var ",
        "public ",
    ];
    let lines: Vec<&str> = t.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if lines.len() < 2 {
        return false;
    }
    let codey = lines
        .iter()
        .filter(|l| {
            l.ends_with([';', '{', '}', '(', ')', ':', ','])
                || STARTS.iter().any(|s| l.starts_with(s))
                || l.starts_with("//")
                || l.contains(" => ")
                || l.contains(" -> ")
        })
        .count();
    codey * 2 > lines.len()
}

/// How long ago, as a card says it: "now", "4 min. ago", "2 hr. ago", "3 days ago".
pub fn ago(secs: u64) -> String {
    match secs {
        0..60 => "now".into(),
        60..3_600 => format!("{} min. ago", secs / 60),
        3_600..86_400 => format!("{} hr. ago", secs / 3_600),
        86_400..172_800 => "1 day ago".into(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// The line at the foot of a card: how long the text is, where a link goes, a colour's
/// channels, an image's size, which files.
pub fn detail(content: &Content, kind: Kind) -> String {
    let count = |n: usize, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    match content {
        Content::Text(text) => {
            let t = text.trim();
            match kind {
                Kind::Link => t.into(),
                Kind::Color => {
                    color(t).map_or_else(String::new, |[r, g, b]| format!("RGB {r}, {g}, {b}"))
                }
                Kind::Code => count(t.lines().count(), "line"),
                _ => count(text.chars().count(), "character"),
            }
        }
        Content::Image { width, height, .. } => format!("{width} \u{d7} {height}"),
        Content::Files(files) => match files.as_slice() {
            [one] => one
                .file_name()
                .map_or_else(|| one.display().to_string(), |n| n.to_string_lossy().into()),
            _ => count(files.len(), "item"),
        },
    }
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
    fn text_is_sorted_into_kinds() {
        assert_eq!(kind_of("https://getdroppy.app/blog"), Kind::Link);
        assert_eq!(kind_of("  www.example.com  "), Kind::Link);
        assert_eq!(kind_of("see https://x.com later"), Kind::Text);
        assert_eq!(kind_of("#6E7BF2"), Kind::Color);
        assert_eq!(kind_of("rgb(110, 123, 242)"), Kind::Color);
        assert_eq!(kind_of("#notacolour"), Kind::Text);
        assert_eq!(kind_of("fn main() {\n    println!(\"hi\");\n}"), Kind::Code);
        assert_eq!(kind_of("def f(x):\n    return x"), Kind::Code);
        assert_eq!(
            kind_of("Launch review moved to 10:00.\nSame room."),
            Kind::Text
        );
        assert_eq!(
            kind_of("Dear Sam,\nThanks for the notes.\nBest, Alex"),
            Kind::Text
        );
    }

    #[test]
    fn a_link_card_names_its_site() {
        assert_eq!(host("https://www.getdroppy.app/blog"), "getdroppy.app");
        assert_eq!(host("HTTP://localhost:3000?x=1"), "localhost:3000");
        assert_eq!(host("www.example.com"), "example.com");
    }

    #[test]
    fn colours_parse_to_rgb() {
        assert_eq!(color("#6E7BF2"), Some([110, 123, 242]));
        assert_eq!(color("#fff"), Some([255, 255, 255]));
        assert_eq!(color("#11223344"), Some([17, 34, 51]));
        assert_eq!(color("RGBA(1, 2, 3, 0.5)"), Some([1, 2, 3]));
        assert_eq!(color("rgb(300, 0, 0)"), None);
        assert_eq!(color("#12345"), None);
    }

    #[test]
    fn ages_read_like_droppy() {
        assert_eq!(ago(5), "now");
        assert_eq!(ago(14 * 60 + 30), "14 min. ago");
        assert_eq!(ago(3 * 3_600), "3 hr. ago");
        assert_eq!(ago(86_400 + 5), "1 day ago");
        assert_eq!(ago(9 * 86_400), "9 days ago");
    }

    #[test]
    fn each_kind_has_its_own_footer() {
        let detail = |c: &Content| detail(c, c.kind());
        let text = |s: &str| Content::Text(s.into());
        assert_eq!(detail(&text("Launch review moved")), "19 characters");
        assert_eq!(detail(&text("é")), "1 character");
        assert_eq!(
            detail(&text(" https://getdroppy.app/blog\n")),
            "https://getdroppy.app/blog"
        );
        assert_eq!(detail(&text("#6E7BF2")), "RGB 110, 123, 242");
        assert_eq!(detail(&text("fn a() {\n}\n")), "2 lines");
        let image = Content::Image {
            width: 1400,
            height: 1050,
            hash: 0,
        };
        assert_eq!(detail(&image), "1400 \u{d7} 1050");
        let one = Content::Files(vec![r"C:\a\notes.txt".into()]);
        assert_eq!(detail(&one), "notes.txt");
        let two = Content::Files(vec!["a".into(), "b".into()]);
        assert_eq!(detail(&two), "2 items");
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
