//! The Shelf widgets page: the droplets in the tray, dragged into order, and those left out.

use gpui::{AnyElement, Context, Div, Window, div, prelude::*, px, rgb, rgba};

use super::controls::{DIM, HOVER, card, row, switch};
use super::window::{CONTENT_WIDTH, Change, SettingsWindow};
use crate::droplets;
use crate::icons::{self, Icon};
use model::droplets::Lineup;

/// A widget being dragged by its row, and what follows the cursor meanwhile.
#[derive(Clone)]
struct Dragged {
    id: &'static str,
    name: &'static str,
}

impl Render for Dragged {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(CONTENT_WIDTH + 20.0))
            .h(px(46.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(10.0))
            .rounded(px(12.0))
            .bg(rgb(0x1c1c1e))
            .border_1()
            .border_color(rgba(0xffffff24))
            .shadow_lg()
            .text_color(rgb(0xffffff))
            .text_size(px(14.0))
            .child(icons::icon(Icon::DragHandle, 16.0, rgba(DIM)))
            .child(self.name)
    }
}

impl SettingsWindow {
    pub(super) fn change_lineup(
        &mut self,
        change: impl FnOnce(&mut Lineup),
        cx: &mut Context<Self>,
    ) {
        let mut lineup = self.lineup.clone();
        change(&mut lineup);
        if lineup == self.lineup {
            return;
        }
        if let Err(err) = droplets::save(&lineup) {
            eprintln!("topsy: could not change the shelf widgets: {err}");
            return;
        }
        self.lineup = lineup.clone();
        cx.notify();
        (self.on_change.clone())(Change::Droplets(lineup), cx);
    }

    pub(super) fn lineup_cards(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let (on, off): (Vec<_>, Vec<_>) =
            (self.lineup.rows(&droplets::ids()).into_iter()).partition(|(_, on)| *on);
        let name = |id| droplets::entry(id).map_or(id, |entry| entry.name);
        let mut shown: Vec<AnyElement> = on
            .into_iter()
            .enumerate()
            .map(|(i, (id, _))| {
                let dragged = Dragged { id, name: name(id) };
                div()
                    .id(("shown", i))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .mx(px(-10.0))
                    .px(px(10.0))
                    .rounded(px(10.0))
                    .hover(|row| row.bg(rgba(HOVER)))
                    .drag_over::<Dragged>(|row, _, _, _| row.bg(rgba(0xffffff1f)))
                    .on_drop(cx.listener(move |view, dragged: &Dragged, _, cx| {
                        view.change_lineup(|lineup| lineup.move_to(dragged.id, i), cx);
                    }))
                    .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.change_lineup(|lineup| lineup.set(id, false), cx);
                    }))
                    .child(icons::icon(Icon::DragHandle, 16.0, rgba(DIM)))
                    .child(
                        row(("shown-row", i), name(id), None, switch(true))
                            .flex_1()
                            .mx_0()
                            .px_0(),
                    )
                    .into_any_element()
            })
            .collect();
        if shown.is_empty() {
            shown.push(
                row(
                    "none",
                    "No widgets in the tray",
                    Some("Turn one on below"),
                    div(),
                )
                .text_color(rgba(DIM))
                .into_any_element(),
            );
        }
        let hidden: Vec<AnyElement> = off
            .into_iter()
            .enumerate()
            .map(|(i, (id, _))| {
                row(("hidden", i), name(id), None, switch(false))
                    .hover(|row| row.bg(rgba(HOVER)))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.change_lineup(|lineup| lineup.set(id, true), cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let mut cards = vec![card(Some("In the tray"), shown)];
        if !hidden.is_empty() {
            cards.push(card(Some("Not shown"), hidden));
        }
        cards
    }
}
