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
use crate::log::LogErr;
use crate::win32::earbuds;
use crate::{Island, log};
use model::activity::{Activities, Activity, Key};

const SETTING: RegString<'static> = RegString::new(r"Software\topsy", "LiveActivities");
static ENABLED: AtomicBool = AtomicBool::new(true);

pub enum Event {
    Set(Key, Activity),
    Remove(Key),
}

/// What the island does when the activities say so.
pub(crate) enum Effect {
    /// They changed at this instant: refresh them, and refit or fold the notch to match.
    Changed(Instant),
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
        events.try_send(Event::Set(key, activity)).log_err();
    });
    crate::app::forward(cx, island, heard, |island, event, cx| {
        island.update_activities(cx, |activity| activity.hear(event, Instant::now()));
    });
}

impl State {
    fn hear(&mut self, event: Event, now: Instant) -> Option<Effect> {
        if !ENABLED.load(Ordering::Relaxed) {
            return None;
        }
        match event {
            Event::Set(key, activity) => {
                log(&format!("activity {key:?} {activity:?}"));
                self.list.set(key, activity, now);
            }
            Event::Remove(key) => {
                log(&format!("activity {key:?} gone"));
                self.list.remove(&key);
            }
        }
        Some(Effect::Changed(now))
    }

    /// Show an activity of topsy's own, as if a watcher had heard it.
    pub(crate) fn show(&mut self, key: Key, activity: Activity, now: Instant) -> Option<Effect> {
        self.hear(Event::Set(key, activity), now)
    }

    fn show_download(&mut self, key: &Key, path: PathBuf) -> Effect {
        log(&format!("activity {key:?} shown in folder"));
        crate::chip::show_in_folder(path);
        self.list.remove(key);
        Effect::Changed(Instant::now())
    }

    /// Expire what has passed and book the next expiry. `hidden_at_rest` is the island resting
    /// hidden; only then is `fullscreen` asked whether to hold the activities back.
    pub(crate) fn refresh(
        &mut self,
        now: Instant,
        hidden_at_rest: bool,
        fullscreen: impl FnOnce() -> bool,
        cx: &mut Context<Island>,
    ) {
        if !ENABLED.load(Ordering::Relaxed) {
            self.list = Activities::default();
        }
        self.list.expire(now);
        self.generation += 1;
        if let Some(at) = self.list.next_expiry() {
            let generation = self.generation;
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(at.saturating_duration_since(now))
                    .await;
                this.update(cx, |this, cx| {
                    this.update_activities(cx, |activity| {
                        (activity.generation == generation).then(|| Effect::Changed(Instant::now()))
                    });
                })
                .ok();
            })
            .detach();
        }
        if hidden_at_rest && !self.list.is_empty() {
            self.held_back = fullscreen();
        }
    }

    /// An activity lasts and no fullscreen app held them back: they keep the notch down.
    pub(crate) fn hold_peek(&self) -> bool {
        !self.list.is_empty() && !self.held_back
    }
}
