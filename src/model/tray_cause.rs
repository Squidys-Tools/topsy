//! Why the island's tray is open, what it shows for it, and which events fold it away early.
//!
//! Pure model (no OS calls, no clock reads): `main.rs` maps hook and drop-target
//! events onto it, so the rules are unit-testable.

use std::time::Duration;

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

    /// What the tray shows, given whether something is playing and whether the shelf is empty.
    /// A drag or a drop is about the shelf, so Now Playing stays out of its way; opened to look,
    /// the player leads, and an empty shelf has nothing to add under it.
    pub fn sections(self, playing: bool, shelf_empty: bool) -> Sections {
        let media = playing && self == TrayCause::Opened;
        Sections {
            media,
            shelf: !(media && shelf_empty),
        }
    }
}

/// The parts of the open tray, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sections {
    pub media: bool,
    pub shelf: bool,
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
    fn now_playing_shows_only_in_a_tray_opened_to_look() {
        let both = Sections {
            media: true,
            shelf: true,
        };
        let shelf = Sections {
            media: false,
            shelf: true,
        };
        // Dragging a file in: the drop zone, never the player.
        assert_eq!(TrayCause::Drag.sections(true, true), shelf);
        assert_eq!(TrayCause::Drag.sections(true, false), shelf);
        // The file just landed: the shelf it landed on.
        assert_eq!(TrayCause::Dropped.sections(true, false), shelf);
        // Opened to look: the player, and the shelf under it once it holds something.
        let player = Sections {
            media: true,
            shelf: false,
        };
        assert_eq!(TrayCause::Opened.sections(true, true), player);
        assert_eq!(TrayCause::Opened.sections(true, false), both);
        assert_eq!(TrayCause::Opened.sections(false, true), shelf);
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
