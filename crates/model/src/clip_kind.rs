//! What a copied thing is: text, a link, a colour, code, an image, files. How the palette lists
//! and describes one is in `clip_list`.

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

/// What kind of text this is.
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

/// A link's site: `https://www.getdroppy.app/blog` is `getdroppy.app`.
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
}
