//! Colouring for a code card: keywords, strings, comments and numbers, found by one pass over
//! the text that knows no language in particular. A card shows a dozen lines, so it only has to
//! look right, not parse.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Keyword,
    Str,
    Comment,
    Number,
}

/// Words most languages reserve, sorted for a binary search.
const KEYWORDS: [&str; 78] = [
    "None",
    "Self",
    "Some",
    "and",
    "as",
    "async",
    "await",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "crate",
    "def",
    "default",
    "defer",
    "do",
    "dyn",
    "elif",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "fn",
    "for",
    "from",
    "func",
    "function",
    "if",
    "impl",
    "implements",
    "import",
    "in",
    "int",
    "interface",
    "is",
    "lambda",
    "let",
    "loop",
    "match",
    "mod",
    "mut",
    "new",
    "nil",
    "not",
    "null",
    "or",
    "package",
    "pass",
    "private",
    "pub",
    "public",
    "raise",
    "return",
    "self",
    "static",
    "struct",
    "switch",
    "this",
    "throw",
    "trait",
    "true",
    "try",
    "type",
    "undefined",
    "unsafe",
    "use",
    "var",
    "void",
    "where",
    "while",
    "with",
    "yield",
];

/// The coloured spans of `code`, in order and not overlapping, as byte ranges.
pub fn highlight(code: &str) -> Vec<(Range<usize>, Token)> {
    let bytes = code.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &code[i..];
        let start = i;
        let token = match bytes[i] {
            b'/' if rest.starts_with("//") => {
                i = line_end(code, i);
                Some(Token::Comment)
            }
            b'#' if line_start(code, i) && !rest.starts_with("#[") && !rest.starts_with("#!") => {
                i = line_end(code, i);
                Some(Token::Comment)
            }
            b'/' if rest.starts_with("/*") => {
                i = rest[2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| i + 2 + end + 2);
                Some(Token::Comment)
            }
            b'\'' if lifetime(code, i) => {
                i += 1;
                None
            }
            quote @ (b'"' | b'\'' | b'`') => match closing(code, i, quote) {
                Some(end) => {
                    i = end;
                    Some(Token::Str)
                }
                None => {
                    i += 1;
                    None
                }
            },
            b'0'..=b'9' => {
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric()
                        || bytes[i] == b'_'
                        || (bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)))
                {
                    i += 1;
                }
                Some(Token::Number)
            }
            b if b.is_ascii_alphabetic() || b == b'_' => {
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let word = &code[start..i];
                KEYWORDS
                    .binary_search(&word)
                    .is_ok()
                    .then_some(Token::Keyword)
            }
            _ => {
                // Skip the rest of a multi-byte character and any non-ASCII word with it.
                i += rest.chars().next().map_or(1, char::len_utf8);
                None
            }
        };
        if let Some(token) = token {
            spans.push((start..i, token));
        }
    }
    spans
}

fn line_end(code: &str, from: usize) -> usize {
    code[from..].find('\n').map_or(code.len(), |at| from + at)
}

fn line_start(code: &str, at: usize) -> bool {
    code[..at]
        .rsplit('\n')
        .next()
        .is_some_and(|before| before.trim().is_empty())
}

/// Whether the `'` at `at` starts a Rust lifetime or label (`&'a`, `<'a, 'b>`, `'outer:`)
/// rather than a quote.
fn lifetime(code: &str, at: usize) -> bool {
    let after = &code[at + 1..];
    let name = after
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(after.len());
    let next = after[name..].chars().next();
    let before = code[..at].trim_end().chars().next_back();
    name > 0
        && next != Some('\'')
        && (matches!(before, Some('&' | '<' | ':' | '+'))
            || matches!(next, None | Some('>' | ',' | ':' | '+' | ';')))
}

/// Just past the quote that closes the one at `open`, skipping escapes. Only a backtick string
/// runs over lines; a lone `'` (a Rust lifetime, an apostrophe) is no string at all.
fn closing(code: &str, open: usize, quote: u8) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\n' if quote != b'`' => return None,
            b if b == quote => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(code: &str) -> Vec<(&str, Token)> {
        highlight(code)
            .into_iter()
            .map(|(range, token)| (&code[range], token))
            .collect()
    }

    #[test]
    fn the_keyword_list_is_sorted() {
        assert!(KEYWORDS.is_sorted());
    }

    #[test]
    fn keywords_strings_numbers_and_comments_are_found() {
        use Token::*;
        assert_eq!(
            words("let x = \"hi\\\" there\"; // done\nreturn 0x1F + 2.5;"),
            [
                ("let", Keyword),
                ("\"hi\\\" there\"", Str),
                ("// done", Comment),
                ("return", Keyword),
                ("0x1F", Number),
                ("2.5", Number),
            ]
        );
        assert_eq!(
            words("def f(x):\n    # twice\n    return x * 2"),
            [
                ("def", Keyword),
                ("# twice", Comment),
                ("return", Keyword),
                ("2", Number),
            ]
        );
        assert_eq!(
            words("/* a\nb */ while"),
            [("/* a\nb */", Comment), ("while", Keyword)]
        );
    }

    #[test]
    fn look_alikes_are_left_plain() {
        use Token::*;
        // A lifetime is not a string, a number inside a name is not a number, a range's dots
        // are not a decimal point, and an attribute is not a comment.
        assert_eq!(
            words("#[derive(Debug)]\nfn f<'a>(v2: &'a str) { for i in 0..10 {} }"),
            [
                ("fn", Keyword),
                ("for", Keyword),
                ("in", Keyword),
                ("0", Number),
                ("10", Number),
            ]
        );
        assert_eq!(
            words("name = 'two words'; c = 'x'"),
            [("'two words'", Str), ("'x'", Str)]
        );
        assert_eq!(words("color: #fff;"), []);
        assert_eq!(words("é \"unclosed"), []);
        assert_eq!(words("`multi\nline`"), [("`multi\nline`", Str)]);
    }
}
