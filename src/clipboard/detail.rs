//! The palette's right-hand pane: a large preview of the selected copy (its text, code
//! coloured, a link's site, a colour's swatch, the image, the files) over what is known about it.

use std::path::Path;

use gpui::{
    Div, FontStyle, FontWeight, HighlightStyle, ObjectFit, SharedString, StyledText, Window, div,
    img, prelude::*, px, rgb, rgba,
};

use super::pictures::{Pictures, Want};
use super::preview::Face;
use super::view::{DIM, RULE, TEXT};
use model::clip_kind::{self, Kind};
use model::clip_list::facts;
use model::clipboard::{Content, Entry};
use model::favicon::icon_host;
use model::highlight::{Token, highlight};

/// The pane's width and the preview's height, in logical pixels.
pub const WIDTH: f32 = 463.0;
const PREVIEW: f32 = 222.0;
const PAD: f32 = 18.0;
const INNER: f32 = WIDTH - 2.0 * PAD;
/// Pictures the preview asks for: a copied image across the pane, a file's icon.
const IMAGE: f32 = INNER;
const FILE_ICON: f32 = 56.0;
const SITE_ICON: f32 = 40.0;
const FACT: f32 = 24.0;
const LABEL: f32 = 100.0;
const CODE_INK: u32 = 0xd6deeb;
const MONO: &str = "Consolas";
/// Whole lines that fit the preview, so none is cut through.
const TEXT_FACE: Face = Face {
    family: None,
    rems: 0.8125,
    width: INNER,
    lines: 8,
    wrap: true,
};
const CODE_FACE: Face = Face {
    family: Some(MONO),
    rems: 0.75,
    width: INNER,
    lines: 9,
    wrap: false,
};
const LINK_FACE: Face = Face {
    family: None,
    rems: 0.75,
    width: INNER,
    lines: 3,
    wrap: true,
};

/// What the pane wants looked up for `entry`: its picture and the name of the app it came from.
pub fn wants(entry: &Entry) -> Vec<Want> {
    let mut wants: Vec<Want> = entry.app.iter().map(|a| Want::Name(a.clone())).collect();
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

pub fn pane(entry: &Entry, kind: Kind, now: u64, pictures: &Pictures, window: &Window) -> Div {
    let app = entry.app.as_deref().map(|exe| pictures.name(exe));
    let rows = facts(entry, kind, app.as_deref(), now)
        .into_iter()
        .map(|(label, value)| {
            div()
                .h(px(FACT))
                .flex_none()
                .flex()
                .items_center()
                .child(
                    div()
                        .w(px(LABEL))
                        .flex_none()
                        .text_color(rgba(DIM))
                        .child(label),
                )
                .child(
                    // Not `truncate`: GPUI keeps a no-wrap text's first measure, taken before
                    // the row knows its width. One wrapped line, of a set width, is cut.
                    div()
                        .w(px(INNER - LABEL))
                        .text_right()
                        .line_clamp(1)
                        .text_ellipsis()
                        .child(value),
                )
        });
    let information = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .px(px(PAD))
        .pt(px(10.0))
        .text_size(px(12.0))
        .child(
            div()
                .h(px(FACT))
                .flex_none()
                .text_color(rgba(DIM))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Information"),
        )
        .children(rows);
    div()
        .w(px(WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(PREVIEW))
                .flex_none()
                .overflow_hidden()
                .p(px(PAD))
                .child(preview(entry, kind, pictures, window)),
        )
        .child(div().h(px(1.0)).flex_none().mx(px(PAD)).bg(rgba(RULE)))
        .child(information)
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
                        .map(|icon| {
                            // On a light tile: many sites draw theirs dark, for light pages.
                            div()
                                .p(px(7.0))
                                .rounded(px(11.0))
                                .bg(rgb(0xf2f2f2))
                                .child(img(icon).size(px(SITE_ICON)))
                        }),
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
