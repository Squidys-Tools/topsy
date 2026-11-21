//! The palette's right-hand pane: the selected copy as one of Droppy's cards, grown to fill it
//! (a white tag naming the app, the kind and how long ago, over a large preview: its text, code
//! coloured, a link's site, a colour's swatch, the image, the files), and its facts as pills.

use std::path::Path;

use gpui::{
    AnyElement, Div, FontStyle, FontWeight, HighlightStyle, ObjectFit, SharedString, StyledText,
    Window, div, img, prelude::*, px, rgb, rgba,
};

use super::pictures::{Pictures, Want};
use super::preview::Face;
use super::view::{self, DIM, ON_PILL, PAD, PILL, SMALL_ICON, TEXT};
use crate::icons::{Icon, icon};
use model::clip_kind::{self, Kind};
use model::clip_list::{ago, details, kind_label};
use model::clipboard::{Content, Entry};
use model::favicon::icon_host;
use model::highlight::{Token, highlight};

/// The pane's width, beside the list, and the card's room round its preview.
pub const WIDTH: f32 = 444.0;
const INSET: f32 = 16.0;
const INNER: f32 = WIDTH - 2.0 * INSET;
/// Pictures the preview asks for: a copied image across the card, a file's icon.
const IMAGE: f32 = INNER;
const FILE_ICON: f32 = 64.0;
const SITE_ICON: f32 = 60.0;
/// Droppy's card, a shade above the glass.
const CARD: u32 = 0x1c1c1e;
const FACT: u32 = 0xffffff14;
const CODE_INK: u32 = 0xd6deeb;
const MONO: &str = "Consolas";
/// Whole lines that fit the card, so none is cut through.
const TEXT_FACE: Face = Face {
    family: None,
    rems: 0.875,
    width: INNER,
    lines: 11,
    wrap: true,
};
const CODE_FACE: Face = Face {
    family: Some(MONO),
    rems: 0.75,
    width: INNER,
    lines: 12,
    wrap: false,
};
const LINK_FACE: Face = Face {
    family: None,
    rems: 0.75,
    width: INNER,
    lines: 3,
    wrap: true,
};

/// What the pane wants looked up for `entry`: its picture, and the icon and name of the app it
/// came from.
pub fn wants(entry: &Entry) -> Vec<Want> {
    let mut wants: Vec<Want> = (entry.app.iter())
        .flat_map(|a| [Want::Name(a.clone()), Want::Shell(a.clone(), SMALL_ICON)])
        .collect();
    match &entry.content {
        Content::Image { .. } => {
            wants.extend(super::image_path(entry.id).map(|p| Want::Shell(p, IMAGE)));
        }
        Content::Files(files) => {
            wants.extend(files.first().map(|f| Want::Shell(f.clone(), FILE_ICON)));
        }
        Content::Text(_) => {}
    }
    wants
}

/// Droppy's white tag: the app's icon and name, the kind, how long ago, and a star if pinned.
fn tag(entry: &Entry, kind: Kind, now: u64, pictures: &Pictures) -> Div {
    let app = entry.app.as_deref();
    let app_icon = app.and_then(|exe| pictures.get(exe, SMALL_ICON));
    let name = app.map(|exe| pictures.name(exe)).filter(|n| !n.is_empty());
    let mut said = kind_label(&entry.content, kind).to_string();
    if let Some(name) = name {
        said = format!("{name} \u{b7} {said}");
    }
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .pl(px(if app_icon.is_some() { 5.0 } else { 10.0 }))
        .pr(px(10.0))
        .py(px(4.0))
        .rounded_full()
        .bg(rgb(PILL))
        .text_color(rgb(ON_PILL))
        .text_size(px(11.5))
        .children(app_icon.map(|image| img(image).size(px(SMALL_ICON)).flex_none()))
        .child(div().font_weight(FontWeight::SEMIBOLD).child(said))
        .child(
            div()
                .text_color(rgba(0x1c1c1e99))
                .child(format!("\u{b7} {}", ago(now.saturating_sub(entry.copied)))),
        )
        .when(entry.pinned, |tag| {
            tag.child(icon(Icon::Star, 11.0, rgb(0xe0453a)))
        })
}

fn fact(label: &'static str, value: String) -> AnyElement {
    div()
        .flex()
        .gap(px(5.0))
        .max_w(px(WIDTH))
        .overflow_hidden()
        .whitespace_nowrap()
        .px(px(10.0))
        .py(px(5.0))
        .rounded_full()
        .bg(rgba(FACT))
        .child(div().text_color(rgba(DIM)).child(label))
        .child(value)
        .into_any_element()
}

pub fn pane(entry: &Entry, kind: Kind, now: u64, pictures: &Pictures, window: &Window) -> Div {
    let card = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .rounded(px(18.0))
        .overflow_hidden()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgba(0xffffff0f))
        .child(
            div()
                .flex_none()
                .flex()
                .p(px(10.0))
                .child(tag(entry, kind, now, pictures)),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .px(px(INSET))
                .pt(px(4.0))
                .pb(px(INSET))
                .child(preview(entry, kind, pictures, window)),
        );
    let facts = details(&entry.content, kind)
        .into_iter()
        .map(|(label, value)| fact(label, value));
    div()
        .w(px(WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .pb(px(PAD))
        .text_size(px(12.0))
        .child(card)
        .child(div().flex().flex_wrap().gap(px(6.0)).children(facts))
}

fn centred() -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
}

fn preview(entry: &Entry, kind: Kind, pictures: &Pictures, window: &Window) -> Div {
    match &entry.content {
        Content::Text(text) => match kind {
            Kind::Link => centred()
                .gap(px(10.0))
                .children(
                    (icon_host(text).and_then(|host| super::icon_path(&host)))
                        .and_then(|path| pictures.site(&path))
                        .map(|icon| view::site_tile(icon, SITE_ICON)),
                )
                .child(
                    div()
                        .w(px(INNER))
                        .text_center()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(SharedString::from(clip_kind::host(text).to_string())),
                )
                .child(
                    div()
                        .w(px(LINK_FACE.width))
                        .text_center()
                        .text_size(LINK_FACE.size())
                        .text_color(rgba(DIM))
                        .child(LINK_FACE.preview(window, text)),
                ),
            Kind::Color => {
                let [r, g, b] = clip_kind::color(text.trim()).unwrap_or_default();
                centred()
                    .gap(px(12.0))
                    .child(
                        div()
                            .w_full()
                            .flex_1()
                            .rounded(px(12.0))
                            .border_1()
                            .border_color(rgba(0xffffff26))
                            .bg(rgb(u32::from_be_bytes([0, r, g, b]))),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(SharedString::from(text.trim().to_string())),
                    )
            }
            Kind::Code => div()
                .font_family(MONO)
                .text_size(CODE_FACE.size())
                .text_color(rgb(CODE_INK))
                .child(
                    div()
                        .whitespace_nowrap()
                        .child(code(CODE_FACE.preview(window, text))),
                ),
            Kind::Text | Kind::Image | Kind::Files => div()
                .w(px(TEXT_FACE.width))
                .text_size(TEXT_FACE.size())
                .text_color(rgba(TEXT))
                .child(TEXT_FACE.preview(window, text)),
        },
        Content::Image { .. } => {
            let image = super::image_path(entry.id).and_then(|p| pictures.get(&p, IMAGE));
            match image {
                Some(image) => {
                    centred().child(img(image).size_full().object_fit(ObjectFit::Contain))
                }
                None => centred(),
            }
        }
        Content::Files(files) => centred()
            .gap(px(6.0))
            .children(
                (files.first())
                    .and_then(|f| pictures.get(f, FILE_ICON))
                    .map(|icon| {
                        img(icon)
                            .size(px(FILE_ICON))
                            .flex_none()
                            .object_fit(ObjectFit::Contain)
                    }),
            )
            .children(files.iter().take(4).map(|f| file_name(f)))
            .children((files.len() > 4).then(|| {
                div()
                    .text_xs()
                    .text_color(rgba(DIM))
                    .child(format!("and {} more", files.len() - 4))
            })),
    }
}

fn file_name(path: &Path) -> Div {
    let name = path.file_name().unwrap_or(path.as_os_str());
    div()
        .w(px(INNER))
        .text_center()
        .text_sm()
        .truncate()
        .child(name.to_string_lossy().to_string())
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
