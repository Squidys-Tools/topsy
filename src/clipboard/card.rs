//! One card in the clipboard strip, drawn the way Droppy draws it: a pill naming the app, the
//! kind and how long ago, a preview of what was copied, and a line of detail at the foot.

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    AnyElement, Div, FontStyle, FontWeight, HighlightStyle, ObjectFit, RenderImage, SharedString,
    StyledText, Window, div, img, prelude::*, px, rgb, rgba,
};

use super::pictures::{Pictures, Want};
use super::preview::Face;
use crate::icons::{Icon, icon};
use crate::model::clip_kind::{self, Kind, ago, detail};
use crate::model::clipboard::{Content, Entry};
use crate::model::favicon::icon_host;
use crate::model::highlight::{Token, highlight};

/// A card's side, in logical pixels.
pub const SIZE: f32 = 208.0;
/// Inside the border. GPUI only truncates text with an ellipsis in an element of definite width.
const INNER: f32 = SIZE - 4.0;
/// The app icon in the pill.
pub const APP_ICON: f32 = 16.0;
/// A file's picture on a files card.
pub const FILE_ICON: f32 = 56.0;
/// A site's icon on a link card.
const SITE_ICON: f32 = 40.0;
const CARD: u32 = 0x1c1c1e;
const PILL: u32 = 0xf2f2f2;
const CODE_PILL: u32 = 0xf0a04b;
const CODE_INK: u32 = 0xd6deeb;
const MONO: &str = "Consolas";
/// Whole lines of a text or code preview that fit a card's body, so none is cut through.
const TEXT: Face = Face {
    family: None,
    rems: 0.875,
    width: INNER - 24.0,
    lines: 5,
    wrap: true,
};
const CODE: Face = Face {
    family: Some(MONO),
    rems: 0.75,
    width: INNER - 24.0,
    lines: 6,
    wrap: false,
};

/// The pictures a card wants: the app's icon, an image's pixels, a file's thumbnail, a link's
/// site icon.
pub fn wants(entry: &Entry, kind: Kind) -> Vec<Want> {
    let mut wants: Vec<Want> = (entry.app.iter())
        .map(|a| Want::Shell(a.clone(), APP_ICON))
        .collect();
    match &entry.content {
        Content::Image { .. } => {
            wants.extend(super::image_path(entry.id).map(|p| Want::Shell(p, SIZE)));
        }
        Content::Files(files) => {
            wants.extend(files.first().map(|f| Want::Shell(f.clone(), FILE_ICON)));
        }
        Content::Text(text) if kind == Kind::Link => wants.extend(site_icon(text)),
        Content::Text(_) => {}
    }
    wants
}

fn site_icon(link: &str) -> Option<Want> {
    let host = icon_host(link)?;
    Some(Want::Site(super::icon_path(&host)?, host))
}

/// The card for `entry`, with `more` (its menu button) at the top right.
pub fn card(
    entry: &Entry,
    kind: Kind,
    selected: bool,
    now: u64,
    pictures: &Pictures,
    more: AnyElement,
    window: &Window,
) -> Div {
    let colour = match &entry.content {
        Content::Text(text) if kind == Kind::Color => clip_kind::color(text.trim()),
        _ => None,
    };
    let background = colour.map_or(rgb(CARD), |[r, g, b]| rgb(u32::from_be_bytes([0, r, g, b])));
    let dark_ink = colour.is_some_and(light);
    let (ink, faint) = if dark_ink {
        (rgb(0x111111), rgba(0x111111a6))
    } else {
        (rgb(0xffffff), rgba(0xffffff8c))
    };
    let picture = |path: &PathBuf| pictures.get(path);
    let app = entry.app.as_ref().and_then(picture);
    let mut label = kind.label().to_string();
    if let Content::Files(files) = &entry.content
        && files.len() == 1
    {
        label = "File".into();
    }
    let pill = div()
        .flex()
        .items_center()
        .gap(px(5.0))
        .px(px(7.0))
        .py(px(3.0))
        .rounded_full()
        .bg(rgb(if kind == Kind::Code { CODE_PILL } else { PILL }))
        .text_color(rgb(0x1c1c1e))
        .text_xs()
        .children(app.map(|icon| img(icon).size(px(APP_ICON)).flex_none()))
        .child(div().font_weight(FontWeight::SEMIBOLD).child(label))
        .child(
            div()
                .text_color(rgba(0x1c1c1e99))
                .child(format!("\u{b7} {}", ago(now.saturating_sub(entry.copied)))),
        );
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .p(px(8.0))
        .child(pill)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .when(entry.pinned, |side| {
                    side.child(
                        div()
                            .size(px(22.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(rgb(PILL))
                            .child(icon(Icon::Star, 12.0, rgb(0xe0453a))),
                    )
                })
                .child(more),
        );
    let footer = div()
        .flex()
        .justify_center()
        .px(px(10.0))
        .py(px(8.0))
        .text_xs()
        .text_color(faint)
        .child(
            div()
                .w(px(INNER - 20.0))
                .text_center()
                // Not `truncate`: GPUI keeps a no-wrap text's first measure, taken before this
                // row knows its width, so it would never cut. One wrapped line is measured again.
                .line_clamp(1)
                .text_ellipsis()
                .child(detail(&entry.content, kind)),
        );
    div()
        .flex()
        .flex_col()
        .flex_none()
        .size(px(SIZE))
        .rounded(px(14.0))
        .overflow_hidden()
        .bg(background)
        .text_color(ink)
        .border_2()
        .border_color(if selected { rgb(0xffffff) } else { rgba(0) })
        .child(header)
        .child(body(entry, kind, &picture, window))
        .child(footer)
}

fn body(
    entry: &Entry,
    kind: Kind,
    picture: &dyn Fn(&PathBuf) -> Option<Arc<RenderImage>>,
    window: &Window,
) -> Div {
    let body = div().flex_1().min_h_0().overflow_hidden();
    match &entry.content {
        Content::Text(text) => match kind {
            Kind::Link => body
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.0))
                .px(px(12.0))
                .children(
                    (icon_host(text).and_then(|host| super::icon_path(&host)))
                        .and_then(|path| picture(&path))
                        .map(|icon| {
                            // On a light tile: many sites draw theirs dark, for light pages.
                            div()
                                .p(px(6.0))
                                .rounded(px(10.0))
                                .bg(rgb(PILL))
                                .child(img(icon).size(px(SITE_ICON)))
                        }),
                )
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .w(px(INNER - 24.0))
                        .text_center()
                        .truncate()
                        .child(SharedString::from(clip_kind::host(text).to_string())),
                ),
            Kind::Color => body.flex().items_center().justify_center().child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .child(SharedString::from(text.trim().to_string())),
            ),
            Kind::Code => body
                .px(px(12.0))
                .font_family(MONO)
                .text_size(CODE.size())
                .text_color(rgb(CODE_INK))
                .child(
                    div()
                        .whitespace_nowrap()
                        .child(code(CODE.preview(window, text))),
                ),
            _ => body
                .px(px(12.0))
                .text_size(TEXT.size())
                .child(div().w(px(TEXT.width)).child(TEXT.preview(window, text))),
        },
        Content::Image { .. } => match super::image_path(entry.id).and_then(|p| picture(&p)) {
            Some(image) => body.child(img(image).size_full().object_fit(ObjectFit::Cover)),
            None => body.bg(rgba(0xffffff0a)),
        },
        Content::Files(files) => body
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .px(px(12.0))
            .children(
                files
                    .first()
                    .and_then(picture)
                    .map(|icon| img(icon).size(px(FILE_ICON)).object_fit(ObjectFit::Contain)),
            )
            .children(files.iter().take(3).map(|f| {
                let name = f.file_name().unwrap_or(f.as_os_str());
                div()
                    .w(px(INNER - 24.0))
                    .text_center()
                    .text_xs()
                    .truncate()
                    .child(name.to_string_lossy().to_string())
            })),
    }
}

/// Code, coloured by [`highlight`].
fn code(text: String) -> StyledText {
    let spans = highlight(&text);
    let style = |token| {
        let (colour, italic) = match token {
            Token::Keyword => (0xc792ea, false),
            Token::Str => (0xc3e88d, false),
            Token::Comment => (0x7f848e, true),
            Token::Number => (0xf78c6c, false),
        };
        HighlightStyle {
            color: Some(rgb(colour).into()),
            font_style: italic.then_some(FontStyle::Italic),
            ..Default::default()
        }
    };
    StyledText::new(text).with_highlights(spans.into_iter().map(|(range, t)| (range, style(t))))
}

/// Whether dark text reads better on this colour.
fn light([r, g, b]: [u8; 3]) -> bool {
    r as u32 * 299 + g as u32 * 587 + b as u32 * 114 > 150_000
}
