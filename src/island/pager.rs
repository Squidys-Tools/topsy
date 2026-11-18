//! The open tray's pages, after Droppy's: Home (Now Playing and the widgets) and the shelf, one
//! at a time. A capsule hanging under the notch shows which is up and switches them, and a
//! sideways swipe (a touchpad, a tilt wheel, Shift and the wheel) turns them. The rules are in
//! `model::pager`; which page shows what is `TrayCause::sections`.

use std::time::Instant;

use gpui::{
    AnyElement, ClickEvent, Context, MouseButton, MouseDownEvent, ScrollHandle, ScrollWheelEvent,
    Window, div, prelude::*, px, rgb, rgba,
};

use crate::frame::{CAPSULE, STRIP};
use crate::icons::{Icon, icon};
use crate::{Island, Mode, log};
use model::pager::{Page, Swipe};
use model::spring::{Spring, SpringConfig};

/// Inside the capsule's 1 px edge.
const INSET: f32 = 3.0;
const INNER: (f32, f32) = (
    CAPSULE.0 - 2.0 * (INSET + 1.0),
    CAPSULE.1 - 2.0 * (INSET + 1.0),
);
/// One page's button. The content slides by as much when the page turns, as the highlight does.
const SEGMENT: f32 = INNER.0 / 2.0;
const ACTIVE: u32 = 0xffffff2e;
const DIM: u32 = 0xffffff8c;

pub struct State {
    page: Page,
    /// How far the content sits from its place, in pixels; springs back to 0 after a turn.
    slide: Spring,
    /// The room under the notch for the capsule, springing out and back with it.
    strip: Spring,
    swipe: Swipe,
    /// The shelf's chips, which take a swipe until they can scroll no further.
    pub chips: ScrollHandle,
}

impl Default for State {
    fn default() -> Self {
        Self {
            page: Page::Home,
            slide: Spring::new(0.0, SpringConfig::ISLAND),
            strip: Spring::new(0.0, SpringConfig::ISLAND),
            swipe: Swipe::default(),
            chips: ScrollHandle::new(),
        }
    }
}

impl State {
    pub fn page(&self) -> Page {
        self.page
    }

    pub fn strip(&self) -> f32 {
        self.strip.value
    }

    pub fn slide(&self) -> f32 {
        self.slide.value
    }

    /// Advance the springs; true while any still moves.
    pub fn step(&mut self, dt: f32) -> bool {
        self.slide.step(dt) | self.strip.step(dt)
    }

    /// The tray opened: Home first, as Droppy does.
    pub fn opened(&mut self) {
        self.page = Page::Home;
        self.slide = Spring::new(0.0, SpringConfig::ISLAND);
    }

    pub fn show_capsule(&mut self, shown: bool) {
        self.strip.set_target(if shown { STRIP } else { 0.0 });
    }
}

impl Island {
    pub(crate) fn turn_to(&mut self, page: Page, cx: &mut Context<Self>) {
        let from = self.pager.page;
        if page == from {
            return;
        }
        log(&format!("page {page:?}"));
        self.pager.page = page;
        // The new page comes in from the side it lies on.
        self.pager.slide.value = (page.index() as f32 - from.index() as f32) * SEGMENT;
        self.pager.slide.velocity = 0.0;
        self.retarget();
        cx.notify();
    }

    /// A sideways swipe turns the page, once the chips under the cursor can scroll no further.
    pub(crate) fn on_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode != Mode::Tray || !self.sections().pager {
            return;
        }
        let delta = event.delta.pixel_delta(window.line_height());
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        let chips = &self.pager.chips;
        if self.pager.page == Page::Shelf && chips.bounds().contains(&event.position) {
            // The chips have already moved by this scroll: look at where they were.
            let before = f32::from(chips.offset().x) - dx;
            let end = -f32::from(chips.max_offset().width);
            let room = if dx < 0.0 { before - end } else { -before };
            if room > 0.5 {
                return;
            }
        }
        let turn = self.pager.swipe.scroll(dx, dy, Instant::now());
        if let Some(step) = turn
            && let Some(page) = self.pager.page.step(step)
        {
            self.turn_to(page, cx);
        }
    }

    /// The capsule's buttons, while it shows: a highlight under the page that is up, sliding with
    /// the turn, and the shelf's count beside its icon.
    pub(crate) fn capsule(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.pager.strip() < 0.5 {
            return None;
        }
        let at = self.pager.page.index() as f32 * SEGMENT - self.pager.slide();
        let highlight = div()
            .absolute()
            .top(px(INSET))
            .left(px(INSET + at))
            .w(px(SEGMENT))
            .h(px(INNER.1))
            .rounded_full()
            .bg(rgba(ACTIVE));
        let count = self.shelf.len();
        let buttons = Page::ALL.map(|page| {
            let color = if page == self.pager.page {
                rgb(0xffffff)
            } else {
                rgba(DIM)
            };
            let (glyph, badge) = match page {
                Page::Home => (Icon::Home, None),
                Page::Shelf => (Icon::Tray, (count > 0).then(|| count.to_string())),
            };
            div()
                .id(("page", page.index()))
                .w(px(SEGMENT))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(4.0))
                .cursor_pointer()
                .child(icon(glyph, 16.0, color))
                .children(badge.map(|badge| {
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(badge)
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _: &MouseDownEvent, _window, cx| cx.stop_propagation()),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    cx.stop_propagation();
                    this.turn_to(page, cx);
                }))
        });
        Some(
            div()
                .id("capsule")
                .relative()
                .size_full()
                .flex()
                .flex_row()
                .items_center()
                .px(px(INSET))
                .child(highlight)
                .children(buttons)
                .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                    this.on_scroll(event, window, cx);
                }))
                .into_any_element(),
        )
    }
}
