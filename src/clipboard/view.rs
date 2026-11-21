//! Drawing the palette: the search field across the top, the list of copies on the left in
//! sections, the selected one's [`detail`] on the right, and a footer naming what Enter does.

use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, MouseButton, MouseUpEvent, ObjectFit,
    SharedString, Window, div, img, prelude::*, px, rgb, rgba,
};

use super::detail;
use super::palette::Palette;
use super::pictures::Want;
use crate::icons::{Icon, icon};
use model::clip_kind::{self, Kind};
use model::clip_list::title;
use model::clipboard::{Content, Entry};
use model::favicon::icon_host;

/// The palette's size, and its bands', in logical pixels.
pub const WIDTH: f32 = 760.0;
pub const HEIGHT: f32 = 500.0;
const SEARCH: f32 = 54.0;
pub const FOOTER: f32 = 40.0;
const LIST: f32 = WIDTH - detail::WIDTH - 1.0;
const LIST_PAD: f32 = 8.0;
const ROW: f32 = 36.0;
const ROW_PAD: f32 = 10.0;
const HEADER: f32 = 28.0;
/// A row's picture: the app's icon, a site's, a file's, an image's.
const ROW_ICON: f32 = 20.0;
const GAP: f32 = 10.0;
const TITLE: f32 = LIST - 2.0 * LIST_PAD - 2.0 * ROW_PAD - ROW_ICON - GAP;
const TARGET_ICON: f32 = 16.0;

pub const TEXT: u32 = 0xffffffeb;
pub const DIM: u32 = 0xffffff80;
pub const FAINT: u32 = 0xffffff59;
pub const RULE: u32 = 0xffffff17;
/// A frosted highlight, as the tray icon's menu has.
pub const SELECT: u32 = 0xffffff21;
const HOVER: u32 = 0xffffff0d;
const KEY: u32 = 0xffffff1a;

/// What a row wants looked up: its app's icon, and an image's, a file's or a site's picture.
pub fn wants(entry: &Entry, kind: Kind) -> Vec<Want> {
    let mut wants: Vec<Want> = (entry.app.iter())
        .map(|a| Want::Shell(a.clone(), ROW_ICON))
        .collect();
    match &entry.content {
        Content::Image { .. } => {
            wants.extend(super::image_path(entry.id).map(|p| Want::Shell(p, ROW_ICON)));
        }
        Content::Files(files) => {
            wants.extend(files.first().map(|f| Want::Shell(f.clone(), ROW_ICON)));
        }
        Content::Text(text) => match kind {
            Kind::Link => wants.extend(site_icon(text)),
            Kind::Text | Kind::Color | Kind::Code | Kind::Image | Kind::Files => {}
        },
    }
    wants
}

/// The footer's picture and name of the app a paste goes to.
pub fn target_wants(exe: &Path) -> [Want; 2] {
    [
        Want::Shell(exe.to_path_buf(), TARGET_ICON),
        Want::Name(exe.to_path_buf()),
    ]
}

fn site_icon(link: &str) -> Option<Want> {
    let host = icon_host(link)?;
    Some(Want::Site(super::icon_path(&host)?, host))
}

fn rule() -> Div {
    div().h(px(1.0)).w_full().flex_none().bg(rgba(RULE))
}

/// A key as the footer draws it.
pub fn key(label: &'static str) -> Div {
    div()
        .h(px(18.0))
        .min_w(px(18.0))
        .px(px(5.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .bg(rgba(KEY))
        .text_size(px(10.5))
        .text_color(rgba(TEXT))
        .child(label)
}

impl Palette {
    fn search(&self, cx: &mut Context<Self>) -> Div {
        let field = if self.query.is_empty() {
            div()
                .text_color(rgba(FAINT))
                .child("Search clipboard history")
        } else {
            div().child(format!("{}\u{258f}", self.query))
        };
        let filter = div()
            .id("filter")
            .flex()
            .items_center()
            .gap(px(6.0))
            .h(px(26.0))
            .px(px(10.0))
            .rounded(px(7.0))
            .border_1()
            .border_color(rgba(RULE))
            .text_size(px(12.0))
            .text_color(rgba(TEXT))
            .hover(|filter| filter.bg(rgba(HOVER)))
            .when(self.pinned_only, |filter| {
                filter.child(icon(Icon::Star, 12.0, rgb(0xf5c451)))
            })
            .child(if self.pinned_only { "Pinned" } else { "All" })
            .child(key("Tab"))
            .on_click(cx.listener(|this, _, _, cx| this.set_pinned_only(!this.pinned_only, cx)));
        div()
            .h(px(SEARCH))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(18.0))
            .child(icon(Icon::Search, 18.0, rgba(DIM)))
            .child(div().flex_1().text_size(px(16.0)).child(field))
            .child(filter)
    }

    /// The row's picture: a colour's swatch, an image's or a file's thumbnail, a site's icon,
    /// else the icon of the app it came from.
    fn row_icon(&self, entry: &Entry, kind: Kind) -> AnyElement {
        let picture = |path: &PathBuf| self.pictures.get(path, ROW_ICON);
        let app = || entry.app.as_ref().and_then(picture);
        let shown = match &entry.content {
            Content::Text(text) => match kind {
                Kind::Color => {
                    let [r, g, b] = clip_kind::color(text.trim()).unwrap_or_default();
                    return div()
                        .size(px(16.0))
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(rgba(0xffffff33))
                        .bg(rgb(u32::from_be_bytes([0, r, g, b])))
                        .into_any_element();
                }
                Kind::Link => {
                    let site = (icon_host(text).and_then(|host| super::icon_path(&host)))
                        .and_then(|path| self.pictures.site(&path));
                    if let Some(site) = site {
                        // On a light tile, as in the preview: many sites draw theirs dark.
                        return div()
                            .size(px(ROW_ICON))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.0))
                            .bg(rgb(0xf2f2f2))
                            .child(img(site).size(px(ROW_ICON - 6.0)))
                            .into_any_element();
                    }
                    app()
                }
                Kind::Text | Kind::Code | Kind::Image | Kind::Files => app(),
            },
            Content::Image { .. } => super::image_path(entry.id).as_ref().and_then(picture),
            Content::Files(files) => files.first().and_then(picture),
        };
        match shown {
            Some(image) => img(image)
                .size(px(ROW_ICON))
                .rounded(px(3.0))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => icon(Icon::Copy, 16.0, rgba(DIM)).into_any_element(),
        }
    }

    fn rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut children = Vec::new();
        let mut last = None;
        for (at, &i) in self.shown.iter().enumerate() {
            let entry = &self.entries[i];
            let kind = self.kinds[i];
            let section = self.section_of(at);
            if section != last
                && let Some(section) = section
            {
                children.push(
                    div()
                        .h(px(HEADER))
                        .flex_none()
                        .flex()
                        .items_end()
                        .pb(px(6.0))
                        .px(px(ROW_PAD))
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgba(DIM))
                        .child(section.label())
                        .into_any_element(),
                );
            }
            last = section;
            let selected = at == self.selected;
            let row = div()
                .id(("row", entry.id as usize))
                .h(px(ROW))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(GAP))
                .px(px(ROW_PAD))
                .rounded(px(8.0))
                .when(selected, |row| row.bg(rgba(SELECT)))
                .when(!selected, |row| row.hover(|row| row.bg(rgba(HOVER))))
                .child(
                    div()
                        .size(px(ROW_ICON))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(self.row_icon(entry, kind)),
                )
                .child(
                    // Cut to one line at a set width; see the detail pane's values.
                    div()
                        .w(px(TITLE))
                        .line_clamp(1)
                        .text_ellipsis()
                        .child(SharedString::from(title(&entry.content, kind))),
                )
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    this.click(at, event.click_count(), window, cx);
                }))
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, _: &MouseUpEvent, _, cx| this.open_panel(at, cx)),
                );
            children.push(row.into_any_element());
        }
        children
    }

    fn body(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(entry) = self.entry(self.selected) else {
            let empty = if self.entries.is_empty() {
                "Nothing copied yet"
            } else if self.pinned_only && self.query.is_empty() {
                "Nothing pinned yet"
            } else {
                "Nothing matches"
            };
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgba(DIM))
                .child(empty)
                .into_any_element();
        };
        let kind = self.kinds[self.shown[self.selected]];
        let list = div()
            .id("rows")
            .w(px(LIST))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .p(px(LIST_PAD))
            .pt(px(0.0))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .children(self.rows(cx));
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(list)
            .child(div().w(px(1.0)).h_full().flex_none().bg(rgba(RULE)))
            .child(detail::pane(
                entry,
                kind,
                super::now(),
                &self.pictures,
                window,
            ))
            .into_any_element()
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let target = self.target.as_deref();
        let label = match target.map(|exe| self.pictures.name(exe)) {
            Some(name) if !name.is_empty() => format!("Paste to {name}"),
            Some(_) | None => "Paste".into(),
        };
        let target_icon = target.and_then(|exe| self.pictures.get(exe, TARGET_ICON));
        let has_entry = self.entry(self.selected).is_some();
        let button = |id: &'static str| {
            div()
                .id(id)
                .h(px(28.0))
                .flex()
                .items_center()
                .gap(px(7.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .when(!has_entry, |button| button.opacity(0.4))
                .when(has_entry, |button| {
                    button.hover(|button| button.bg(rgba(HOVER)))
                })
        };
        let paste = button("paste")
            .children(target_icon.map(|image| img(image).size(px(TARGET_ICON)).flex_none()))
            .child(label)
            .child(key("\u{21b5}"))
            .on_click(cx.listener(|this, _, window, _| this.take(this.selected, true, window)));
        let actions = button("actions")
            .when(self.panel.is_some(), |button| button.bg(rgba(SELECT)))
            .child("Actions")
            .child(div().flex().gap(px(3.0)).child(key("Ctrl")).child(key("K")))
            .on_click(cx.listener(|this, _, _, cx| match this.panel {
                Some(_) => this.close_panel(cx),
                None => this.open_panel(this.selected, cx),
            }));
        div()
            .h(px(FOOTER))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(16.0))
            .pr(px(6.0))
            .bg(rgba(0x0000002e))
            .text_size(px(12.0))
            .child(icon(Icon::Copy, 14.0, rgba(DIM)))
            .child(
                div()
                    .flex_1()
                    .pl(px(6.0))
                    .text_color(rgba(DIM))
                    .child("Clipboard History"),
            )
            .child(paste)
            .child(div().w(px(1.0)).h(px(14.0)).mx(px(4.0)).bg(rgba(RULE)))
            .child(actions)
    }
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frame
            .sync(WIDTH, HEIGHT, 0.0, window.scale_factor(), false, cx);
        let viewport = window.viewport_size();
        let content = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .text_color(rgba(TEXT))
            .text_size(px(13.0))
            .child(self.search(cx))
            .child(rule())
            .child(self.body(window, cx))
            .child(rule())
            .child(self.footer(cx))
            .children(self.panel_layer(cx))
            .into_any_element();
        div()
            .id("palette")
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Palette::on_key))
            .children(self.frame.draw(viewport, HEIGHT, content, None))
    }
}
