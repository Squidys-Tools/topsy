//! The palette's actions panel (Ctrl+K, the footer's Actions, a right-click): what can be done
//! with the selected copy, drawn in the palette itself rather than as a Win32 menu, so it is the
//! same dark glass and never takes the focus away. It has a search of its own; arrows choose,
//! Enter runs, Esc or a click outside closes it.

use gpui::{
    AnyElement, Context, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, SharedString,
    Window, div, prelude::*, px, rgba,
};

use super::palette::Palette;
use super::view::{DIM, FAINT, FOOTER, SELECT, TEXT, key};
use model::clip_actions::{Action, actions};
use model::clip_kind::Kind;
use model::clip_list::title;
use model::menu::step;

const WIDTH: f32 = 300.0;
const ROW: f32 = 34.0;
const PAD: f32 = 6.0;
const RED: u32 = 0xff6961ff;
/// Darker than the palette's glass, so the panel lifts off what it covers.
const FILL: u32 = 0x161618f7;

/// The open panel: whose actions, what its search says, and which row is highlighted.
pub struct Panel {
    pub(super) id: u64,
    query: String,
    highlight: Option<usize>,
}

impl Palette {
    pub(super) fn open_panel(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(id) = self.entry(at).map(|e| e.id) else {
            return;
        };
        self.select(at, cx);
        self.panel = Some(Panel {
            id,
            query: String::new(),
            highlight: Some(0),
        });
        cx.notify();
    }

    pub(super) fn close_panel(&mut self, cx: &mut Context<Self>) {
        self.panel = None;
        cx.notify();
    }

    /// The panel's entry, where it is in the list, and the actions its search leaves.
    fn panel_actions(&self) -> Option<(usize, Vec<Action>)> {
        let panel = self.panel.as_ref()?;
        let at = self.position(panel.id)?;
        let pinned = self.entry(at)?.pinned;
        Some((at, actions(pinned, &panel.query)))
    }

    fn run(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let Some((at, _)) = self.panel_actions() else {
            return self.close_panel(cx);
        };
        self.close_panel(cx);
        match action {
            Action::Paste => self.take(at, true, window),
            Action::Copy => self.take(at, false, window),
            Action::Pin | Action::Unpin => self.pin(at),
            Action::Delete => self.remove(at),
        }
    }

    pub(super) fn on_panel_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = &event.keystroke;
        let command = key.modifiers.control || key.modifiers.alt || key.modifiers.platform;
        let Some((_, listed)) = self.panel_actions() else {
            return self.close_panel(cx);
        };
        let Some(panel) = self.panel.as_mut() else {
            return;
        };
        match key.key.as_str() {
            "escape" if !panel.query.is_empty() => panel.query.clear(),
            "escape" => return self.close_panel(cx),
            "k" if key.modifiers.control => return self.close_panel(cx),
            "enter" => {
                if let Some(action) = panel.highlight.and_then(|at| listed.get(at)) {
                    return self.run(*action, window, cx);
                }
                return;
            }
            "up" | "down" => {
                let choosable = vec![true; listed.len()];
                panel.highlight = step(&choosable, panel.highlight, key.key == "down");
            }
            "backspace" => {
                panel.query.pop();
                panel.highlight = Some(0);
            }
            "space" if !command => {
                panel.query.push(' ');
                panel.highlight = Some(0);
            }
            _ => match key.key_char.as_deref() {
                Some(typed) if !command && !typed.chars().any(char::is_control) => {
                    panel.query.push_str(typed);
                    panel.highlight = Some(0);
                }
                _ => return,
            },
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn panel_row(
        &self,
        at: usize,
        action: Action,
        lit: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let keys = action.keys().iter().map(|label| key(label));
        div()
            .id(("action", at))
            .h(px(ROW))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(px(12.0))
            .pr(px(7.0))
            .rounded_full()
            .when(lit, |row| row.bg(rgba(SELECT)))
            .when(action.destructive(), |row| row.text_color(rgba(RED)))
            .child(div().flex_1().child(action.label()))
            .child(div().flex().gap(px(3.0)).children(keys))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered && let Some(panel) = this.panel.as_mut() {
                    panel.highlight = Some(at);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.run(action, window, cx)))
            .into_any_element()
    }

    /// The panel over the bottom right of the palette, and a layer behind it that closes it on
    /// a click anywhere else.
    pub(super) fn panel_layer(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panel = self.panel.as_ref()?;
        let (at, listed) = self.panel_actions()?;
        let entry = self.entry(at)?;
        let kind: Kind = self.kinds[self.shown[at]];
        let mut rows = Vec::new();
        for (row, action) in listed.iter().enumerate() {
            if action.destructive() && row > 0 {
                let rule = div()
                    .h(px(1.0))
                    .my(px(4.0))
                    .mx(px(12.0))
                    .bg(rgba(0xffffff17));
                rows.push(rule.into_any_element());
            }
            rows.push(self.panel_row(row, *action, panel.highlight == Some(row), cx));
        }
        let empty = listed.is_empty().then(|| {
            div()
                .h(px(ROW))
                .flex()
                .items_center()
                .px(px(10.0))
                .text_color(rgba(DIM))
                .child("No matching actions")
        });
        let search = if panel.query.is_empty() {
            div().text_color(rgba(FAINT)).child("Search for actions")
        } else {
            div().child(format!("{}\u{258f}", panel.query))
        };
        let card = div()
            .id("panel")
            .absolute()
            .right(px(12.0))
            .bottom(px(FOOTER))
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .p(px(PAD))
            .rounded(px(20.0))
            .bg(rgba(FILL))
            .border_1()
            .border_color(rgba(0xffffff1f))
            .shadow_lg()
            .occlude()
            .text_size(px(13.0))
            .text_color(rgba(TEXT))
            .child(
                // One line at a set width; see the detail pane's values.
                div()
                    .px(px(12.0))
                    .py(px(6.0))
                    .w(px(WIDTH - 2.0 * PAD))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgba(FAINT))
                    .line_clamp(1)
                    .text_ellipsis()
                    .child(SharedString::from(
                        title(&entry.content, kind).to_uppercase(),
                    )),
            )
            .child(div().flex().flex_col().children(rows).children(empty))
            .child(
                div()
                    .mt(px(6.0))
                    .h(px(34.0))
                    .flex()
                    .items_center()
                    .px(px(14.0))
                    .rounded_full()
                    .bg(rgba(0xffffff12))
                    .child(search),
            );
        let close =
            |this: &mut Palette, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Palette>| {
                this.close_panel(cx);
            };
        Some(
            div()
                .id("panel-layer")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .on_mouse_down(MouseButton::Left, cx.listener(close))
                .on_mouse_down(MouseButton::Right, cx.listener(close))
                .child(card)
                .into_any_element(),
        )
    }
}
