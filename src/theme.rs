//! iOS / iPadOS (dark) look: black background, inset-grouped lists,
//! large bold titles, blue text buttons, switches, steppers, icon tiles.

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontFamily, FontId, Frame, Layout, Margin, Rect, Response, RichText, Sense,
    Stroke, Ui, vec2,
};
use std::path::Path;
use std::sync::Arc;

pub const BG: Color32 = Color32::BLACK;
pub const SIDEBAR: Color32 = Color32::from_rgb(18, 18, 20);
pub const CELL: Color32 = Color32::from_rgb(28, 28, 30);
pub const CELL2: Color32 = Color32::from_rgb(44, 44, 46);
pub const CELL3: Color32 = Color32::from_rgb(58, 58, 60);
pub const SEP: Color32 = Color32::from_rgb(56, 56, 58);
pub const LABEL: Color32 = Color32::from_rgb(255, 255, 255);
pub const GRAY: Color32 = Color32::from_rgb(142, 142, 147);
pub const GRAY3: Color32 = Color32::from_rgb(72, 72, 74);
pub const BLUE: Color32 = Color32::from_rgb(10, 132, 255);
pub const GREEN: Color32 = Color32::from_rgb(48, 209, 88);
pub const RED: Color32 = Color32::from_rgb(255, 69, 58);
pub const ORANGE: Color32 = Color32::from_rgb(255, 159, 10);
pub const YELLOW: Color32 = Color32::from_rgb(255, 214, 10);
pub const PURPLE: Color32 = Color32::from_rgb(191, 90, 242);
pub const TEAL: Color32 = Color32::from_rgb(64, 200, 224);
pub const INDIGO: Color32 = Color32::from_rgb(94, 92, 230);
pub const PINK: Color32 = Color32::from_rgb(255, 55, 95);

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Windows: Segoe UI (+ Semibold) from the system, nothing bundled.
/// Elsewhere (test builds): egui's default fonts + DejaVu Sans Bold.
pub fn fonts(ctx: &egui::Context) {
    let mut f = if cfg!(windows) { egui::FontDefinitions::empty() } else { egui::FontDefinitions::default() };
    let win = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let wf = |n: &str| Path::new(&win).join("Fonts").join(n);
    let mut load = |key: &str, paths: &[std::path::PathBuf]| -> bool {
        for p in paths {
            if let Ok(b) = std::fs::read(p) {
                f.font_data.insert(key.into(), Arc::new(egui::FontData::from_owned(b)));
                return true;
            }
        }
        false
    };
    let reg = load("ui", &[wf("segoeui.ttf")]);
    let semi = load("semi", &[wf("seguisb.ttf"), wf("segoeuib.ttf"), "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf".into()]);
    let sym = load("sym", &[wf("seguisym.ttf")]);
    let emo = load("emoji", &[wf("seguiemj.ttf")]);
    let mono = load("mono", &[wf("consola.ttf")]);
    let prop = f.families.entry(FontFamily::Proportional).or_default();
    if reg {
        prop.insert(0, "ui".into());
    }
    for (ok, k) in [(sym, "sym"), (emo, "emoji")] {
        if ok {
            prop.push(k.into());
        }
    }
    let prop_list = prop.clone();
    let m = f.families.entry(FontFamily::Monospace).or_default();
    if mono {
        m.insert(0, "mono".into());
    }
    m.extend(prop_list.iter().cloned());
    let mut b = vec![];
    if semi {
        b.push("semi".to_string());
    }
    b.extend(prop_list);
    f.families.insert(FontFamily::Name("bold".into()), b);
    ctx.set_fonts(f);
}

pub fn apply(ctx: &egui::Context) {
    fonts(ctx);
    let mut s = (*ctx.style()).clone();
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = CELL;
    v.window_stroke = Stroke::NONE;
    v.window_corner_radius = CornerRadius::same(14);
    v.window_shadow = egui::Shadow { offset: [0, 12], blur: 40, spread: 0, color: Color32::from_black_alpha(160) };
    v.popup_shadow = egui::Shadow { offset: [0, 8], blur: 30, spread: 0, color: Color32::from_black_alpha(150) };
    v.menu_corner_radius = CornerRadius::same(13);
    v.extreme_bg_color = CELL2;
    v.faint_bg_color = CELL;
    v.selection.bg_fill = BLUE.gamma_multiply(0.6);
    v.selection.stroke = Stroke::new(1.0f32, LABEL);
    v.hyperlink_color = BLUE;
    v.text_cursor.stroke = Stroke::new(2.0f32, BLUE);
    let r = CornerRadius::same(8);
    let w = &mut v.widgets;
    w.noninteractive.bg_stroke = Stroke::new(0.5f32, SEP);
    w.noninteractive.fg_stroke = Stroke::new(1.0f32, LABEL);
    for (st, bg) in [(&mut w.inactive, CELL2), (&mut w.hovered, CELL3), (&mut w.active, GRAY3), (&mut w.open, CELL3)] {
        st.bg_fill = bg;
        st.weak_bg_fill = bg;
        st.bg_stroke = Stroke::NONE;
        st.fg_stroke = Stroke::new(1.5f32, LABEL);
        st.corner_radius = r;
        st.expansion = 0.0;
    }
    s.visuals = v;
    s.spacing.item_spacing = vec2(8.0, 6.0);
    s.spacing.button_padding = vec2(10.0, 5.0);
    s.spacing.interact_size = vec2(40.0, 28.0);
    s.spacing.combo_width = 140.0;
    s.spacing.scroll.floating = true;
    use egui::TextStyle as T;
    s.text_styles = [
        (T::Small, FontId::proportional(12.5)),
        (T::Body, FontId::proportional(15.0)),
        (T::Button, FontId::proportional(15.0)),
        (T::Heading, bold(20.0)),
        (T::Monospace, FontId::monospace(13.5)),
    ]
    .into();
    ctx.set_style(s);
}

// ---------------- text ----------------

pub fn large_title(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).font(bold(32.0)).color(LABEL));
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).size(15.0).color(GRAY));
    }
    ui.add_space(12.0);
}

pub fn gray(s: impl Into<String>) -> RichText {
    RichText::new(s.into()).color(GRAY)
}

pub fn semibold(s: impl Into<String>, size: f32) -> RichText {
    RichText::new(s.into()).font(bold(size))
}

// ---------------- buttons & controls ----------------

/// Plain tinted text button, like an iOS bar button.
pub fn text_btn(ui: &mut Ui, text: &str, col: Color32, strong: bool, enabled: bool) -> bool {
    let col = if enabled { col } else { GRAY3 };
    let t = if strong { semibold(text, 15.0).color(col) } else { RichText::new(text).size(15.0).color(col) };
    ui.add_enabled(enabled, egui::Button::new(t).frame(false)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Filled capsule button (prominent).
pub fn filled_btn(ui: &mut Ui, text: &str, col: Color32) -> bool {
    ui.add(egui::Button::new(semibold(text, 14.0).color(LABEL)).fill(col).corner_radius(CornerRadius::same(255)).min_size(vec2(0.0, 28.0)))
        .clicked()
}

/// iOS switch.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(51.0, 31.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.18);
    let p = ui.painter();
    let bg = Color32::from_rgb(57, 57, 61).lerp_to_gamma(GREEN, t);
    p.rect_filled(rect, CornerRadius::same(255), bg);
    let r = 13.5;
    let cx = egui::lerp((rect.left() + 2.0 + r)..=(rect.right() - 2.0 - r), t);
    p.circle_filled(egui::pos2(cx, rect.center().y + 0.5), r + 0.5, Color32::from_black_alpha(60));
    p.circle_filled(egui::pos2(cx, rect.center().y), r, LABEL);
    resp
}

/// iOS stepper ( − | + ). Returns -1 / +1 when tapped.
pub fn stepper(ui: &mut Ui) -> i32 {
    let (rect, resp) = ui.allocate_exact_size(vec2(94.0, 30.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::same(8), CELL2);
    let (l, r) = rect.split_left_right_at_fraction(0.5);
    p.line_segment([egui::pos2(rect.center().x, rect.top() + 7.0), egui::pos2(rect.center().x, rect.bottom() - 7.0)], Stroke::new(1.0f32, GRAY3));
    let mut d = 0;
    for (half, sign) in [(l, -1), (r, 1)] {
        let hr = ui.interact(half, resp.id.with(sign), Sense::click());
        if hr.hovered() {
            p.rect_filled(half.shrink(2.0), CornerRadius::same(6), CELL3);
        }
        let c = half.center();
        let s = Stroke::new(2.0f32, LABEL);
        p.line_segment([c - vec2(6.0, 0.0), c + vec2(6.0, 0.0)], s);
        if sign > 0 {
            p.line_segment([c - vec2(0.0, 6.0), c + vec2(0.0, 6.0)], s);
        }
        if hr.clicked() {
            d = sign;
        }
    }
    d
}

/// Rounded-square icon tile with a white glyph (Settings-app style).
pub fn tile(ui: &mut Ui, glyph: &str, col: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(29.0, 29.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(7), col);
    p.text(rect.center(), egui::Align2::CENTER_CENTER, glyph, bold(14.0), LABEL);
}

pub fn chevron(ui: &mut Ui, open: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(14.0, 20.0), Sense::hover());
    let c = rect.center();
    let s = Stroke::new(2.0f32, GRAY3);
    let pts = if open {
        [c + vec2(-5.0, -2.5), c + vec2(0.0, 2.5), c + vec2(5.0, -2.5)]
    } else {
        [c + vec2(-2.5, -5.0), c + vec2(2.5, 0.0), c + vec2(-2.5, 5.0)]
    };
    ui.painter().line_segment([pts[0], pts[1]], s);
    ui.painter().line_segment([pts[1], pts[2]], s);
}

/// Rounded search field with a drawn magnifier.
pub fn search(ui: &mut Ui, text: &mut String, hint: &str) -> Response {
    Frame::new()
        .fill(CELL2)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 22.0), Sense::hover());
                let c = rect.center() - vec2(1.0, 1.0);
                ui.painter().circle_stroke(c, 5.0, Stroke::new(1.6f32, GRAY));
                ui.painter().line_segment([c + vec2(3.6, 3.6), c + vec2(7.0, 7.0)], Stroke::new(1.8f32, GRAY));
                ui.add(egui::TextEdit::singleline(text).hint_text(gray(hint)).frame(false).desired_width(f32::INFINITY))
            })
            .inner
        })
        .inner
}

/// Borderless, right-aligned text field as used in iOS forms.
pub fn text_field(ui: &mut Ui, text: &mut String, w: f32) -> Response {
    ui.add(egui::TextEdit::singleline(text).frame(false).horizontal_align(Align::RIGHT).desired_width(w).text_color(GRAY))
}

// ---------------- inset grouped sections ----------------

pub struct Section<'a> {
    pub ui: &'a mut Ui,
    n: usize,
    indent: f32,
}

impl Section<'_> {
    /// One list row (44pt min). Separators are inset like iOS.
    pub fn row<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.row_indent(16.0, add)
    }
    /// Row whose following separator starts at `indent` (e.g. after an icon tile).
    pub fn row_indent<R>(&mut self, indent: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
        if self.n > 0 {
            let top = self.ui.cursor().top();
            let rect = self.ui.max_rect();
            self.ui.painter().hline((rect.left() + self.indent)..=rect.right(), top, Stroke::new(0.6f32, SEP));
        }
        self.indent = indent;
        self.n += 1;
        Frame::new()
            .inner_margin(Margin { left: 16, right: 14, top: 7, bottom: 7 })
            .show(self.ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.set_min_height(30.0);
                    add(ui)
                })
                .inner
            })
            .inner
    }
    /// label  ……  [trailing widgets]
    pub fn kv<R>(&mut self, label: impl Into<RichText>, trailing: impl FnOnce(&mut Ui) -> R) -> R {
        let label = label.into();
        self.row(|ui| {
            ui.label(label);
            ui.with_layout(Layout::right_to_left(Align::Center), trailing).inner
        })
    }
}

pub fn section<R>(ui: &mut Ui, header: Option<&str>, footer: Option<&str>, add: impl FnOnce(&mut Section) -> R) -> R {
    section_c(ui, header, footer, CELL, add)
}

pub fn section_c<R>(ui: &mut Ui, header: Option<&str>, footer: Option<&str>, fill: Color32, add: impl FnOnce(&mut Section) -> R) -> R {
    if let Some(h) = header {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.label(RichText::new(h.to_uppercase()).size(12.5).color(GRAY));
        });
        ui.add_space(2.0);
    }
    let r = Frame::new().fill(fill).corner_radius(CornerRadius::same(12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut s = Section { ui, n: 0, indent: 16.0 };
        add(&mut s)
    });
    if let Some(f) = footer {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.add(egui::Label::new(RichText::new(f).size(12.5).color(GRAY)).wrap());
        });
    }
    ui.add_space(22.0);
    r.inner
}

/// Make an already-drawn area clickable (sidebar items, expandable rows).
pub fn click_area(ui: &mut Ui, rect: Rect, id: egui::Id) -> Response {
    ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand)
}
