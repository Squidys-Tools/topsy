//! Drawing the tray icon's menu and its submenu: Droppy's menu-bar menu, rows of a fixed height
//! with a gutter for an icon or a check, a highlight that follows the pointer and the arrows,
//! and greyed rows that can't be chosen.

use gpui::{
    AnyElement, Context, Div, Hsla, SharedString, Stateful, Window, div, prelude::*, px, rgb, rgba,
};

use super::lines::{GAP, HEADER, Item, Line, PAD, ROW, SUB_WIDTH, WIDTH, height};
use super::menu::{Menu, Submenu};
use crate::icons::{self, Icon};
use crate::settings::Setting;
use crate::update;
use model::menu::shown;
use model::update::Nightly;

const TEXT: u32 = 0xffffffff;
const DIM: u32 = 0xffffff73;
/// A frosted highlight rather than an accent colour, so the row lifts out of the glass.
const SELECT: u32 = 0xffffff26;
const RULE: u32 = 0xffffff1a;
const GREEN: u32 = 0x34c759;
const ON_GREEN: u32 = 0x04210c;
const GUTTER: f32 = 15.0;

/// One row: the gutter, the label, and what sits at its right end.
fn row(
    id: SharedString,
    highlighted: bool,
    enabled: bool,
    gutter: Option<AnyElement>,
    label: &str,
    detail: Option<AnyElement>,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(ROW))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(9.0))
        .pl(px(8.0))
        .pr(px(10.0))
        .rounded(px(7.0))
        .when(highlighted, |row| row.bg(rgba(SELECT)))
        .when(!enabled, |row| row.opacity(0.35))
        .child(
            div()
                .w(px(GUTTER))
                .flex_none()
                .flex()
                .justify_center()
                .children(gutter),
        )
        .child(div().flex_1().child(shown(label)))
        .children(detail)
}

fn detail(text: String, highlighted: bool) -> AnyElement {
    let color = if highlighted {
        rgba(0xffffffd9)
    } else {
        rgba(DIM)
    };
    div()
        .text_size(px(11.5))
        .text_color(color)
        .child(text)
        .into_any_element()
}

fn glyph(icon: Icon, color: impl Into<Hsla>) -> AnyElement {
    icons::icon(icon, 15.0, color).into_any_element()
}

fn files(count: usize) -> Option<String> {
    match count {
        0 => None,
        1 => Some("1 file".into()),
        n => Some(format!("{n} files")),
    }
}

/// The app's tile, its name and build, and the update's pill when one is waiting.
fn header(cx: &mut Context<Menu>) -> impl IntoElement + use<> {
    let tile = div()
        .size(px(32.0))
        .flex_none()
        .flex()
        .justify_center()
        .rounded(px(9.0))
        .bg(rgb(0x000000))
        .border_1()
        .border_color(rgba(0xffffff38))
        .child(
            div()
                .mt(px(11.0))
                .w(px(17.0))
                .h(px(7.0))
                .rounded_b(px(5.0))
                .bg(rgb(0xffffff)),
        );
    let build = match Nightly::parse(update::VERSION) {
        Some(nightly) => format!("Nightly {}", nightly.short()),
        None => format!("Version {}", update::VERSION),
    };
    let pill = update::ready().map(|version| {
        div()
            .id("update-pill")
            .ml_auto()
            .px(px(9.0))
            .py(px(2.0))
            .rounded_full()
            .bg(rgb(GREEN))
            .text_color(rgb(ON_GREEN))
            .text_size(px(10.5))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child("Update")
            .on_click(cx.listener(move |menu, _, _, cx| menu.choose(Item::Update(version), cx)))
    });
    div()
        .h(px(HEADER))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(8.0))
        .child(tile)
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("topsy"),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgba(0xffffff80))
                        .child(build),
                ),
        )
        .children(pill)
}

fn gap() -> Div {
    div()
        .h(px(GAP))
        .flex_none()
        .flex()
        .items_center()
        .px(px(8.0))
        .child(div().h(px(1.0)).w_full().bg(rgba(RULE)))
}

impl Menu {
    fn line(&self, at: usize, line: Line, cx: &mut Context<Self>) -> AnyElement {
        let item = match line {
            Line::Header => return header(cx).into_any_element(),
            Line::Gap => return gap().into_any_element(),
            Line::Item(item) => item,
        };
        let enabled = self.enabled(item);
        let lit = enabled && self.highlight == Some(at);
        let icon_color = if lit { rgb(0xffffff) } else { rgba(0xffffffd9) };
        let gutter = match item {
            Item::Show => Some(glyph(Icon::Tray, icon_color)),
            Item::Clear => Some(glyph(Icon::Close, icon_color)),
            Item::Basket => Some(glyph(Icon::Inbox, icon_color)),
            Item::Player(_) => Some(glyph(Icon::Open, icon_color)),
            Item::Clipboard => Some(glyph(Icon::Copy, icon_color)),
            Item::Update(_) => Some(glyph(Icon::Download, rgb(GREEN))),
            Item::Options | Item::Settings | Item::AddHeadset | Item::Quit => None,
        };
        let right = match item {
            Item::Show => files(self.counts.shelf).map(|text| detail(text, lit)),
            Item::Basket => files(self.counts.basket).map(|text| detail(text, lit)),
            Item::Clipboard => Some(detail("Ctrl+Alt+V".into(), lit)),
            Item::Options => Some(glyph(Icon::ChevronRight, icon_color)),
            Item::Update(version) => Some(detail(version.short(), lit)),
            Item::Clear | Item::Player(_) | Item::Settings | Item::AddHeadset | Item::Quit => None,
        };
        let row = row(
            SharedString::from(format!("item-{at}")),
            lit,
            enabled,
            gutter,
            item.label(),
            right,
        );
        if !enabled {
            return row.into_any_element();
        }
        row.on_hover(cx.listener(move |menu, hovered: &bool, _, cx| menu.hover(at, *hovered, cx)))
            .on_click(cx.listener(move |menu, _, _, cx| menu.choose(item, cx)))
            .into_any_element()
    }
}

impl Render for Menu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tall = height(&self.lines);
        self.frame
            .sync(WIDTH, tall, 0.0, window.scale_factor(), false, cx);
        let viewport = window.viewport_size();
        let lines: Vec<AnyElement> = (self.lines.clone().into_iter().enumerate())
            .map(|(at, line)| self.line(at, line, cx))
            .collect();
        let content = div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(PAD))
            .children(lines)
            .into_any_element();
        div()
            .id("menu")
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .text_color(rgba(TEXT))
            .text_size(px(12.5))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Menu::on_key))
            .children(self.frame.draw(viewport, tall, content, None))
    }
}

impl Render for Submenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tall = PAD * 2.0 + ROW * Setting::ALL.len() as f32;
        self.frame
            .sync(SUB_WIDTH, tall, 0.0, window.scale_factor(), false, cx);
        let viewport = window.viewport_size();
        let (on, lit) = self
            .menu
            .upgrade()
            .map_or(([false; Setting::ALL.len()], None), |menu| {
                let menu = menu.read(cx);
                (menu.on, menu.sub_highlight)
            });
        let rows = Setting::ALL.into_iter().enumerate().map(|(at, setting)| {
            let lit = lit == Some(at);
            let tick = on[at].then(|| glyph(Icon::Tick, rgb(0xffffff)));
            let (hover, click) = (self.menu.clone(), self.menu.clone());
            row(
                SharedString::from(format!("setting-{at}")),
                lit,
                true,
                tick,
                setting.menu_label(),
                None,
            )
            .on_hover(move |hovered: &bool, _, cx| {
                hover
                    .update(cx, |menu, cx| menu.hover_sub(at, *hovered, cx))
                    .ok();
            })
            .on_click(move |_, _, cx| {
                click
                    .update(cx, |menu, cx| menu.choose_setting(setting, cx))
                    .ok();
            })
        });
        let content = div()
            .size_full()
            .flex()
            .flex_col()
            .p(px(PAD))
            .children(rows)
            .into_any_element();
        div()
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .text_color(rgba(TEXT))
            .text_size(px(12.5))
            .children(self.frame.draw(viewport, tall, content, None))
    }
}
