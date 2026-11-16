//! The panel's layout, after Droppy's: a header (the Transform icon, what is in it, close), the
//! files as thumbnails with an Add tile, the grid of actions that suit them, the chosen
//! action's options, and the button that runs it. While it runs, the result cards take over.

use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, IntoElement, ObjectFit, Render, Window,
    canvas, div, img, prelude::*, px, rgb, rgba,
};

use super::widgets::{ACCENT, DIM, FAINT, ICONS, button, primary, tile};
use super::{Panel, RADIUS, State, THUMB, TILE, WIDTH};
use crate::model::transform::{Action, actions, size, summary};

const PAD: f32 = 14.0;

fn glyph(action: Action) -> AnyElement {
    let c = match action {
        Action::Compress => '\u{e73f}',
        Action::Convert => '\u{e895}',
        Action::Resize => '\u{e8a3}',
        Action::Trim => '\u{e8c6}',
        Action::MakeGif => '\u{f4a9}',
        Action::RemoveBackground => '\u{e8d4}',
        Action::Rotate => '\u{e7ad}',
        Action::RemoveMetadata => '\u{ed1a}',
        Action::CombinePdf => '\u{e8c8}',
        Action::Zip => '\u{f012}',
        Action::Rename => return rename_glyph(),
    };
    div()
        .font_family(ICONS)
        .text_size(px(18.0))
        .child(c.to_string())
        .into_any_element()
}

/// Rename's icon, as Droppy draws it: a letter and a text cursor beside it.
fn rename_glyph() -> AnyElement {
    let serif = || div().w(px(6.0)).h(px(1.5)).bg(rgb(0xffffff));
    div()
        .flex()
        .items_center()
        .gap(px(1.0))
        .child(div().text_size(px(19.0)).child("A"))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .child(serif())
                .child(div().w(px(1.5)).h(px(16.0)).bg(rgb(0xffffff)))
                .child(serif()),
        )
        .into_any_element()
}

/// The Transform icon: a lilac page with a white badge of blue arrows.
fn badge() -> Div {
    div()
        .relative()
        .flex_none()
        .size(px(36.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(10.0))
        .bg(rgb(0x232326))
        .border_1()
        .border_color(rgba(0xffffff14))
        .font_family(ICONS)
        .child(
            div()
                .text_size(px(19.0))
                .text_color(rgb(0x9c9aff))
                .child("\u{e729}"),
        )
        .child(
            div()
                .absolute()
                .right(px(4.0))
                .bottom(px(4.0))
                .size(px(14.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(rgb(0xffffff))
                .text_size(px(9.0))
                .text_color(rgb(0x0a84ff))
                .child("\u{e895}"),
        )
}

impl Panel {
    fn header(&self, cx: &mut Context<Self>) -> Div {
        let bytes = self.files.iter().map(|f| f.bytes).sum();
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .on_mouse_down(gpui::MouseButton::Left, |_, window, _| {
                window.start_window_move()
            })
            .child(badge())
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Transform"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgba(DIM))
                            .child(summary(&self.kinds(), bytes)),
                    ),
            )
            .child(
                div()
                    .id("close")
                    .flex_none()
                    .size(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(rgba(0xffffff1c))
                    .font_family(ICONS)
                    .text_size(px(11.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(0xffffff2e)))
                    .child("\u{e8bb}")
                    .on_click(cx.listener(|panel, _: &ClickEvent, window, _| panel.close(window))),
            )
    }

    fn files_row(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let running = self.running();
        let tiles =
            self.files.iter().enumerate().map(|(i, file)| {
                let name = file
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let picture = match &file.thumb {
                    Some(thumb) => img(thumb.clone())
                        .size_full()
                        .rounded(px(10.0))
                        .object_fit(ObjectFit::Fill)
                        .into_any_element(),
                    None => div().into_any_element(),
                };
                div()
                    .id(i)
                    .group("file")
                    .relative()
                    .w(px(TILE))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .h(px(THUMB))
                            .rounded(px(10.0))
                            .bg(rgba(FAINT))
                            .child(picture),
                    )
                    .child(
                        div()
                            .mt(px(5.0))
                            .w_full()
                            .text_center()
                            .text_size(px(11.0))
                            .line_height(px(14.0))
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .line_height(px(13.0))
                            .text_color(rgba(DIM))
                            .child(if file.bytes > 0 {
                                size(file.bytes)
                            } else {
                                String::new()
                            }),
                    )
                    .when(!running && self.files.len() > 1, |s| {
                        s.child(
                            div()
                                .id(("remove", i))
                                .absolute()
                                .top(px(-4.0))
                                .right(px(-4.0))
                                .size(px(16.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .bg(rgb(0x3a3a3c))
                                .font_family(ICONS)
                                .text_size(px(7.0))
                                .invisible()
                                .group_hover("file", |s| s.visible())
                                .child("\u{e711}")
                                .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                                    panel.remove(i, cx)
                                })),
                        )
                    })
            });
        let add = div()
            .id("add")
            .w(px(TILE))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .cursor_pointer()
            .child(
                div()
                    .w_full()
                    .h(px(THUMB))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(10.0))
                    .bg(rgba(FAINT))
                    .hover(|s| s.bg(rgba(0xffffff24)))
                    .font_family(ICONS)
                    .text_size(px(16.0))
                    .child("\u{e710}"),
            )
            .child(
                div()
                    .mt(px(5.0))
                    .text_size(px(11.0))
                    .line_height(px(14.0))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Add"),
            )
            .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| panel.browse(cx)));
        div()
            .id("files")
            .flex()
            .gap(px(4.0))
            .pt(px(4.0))
            .overflow_x_scroll()
            .children(tiles)
            .when(!running, |s| s.child(add))
    }

    fn grid(&self, cx: &mut Context<Self>) -> Div {
        let tiles = actions(&self.kinds()).into_iter().map(|action| {
            tile(
                action.label(),
                glyph(action),
                action.label(),
                self.action == Some(action),
            )
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| panel.pick(action, cx)))
        });
        div()
            .flex()
            .flex_wrap()
            .gap_x(px(4.0))
            .gap_y(px(10.0))
            .children(tiles)
    }

    fn footer(&self, cx: &mut Context<Self>) -> Div {
        let Some(action) = self.action else {
            return div();
        };
        match &self.state {
            // The cards show the run; the panel is hidden meanwhile.
            State::Running { .. } => {
                div().child(primary("run", self.button_label(action).0, false))
            }
            State::Fetching { progress, .. } => {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_xs()
                            .child("Getting FFmpeg…")
                            .child(button("cancel", "Cancel").on_click(
                                cx.listener(|panel, _: &ClickEvent, _, _| panel.cancel()),
                            )),
                    )
                    .child(
                        div().h(px(6.0)).rounded_full().bg(rgba(0xffffff1f)).child(
                            div()
                                .h_full()
                                .w(px((WIDTH - 2.0 * PAD) * progress.clamp(0.0, 1.0)))
                                .rounded_full()
                                .bg(rgb(ACCENT)),
                        ),
                    )
            }
            State::Failed(message) => div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0xff453a))
                        .child(message.clone()),
                )
                .child(self.run_button(action, cx)),
            State::Ready => self.run_button(action, cx),
        }
    }

    fn run_button(&self, action: Action, cx: &mut Context<Self>) -> Div {
        let (label, enabled) = self.button_label(action);
        div().child(primary("run", label, enabled).when(enabled, |s| {
            s.on_click(cx.listener(|panel, _: &ClickEvent, _, cx| panel.start(cx)))
        }))
    }
}

impl Render for Panel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = self.options_card(cx);
        let this = cx.entity().downgrade();
        // Measures the laid-out content, so the window can be made exactly as tall.
        let measure = canvas(
            move |bounds, window, _| {
                let height = f32::from(bounds.size.height);
                window.on_next_frame(move |window, cx| {
                    this.update(cx, |panel, cx| panel.fit_height(height, window, cx))
                        .ok();
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let content = div()
            .relative()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(PAD))
            .child(measure)
            .child(self.header(cx))
            .child(self.files_row(cx))
            .child(self.grid(cx))
            .children(options.map(|o| o.mt(px(2.0))))
            .child(self.footer(cx));
        div()
            .id("panel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_drop(cx.listener(|panel, paths: &gpui::ExternalPaths, _, cx| {
                panel.add(paths.paths().to_vec(), cx)
            }))
            .size_full()
            .rounded(px(RADIUS))
            .bg(rgb(0x0c0c0e))
            .border_1()
            .border_color(rgba(0xffffff1f))
            .text_color(rgb(0xffffff))
            .text_sm()
            .child(content)
    }
}
