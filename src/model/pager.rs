//! The open tray's pages, after Droppy's: Home (Now Playing and the widgets) and the shelf, one at
//! a time, switched by the capsule under the notch or by a sideways swipe.

use std::time::{Duration, Instant};

/// In the order the capsule shows them, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    Shelf,
}

impl Page {
    pub const ALL: [Page; 2] = [Page::Home, Page::Shelf];

    pub fn index(self) -> usize {
        match self {
            Page::Home => 0,
            Page::Shelf => 1,
        }
    }

    /// The page `by` places along, if there is one.
    pub fn step(self, by: Step) -> Option<Page> {
        let to = match by {
            Step::Back => self.index().checked_sub(1)?,
            Step::On => self.index() + 1,
        };
        Page::ALL.get(to).copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// To the page on the left.
    Back,
    /// To the page on the right.
    On,
}

/// How far a swipe travels sideways before it turns the page.
const TRAVEL: f32 = 30.0;
/// A swipe is mostly sideways: this much more across than down.
const SIDEWAYS: f32 = 1.5;
/// A pause this long between scroll events ends a swipe.
const QUIET: Duration = Duration::from_millis(250);

/// Turns a stream of scroll deltas into page turns: one per swipe, however long it runs, so a
/// flick turns one page and a long swipe doesn't run on through the next.
#[derive(Clone, Copy, Debug, Default)]
pub struct Swipe {
    travel: f32,
    last: Option<Instant>,
    turned: bool,
}

impl Swipe {
    /// A scroll of `dx`, `dy` (logical pixels, GPUI's sign: negative `dx` moves the content left,
    /// so the page on the right comes in).
    pub fn scroll(&mut self, dx: f32, dy: f32, now: Instant) -> Option<Step> {
        if self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) > QUIET)
        {
            *self = Swipe::default();
        }
        self.last = Some(now);
        if self.turned || dx.abs() <= SIDEWAYS * dy.abs() {
            return None;
        }
        self.travel += dx;
        if self.travel.abs() < TRAVEL {
            return None;
        }
        self.turned = true;
        Some(if self.travel < 0.0 {
            Step::On
        } else {
            Step::Back
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_step_within_the_ends() {
        assert_eq!(Page::Home.step(Step::On), Some(Page::Shelf));
        assert_eq!(Page::Shelf.step(Step::Back), Some(Page::Home));
        assert_eq!(Page::Home.step(Step::Back), None);
        assert_eq!(Page::Shelf.step(Step::On), None);
    }

    #[test]
    fn a_swipe_turns_one_page_however_long_it_runs() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut swipe = Swipe::default();
        assert_eq!(swipe.scroll(-20.0, 0.0, at(0)), None, "not far enough yet");
        assert_eq!(swipe.scroll(-20.0, 0.0, at(16)), Some(Step::On));
        assert_eq!(swipe.scroll(-200.0, 0.0, at(32)), None, "the same swipe");
        assert_eq!(
            swipe.scroll(-200.0, 0.0, at(200)),
            None,
            "still the same swipe"
        );
        assert_eq!(
            swipe.scroll(60.0, 0.0, at(500)),
            Some(Step::Back),
            "after a pause, a new swipe"
        );
    }

    #[test]
    fn a_flick_turns_at_once_and_scrolling_down_never_does() {
        let now = Instant::now();
        assert_eq!(Swipe::default().scroll(-60.0, 0.0, now), Some(Step::On));
        let mut swipe = Swipe::default();
        for _ in 0..10 {
            assert_eq!(swipe.scroll(-20.0, 40.0, now), None, "mostly down");
        }
    }
}
