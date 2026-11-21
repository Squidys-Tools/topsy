//! Drawing the palette in the notch's own materials: black at the top giving way to glass, pills
//! for rows and buttons, white for what is chosen. The search field and a filter across the top,
//! the copies down the left in sections, the selected one's [`detail`] on the right, and a footer
//! naming what Enter does.

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
use model::clip_list::{short_ago, title};
use model::clipboard::{Content, Entry};
use model::favicon::icon_host;

/// The palette's body, and its bands, in logical pixels; the notch's ears are beside it.
pub const WIDTH: f32 = 760.0;
pub const HEIGHT: f32 = 500.0;
const SEARCH: f32 = 60.0;
pub const FOOTER: f32 = 46.0;
pub const PAD: f32 = 10.0;
const LIST: f32 = 286.0;
const ROW: f32 = 38.0;
const ROW_PAD: f32 = 12.0;
const HEADER: f32 = 30.0;
/// A row's picture: the app's icon, a site's, a file's, an image's.
const ROW_ICON: f32 = 20.0;
const GAP: f32 = 10.0;
const AGE: f32 = 30.0;
const TITLE: f32 = LIST - 2.0 * ROW_PAD - ROW_ICON - 2.0 * GAP - AGE;
pub const SMALL_ICON: f32 = 16.0;

pub const TEXT: u32 = 0xffffffeb;
pub const DIM: u32 = 0xffffff85;
pub const FAINT: u32 = 0xffffff57;
/// The notch's frosted white, for what is chosen and for buttons.
pub const SELECT: u32 = 0xffffff29;
pub const HOVER: u32 = 0xffffff10;
pub const FROST: u32 = 0xffffff1f;
/// Droppy's white pill and the ink on it.
pub const PILL: u32 = 0xf2f2f2;
pub const ON_PILL: u32 = 0x1c1c1e;
const KEY: u32 = 0xffffff24;

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
        Want::Shell(exe.to_path_buf(), SMALL_ICON),
        Want::Name(exe.to_path_buf()),
    ]
}

fn site_icon(link: &str) -> Option<Want> {
    let host = icon_host(link)?;
    Some(Want::Site(super::icon_path(&host)?, host))
}

/// A key, as a little pill.
pub fn key(label: &'static str) -> Div {
    div()
        .h(px(20.0))
        .min_w(px(20.0))
        .px(px(6.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgba(KEY))
        .text_size(px(10.5))
        .text_color(rgba(TEXT))
        .child(label)
}

/// A light tile behind a site's icon: many sites draw theirs dark, for light pages.
pub fn site_tile(image: std::sync::Arc<gpui::RenderImage>, size: f32) -> Div {
    div()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size * 0.28))
        .bg(rgb(PILL))
        .child(img(image).size(px(size * 0.7)))
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
        let option = |label: &'static str, pinned: bool| {
            let on = self.pinned_only == pinned;
            div()
                .id(label)
                .px(px(12.0))
                .py(px(4.0))
                .rounded_full()
                .when(on, |option| {
                    option
                        .bg(rgb(PILL))
                        .text_color(rgb(ON_PILL))
                        .font_weight(FontWeight::SEMIBOLD)
                })
                .when(!on, |option| {
                    option
                        .text_color(rgba(DIM))
                        .hover(|option| option.text_color(rgba(TEXT)))
                })
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_pinned_only(pinned, cx)))
        };
        let filter = div()
            .flex()
            .gap(px(2.0))
            .p(px(3.0))
            .rounded_full()
            .bg(rgba(0xffffff14))
            .text_size(px(12.0))
            .child(option("All", false))
            .child(option("Pinned", true));
        let close = div()
            .id("close")
            .size(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(rgba(FROST))
            .hover(|close| close.bg(rgba(SELECT)))
            .child(icon(Icon::Close, 12.0, rgb(0xffffff)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.fold(cx);
            }));
        div()
            .h(px(SEARCH))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.0))
            .pl(px(22.0))
            .pr(px(16.0))
            .child(icon(Icon::Search, 18.0, rgba(DIM)))
            .child(div().flex_1().text_size(px(17.0)).child(field))
            .child(filter)
            .child(close)
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
                        .size(px(18.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(rgba(0xffffff33))
                        .bg(rgb(u32::from_be_bytes([0, r, g, b])))
                        .into_any_element();
                }
                Kind::Link => {
                    let site = (icon_host(text).and_then(|host| super::icon_path(&host)))
                        .and_then(|path| self.pictures.site(&path));
                    if let Some(site) = site {
                        return site_tile(site, ROW_ICON).into_any_element();
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
                .rounded(px(5.0))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => icon(Icon::Copy, 16.0, rgba(DIM)).into_any_element(),
        }
    }

    fn rows(&self, now: u64, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgba(FAINT))
                        .child(section.label().to_uppercase())
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
                .rounded_full()
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
                    // Not `truncate`: GPUI keeps a no-wrap text's first measure, taken before
                    // the row knows its width. One wrapped line, of a set width, is cut.
                    div()
                        .w(px(TITLE))
                        .line_clamp(1)
                        .text_ellipsis()
                        .child(SharedString::from(title(&entry.content, kind))),
                )
                .child(
                    div()
                        .w(px(AGE))
                        .flex_none()
                        .text_right()
                        .text_size(px(11.5))
                        .text_color(rgba(if selected { DIM } else { FAINT }))
                        .child(short_ago(now.saturating_sub(entry.copied))),
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
        let now = super::now();
        let list = div()
            .id("rows")
            .w(px(LIST))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .pb(px(PAD))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .children(self.rows(now, cx));
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap(px(PAD))
            .px(px(PAD))
            .child(list)
            .child(detail::pane(entry, kind, now, &self.pictures, window))
            .into_any_element()
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let target = self.target.as_deref();
        let label = match target.map(|exe| self.pictures.name(exe)) {
            Some(name) if !name.is_empty() => format!("Paste to {name}"),
            Some(_) | None => "Paste".into(),
        };
        let target_icon = target.and_then(|exe| self.pictures.get(exe, SMALL_ICON));
        let has_entry = self.entry(self.selected).is_some();
        let button = |id: &'static str| {
            div()
                .id(id)
                .h(px(30.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .pl(px(12.0))
                .pr(px(5.0))
                .rounded_full()
                .when(!has_entry, |button| button.opacity(0.4))
        };
        let paste = button("paste")
            .bg(rgb(PILL))
            .text_color(rgb(ON_PILL))
            .font_weight(FontWeight::SEMIBOLD)
            .children(target_icon.map(|image| img(image).size(px(SMALL_ICON)).flex_none()))
            .child(label)
            .child(
                key("\u{21b5}")
                    .bg(rgba(0x0000001a))
                    .text_color(rgb(ON_PILL)),
            )
            .on_click(cx.listener(|this, _, window, _| this.take(this.selected, true, window)));
        let open = self.panel.is_some();
        let actions = button("actions")
            .bg(rgba(if open { SELECT } else { FROST }))
            .when(!open, |button| {
                button.hover(|button| button.bg(rgba(SELECT)))
            })
            .child("Actions")
            .child(div().flex().gap(px(3.0)).child(key("Ctrl")).child(key("K")))
            .on_click(cx.listener(|this, _, _, cx| match this.panel {
                Some(_) => this.close_panel(cx),
                None => this.open_panel(this.selected, cx),
            }));
        let count = match self.shown.len() {
            0 => "Clipboard".to_string(),
            all => format!("{} of {all}", self.selected + 1),
        };
        div()
            .h(px(FOOTER))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(px(20.0))
            .pr(px(12.0))
            .text_size(px(12.0))
            .child(icon(Icon::Copy, 14.0, rgba(DIM)))
            .child(div().flex_1().text_color(rgba(DIM)).child(count))
            .child(paste)
            .child(actions)
    }
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.step(window, cx);
        let height = self.height.value;
        self.frame
            .sync(WIDTH, height, 0.0, window.scale_factor(), false, cx);
        let viewport = window.viewport_size();
        // Laid out at its full height and revealed as the notch grows, fading in with it.
        let shown = (height / HEIGHT).clamp(0.0, 1.0);
        let palette = div()
            .relative()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .flex_none()
            .flex()
            .flex_col()
            .opacity(shown)
            .text_color(rgba(TEXT))
            .text_size(px(13.0))
            .child(self.search(cx))
            .child(self.body(window, cx))
            .child(self.footer(cx))
            .children(self.panel_layer(cx));
        let content = div()
            .size_full()
            .overflow_hidden()
            .child(palette)
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
