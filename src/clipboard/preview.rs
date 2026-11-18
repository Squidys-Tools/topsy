//! The text a card previews: the start of a copy, cut after the lines the card has room for and
//! ended with an ellipsis. GPUI's line clamp can't do the ellipsis (see `model::clamp`), so the
//! text system lays the text out here and the cut is made before the card ever sees it.

use std::ops::Range;

use gpui::{Pixels, Rems, TextRun, Window, black, px, rems};

use model::clamp::{ELLIPSIS, clamp};

/// How much of a long text is laid out at all, so a card never shapes a whole copied log.
const LINES: usize = 12;
const CHARS: usize = 400;
/// An ellipsis that fits to the fraction of a pixel would still wrap on rounding.
const SLACK: f32 = 1.0;

/// How a preview is set. The element showing it takes its size and width from here too, so
/// the text is cut where it is laid out.
pub struct Face {
    pub family: Option<&'static str>,
    pub rems: f32,
    /// Logical pixels.
    pub width: f32,
    /// Lines the card has room for.
    pub lines: usize,
    /// Whether a long line wraps, or runs off the card.
    pub wrap: bool,
}

impl Face {
    pub fn size(&self) -> Rems {
        rems(self.rems)
    }

    /// The start of `text`, in at most `self.lines` lines of `self.width`, the last ending in
    /// an ellipsis when more of the text is left out.
    pub fn preview(&self, window: &Window, text: &str) -> String {
        let text = head(text);
        self.cut(window, &text).unwrap_or(text)
    }

    fn cut(&self, window: &Window, text: &str) -> Option<String> {
        let mut font = window.text_style().font();
        if let Some(family) = self.family {
            font.family = family.into();
        }
        let size = self.size().to_pixels(window.rem_size());
        let run = |len| TextRun {
            len,
            font: font.clone(),
            color: black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let system = window.text_system();
        let wrap = self.wrap.then(|| px(self.width));
        let paragraphs = system
            .shape_text(
                text.to_string().into(),
                size,
                &[run(text.len())],
                wrap,
                None,
            )
            .ok()?;

        let mut lines = Vec::new();
        let mut starts = Vec::new();
        let mut at = 0;
        for paragraph in &paragraphs {
            starts.push(at);
            let wraps = (paragraph.wrap_boundaries().iter())
                .map(|b| paragraph.runs()[b.run_ix].glyphs[b.glyph_ix].index);
            let mut from = 0;
            for to in wraps.chain([paragraph.text.len()]) {
                lines.push(at + from..at + to);
                from = to;
            }
            at += paragraph.text.len() + 1;
        }

        let ellipsis = system.layout_line(
            ELLIPSIS.encode_utf8(&mut [0; 4]),
            size,
            &[run(ELLIPSIS.len_utf8())],
            None,
        );
        let room: Pixels = px(self.width - SLACK) - ellipsis.width;
        let fits = |range: Range<usize>| {
            let first = starts.partition_point(|&start| start <= range.start) - 1;
            let layout = &paragraphs[first].unwrapped_layout;
            let x = |at: usize| layout.x_for_index(at - starts[first]);
            x(range.end) - x(range.start) <= room
        };
        Some(clamp(text, &lines, self.lines, fits).into_owned())
    }
}

/// The start of a long text.
fn head(text: &str) -> String {
    let mut out: String = text
        .trim()
        .lines()
        .take(LINES)
        .collect::<Vec<_>>()
        .join("\n");
    if let Some((cut, _)) = out.char_indices().nth(CHARS) {
        out.truncate(cut);
    }
    out
}
