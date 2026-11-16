//! Dark "liquid glass" look: translucent rounded cards with hairline
//! highlights floating over soft, blurred colour light.

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, Pos2, Shadow, Stroke, Ui, Vec2, vec2};
use std::f32::consts::TAU;

pub const ACCENT: Color32 = Color32::from_rgb(10, 132, 255); // iOS system blue
pub const GOOD: Color32 = Color32::from_rgb(48, 209, 88);
pub const BAD: Color32 = Color32::from_rgb(255, 105, 97);
pub const WARN: Color32 = Color32::from_rgb(255, 214, 10);

pub fn w(a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(255, 255, 255, a)
}

pub fn apply(ctx: &egui::Context) {
    let mut s = (*ctx.style()).clone();
    let v = &mut s.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = Color32::from_rgba_unmultiplied(30, 32, 42, 238);
    v.window_stroke = Stroke::new(1.0f32, w(46));
    v.window_corner_radius = CornerRadius::same(22);
    v.window_shadow = Shadow { offset: [0, 16], blur: 40, spread: 0, color: Color32::from_black_alpha(140) };
    v.popup_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(120) };
    v.menu_corner_radius = CornerRadius::same(14);
    v.extreme_bg_color = Color32::from_rgba_unmultiplied(0, 0, 0, 80);
    v.faint_bg_color = w(10);
    v.code_bg_color = w(12);
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(10, 132, 255, 120);
    v.selection.stroke = Stroke::new(1.0f32, Color32::WHITE);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = WARN;
    v.error_fg_color = BAD;
    let r = CornerRadius::same(11);
    let ws = &mut v.widgets;
    ws.noninteractive.bg_fill = w(8);
    ws.noninteractive.weak_bg_fill = w(8);
    ws.noninteractive.bg_stroke = Stroke::new(1.0f32, w(22));
    ws.noninteractive.fg_stroke = Stroke::new(1.0f32, w(225));
    ws.noninteractive.corner_radius = r;
    for (st, bg, line, fg) in [(&mut ws.inactive, 20, 28, 235), (&mut ws.hovered, 36, 80, 255), (&mut ws.open, 36, 60, 255)] {
        st.bg_fill = w(bg);
        st.weak_bg_fill = w(bg);
        st.bg_stroke = Stroke::new(1.0f32, w(line));
        st.fg_stroke = Stroke::new(1.5f32, w(fg));
        st.corner_radius = r;
        st.expansion = 0.0;
    }
    ws.active.bg_fill = Color32::from_rgba_unmultiplied(10, 132, 255, 170);
    ws.active.weak_bg_fill = Color32::from_rgba_unmultiplied(10, 132, 255, 170);
    ws.active.bg_stroke = Stroke::new(1.0f32, w(140));
    ws.active.fg_stroke = Stroke::new(1.5f32, Color32::WHITE);
    ws.active.corner_radius = r;

    s.spacing.item_spacing = vec2(10.0, 8.0);
    s.spacing.button_padding = vec2(12.0, 5.0);
    s.spacing.interact_size.y = 26.0;
    s.spacing.combo_width = 120.0;
    use egui::{FontFamily as F, FontId, TextStyle as T};
    s.text_styles = [
        (T::Small, FontId::new(11.5, F::Proportional)),
        (T::Body, FontId::new(14.5, F::Proportional)),
        (T::Button, FontId::new(14.0, F::Proportional)),
        (T::Heading, FontId::new(19.0, F::Proportional)),
        (T::Monospace, FontId::new(13.5, F::Monospace)),
    ]
    .into();
    ctx.set_style(s);
}

/// Radial light blob: bright centre fading to nothing (a cheap stand-in for blur).
fn blob(p: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    let mut m = egui::Mesh::default();
    let n = 64u32;
    m.colored_vertex(c, col);
    let mid = col.gamma_multiply(0.45);
    for i in 0..n {
        let a = i as f32 / n as f32 * TAU;
        m.colored_vertex(c + Vec2::angled(a) * r * 0.5, mid);
    }
    for i in 0..n {
        let a = i as f32 / n as f32 * TAU;
        m.colored_vertex(c + Vec2::angled(a) * r, Color32::TRANSPARENT);
    }
    for i in 0..n {
        let j = (i + 1) % n;
        m.add_triangle(0, 1 + i, 1 + j);
        m.add_triangle(1 + i, 1 + n + i, 1 + n + j);
        m.add_triangle(1 + i, 1 + n + j, 1 + j);
    }
    p.add(m);
}

pub fn background(ctx: &egui::Context) {
    #[allow(deprecated)]
    let rect = ctx.screen_rect();
    let p = ctx.layer_painter(egui::LayerId::background());
    p.rect_filled(rect, 0.0, Color32::from_rgb(9, 10, 16));
    let (sz, o) = (rect.size(), rect.min);
    let at = |x: f32, y: f32| o + vec2(sz.x * x, sz.y * y);
    let m = sz.x.max(sz.y);
    blob(&p, at(0.12, 0.10), m * 0.55, Color32::from_rgba_unmultiplied(40, 90, 255, 95));
    blob(&p, at(0.92, 0.18), m * 0.45, Color32::from_rgba_unmultiplied(150, 70, 255, 80));
    blob(&p, at(0.70, 0.95), m * 0.55, Color32::from_rgba_unmultiplied(0, 190, 200, 60));
    blob(&p, at(0.05, 0.95), m * 0.35, Color32::from_rgba_unmultiplied(255, 70, 150, 45));
}

fn glass_frame(radius: u8, pad: i8, alpha: u8) -> Frame {
    Frame::new()
        .fill(w(alpha))
        .stroke(Stroke::new(1.0f32, w(32)))
        .corner_radius(CornerRadius::same(radius))
        .inner_margin(Margin::same(pad))
        .shadow(Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(70) })
}

/// Specular hairline along the top edge, like light catching glass.
fn sheen(ui: &Ui, rect: egui::Rect, radius: f32) {
    let y = rect.top() + 0.5;
    let p = ui.painter();
    let (l, r) = (rect.left() + radius, rect.right() - radius);
    let n = 24;
    for i in 0..n {
        let t0 = i as f32 / n as f32;
        let t1 = (i + 1) as f32 / n as f32;
        let a = (1.0 - ((t0 + t1) - 1.0).abs()) * 110.0; // brightest in the middle
        p.line_segment([Pos2::new(l + (r - l) * t0, y), Pos2::new(l + (r - l) * t1, y)], Stroke::new(1.0f32, w(a as u8)));
    }
}

/// A glass card filling the available width.
pub fn card<R>(ui: &mut Ui, title: Option<&str>, add: impl FnOnce(&mut Ui) -> R) -> R {
    let r = glass_frame(20, 16, 16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if let Some(t) = title {
            ui.label(egui::RichText::new(t).size(17.0).strong().color(w(245)));
            ui.add_space(4.0);
        }
        add(ui)
    });
    sheen(ui, r.response.rect, 20.0);
    ui.add_space(6.0);
    r.inner
}

/// Panel frame: transparent outer area with a floating glass sheet inside.
pub fn panel_frame(alpha: u8) -> Frame {
    glass_frame(22, 12, alpha).outer_margin(Margin::same(8))
}

pub fn pill(ui: &mut Ui, text: &str, col: Color32) {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(col).size(13.0))
            .fill(col.gamma_multiply(0.16))
            .stroke(Stroke::new(1.0f32, col.gamma_multiply(0.5)))
            .corner_radius(CornerRadius::same(255))
            .sense(egui::Sense::hover()),
    );
}

pub fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_string()).strong().color(Color32::WHITE))
        .fill(ACCENT)
        .corner_radius(CornerRadius::same(255))
}
