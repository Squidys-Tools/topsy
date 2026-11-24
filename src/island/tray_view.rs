//! What the island shows: the peek's line, and the tray's layout (Home's player and droplets, the
//! shelf's header and its chips, or the settings, as the page and `tray_cause::sections` say).

mod shelf_view;

use std::time::Instant;

use gpui::{Context, Div, ScrollWheelEvent, div, prelude::*, px, rgba};

use crate::droplets::{self, Droplet, Effect};
use crate::island::PEEK_LINGER;
use crate::{Island, Mode, activity, frame, hud, now_playing, settings, win32};
use model::pager::Page;
use model::tray_cause::{self, Sections, TrayCause};

impl Island {
    pub(crate) fn sections(&self) -> Sections {
        let playing = self.media.now.card().is_some();
        tray_cause::sections(self.pager.page(), playing, self.droplets.has_row())
    }

    /// Home has something to show: a player or droplets' cards.
    fn home_has_content(&self) -> bool {
        self.media.now.card().is_some() || self.droplets.has_row()
    }

    /// Open the tray for `cause`, or, already open, turn to the page a drag or a drop is about.
    pub(crate) fn show_tray_for(&mut self, cause: TrayCause, cx: &mut Context<Self>) {
        self.tray_cause = cause;
        if self.mode == Mode::Tray && cause != TrayCause::Opened {
            self.turn_to(cause.page(self.home_has_content()), cx);
        }
        self.set_mode(Mode::Tray, cx);
    }

    /// The droplets' row is on screen: a card's redraws are worth booking.
    fn row_visible(&self) -> bool {
        self.mode == Mode::Tray && self.sections().widgets
    }

    /// The one way into the droplets from their listeners and tasks: `update` changes their state,
    /// told whether the row is on screen, and the island does what it asks in turn.
    pub(crate) fn update_droplets(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut droplets::State, bool, &mut Context<Self>) -> Option<Effect>,
    ) {
        let visible = self.row_visible();
        match update(&mut self.droplets, visible, cx) {
            Some(Effect::Pressed) => self.press = None,
            Some(Effect::Activity(key, activity)) => {
                self.update_activities(cx, |activities| {
                    activities.show(key, activity, Instant::now())
                });
            }
            Some(Effect::Refit) if self.mode == Mode::Tray => self.set_mode(Mode::Tray, cx),
            Some(Effect::Refit) | None => {}
        }
    }

    /// [`Self::update_droplets`] for one droplet, the one of type `D`.
    pub(crate) fn update_droplet<D: Droplet>(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut D, bool, &mut Context<Self>) -> Option<Effect>,
    ) {
        self.update_droplets(cx, |droplets, visible, cx| {
            update(droplets.get_mut::<D>()?, visible, cx)
        });
    }

    fn media_shown(&self) -> now_playing::Shown {
        let tray = self.mode == Mode::Tray;
        now_playing::Shown {
            open: self.mode != Mode::Hidden,
            tray,
            player: tray && self.sections().media,
            peek: self.mode == Mode::Peek,
        }
    }

    /// The one way into Now Playing from its listeners and tasks: `update` changes its state,
    /// told what of the island is on screen, and the island does what it asks in turn.
    pub(crate) fn update_media(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(
            &mut now_playing::Media,
            now_playing::Shown,
            &mut Context<Self>,
        ) -> Option<now_playing::Effect>,
    ) {
        let shown = self.media_shown();
        match update(&mut self.media, shown, cx) {
            Some(now_playing::Effect::Pressed) => self.press = None,
            Some(now_playing::Effect::Refit) => self.fit_tray(),
            Some(now_playing::Effect::PulledOut) => self.fold(cx),
            None => {}
        }
    }

    /// The one way into the settings page from its listeners: `update` changes its state, and
    /// the island applies what was changed.
    pub(crate) fn update_settings(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut settings::State, &mut Context<Self>) -> Option<settings::Effect>,
    ) {
        match update(&mut self.settings, cx) {
            Some(settings::Effect::Setting(setting, on)) => self.on_setting(setting, on, cx),
            Some(settings::Effect::Lineup(lineup)) => {
                let open = self.mode == Mode::Tray;
                self.update_droplets(cx, |droplets, _, cx| droplets.set_lineup(lineup, open, cx));
            }
            Some(settings::Effect::Redraw) | None => {}
        }
        cx.notify();
    }

    /// A click on the notch opens the tray, or the settings' About while the update HUD offers
    /// the update.
    pub(crate) fn clicked(&mut self, cx: &mut Context<Self>) {
        if self.hud.shows_update() {
            self.settings.show(settings::Section::About);
            self.open_tray(TrayCause::Settings, cx);
        } else if self.mode != Mode::Tray {
            self.open_tray(TrayCause::Opened, cx);
        }
    }

    /// The one way into the HUD from its listeners and tasks: `update` changes its state, told
    /// the island's mode, and the island does what it asks in turn.
    pub(crate) fn update_hud(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut hud::State, Mode, &mut Context<Self>) -> Option<hud::Effect>,
    ) {
        let mode = self.mode;
        match update(&mut self.hud, mode, cx) {
            Some(hud::Effect::Shown) if self.mode == Mode::Tray => cx.notify(),
            Some(hud::Effect::Shown) => {
                // The HUD folds the notch itself; a collapse booked earlier must not cut it short.
                self.collapse_generation += 1;
                self.set_mode(Mode::Peek, cx);
            }
            Some(hud::Effect::Done) if self.mode != Mode::Peek => cx.notify(),
            Some(hud::Effect::Done)
                if self.resting() != Mode::Peek && self.hwnd.is_some_and(win32::cursor_over) =>
            {
                // Being looked at: back to the plain peek, which folds once the cursor leaves.
                self.set_mode(Mode::Peek, cx);
                self.schedule_collapse(PEEK_LINGER, cx);
            }
            Some(hud::Effect::Done) => self.set_mode(self.resting(), cx),
            None => {}
        }
    }

    /// The one way into the Live Activities from their listeners and tasks: `update` changes
    /// them, and the island does what they ask in turn.
    pub(crate) fn update_activities(
        &mut self,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut activity::State) -> Option<activity::Effect>,
    ) {
        match update(&mut self.activity) {
            Some(activity::Effect::Changed(now)) => self.activities_changed(now, cx),
            None => {}
        }
    }

    fn activities_changed(&mut self, now: Instant, cx: &mut Context<Self>) {
        let hwnd = self.hwnd;
        let hidden = self.rest == Mode::Hidden;
        (self.activity).refresh(now, hidden, || frame::fullscreen(hwnd), cx);
        match self.mode {
            // Each folds back to `resting` when it is done; the tray shows no activities.
            Mode::Tray => {}
            _ if self.hud.is_shown() => {}
            // Being looked at: keep the peek, sized to what it now shows.
            Mode::Peek if self.hwnd.is_some_and(win32::cursor_over) => {
                self.set_mode(Mode::Peek, cx)
            }
            _ => {
                let resting = self.resting();
                if self.mode == resting && resting != Mode::Peek {
                    // Hidden, so nothing of it is on screen to redraw.
                    return;
                }
                if self.mode != resting {
                    self.collapse_generation += 1;
                }
                self.set_mode(resting, cx);
            }
        }
    }

    /// Folded away, or held down in the peek by an activity: either way a drag up here opens it.
    pub(crate) fn at_rest(&self) -> bool {
        self.mode == Mode::Hidden || (self.mode == Mode::Peek && self.mode == self.resting())
    }

    /// What the island folds back to: the peek while an activity lasts, else its rest.
    pub(crate) fn resting(&self) -> Mode {
        if self.rest == Mode::Hidden && self.activity.hold_peek() {
            Mode::Peek
        } else {
            self.rest
        }
    }

    /// What the peek says: the HUD, the Live Activities, the playing title with its art, or what
    /// is on the shelf.
    pub(crate) fn peek_content(&self, cx: &mut Context<Self>) -> Div {
        if let Some(hud) = self.hud.peek() {
            return hud;
        }
        if let Some(activities) = self.activity.peek(cx) {
            return activities;
        }
        let count = self.shelf.len();
        if let Some(hint) = (self.media).peek_hint(count, self.media_shown(), cx) {
            return hint;
        }
        let label = match count {
            0 => "topsy".to_string(),
            n => format!("{n} on shelf"),
        };
        div().text_xs().child(label)
    }

    /// Fold back to rest now, cancelling any collapse already scheduled (Escape, the tray icon).
    pub(crate) fn fold(&mut self, cx: &mut Context<Self>) {
        self.collapse_generation += 1;
        self.set_mode(self.resting(), cx);
    }

    /// The notch body's size for `mode`; the tray's height is its sections', and it widens to fit
    /// the droplets' row.
    pub(crate) fn size_for(&self, mode: Mode) -> (f32, f32) {
        let (w, h) = mode.size();
        if mode == Mode::Peek && self.hud.is_shown() {
            return crate::hud::SIZE;
        }
        if mode == Mode::Peek
            && let Some(size) = self.activity.size()
        {
            return size;
        }
        if mode != Mode::Tray {
            return (w, h);
        }
        let sections = self.sections();
        let player = if sections.media {
            now_playing::HEIGHT
        } else {
            0.0
        };
        let (row, w) = match sections.widgets {
            true => (droplets::HEIGHT, w.max(self.droplets.width())),
            false => (0.0, w),
        };
        let (shelf, w) = match (sections.shelf, sections.settings) {
            (true, _) => (h, w.max(frame::SHELF_WIDTH)),
            (false, true) => (h, w.max(settings::WIDTH)),
            (false, false) => (0.0, w),
        };
        let empty = if sections.is_empty() { EMPTY_HOME } else { 0.0 };
        (w, player + row + shelf + empty)
    }

    /// Spring to the size the mode and its sections want, and the page capsule out or away.
    pub(crate) fn retarget(&mut self) {
        let (w, h) = self.size_for(self.mode);
        self.width.set_target(w);
        self.height.set_target(h);
        self.pager.show_capsule(self.mode == Mode::Tray);
    }

    /// The tray has just opened: on the page its cause wants, the droplets told, nothing picked
    /// on the shelf.
    pub(crate) fn opened(&mut self, cx: &mut Context<Self>) {
        self.shelf.deselect();
        let page = self.tray_cause.page(self.home_has_content());
        if page == Page::Settings {
            self.settings.reload();
        }
        self.pager.opened(page);
        self.droplets.opened(cx);
    }

    /// The open tray springs to its sections' size when they change under it: the track stops,
    /// the first file lands.
    pub(crate) fn fit_tray(&mut self) {
        if self.mode == Mode::Tray {
            self.retarget();
        }
    }

    /// The expanded tray: the player and the droplets, or the shelf, as its sections say. After a
    /// page turn it slides in from the page's side.
    pub(crate) fn tray_content(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let sections = self.sections();
        // Fixed size and centred in the window, so it is revealed (not squashed) while the
        // pill springs open.
        let (w, h) = self.size_for(Mode::Tray);
        let slide = self.pager.slide();
        div()
            .id("tray")
            .w(px(w))
            .h(px(h))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .relative()
            .left(px(slide))
            .opacity(1.0 - (slide.abs() / 60.0).min(0.7))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                this.on_scroll(event, window, cx);
            }))
            .children(
                (sections.media)
                    .then(|| self.media.player(self.media_shown(), cx))
                    .flatten(),
            )
            .children((sections.widgets).then(|| self.droplets.row(self.row_visible(), cx)))
            .children(sections.shelf.then(|| self.shelf_section(cx)))
            .children(sections.settings.then(|| self.settings.view(h, cx)))
            .children(sections.is_empty().then(empty_home))
            .children(self.hud.over_tray())
    }
}

/// Home's height with nothing on it.
const EMPTY_HOME: f32 = 80.0;

/// Home turned to with nothing playing and no droplet's card on.
fn empty_home() -> Div {
    div()
        .w_full()
        .h(px(EMPTY_HOME))
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(rgba(0xffffff8c))
        .child("Nothing playing")
}
