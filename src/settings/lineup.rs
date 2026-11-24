//! The widgets on the Shelf page: one icon each, ticked when it is in the tray, the ticked ones
//! dragged into the order the tray shows them.

use gpui::{Context, Div, FontWeight, Stateful, Window, div, prelude::*, px, rgb, rgba};

use super::controls::{BLUE, DIM, TEXT, section};
use super::window::{Change, SettingsWindow};
use crate::droplets;
use crate::icons::{self, Icon};
use model::droplets::Lineup;

const ICON: f32 = 46.0;

/// A widget being dragged by its icon, and what follows the cursor meanwhile.
#[derive(Clone)]
struct Dragged {
    id: &'static str,
    icon: Icon,
    tint: u32,
}

impl Render for Dragged {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        square(self.icon, self.tint).shadow_lg().opacity(0.9)
    }
}

fn square(icon: Icon, tint: u32) -> Div {
    div()
        .size(px(ICON))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(12.0))
        .bg(rgb(tint))
        .child(icons::icon(icon, 22.0, rgb(TEXT)))
}

/// A widget's icon over its name, with a tick in the corner when it is on.
fn widget(i: usize, icon: Icon, tint: u32, name: &'static str, on: bool) -> Stateful<Div> {
    let tick = div()
        .absolute()
        .top(px(-4.0))
        .right(px(4.0))
        .size(px(18.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgb(BLUE))
        .border_2()
        .border_color(rgb(0x262628))
        .child(icons::icon(Icon::Tick, 10.0, rgb(TEXT)));
    div()
        .id(("widget", i))
        .relative()
        .w(px(68.0))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(5.0))
        .py(px(4.0))
        .rounded(px(10.0))
        .when(!on, |widget| widget.opacity(0.4))
        .hover(|widget| widget.bg(rgba(0xffffff0d)))
        .child(square(icon, tint))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(if on { rgb(TEXT) } else { rgba(DIM) })
                .child(name),
        )
        .children(on.then_some(tick))
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

    /// The widgets that are on, in order, as the shelf's preview draws them.
    pub(super) fn shown_widgets(&self) -> Vec<(Icon, u32)> {
        (self.lineup.ids().iter())
            .filter_map(|id| droplets::entry(id))
            .map(|entry| (entry.icon, entry.tint))
            .collect()
    }

    pub(super) fn widgets(&self, cx: &mut Context<Self>) -> Div {
        let rows = self.lineup.rows(&droplets::ids());
        let shown = rows.iter().filter(|(_, on)| *on).count();
        let widgets = rows.into_iter().enumerate().filter_map(|(i, (id, on))| {
            let entry = droplets::entry(id)?;
            let (icon, tint) = (entry.icon, entry.tint);
            let widget = widget(i, icon, tint, entry.name, on).on_click(cx.listener(
                move |view, _, _, cx| view.change_lineup(|lineup| lineup.set(id, !on), cx),
            ));
            Some(if on {
                widget
                    .on_drag(Dragged { id, icon, tint }, |dragged, _, _, cx| {
                        cx.new(|_| dragged.clone())
                    })
                    .drag_over::<Dragged>(|widget, _, _, _| widget.bg(rgba(0x1f7aff40)))
                    .on_drop(cx.listener(move |view, dragged: &Dragged, _, cx| {
                        view.change_lineup(|lineup| lineup.move_to(dragged.id, i), cx);
                    }))
            } else {
                widget
            })
        });
        let grid = div()
            .flex()
            .flex_wrap()
            .gap(px(6.0))
            .p(px(12.0))
            .children(widgets)
            .into_any_element();
        let hint = div()
            .px(px(14.0))
            .py(px(9.0))
            .text_size(px(11.5))
            .text_color(rgba(DIM))
            .font_weight(FontWeight::NORMAL)
            .child(if shown == 0 {
                "No widgets in the tray. Click one to add it."
            } else {
                "Click a widget to add or remove it; drag the ticked ones into the order the tray shows them."
            })
            .into_any_element();
        section(Some("Widgets"), vec![grid, hint])
    }
}
