//! The floating player: Droppy's, pulled out of the notch to float anywhere on the desktop, at
//! one of three sizes (`model::player`). It is a glass window of its own, made at launch as the
//! basket's is so that showing it never takes the foreground, and it hears the same media
//! updates as the island, so it keeps its own copy of what is playing.

mod view;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{App, Bounds, Context, Entity, RenderImage, point, prelude::*, px, size};
use windows::Win32::Foundation::HWND;

use crate::autostart::RegString;
use crate::booking::Booking;
use crate::config::Lens;
use crate::frame::{Frame, Glass, open_glass};
use crate::log::LogErr;
use crate::now_playing::bars::Bars;
use crate::now_playing::session::Update;
use crate::{config, icon, log, tray, win32};
use model::float::{self, MARGIN};
use model::media::{Event, NowPlaying};
use model::pager::Swipe;
use model::player::{Saved, Size};
use model::spring::{Spring, SpringConfig};

pub(crate) use view::COVER;

const SAVED: RegString<'static> = RegString::new(r"Software\topsy", "Player");
/// Where it springs out from, as a share of its size.
const FLY_IN_FROM: f32 = 0.82;
/// A move is saved once the player has stayed put this long.
const SAVE_AFTER: Duration = Duration::from_millis(600);

/// The floating player as the island holds it: its view, and whether it is out, which the
/// island reads without reading the view (that would redraw the island with it).
pub(crate) struct Handle {
    pub(crate) player: Entity<Player>,
    out: Rc<Cell<bool>>,
}

impl Handle {
    pub(crate) fn is_out(&self) -> bool {
        self.out.get()
    }
}

pub(crate) struct Player {
    hwnd: Option<HWND>,
    frame: Frame,
    width: Spring,
    height: Spring,
    last_frame: Option<Instant>,
    open: bool,
    out: Rc<Cell<bool>>,
    size: Size,
    /// Where it was last centred, physical screen pixels: it comes back there.
    center: Option<(i32, i32)>,
    now: NowPlaying,
    art: Option<Arc<RenderImage>>,
    /// The art at the expanded size.
    cover: Option<Arc<RenderImage>>,
    accent: Option<[u8; 3]>,
    bars: Bars,
    metering: Booking,
    ticking: Booking,
    /// The pointer is over it: only then do the bars move.
    hovered: bool,
    swipe: Swipe,
    /// Hand the window to Windows' move loop once it is in place: it was pulled out of the
    /// notch and the button is still held.
    grab: bool,
    /// Kept in the registry; off for scripted runs.
    persist: bool,
    /// Bumped by each move, so only the last one's save lands.
    moves: u64,
}

/// Make the floating player's window, hidden, and bring it back if it was left out. `None`
/// when the window can't be made: the tray's player stays in the notch.
pub(crate) fn open(cx: &mut App, glass: Glass, black: bool, previous: HWND) -> Option<Handle> {
    let config = config::get();
    let persist = config.state.is_none() && config.player.is_none();
    let saved = persist
        .then(|| SAVED.value().as_deref().and_then(Saved::parse))
        .flatten();
    let out = Rc::new(Cell::new(false));
    let tiny = Bounds::new(point(px(0.0), px(0.0)), size(px(1.0), px(1.0)));
    let window = open_glass(cx, tiny, glass, black, None, previous, {
        let out = out.clone();
        move |hwnd, glass, backdrop, window, cx| {
            let scale = window.scale_factor();
            let backdrop = match (backdrop, hwnd) {
                (Some(backdrop), Some(hwnd)) if config.lens != Lens::Off => {
                    Some(backdrop.with_floating_lens(hwnd, view::dims(Size::Expanded), cx))
                }
                (backdrop, _) => backdrop,
            };
            let frame = Frame::new(hwnd, scale, glass, black, backdrop).floating();
            cx.new(|cx| {
                // Moved by its caption: float from where it was put.
                cx.observe_window_bounds(window, |player: &mut Player, _, cx| player.moved(cx))
                    .detach();
                Player::new(hwnd, frame, out, persist, saved)
            })
        }
    });
    let player = window.and_then(|window| window.entity(cx)).log_err()?;
    tray::set_player_out(false);
    let start = match (config.player, saved) {
        (Some(size), _) => Some((size, None)),
        (None, Some(saved)) if saved.open => Some((saved.size, Some(saved.center))),
        (None, Some(_) | None) => None,
    };
    if let Some((size, center)) = start {
        player.update(cx, |player, cx| {
            player.size = size;
            player.open_at(center.or_else(screen_middle), cx);
        });
    }
    Some(Handle { player, out })
}

/// The middle of the work area under the pointer.
fn screen_middle() -> Option<(i32, i32)> {
    let (_, work, _) = win32::cursor_monitor()?;
    Some(((work.left + work.right) / 2, (work.top + work.bottom) / 2))
}

impl Player {
    fn new(
        hwnd: Option<HWND>,
        frame: Frame,
        out: Rc<Cell<bool>>,
        persist: bool,
        saved: Option<Saved>,
    ) -> Self {
        let size = saved.map_or(Size::Player, |saved| saved.size);
        Self {
            hwnd,
            frame,
            width: Spring::new(view::dims(size).0 * FLY_IN_FROM, SpringConfig::ISLAND),
            height: Spring::new(0.0, SpringConfig::ISLAND),
            last_frame: None,
            open: false,
            out,
            size,
            center: saved.map(|saved| saved.center),
            now: NowPlaying::default(),
            art: None,
            cover: None,
            accent: None,
            bars: Bars::default(),
            metering: Booking::default(),
            ticking: Booking::default(),
            hovered: false,
            swipe: Swipe::default(),
            grab: false,
            persist,
            moves: 0,
        }
    }

    /// The island's media update, for the player's own copy.
    pub(crate) fn apply(&mut self, update: Update, cx: &mut Context<Self>) {
        let track = matches!(update.event, Event::Track(_) | Event::Gone);
        let mut changed = self.now.apply(update.event);
        if track {
            changed |= self.art.is_some() || update.art.is_some();
            self.accent = update.art.as_ref().and_then(|art| art.accent);
            let (art, cover) = update.art.map(|art| (art.image, art.cover)).unzip();
            // The art is the island's too, which lets it go; the cover is the player's own.
            let old_art = std::mem::replace(&mut self.art, art);
            if let Some(old) = std::mem::replace(&mut self.cover, cover)
                && old_art.is_none_or(|art| !Arc::ptr_eq(&art, &old))
            {
                icon::forget(vec![old], cx);
            }
        }
        if changed && self.open {
            cx.notify();
        }
    }

    pub(crate) fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.close(cx);
        } else {
            self.open_at(self.center, cx);
        }
    }

    /// Open centred on `center` (physical screen pixels), or under the pointer, kept whole on
    /// that monitor.
    fn open_at(&mut self, center: Option<(i32, i32)>, cx: &mut Context<Self>) {
        let place = match center {
            Some(at) => win32::monitor_at(at).map(|(work, scale)| (at, work, scale)),
            None => win32::cursor_monitor(),
        };
        let Some((at, work, scale)) = place else {
            return;
        };
        let (w, h) = view::dims(self.size);
        let px = |v: f32| (v * scale).round() as i32;
        let center = float::center_under(at, (px(w), px(h)), work, px(MARGIN));
        log(&format!("player at {center:?} ({:?})", self.size));
        self.frame.float_at(center, scale);
        self.center = Some(center);
        self.width = Spring::new(w * FLY_IN_FROM, SpringConfig::ISLAND);
        self.height = Spring::new(h * FLY_IN_FROM, SpringConfig::ISLAND);
        self.set_open(true, cx);
    }

    /// Pulled out of the notch: up at once at its full size with its art under the pointer,
    /// then moved by the button still held.
    pub(crate) fn pull_out(&mut self, cx: &mut Context<Self>) {
        let Some((cursor, _, scale)) = win32::cursor_monitor() else {
            return;
        };
        let (w, h) = view::dims(self.size);
        let (grip_x, grip_y) = view::grip(self.size);
        let px = |v: f32| (v * scale).round() as i32;
        let center = (
            cursor.0 + px(w / 2.0 - grip_x),
            cursor.1 + px(h / 2.0 - grip_y),
        );
        log(&format!(
            "player pulled out at {center:?} ({:?})",
            self.size
        ));
        self.frame.float_at(center, scale);
        self.center = Some(center);
        self.width = Spring::new(w, SpringConfig::ISLAND);
        self.height = Spring::new(h, SpringConfig::ISLAND);
        self.grab = true;
        self.set_open(true, cx);
    }

    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        log("player closed");
        let (w, _) = view::dims(self.size);
        self.width.set_target(w * FLY_IN_FROM);
        self.height.set_target(0.0);
        self.set_open(false, cx);
    }

    fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open = open;
        self.out.set(open);
        tray::set_player_out(open);
        if open {
            let (w, h) = view::dims(self.size);
            self.width.set_target(w);
            self.height.set_target(h);
        }
        self.save();
        cx.notify();
    }

    /// To `size`, grown or shrunk about its centre, then nudged back whole onto its monitor.
    fn resize(&mut self, size: Size, cx: &mut Context<Self>) {
        log(&format!("player {size:?}"));
        self.size = size;
        let (w, h) = view::dims(size);
        if let Some(center) = self.frame.center()
            && let Some((work, scale)) = win32::monitor_at(center)
        {
            let px = |v: f32| (v * scale).round() as i32;
            let center = float::center_under(center, (px(w), px(h)), work, px(MARGIN));
            self.frame.float_at(center, scale);
            self.center = Some(center);
        }
        self.width.set_target(w);
        self.height.set_target(h);
        self.save();
        cx.notify();
    }

    /// Windows moved the window (its caption dragged): float from there, and save it once it
    /// has stayed put.
    fn moved(&mut self, cx: &mut Context<Self>) {
        self.frame.follow_window(cx);
        let center = self.frame.center();
        if !self.open || center == self.center {
            return;
        }
        self.center = center;
        self.moves += 1;
        let moves = self.moves;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_AFTER).await;
            this.update(cx, |this, _| {
                if this.moves == moves {
                    this.save();
                }
            })
            .ok();
        })
        .detach();
    }

    fn save(&self) {
        let Some(center) = self.center.filter(|_| self.persist) else {
            return;
        };
        let saved = Saved {
            center,
            size: self.size,
            open: self.open,
        };
        SAVED.set(&saved.serialize()).log_err();
    }

    /// The level bars are on screen and moving: something plays, the size shows them, and the
    /// pointer is over the player.
    fn bars_live(&self) -> bool {
        self.open && self.hovered && self.size != Size::Mini && self.now.hint().is_some()
    }

    fn meter_frame(&mut self, cx: &mut Context<Self>) {
        // A hover whose "leave" went missing must not keep the bars, and the meter, going.
        let live = self.bars_live() && self.hwnd.is_some_and(win32::cursor_over);
        if self.bars.frame(live) && self.open {
            cx.notify();
        }
    }

    pub(crate) fn set_black(&mut self, black: bool, cx: &mut Context<Self>) {
        self.frame.set_black(black, cx);
        cx.notify();
    }
}
