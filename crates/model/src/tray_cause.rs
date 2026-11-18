//! Why the island's tray is open, what it shows for it, and which events fold it away early.
//!
//! Pure model (no OS calls, no clock reads): `main.rs` maps hook and drop-target
//! events onto it, so the rules are unit-testable.

use std::time::Duration;

use crate::pager::Page;

/// What the tray was opened for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCause {
    /// A drag up to the edge: a drag is in progress and the tray is its drop
    /// target. It folds when the drag ends, or sooner if the payload proves undroppable.
    Drag,
    /// A drop landed: open so the file is seen to land.
    Dropped,
    /// A click on the notch or the tray icon: opened to look.
    Opened,
}

impl TrayCause {
    /// The drop target refused the drag crossing the island, so its payload can
    /// never land. A tray that opened only to catch that drag has nothing to wait
    /// for; any other tray is not about this drag and stays.
    pub fn closed_by_refusal(self) -> bool {
        matches!(self, TrayCause::Drag)
    }

    /// How long the tray waits before folding on its own; a drag's tray waits for the drag.
    pub fn folds_after(self) -> Option<Duration> {
        match self {
            TrayCause::Drag => None,
            TrayCause::Dropped => Some(Duration::from_millis(4000)),
            // Time for the cursor to arrive.
            TrayCause::Opened => Some(Duration::from_millis(5000)),
        }
    }

    /// What the tray shows on `page`, given whether something is playing and whether any widgets
    /// are on. Opened to look, with Now Playing or the widgets to show, the tray has two pages:
    /// Home holds them, the shelf the files. A drag or a drop is about the shelf, so it shows the
    /// shelf alone, as does a tray with nothing for Home.
    pub fn sections(self, page: Page, playing: bool, widgets: bool) -> Sections {
        let pager = self == TrayCause::Opened && (playing || widgets);
        let home = pager && page == Page::Home;
        Sections {
            media: home && playing,
            widgets: home && widgets,
            shelf: !home,
            pager,
        }
    }
}

/// The parts of the open tray, top to bottom, and whether the page capsule hangs under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sections {
    pub media: bool,
    pub widgets: bool,
    pub shelf: bool,
    pub pager: bool,
}

/// The held drag, as far as the tray or the basket is concerned. Once the drop target has refused
/// it, its payload can never land, so nothing reopens them for it until the button comes up: the
/// basket opens under the pointer, and would otherwise flash on every jiggle.
#[derive(Clone, Copy, Debug, Default)]
pub struct HeldDrag {
    refused: bool,
}

impl HeldDrag {
    pub fn refuse(&mut self) {
        self.refused = true;
    }

    pub fn release(&mut self) {
        self.refused = false;
    }

    pub fn may_open_tray(self) -> bool {
        !self.refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_opened_tray_folds_when_the_drag_is_refused() {
        // The hook can't tell what a drag carries; when the drop target
        // refuses the payload, the tray that opened to catch it has nothing to
        // wait for.
        assert!(TrayCause::Drag.closed_by_refusal());
        assert_eq!(TrayCause::Drag.folds_after(), None);
    }

    #[test]
    fn a_tray_not_opened_for_the_drag_ignores_its_refusal() {
        // It didn't open for the drag: a click-opened shelf shouldn't vanish
        // because a text selection happened to cross it.
        for cause in [TrayCause::Opened, TrayCause::Dropped] {
            assert!(!cause.closed_by_refusal());
            assert!(cause.folds_after().is_some());
        }
    }

    #[test]
    fn home_and_its_pager_show_only_in_a_tray_opened_to_look() {
        let sections = |media, widgets, shelf, pager| Sections {
            media,
            widgets,
            shelf,
            pager,
        };
        let shelf = sections(false, false, true, false);
        // Dragging a file in, or one just landed: the shelf alone, whatever the page.
        for cause in [TrayCause::Drag, TrayCause::Dropped] {
            assert_eq!(cause.sections(Page::Home, true, true), shelf);
        }
        // Opened to look: Home holds the player and the widgets, the shelf its own page.
        let opened = TrayCause::Opened;
        assert_eq!(
            opened.sections(Page::Home, true, true),
            sections(true, true, false, true)
        );
        assert_eq!(
            opened.sections(Page::Home, false, true),
            sections(false, true, false, true)
        );
        assert_eq!(
            opened.sections(Page::Shelf, true, true),
            sections(false, false, true, true)
        );
        // Nothing for Home: the shelf, with no pages to switch between.
        assert_eq!(opened.sections(Page::Home, false, false), shelf);
    }

    #[test]
    fn a_refused_drag_cannot_reopen_the_tray_until_released() {
        let mut drag = HeldDrag::default();
        assert!(drag.may_open_tray());
        drag.refuse();
        assert!(!drag.may_open_tray(), "a further jiggle of the same drag");
        drag.release();
        assert!(drag.may_open_tray(), "the next drag starts fresh");
    }
}
