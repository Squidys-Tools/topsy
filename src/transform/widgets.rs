//! The Transform panel's controls, drawn after Droppy's: an option row (label left, control
//! right), a segmented control, a slider, a pop-up picker and a one-line text field.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, Div, ElementId, FontWeight, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, SharedString, Stateful, Window, canvas, div, prelude::*, px, rgb, rgba,
};

pub const ACCENT: u32 = 0x5e5ce6;
pub const BLUE: u32 = 0x0a84ff;
pub const ICONS: &str = "Segoe Fluent Icons";
pub const DIM: u32 = 0xffffff8c;
pub const FAINT: u32 = 0xffffff14;

/// One option: its label on the left and its control on the right.
pub fn row(label: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .min_h(px(30.0))
        .child(div().flex_none().child(label.into()))
        .child(control)
}

/// A muted line of help under the options.
pub fn note(text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .text_color(rgba(DIM))
        .line_height(px(16.0))
        .child(text.into())
}

/// macOS-style segmented control; `pick` hears the chosen index.
pub fn segmented(
    id: impl Into<ElementId>,
    choices: &[&'static str],
    at: usize,
    pick: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let pick = Rc::new(pick);
    div()
        .id(id)
        .flex()
        .p(px(2.0))
        .gap(px(2.0))
        .rounded(px(7.0))
        .bg(rgba(FAINT))
        .text_xs()
        .children(choices.iter().enumerate().map(|(i, &label)| {
            let pick = pick.clone();
            div()
                .id(i)
                .px(px(10.0))
                .py(px(3.0))
                .rounded(px(5.0))
                .when(i == at, |s| s.bg(rgb(BLUE)).font_weight(FontWeight::MEDIUM))
                .when(i != at, |s| s.hover(|s| s.bg(rgba(0xffffff12))))
                .child(label)
                .on_click(move |_, window, cx| pick(i, window, cx))
        }))
}

/// A pop-up button: shows the current choice with up/down chevrons; `open` shows the menu.
pub fn picker(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    enabled: bool,
    open: impl Fn(&mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.0))
        .pl(px(10.0))
        .pr(px(6.0))
        .py(px(3.0))
        .rounded(px(6.0))
        .bg(rgba(FAINT))
        .text_xs()
        .when(!enabled, |s| s.text_color(rgba(0xffffff59)))
        .when(enabled, |s| {
            s.hover(|s| s.bg(rgba(0xffffff24)))
                .on_click(move |_, window, cx| open(window, cx))
        })
        .child(label.into())
        .child(
            div()
                .font_family(ICONS)
                .text_size(px(8.0))
                .flex()
                .flex_col()
                .line_height(px(7.0))
                .child("\u{e70e}")
                .child("\u{e70d}"),
        )
}

/// A slider from 0 to 1 with a round knob, `width` wide; `set` hears new values while dragged.
pub fn slider(
    id: impl Into<ElementId>,
    value: f32,
    width: f32,
    set: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let value = value.clamp(0.0, 1.0);
    let track: Rc<Cell<Bounds<Pixels>>> = Rc::default();
    let set = Rc::new(set);
    let at = {
        let track = track.clone();
        move |x: Pixels| {
            let b = track.get();
            if b.size.width <= px(0.0) {
                return None;
            }
            Some(((x - b.origin.x) / b.size.width).clamp(0.0, 1.0))
        }
    };
    let (down_at, move_at) = (at.clone(), at);
    let (down_set, move_set) = (set.clone(), set);
    let knob = 16.0;
    div()
        .id(id)
        .relative()
        .w(px(width))
        .h(px(knob))
        .flex()
        .items_center()
        .child(
            canvas(move |bounds, _, _| track.set(bounds), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .w_full()
                .h(px(4.0))
                .rounded_full()
                .bg(rgba(0xffffff26))
                .child(
                    div()
                        .w(px(width * value))
                        .h_full()
                        .rounded_full()
                        .bg(rgb(BLUE)),
                ),
        )
        .child(
            div()
                .absolute()
                .left(px((width - knob) * value))
                .size(px(knob))
                .rounded_full()
                .bg(rgb(0xffffff))
                .shadow_sm(),
        )
        .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, window, cx| {
            if let Some(v) = down_at(e.position.x) {
                down_set(v, window, cx);
            }
        })
        .on_mouse_move(move |e: &MouseMoveEvent, window, cx| {
            if e.pressed_button == Some(MouseButton::Left)
                && let Some(v) = move_at(e.position.x)
            {
                move_set(v, window, cx);
            }
        })
}

/// A one-line text field showing `text`, with a caret while `focused`. Typing reaches it
/// through the panel's key handler; a click focuses it.
pub fn field(
    id: impl Into<ElementId>,
    text: &str,
    placeholder: &'static str,
    focused: bool,
    width: f32,
) -> Stateful<Div> {
    let shown: AnyElement = if text.is_empty() && !focused {
        div()
            .text_color(rgba(0xffffff59))
            .child(placeholder)
            .into_any_element()
    } else {
        div()
            .child(SharedString::from(text.to_owned()))
            .into_any_element()
    };
    div()
        .id(id)
        .w(px(width))
        .flex()
        .items_center()
        .px(px(8.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .bg(rgba(0x00000066))
        .border_1()
        .border_color(if focused { rgb(BLUE) } else { rgba(0xffffff1f) })
        .text_xs()
        .overflow_hidden()
        .child(shown)
        .when(focused, |s| {
            s.child(div().w(px(1.0)).h(px(13.0)).bg(rgb(0xffffff)))
        })
}

/// An action tile in the grid: a square-ish button with its glyph and the label under it.
pub fn tile(
    id: impl Into<ElementId>,
    glyph: char,
    label: &'static str,
    selected: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(80.0))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.0))
        .cursor_pointer()
        .child(
            div()
                .w_full()
                .h(px(44.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(10.0))
                .font_family(ICONS)
                .text_size(px(17.0))
                .bg(if selected { rgb(ACCENT) } else { rgba(FAINT) })
                .when(!selected, |s| s.hover(|s| s.bg(rgba(0xffffff24))))
                .child(glyph.to_string()),
        )
        .child(
            div()
                .w_full()
                .text_center()
                .text_size(px(11.0))
                .line_height(px(14.0))
                .when(selected, |s| s.font_weight(FontWeight::SEMIBOLD))
                .when(!selected, |s| s.text_color(rgba(0xffffffb3)))
                .child(label),
        )
}

/// The big button at the bottom, in the accent colour.
pub fn primary(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    enabled: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .h(px(36.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .font_weight(FontWeight::SEMIBOLD)
        .text_sm()
        .bg(rgb(ACCENT))
        .when(!enabled, |s| s.opacity(0.4))
        .when(enabled, |s| {
            s.cursor_pointer().hover(|s| s.bg(rgb(0x6e6cf0)))
        })
        .child(label.into())
}

/// A small text button.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(10.0))
        .py(px(3.0))
        .rounded(px(6.0))
        .text_xs()
        .bg(rgba(FAINT))
        .cursor_pointer()
        .hover(|s| s.bg(rgba(0xffffff24)))
        .child(label.into())
}
