//! Painting a frame: the glass fill, the ears and rim, and the page capsule under the notch.

use gpui::{
    AnyElement, Background, Div, ExternalPaths, Pixels, Size, canvas, div, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgb, rgba,
};

use super::{
    BLACK_UNTIL, CAPSULE, CAPSULE_EDGE, CAPSULE_FILL, DWM_RADIUS, Frame, Glass, LENS_FLOOR, PEEK,
    RIM,
};

impl Frame {
    /// The page capsule holding `capsule`, the body holding `content`, then the ears and rim
    /// drawn over it, for a window of `viewport`'s size on its way to a body `target_height` tall.
    pub fn draw(
        &self,
        viewport: Size<Pixels>,
        target_height: f32,
        content: AnyElement,
        capsule: Option<AnyElement>,
    ) -> [AnyElement; 3] {
        let (notch, strip) = self.outline(viewport);
        let look = self.look();
        let black = rgba(0x000000ff);
        let body = |fill: Background| -> Div {
            div()
                .absolute()
                .top_0()
                .bg(fill)
                .overflow_hidden()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(0x14213d)))
                .child(content)
        };

        // Black at the top, giving way to glass near the bottom: Droppy's dynamic glass, on the
        // basket as on the notch. The closed notch is black all the way down, like the
        // real one. The band is measured on the height the body is growing to, so the glass
        // rises out of the black as it opens rather than the black sliding down with it.
        let h = notch.height.max(1.0);
        let reach = target_height.max(h);
        let black_px = (reach * BLACK_UNTIL).max(PEEK.1).min(reach);
        let black_until = (black_px / h).min(1.0);
        let glass_share = if reach > black_px {
            ((h - black_px) / (reach - black_px)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let edge_fill = |floor: u32| {
            let alpha = 255.0 - glass_share * (255.0 - (floor & 0xff) as f32);
            let bottom = (floor & 0xffffff00) | alpha.round() as u32;
            linear_gradient(
                180.0,
                linear_color_stop(black, black_until),
                linear_color_stop(rgba(bottom), 1.0),
            )
        };
        let fill = match look {
            Glass::Blur | Glass::HostBlur if self.lensed => edge_fill(LENS_FLOOR),
            Glass::Blur | Glass::HostBlur | Glass::Tint => edge_fill(self.floor),
            Glass::Acrylic => edge_fill(0x00000000),
            Glass::Solid => black.into(),
            Glass::Clear => rgba(0x00000000).into(),
        };
        let (body, radius) = if self.is_floating() {
            let radius = self.float_radius(h);
            (body(fill).left_0().size_full().rounded(px(radius)), radius)
        } else {
            let radius = if look.has_ears() {
                notch.radius
            } else {
                DWM_RADIUS
            };
            let body = body(fill)
                .left(px(notch.ear_slot))
                .w(px(notch.body_width))
                .h(px(notch.height))
                .rounded_bl(px(radius))
                .rounded_br(px(radius));
            (body, radius)
        };

        // The ears, black like the edge they join, and the rim along the bottom. The ears are
        // softer than a circle, as Droppy's are: a cubic with its handles halfway in.
        let ear = notch.ear;
        let slot = notch.ear_slot;
        let rim_from = black_px.min(h);
        let rim = look != Glass::Solid && notch.height >= PEEK.1;
        let shapes = canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let o = bounds.origin;
                let width: f32 = bounds.size.width.into();
                let p = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
                if ear >= 0.5 {
                    for (edge, out) in [(slot, -1.0f32), (width - slot, 1.0)] {
                        let mut path = gpui::PathBuilder::fill();
                        path.move_to(p(edge + out * ear, 0.0));
                        path.line_to(p(edge, 0.0));
                        path.line_to(p(edge, ear));
                        path.cubic_bezier_to(
                            p(edge + out * ear, 0.0),
                            p(edge, ear * 0.5),
                            p(edge + out * ear * 0.5, 0.0),
                        );
                        if let Ok(path) = path.build() {
                            window.paint_path(path, black);
                        }
                    }
                }
                if rim {
                    // The stroke is centred on the path: half a pixel in keeps it whole.
                    let (l, r, b) = (slot + 0.5, width - slot - 0.5, h - 0.5);
                    let c = radius.min((b - rim_from).max(0.0));
                    let k = 0.45;
                    let mut path = gpui::PathBuilder::stroke(px(1.0));
                    path.move_to(p(l, rim_from));
                    path.line_to(p(l, b - c));
                    path.cubic_bezier_to(p(l + c, b), p(l, b - c * k), p(l + c * k, b));
                    path.line_to(p(r - c, b));
                    path.cubic_bezier_to(p(r, b - c), p(r - c * k, b), p(r, b - c * k));
                    path.line_to(p(r, rim_from));
                    if let Ok(path) = path.build() {
                        let glow = linear_gradient(
                            180.0,
                            linear_color_stop(rgba(RIM & 0xffffff00), 0.0),
                            linear_color_stop(rgba(RIM), 1.0),
                        );
                        window.paint_path(path, glow);
                    }
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .w_full()
        .h(px(notch.height));

        // The capsule slides out from under the notch, so it shows only below it.
        let capsule_fill = match look {
            Glass::Blur | Glass::HostBlur | Glass::Tint => rgba(CAPSULE_FILL),
            Glass::Solid => black,
            Glass::Acrylic | Glass::Clear => rgba(0x00000000),
        };
        let capsule = capsule.filter(|_| strip >= 0.5).map(|inner| {
            div()
                .absolute()
                .top(px(notch.height))
                .left_0()
                .w_full()
                .h(px(strip))
                .overflow_hidden()
                .flex()
                .flex_col()
                .justify_end()
                .items_center()
                .child(
                    div()
                        .w(px(CAPSULE.0))
                        .h(px(CAPSULE.1))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(capsule_fill)
                        .border_1()
                        .border_color(rgba(CAPSULE_EDGE))
                        .child(inner),
                )
        });
        [
            capsule.map_or_else(|| div().into_any_element(), IntoElement::into_any_element),
            body.into_any_element(),
            shapes.into_any_element(),
        ]
    }
}
