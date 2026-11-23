//! What the tray icon's menu lists, top to bottom, and how tall each line is.

use crate::update;
use model::update::Nightly;

/// The menu's and the submenu's widths, in logical pixels.
pub(super) const WIDTH: f32 = 250.0;
pub(super) const SUB_WIDTH: f32 = 200.0;
/// Room round the rows, and each kind of line's height.
pub(super) const PAD: f32 = 6.0;
pub(super) const HEADER: f32 = 46.0;
pub(super) const GAP: f32 = 11.0;
pub(super) const ROW: f32 = 26.0;

/// A command in the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Item {
    Show,
    Clear,
    Basket,
    Clipboard,
    Options,
    Settings,
    AddHeadset,
    Update(Nightly),
    Quit,
}

impl Item {
    /// The label, with `&` before its access key.
    pub(super) fn label(self) -> &'static str {
        match self {
            Item::Show => "&Show shelf",
            Item::Clear => "&Clear shelf",
            Item::Basket => "Show bas&ket",
            Item::Clipboard => "Clipboard &history",
            Item::Options => "&Options",
            Item::Settings => "Se&ttings…",
            Item::AddHeadset => "&Add my headset…",
            Item::Update(_) => "&Restart to update",
            Item::Quit => "&Quit topsy",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Line {
    Header,
    Gap,
    Item(Item),
}

impl Line {
    pub(super) fn height(self) -> f32 {
        match self {
            Line::Header => HEADER,
            Line::Gap => GAP,
            Line::Item(_) => ROW,
        }
    }
}

pub(super) fn height(lines: &[Line]) -> f32 {
    PAD * 2.0 + lines.iter().map(|line| line.height()).sum::<f32>()
}

/// Where line `at` starts, from the top of the window.
pub(super) fn top_of(lines: &[Line], at: usize) -> f32 {
    PAD + lines[..at].iter().map(|line| line.height()).sum::<f32>()
}

pub(super) fn lines() -> Vec<Line> {
    let mut lines = vec![
        Line::Header,
        Line::Gap,
        Line::Item(Item::Show),
        Line::Item(Item::Clear),
        Line::Item(Item::Basket),
        Line::Item(Item::Clipboard),
        Line::Gap,
        Line::Item(Item::Options),
        Line::Item(Item::Settings),
        Line::Item(Item::AddHeadset),
        Line::Gap,
    ];
    if let Some(version) = update::ready() {
        lines.push(Line::Item(Item::Update(version)));
    }
    lines.push(Line::Item(Item::Quit));
    lines
}
