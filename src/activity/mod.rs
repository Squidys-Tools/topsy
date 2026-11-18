//! Live Activities, after Droppy's: while a browser download runs, or for a moment when
//! Bluetooth audio connects or leaves, the notch stays down a little wider and shows it. The rules
//! are in `model::activity`; this hears the watchers and keeps the notch down while any last.

mod downloads;
mod view;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use gpui::{App, Context, Entity};

use crate::autostart::RegString;
use crate::frame::Mode;
use crate::model::activity::{Activities, Activity, Key};
use crate::win32::{self, earbuds};
use crate::{Island, log};

const SETTING: RegString<'static> = RegString::new(r"Software\topsy", "LiveActivities");
static ENABLED: AtomicBool = AtomicBool::new(true);

pub enum Event {
    Set(Key, Activity),
    Remove(Key),
}

#[derive(Default)]
pub struct State {
    list: Activities,
    /// Bumped on every change, so only the latest timer expires the passing ones.
    generation: u64,
    /// A fullscreen app had the screen at the last change: stay out of its way.
    held_back: bool,
}

/// On unless switched off (`LiveActivities` = `off`).
pub fn is_enabled() -> bool {
    SETTING.value().as_deref() != Some("off")
}

pub fn set_enabled(on: bool) -> windows::core::Result<()> {
    if on {
        SETTING.remove()?;
    } else {
        SETTING.set("off")?;
    }
    ENABLED.store(on, Ordering::Relaxed);
    Ok(())
}

/// Start watching the Downloads folder and Bluetooth audio.
pub fn start(cx: &mut App, island: &Entity<Island>) {
    ENABLED.store(is_enabled(), Ordering::Relaxed);
    let (events, heard) = async_channel::unbounded();
    downloads::watch(events.clone());
    earbuds::watch(move |seen| {
        let key = Key::Device(seen.container);
        let activity = if seen.connected {
            Activity::Connected {
                name: seen.name,
                battery: seen.battery,
            }
        } else {
            Activity::Disconnected { name: seen.name }
        };
        let _ = events.try_send(Event::Set(key, activity));
    });
    crate::app::forward(cx, island, heard, Island::on_activity);
}

impl Island {
    fn on_activity(&mut self, event: Event, cx: &mut Context<Self>) {
        if !ENABLED.load(Ordering::Relaxed) {
            return;
        }
        let now = Instant::now();
        match event {
            Event::Set(key, activity) => {
                log(&format!("activity {key:?} {activity:?}"));
                self.activity.list.set(key, activity, now);
            }
            Event::Remove(key) => {
                log(&format!("activity {key:?} gone"));
                self.activity.list.remove(&key);
            }
        }
        self.activities_changed(now, cx);
    }

    fn show_download(&mut self, key: &Key, path: PathBuf, cx: &mut Context<Self>) {
        log(&format!("activity {key:?} shown in folder"));
        crate::chip::show_in_folder(path);
        self.activity.list.remove(key);
        self.activities_changed(Instant::now(), cx);
    }

    pub(crate) fn activities_changed(&mut self, now: Instant, cx: &mut Context<Self>) {
        if !ENABLED.load(Ordering::Relaxed) {
            self.activity.list = Activities::default();
        }
        self.activity.list.expire(now);
        self.activity.generation += 1;
        if let Some(at) = self.activity.list.next_expiry() {
            let generation = self.activity.generation;
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(at.saturating_duration_since(now))
                    .await;
                this.update(cx, |this, cx| {
                    if this.activity.generation == generation {
                        this.activities_changed(Instant::now(), cx);
                    }
                })
                .ok();
            })
            .detach();
        }
        if self.rest == Mode::Hidden && !self.activity.list.is_empty() {
            self.activity.held_back = self.fullscreen();
        }
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
        if self.rest == Mode::Hidden && !self.activity.list.is_empty() && !self.activity.held_back {
            Mode::Peek
        } else {
            self.rest
        }
    }
}
