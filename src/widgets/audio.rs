//! The Audio card, after Droppy's Audio Control: a row per app playing through the output
//! device, with its icon, its name, a volume slider and a mute button. The list is
//! `model::mixer`; the sessions come from `win32::mixer`'s thread, which runs while the card is
//! in the lineup and costs nothing between events.

use std::sync::Arc;

use gpui::{
    AnyElement, ClickEvent, Context, FontWeight, MouseButton, RenderImage, Task, div, img,
    prelude::*, px, rgb, rgba,
};

use super::keep_press;
use crate::icons::{self, Icon};
use crate::log::LogErr;
use crate::transform::widgets::slider;
use crate::win32::mixer::{Event, Watch};
use crate::{Island, icon};
use model::mixer::{App, Mixer};

pub const WIDTH: f32 = 240.0;
const ROW: f32 = 34.0;
const ICON: f32 = 20.0;
const NAME: f32 = 62.0;
const BUTTON: f32 = 22.0;
const GAP: f32 = 8.0;
const SLIDER: f32 = WIDTH - ICON - NAME - BUTTON - 3.0 * GAP;
const DIM: u32 = 0xffffff80;

#[derive(Default)]
pub struct State {
    mixer: Mixer<Arc<RenderImage>>,
    listening: Option<Listening>,
}

/// The sessions' thread and the task bringing its events in; dropping it stops both.
struct Listening {
    watch: Watch,
    _events: Task<()>,
}

impl State {
    /// The tray opened: listen, or catch up with a change of output device.
    pub(super) fn opened(&mut self, cx: &mut Context<Island>) {
        match &self.listening {
            Some(listening) => listening.watch.refresh(),
            None => self.listen(cx),
        }
    }

    /// Start the sessions' thread, unless it runs already.
    pub(super) fn listen(&mut self, cx: &mut Context<Island>) {
        if self.listening.is_some() {
            return;
        }
        let (sender, events) = async_channel::unbounded();
        let task = cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let heard = this.update(cx, |island, cx| {
                    island.update_widgets(cx, |widgets, visible, cx| {
                        widgets.audio.hear(event, visible, cx);
                        None
                    });
                });
                if heard.is_err() {
                    break;
                }
            }
        });
        self.listening = Some(Listening {
            watch: Watch::start(sender),
            _events: task,
        });
    }

    /// Switched off: the thread stops and the list goes.
    pub(super) fn stop(&mut self, cx: &mut Context<Island>) {
        self.listening = None;
        let pictures = std::mem::take(&mut self.mixer).into_pictures();
        icon::forget(pictures, cx);
    }

    fn hear(&mut self, event: Event, visible: bool, cx: &mut Context<Island>) {
        let changed = match event {
            Event::Added(session, picture) => {
                icon::forget(self.mixer.add(session, picture).into_iter().collect(), cx);
                true
            }
            Event::Changed { id, volume, muted } => self.mixer.changed(&id, volume, muted),
            Event::Gone(id) => match self.mixer.remove(&id) {
                Some(picture) => {
                    icon::forget(picture.into_iter().collect(), cx);
                    true
                }
                None => false,
            },
        };
        if changed && visible {
            cx.notify();
        }
    }

    fn apply(&self, key: &str, set: Option<(f32, bool)>, cx: &mut Context<Island>) {
        if let Some((volume, muted)) = set
            && let Some(listening) = &self.listening
        {
            listening.watch.set(key, volume, muted);
            cx.notify();
        }
    }

    pub(super) fn card(&self, cx: &mut Context<Island>) -> AnyElement {
        let apps = self.mixer.apps();
        let card = div()
            .id("audio")
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, cx.listener(keep_press));
        if apps.is_empty() {
            return card
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .child(icons::icon(Icon::VolumeHigh, 22.0, rgba(DIM)))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgba(DIM))
                        .child("No apps playing"),
                )
                .into_any_element();
        }
        let rows: Vec<_> = (apps.iter().enumerate())
            .map(|(i, app)| row(i, app, cx))
            .collect();
        card.overflow_y_scroll()
            .when(apps.len() < 3, |card| card.justify_center())
            .children(rows)
            .into_any_element()
    }
}

/// One app: its icon, its name, the slider and the mute button.
fn row(
    i: usize,
    app: &App<Arc<RenderImage>>,
    cx: &mut Context<Island>,
) -> impl IntoElement + use<> {
    let picture = match &app.picture {
        Some(picture) => img(picture.clone()).size(px(ICON)).into_any_element(),
        None => icons::icon(Icon::VolumeHigh, ICON - 4.0, rgba(DIM)).into_any_element(),
    };
    let level = if app.muted { 0.0 } else { app.volume };
    let island = cx.weak_entity();
    let key = app.key.clone();
    let slide = slider(("audio-volume", i), level, SLIDER, move |volume, _, cx| {
        island
            .update(cx, |island, cx| {
                island.update_widgets(cx, |widgets, _, cx| {
                    let set = widgets.audio.mixer.slide(&key, volume);
                    widgets.audio.apply(&key, set, cx);
                    None
                });
            })
            .log_err();
    });
    let glyph = match level {
        _ if app.muted => Icon::VolumeMute,
        0.0 => Icon::VolumeOff,
        l if l < 0.5 => Icon::VolumeLow,
        _ => Icon::VolumeHigh,
    };
    let key = app.key.clone();
    let mute = div()
        .id(("audio-mute", i))
        .size(px(BUTTON))
        .flex_shrink_0()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(rgba(0xffffff14)))
        .child(icons::icon(glyph, 14.0, rgb(0xffffff)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            let set = this.widgets.audio.mixer.toggle_mute(&key);
            this.widgets.audio.apply(&key, set, cx);
        }));
    div()
        .h(px(ROW))
        .flex_shrink_0()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(GAP))
        .child(div().size(px(ICON)).flex_shrink_0().child(picture))
        .child(
            div()
                .w(px(NAME))
                .flex_shrink_0()
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                // Not `truncate`: GPUI keeps a no-wrap text's first measure, so it would never cut.
                .line_clamp(1)
                .text_ellipsis()
                .child(app.name.clone()),
        )
        .child(slide)
        .child(mute)
}
