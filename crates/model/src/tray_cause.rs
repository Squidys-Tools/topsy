//! Why the island's tray is open, what it shows for it, and which events fold it away early.
//!
//! Pure model (no OS calls, no clock reads): the island maps hook and drop-target
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
    /// *Settings…* from a menu, or a click on the update HUD: opened on the settings.
    Settings,
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
            TrayCause::Opened | TrayCause::Settings => Some(Duration::from_millis(5000)),
        }
    }

    /// Opened by hand, to look or to change something: Escape folds it. A drag's or a drop's tray
    /// is not the user's to dismiss with a key they may be using elsewhere.
    pub fn escape_folds(self) -> bool {
        match self {
            TrayCause::Opened | TrayCause::Settings => true,
            TrayCause::Drag | TrayCause::Dropped => false,
        }
    }

    /// The page the tray opens on, given whether Home has anything (something playing, a
    /// widget on). A drag or a drop is about the shelf; opened to look, Home first, as Droppy
    /// does, unless it is empty.
    pub fn page(self, home: bool) -> Page {
        match self {
            TrayCause::Opened if home => Page::Home,
            TrayCause::Opened | TrayCause::Drag | TrayCause::Dropped => Page::Shelf,
            TrayCause::Settings => Page::Settings,
        }
    }
}

/// What the open tray shows on `page`, given whether something is playing and whether any
/// widgets are on: Home holds the player and the widgets, the shelf its files, the settings page
/// the settings. The page capsule
/// hangs under every open tray, whatever it shows.
pub fn sections(page: Page, playing: bool, widgets: bool) -> Sections {
    let home = page == Page::Home;
    Sections {
        media: home && playing,
        widgets: home && widgets,
        shelf: page == Page::Shelf,
        settings: page == Page::Settings,
    }
}

/// The parts of the open tray, top to bottom. None of them is Home with nothing on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sections {
    pub media: bool,
    pub widgets: bool,
    pub shelf: bool,
    pub settings: bool,
}

impl Sections {
    pub fn is_empty(self) -> bool {
        !(self.media || self.widgets || self.shelf || self.settings)
    }
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
        for cause in [TrayCause::Opened, TrayCause::Settings, TrayCause::Dropped] {
            assert!(!cause.closed_by_refusal());
            assert!(cause.folds_after().is_some());
        }
    }

    #[test]
    fn a_drag_or_drop_opens_on_the_shelf_and_a_look_on_home_unless_empty() {
        for home in [true, false] {
            assert_eq!(TrayCause::Drag.page(home), Page::Shelf);
            assert_eq!(TrayCause::Dropped.page(home), Page::Shelf);
        }
        assert_eq!(TrayCause::Opened.page(true), Page::Home);
        assert_eq!(TrayCause::Opened.page(false), Page::Shelf);
        assert_eq!(TrayCause::Settings.page(true), Page::Settings);
    }

    #[test]
    fn each_page_shows_its_own_sections() {
        let shown = |media, widgets, shelf, settings| Sections {
            media,
            widgets,
            shelf,
            settings,
        };
        let home = shown(true, true, false, false);
        assert_eq!(sections(Page::Home, true, true), home);
        let widgets = shown(false, true, false, false);
        assert_eq!(sections(Page::Home, false, true), widgets);
        let shelf = shown(false, false, true, false);
        assert_eq!(sections(Page::Shelf, true, true), shelf);
        let settings = shown(false, false, false, true);
        assert_eq!(sections(Page::Settings, true, true), settings);
        // Turned to Home with nothing on it: an empty page, which the tray says so on.
        assert!(sections(Page::Home, false, false).is_empty());
        assert!(!sections(Page::Shelf, false, false).is_empty());
        assert!(!sections(Page::Settings, false, false).is_empty());
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
