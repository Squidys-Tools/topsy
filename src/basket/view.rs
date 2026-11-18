//! How the basket draws, measured from Droppy's: a handle and two round buttons along the top,
//! then "Drop files here" while it is empty, or the stack of what it holds over a pill that
//! counts them. The pill opens the list, a tile per file, and gives way to the name field
//! while a file is renamed.

use std::time::Instant;

use gpui::{
    AnyElement, ClickEvent, Context, Div, ElementId, ExternalPaths, FontWeight, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Stateful, Window, div, hsla, img,
    prelude::*, px, rgb, rgba,
};

use super::rename::Rename;
use super::{Basket, CARD, Grip, LIST, THUMB};
use crate::chip::DRAG_OUT_THRESHOLD;
use crate::icons::{Icon, icon};
use crate::model::basket::{shows_content, stack_depth, summary};
use crate::model::shelf::Item;

/// Sampled from Droppy's: the round buttons, the handle and the pill are white over the black,
/// the hint and the pill's words a mid grey.
const BUTTON: u32 = 0xffffff1f;
const HANDLE: u32 = 0xffffff3e;
const PILL: u32 = 0xffffff22;
const GREY: u32 = 0x8b8b8bff;
/// The rename field's ring and selection: Windows' default accent.
const ACCENT: u32 = 0x0a84ffff;
/// A name that can't be used.
const WARN: u32 = 0xff9f0aff;
/// The top strip: buttons, handle, and room to move the basket by.
const HEADER: f32 = 52.0;
const BUTTON_SIZE: f32 = 28.0;
const BUTTON_INSET: f32 = 13.0;
const TILE: (f32, f32) = (64.0, 78.0);
const TILE_THUMB: f32 = 48.0;

impl Render for Basket {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let dt = self
            .last_frame
            .map(|t| now.duration_since(t).as_secs_f32())
            .unwrap_or(1.0 / 120.0);
        let moving = self.width.step(dt) | self.height.step(dt);
        self.last_frame = moving.then_some(now);
        if moving {
            window.request_animation_frame();
        }
        self.frame.sync(
            self.width.value,
            self.height.value,
            self.height.target,
            window,
            cx,
        );
        let viewport = window.viewport_size();
        let content = if self.open {
            self.card(cx).into_any_element()
        } else {
            div().into_any_element()
        };
        div()
            .id("basket")
            .relative()
            .w(viewport.width)
            .h(viewport.height)
            .text_color(rgb(0xffffff))
            .children(self.frame.draw(viewport, self.height.target, content))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                let names: Vec<_> = (paths.paths().iter())
                    .filter_map(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .collect();
                crate::log(&format!("basket drop {names:?}"));
                this.add(paths.paths().to_vec(), cx);
            }))
    }
}

/// A round button in a top corner.
fn button(id: &'static str, glyph: Icon, left: bool) -> Stateful<Div> {
    let at = div()
        .id(id)
        .absolute()
        .top(px(BUTTON_INSET + 1.0))
        .size(px(BUTTON_SIZE))
        .rounded_full()
        .bg(rgba(BUTTON))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(rgba(0xffffff33)))
        .child(icon(glyph, 14.0, rgba(0xffffffcc)));
    if left {
        at.left(px(BUTTON_INSET))
    } else {
        at.right(px(BUTTON_INSET))
    }
}

/// A file's picture: the shell's thumbnail filling a rounded square, or its icon whole inside
/// one; a coloured type badge until it arrives.
fn picture(item: &Item, icon: Option<&std::sync::Arc<gpui::RenderImage>>, side: f32) -> Div {
    let radius = side / 8.0;
    let frame = div().size(px(side)).flex_shrink_0().rounded(px(radius));
    match icon {
        Some(icon) if shows_content(item) => frame.overflow_hidden().child(
            img(icon.clone())
                .size(px(side))
                .rounded(px(radius))
                .object_fit(ObjectFit::Cover),
        ),
        Some(icon) => frame.flex().items_center().justify_center().child(
            img(icon.clone())
                .size(px(side * 0.86))
                .object_fit(ObjectFit::Contain),
        ),
        None => frame
            .bg(hsla(item.badge_hue(), 0.55, 0.45, 1.0))
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .font_weight(FontWeight::BOLD)
            .child(item.badge_label()),
    }
}

impl Basket {
    /// The card at its full size, centred in the window, so it is revealed (not squashed) while
    /// the window springs open around it.
    fn card(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (w, h) = if self.list { LIST } else { CARD };
        let body = if self.files.is_empty() {
            self.hint().into_any_element()
        } else if self.list {
            self.tiles(cx).into_any_element()
        } else {
            self.stack(cx).into_any_element()
        };
        div()
            .w(px(w))
            .h(px(h))
            .flex_shrink_0()
            .relative()
            .child(body)
            .children((!self.files.is_empty() && self.rename.is_none()).then(|| self.pill(h, cx)))
            .children(
                self.rename
                    .as_ref()
                    .map(|rename| self.name_field(rename, w, h, cx)),
            )
            .child(self.header(cx))
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let reach = BUTTON_INSET + BUTTON_SIZE + 4.0;
        div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(px(HEADER))
            // Between the buttons: a caption Windows moves the basket by (GPUI's
            // `start_window_move` does nothing on Windows).
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(reach))
                    .right(px(reach))
                    .h(px(HEADER))
                    .window_control_area(gpui::WindowControlArea::Drag)
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .mt(px(11.0))
                            .w(px(45.0))
                            .h(px(5.0))
                            .rounded_full()
                            .bg(rgba(HANDLE)),
                    ),
            )
            .child(
                button("basket-clear", Icon::Close, true).on_click(cx.listener(
                    |this, _: &ClickEvent, _window, cx| {
                        cx.stop_propagation();
                        this.clear(cx);
                    },
                )),
            )
            .child(
                button("basket-hide", Icon::ChevronDown, false).on_click(cx.listener(
                    |this, _: &ClickEvent, _window, cx| {
                        cx.stop_propagation();
                        this.close(cx);
                    },
                )),
            )
    }

    /// Empty: what to do, under the tray's own icon.
    fn hint(&self) -> impl IntoElement + use<> {
        div()
            .absolute()
            .top(px(62.0))
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.0))
            .child(icon(Icon::Inbox, 42.0, rgba(GREY)))
            .child(
                div()
                    .text_size(px(16.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgba(GREY))
                    .child("Drop files here"),
            )
    }

    /// The newest file in front, up to two more peeking out above it, smaller and dimmer. The
    /// whole stack drags out as one.
    fn stack(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let items = self.files.items();
        let depth = stack_depth(items.len());
        let mut cards: Vec<AnyElement> = Vec::new();
        // Back to front.
        for k in (0..depth).rev() {
            let item = &items[items.len() - 1 - k];
            let side = THUMB - 12.0 * k as f32;
            let top = 60.0 - 7.0 * k as f32;
            cards.push(
                div()
                    .absolute()
                    .top(px(top))
                    .left(px((CARD.0 - side) / 2.0))
                    .opacity(1.0 - 0.3 * k as f32)
                    .shadow_md()
                    .rounded(px(side / 8.0))
                    .child(picture(item, self.icons.get(&item.path), side))
                    .into_any_element(),
            );
        }
        self.grip(div().id("basket-stack"), Grip::Stack, cx)
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .cursor_grab()
            .children(cards)
            // On the release, as the clipboard strip's: opened on the press, the release still
            // goes to the basket and the menu never gets the keyboard.
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, _: &MouseUpEvent, window, cx| {
                    cx.stop_propagation();
                    this.stack_menu(window, cx);
                }),
            )
    }

    /// Press-and-move on `el` drags `grip`'s files out; a click without moving is left to `el`.
    fn grip(&self, el: Stateful<Div>, grip: Grip, cx: &mut Context<Self>) -> Stateful<Div> {
        el.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _window, _cx| {
                this.press = Some((grip, event.position));
            }),
        )
        .on_mouse_move(
            cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                if !event.dragging() {
                    this.press = None;
                    return;
                }
                let Some((pressed, start)) = this.press else {
                    return;
                };
                let dx = f32::from(event.position.x - start.x);
                let dy = f32::from(event.position.y - start.y);
                if pressed == grip && dx * dx + dy * dy >= DRAG_OUT_THRESHOLD.powi(2) {
                    this.press = None;
                    this.begin_drag_out(grip, cx);
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _window, _cx| this.press = None),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _window, _cx| this.press = None),
        )
    }

    /// How many, of what; opens the list, and closes it again.
    fn pill(&self, height: f32, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let glyph = if self.list {
            Icon::ChevronLeft
        } else {
            Icon::ChevronRight
        };
        div()
            .absolute()
            .top(px(height - 41.0))
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("basket-pill")
                    .h(px(28.0))
                    .pl(px(12.0))
                    .pr(px(8.0))
                    .rounded_full()
                    .bg(rgba(PILL))
                    .flex()
                    .items_center()
                    .gap(px(3.0))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgba(0xffffff33)))
                    .text_size(px(13.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgba(GREY))
                    .child(summary(self.files.items()))
                    .child(icon(glyph, 11.0, rgba(GREY)))
                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                        this.toggle_list(cx);
                    })),
            )
    }

    /// Every file as a tile: drag one out, click to open it, right-click for its actions.
    fn tiles(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let renaming = self.rename.as_ref().map(|r| r.path.clone());
        let tiles: Vec<_> = (self.files.items().iter().enumerate())
            .map(|(i, item)| {
                let lit = renaming.as_ref() == Some(&item.path);
                self.grip(
                    div().id(ElementId::NamedInteger("tile".into(), i as u64)),
                    Grip::Tile(i),
                    cx,
                )
                .w(px(TILE.0))
                .h(px(TILE.1))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(5.0))
                .pt(px(6.0))
                .rounded(px(10.0))
                .cursor_pointer()
                .hover(|style| style.bg(rgba(0xffffff1a)))
                .when(lit, |tile| tile.bg(rgba(ACCENT & 0xffffff00 | 0x55)))
                .child(picture(item, self.icons.get(&item.path), TILE_THUMB))
                .child(
                    div()
                        .w(px(TILE.0 - 6.0))
                        .text_xs()
                        .text_center()
                        .truncate()
                        .child(item.name()),
                )
                .on_click({
                    let item = item.clone();
                    move |_: &ClickEvent, _window, _cx| crate::chip::open(item.clone())
                })
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |this, _: &MouseUpEvent, window, cx| {
                        cx.stop_propagation();
                        this.tile_menu(i, window, cx);
                    }),
                )
            })
            .collect();
        div()
            .id("basket-tiles")
            .absolute()
            .top(px(HEADER))
            .left(px(10.0))
            .right(px(10.0))
            .bottom(px(50.0))
            .overflow_y_scroll()
            .flex()
            .flex_row()
            .flex_wrap()
            .content_start()
            .gap(px(6.0))
            .children(tiles)
    }

    /// The new name being typed, where the pill was: the selection lit, the caret where it is
    /// typed. A name that can't be used says why above it.
    fn name_field(
        &self,
        rename: &Rename,
        w: f32,
        h: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let field = &rename.field;
        let text = field.text();
        let sel = field.selection();
        let caret = div()
            .w(px(1.5))
            .h(px(15.0))
            .flex_shrink_0()
            .bg(rgb(0xffffff));
        // A long name shows the end it is being typed at.
        let long = text.chars().count() > 22;
        let tail = long && text[..field.caret()].chars().count() > 18;
        let span = |t: &str| div().flex_shrink_0().child(t.to_owned());
        div()
            .absolute()
            .top(px(h
                - 41.0
                - if rename.problem.is_some() { 18.0 } else { 0.0 }))
            .left(px(12.0))
            .w(px(w - 24.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .children(rename.problem.clone().map(|problem| {
                div()
                    .text_size(px(11.0))
                    .text_color(rgba(WARN))
                    .truncate()
                    .child(problem)
            }))
            .child(
                div()
                    .id("basket-rename")
                    .track_focus(&self.focus)
                    .on_key_down(cx.listener(Self::on_rename_key))
                    .w_full()
                    .h(px(28.0))
                    .px(px(8.0))
                    .rounded(px(8.0))
                    .bg(rgba(0x0000004d))
                    .border_1()
                    .border_color(rgba(ACCENT))
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .map(|row| match (long, tail) {
                        (false, _) => row.justify_center(),
                        (true, false) => row.justify_start(),
                        (true, true) => row.justify_end(),
                    })
                    .whitespace_nowrap()
                    .text_size(px(13.0))
                    .child(span(&text[..sel.start]))
                    .children(sel.is_empty().then_some(caret))
                    .child(span(&text[sel.clone()]).bg(rgba(ACCENT & 0xffffff00 | 0x99)))
                    .child(span(&text[sel.end..])),
            )
    }
}
