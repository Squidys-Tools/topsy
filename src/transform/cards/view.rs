//! How a result card is drawn.

use super::*;

impl Cards {
    fn card(&self, index: usize, card: &Card, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        let done = matches!(card.status, Status::Done { .. });
        let picture = match (done, &card.sharp, &card.soft) {
            (true, Some(image), _) | (false, _, Some(image)) => img(image.clone())
                .absolute()
                .size_full()
                .rounded(px(RADIUS))
                .object_fit(ObjectFit::Fill)
                .into_any_element(),
            _ => div().into_any_element(),
        };
        let shade = linear_gradient(
            180.0,
            linear_color_stop(rgba(0x00000000), 0.35),
            linear_color_stop(rgba(0x000000b8), 1.0),
        );
        let opened = match &card.status {
            Status::Done { output, .. } => output.clone(),
            _ => card.source.clone(),
        };
        let left = if done {
            circle(("dismiss", index), glyph('\u{e8bb}', 10.0))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.dismiss(index, cx)))
        } else {
            let stop = div().size(px(10.0)).rounded(px(2.0)).bg(rgb(0x1c1c1e));
            circle(("stop", index), stop.into_any_element()).on_click(
                cx.listener(|this, _: &ClickEvent, _, _| {
                    this.cancel.store(true, Ordering::Relaxed)
                }),
            )
        };
        let right = circle(("open", index), glyph('\u{e8b9}', 13.0)).on_click(move |_, _, _| {
            let path = opened.clone();
            std::thread::spawn(move || win32::open_path(&path));
        });
        div()
            .id(index)
            .relative()
            .flex_none()
            .w(px(CARD_W))
            .h(px(CARD_H))
            .rounded(px(RADIUS))
            .bg(rgb(0x1c1c1e))
            .child(picture)
            .when(!done, |s| {
                s.child(
                    div()
                        .absolute()
                        .size_full()
                        .rounded(px(RADIUS))
                        .bg(rgba(0x00000033)),
                )
            })
            .child(div().absolute().size_full().rounded(px(RADIUS)).bg(shade))
            .child(left.absolute().top(px(10.0)).left(px(10.0)))
            .child(right.absolute().top(px(10.0)).right(px(10.0)))
            .child(
                div()
                    .absolute()
                    .left(px(14.0))
                    .right(px(14.0))
                    .bottom(px(12.0))
                    .child(self.caption(&card.status)),
            )
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(rgba(0xffffff2e)),
            )
    }

    fn caption(&self, status: &Status) -> AnyElement {
        let working = |label: String, progress: f32| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(label),
                )
                .child(
                    div().h(px(4.0)).rounded_full().bg(rgba(0xffffff40)).child(
                        div()
                            .h_full()
                            .w(px((CARD_W - 28.0) * progress.clamp(0.0, 1.0)))
                            .rounded_full()
                            .bg(rgb(0xffffff)),
                    ),
                )
                .into_any_element()
        };
        match status {
            Status::Waiting => working("Waiting".into(), 0.0),
            Status::Working { label, progress } => working(
                label
                    .clone()
                    .unwrap_or_else(|| format!("{}…", self.action.label())),
                *progress,
            ),
            Status::Done { made, .. } => {
                let headline = match made.headline() {
                    Headline::Sizes(before, after) => div()
                        .flex()
                        .gap(px(7.0))
                        .child(div().text_color(rgb(0xff6961)).child(before))
                        .child(div().text_color(rgba(0xffffffd9)).child("→"))
                        .child(div().text_color(rgb(0xa9a8ff)).child(after)),
                    Headline::Text(text) => div().child(text),
                };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .font_family(MONO)
                    .font_weight(FontWeight::BOLD)
                    .child(headline.text_size(px(18.0)).line_height(px(22.0)))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .line_height(px(15.0))
                            .truncate()
                            .child(made.detail()),
                    )
                    .into_any_element()
            }
        }
    }
}

/// A white round button on a card's corner.
fn circle(id: impl Into<gpui::ElementId>, content: AnyElement) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgba(0xf5f5f7eb))
        .text_color(rgb(0x1c1c1e))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0xffffff)))
        .child(content)
}

fn glyph(c: char, size: f32) -> AnyElement {
    div()
        .font_family(ICONS)
        .text_size(px(size))
        .child(c.to_string())
        .into_any_element()
}

impl Render for Cards {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cards: Vec<_> = (self.cards.iter().enumerate())
            .filter(|(_, card)| !card.gone)
            .map(|(i, card)| self.card(i, card, cx))
            .collect();
        let column = div()
            .id("cards")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .overflow_y_scroll()
            .text_color(rgb(0xffffff))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.hover(*hovered, cx)))
            .children(cards);
        if self.leaving {
            column
                .with_animation("leave", Animation::new(FADE), |s, t| s.opacity(1.0 - t))
                .into_any_element()
        } else {
            column.into_any_element()
        }
    }
}
