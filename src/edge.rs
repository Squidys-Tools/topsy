//! Reaching for the hidden notch: the cursor resting on the screen's top row, above the notch.
//!
//! Pure logic (no Win32), fed by the mouse hook. It only reports changes, so the hook sends
//! nothing while the cursor moves about elsewhere.

/// The strip of the top row that reveals the notch, in physical screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeZone {
    pub left: i32,
    /// Exclusive.
    pub right: i32,
    /// The monitor's top row.
    pub top: i32,
}

impl EdgeZone {
    /// A zone `width` wide centred on `center_x`.
    pub fn centered(center_x: i32, top: i32, width: i32) -> Self {
        let left = center_x - width / 2;
        Self {
            left,
            right: left + width,
            top,
        }
    }

    /// Only the top row itself: above it is another monitor, and the cursor is passing through.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        y == self.top && (self.left..self.right).contains(&x)
    }
}

pub struct EdgeWatch {
    zone: EdgeZone,
    at_edge: bool,
}

impl EdgeWatch {
    pub fn new(zone: EdgeZone) -> Self {
        Self {
            zone,
            at_edge: false,
        }
    }

    /// Feed a pointer move. Returns the new state when the cursor arrives at or leaves the zone.
    pub fn moved(&mut self, x: i32, y: i32) -> Option<bool> {
        let at_edge = self.zone.contains(x, y);
        (at_edge != self.at_edge).then(|| {
            self.at_edge = at_edge;
            at_edge
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watch() -> EdgeWatch {
        EdgeWatch::new(EdgeZone::centered(1000, 0, 200))
    }

    #[test]
    fn arriving_on_the_top_row_above_the_notch_reports_once() {
        let mut w = watch();
        assert_eq!(w.moved(1000, 300), None);
        assert_eq!(w.moved(1000, 0), Some(true));
        assert_eq!(w.moved(1040, 0), None, "sliding along the edge is no news");
        assert_eq!(w.moved(1040, 1), Some(false));
        assert_eq!(w.moved(1040, 5), None);
    }

    #[test]
    fn the_top_row_beside_the_notch_does_not_count() {
        let mut w = watch();
        assert_eq!(w.moved(899, 0), None);
        assert_eq!(w.moved(1100, 0), None, "right edge is exclusive");
        assert_eq!(w.moved(900, 0), Some(true));
        assert_eq!(w.moved(1100, 0), Some(false));
    }

    #[test]
    fn passing_into_a_monitor_above_does_not_count() {
        let mut w = EdgeWatch::new(EdgeZone::centered(1000, 1440, 200));
        assert_eq!(w.moved(1000, 1439), None);
        assert_eq!(w.moved(1000, -20), None);
        assert_eq!(w.moved(1000, 1440), Some(true));
    }
}
