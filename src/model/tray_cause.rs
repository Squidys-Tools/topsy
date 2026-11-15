//! Why the island's tray is open, and which events fold it away early.
//!
//! Pure model (no OS calls, no clock reads): `main.rs` maps hook and drop-target
//! events onto it, so the rules are unit-testable.

use std::time::Duration;

/// What the tray was opened for. `Timed` carries how long it waits before folding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCause {
    /// A jiggle: a drag is in progress and the tray is its drop target. It folds
    /// when the drag ends — or sooner, if the payload proves undroppable.
    Drag,
    /// A click, the tray icon or a landed drop: it folds on its own timer.
    Timed(Duration),
}

impl TrayCause {
    /// The drop target refused the drag crossing the island, so its payload can
    /// never land. A tray that opened only to catch that drag has nothing to wait
    /// for; any other tray is not about this drag and stays.
    pub fn closed_by_refusal(self) -> bool {
        matches!(self, TrayCause::Drag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_opened_tray_folds_when_the_drag_is_refused() {
        // The hook can't tell what a jiggle is dragging; when the drop target
        // refuses the payload, the tray that opened to catch it has nothing to
        // wait for.
        assert!(TrayCause::Drag.closed_by_refusal());
    }

    #[test]
    fn a_timed_tray_ignores_refused_drags() {
        // It didn't open for the drag: a click-opened shelf shouldn't vanish
        // because a text selection happened to cross it.
        assert!(!TrayCause::Timed(Duration::from_secs(4)).closed_by_refusal());
    }
}
