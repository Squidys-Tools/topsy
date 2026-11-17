//! Where a card's preview ends. GPUI's own line clamp can't mark a cut: it truncates the whole
//! string to `width * lines` before wrapping, which lands past the last visible line, and it
//! counts only soft wraps, so a text of short lines is never clamped at all. So the card asks
//! the text system where each line falls and cuts the text itself (see `clipboard::preview`).

use std::borrow::Cow;
use std::ops::Range;

pub const ELLIPSIS: char = '\u{2026}';

/// `text` cut after its `max`th line, the last one ending in an ellipsis, when more follows.
/// `lines` are the byte ranges the text is laid out in, in order; `fits(range)` says whether
/// `text[range]` and the ellipsis fit on one line, and characters come off the end until it does.
pub fn clamp<'a>(
    text: &'a str,
    lines: &[Range<usize>],
    max: usize,
    fits: impl Fn(Range<usize>) -> bool,
) -> Cow<'a, str> {
    let Some(last) = max.checked_sub(1).and_then(|at| lines.get(at)) else {
        return Cow::Borrowed(text);
    };
    if lines.len() <= max {
        return Cow::Borrowed(text);
    }
    let mut line = text[last.clone()].trim_end();
    while !line.is_empty() && !fits(last.start..last.start + line.len()) {
        let drop = line.chars().next_back().map_or(0, char::len_utf8);
        line = line[..line.len() - drop].trim_end();
    }
    Cow::Owned(format!("{}{line}{ELLIPSIS}", &text[..last.start]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines of `text`: one per `\n`, and a soft wrap at each of `wraps` (byte offsets,
    /// where the next line starts; the space before stays on the line above, as in GPUI).
    fn lines(text: &str, wraps: &[usize]) -> Vec<Range<usize>> {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
        starts.extend(wraps);
        starts.sort();
        let ends = starts[1..].iter().copied().chain([text.len()]);
        (starts.iter().copied().zip(ends))
            .map(|(start, end)| start..start + text[start..end].trim_end_matches('\n').len())
            .collect()
    }

    /// A line has room for `room` characters, ellipsis included.
    fn room(text: &str, room: usize) -> impl Fn(Range<usize>) -> bool + '_ {
        move |range| text[range].chars().count() < room
    }

    #[test]
    fn text_that_fits_is_left_alone() {
        let text = "one\ntwo";
        let out = clamp(text, &lines(text, &[]), 2, room(text, 9));
        assert!(matches!(out, Cow::Borrowed("one\ntwo")));
    }

    #[test]
    fn a_cut_ends_in_an_ellipsis() {
        let text = "one\ntwo\nthree";
        let out = clamp(text, &lines(text, &[]), 2, room(text, 9));
        assert_eq!(out, "one\ntwo\u{2026}");
    }

    #[test]
    fn a_soft_wrapped_line_loses_its_trailing_space() {
        let text = "alpha beta gamma delta";
        let out = clamp(text, &lines(text, &[6, 11]), 2, room(text, 9));
        assert_eq!(out, "alpha beta\u{2026}");
    }

    #[test]
    fn a_full_last_line_gives_up_characters_for_the_ellipsis() {
        let text = "one\ntwo three\nfour";
        let out = clamp(text, &lines(text, &[]), 2, room(text, 6));
        assert_eq!(out, "one\ntwo t\u{2026}");
    }

    #[test]
    fn a_space_left_by_the_cut_is_dropped_too() {
        let text = "one\ntwo three\nfour";
        let out = clamp(text, &lines(text, &[]), 2, room(text, 5));
        assert_eq!(out, "one\ntwo\u{2026}");
    }

    #[test]
    fn a_blank_last_line_is_just_the_ellipsis() {
        let text = "one\n\ntwo";
        let out = clamp(text, &lines(text, &[]), 2, room(text, 9));
        assert_eq!(out, "one\n\u{2026}");
    }

    #[test]
    fn multibyte_text_is_cut_between_characters() {
        let text = "ééé\nààààà\nx";
        let out = clamp(text, &lines(text, &[]), 2, room(text, 4));
        assert_eq!(out, "ééé\nààà\u{2026}");
    }

    #[test]
    fn nothing_to_show_leaves_the_text() {
        let text = "one\ntwo";
        let out = clamp(text, &lines(text, &[]), 0, room(text, 9));
        assert!(matches!(out, Cow::Borrowed(_)));
    }
}
