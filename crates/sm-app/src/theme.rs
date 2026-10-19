//! Identidad visual Smart Mix (universo Avant Garden).
//!
//! Estética oscura, editorial y refinada: papel cálido sobre negro tinta,
//! un acento chartreuse eléctrico y tipografía serif para títulos.
//! Sistema visual separado de la lógica: todos los colores viven aquí.

use egui::{Color32, Rgba};

#[derive(Debug, Clone)]
pub struct Theme {
    pub bg: Color32,
    pub surface: Color32,
    pub surface2: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub accent_dim: Color32,
    pub danger: Color32,
    pub deck_a: Color32,
    pub deck_b: Color32,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            bg: Color32::from_rgb(10, 10, 11),
            surface: Color32::from_rgb(18, 18, 20),
            surface2: Color32::from_rgb(26, 26, 30),
            border: Color32::from_rgb(42, 42, 48),
            text: Color32::from_rgb(236, 231, 221),
            text_dim: Color32::from_rgb(143, 139, 130),
            accent: Color32::from_rgb(180, 244, 97),
            accent_dim: Color32::from_rgb(107, 148, 64),
            danger: Color32::from_rgb(244, 97, 107),
            deck_a: Color32::from_rgb(180, 244, 97),
            deck_b: Color32::from_rgb(167, 139, 250),
        }
    }
}

/// Color por nivel de energía 1..=10 (frío → cálido).
pub fn energy_color(level: f64) -> Color32 {
    let l = level.clamp(1.0, 10.0) / 10.0;
    let r = (40.0 + 215.0 * l.powf(1.4)) as u8;
    let g = (150.0 + 60.0 * (1.0 - l)) as u8;
    let b = (170.0 * (1.0 - l).powf(1.5) + 30.0) as u8;
    Color32::from_rgb(r, g, b)
}

/// Color de la celda de clave Camelot (misma familia = matiz similar).
pub fn key_color(code: &str) -> Color32 {
    let k = sm_model::camelot::CamelotKey::parse(code);
    let hue = match k {
        Some(k) => (k.num as f32 - 1.0) / 12.0,
        None => 0.5,
    };
    let c = Rgba::from_hsv(hue, 0.28, 0.95);
    Color32::from_rgba_premultiplied(
        (c.r() * 255.0) as u8,
        (c.g() * 255.0) as u8,
        (c.b() * 255.0) as u8,
        255,
    )
}

pub fn install(ctx: &egui::Context) {
    let t = Theme::default();
    let mut v = egui::Visuals::dark();
    v.panel_fill = t.bg;
    v.window_fill = t.surface;
    v.extreme_bg_color = t.bg;
    v.faint_bg_color = t.surface;
    v.code_bg_color = t.surface2;
    v.widgets.noninteractive.bg_fill = t.surface;
    v.widgets.inactive.bg_fill = t.surface2;
    v.widgets.hovered.bg_fill = t.surface2;
    v.widgets.active.bg_fill = t.border;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, t.text_dim);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, t.text);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, t.text);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, t.accent);
    v.selection.bg_fill = Color32::from_rgba_premultiplied(180, 244, 97, 36);
    v.selection.stroke = egui::Stroke::new(1.0, t.accent);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, t.border);
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, t.border);
    v hyperlink_color = t.accent;
    ctx.set_visuals(v);

    // Tipografía editorial: serif (Georgia) para Heading; resto por defecto.
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/georgia.ttf") {
        fonts
            .font_data
            .insert("georgia".into(), egui::FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(egui::FontFamily::Name("Heading".into()))
            .or_default()
            .push("georgia".into());
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "georgia".into());
    }
    if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/georgiab.ttf") {
        fonts
            .font_data
            .insert("georgia-bold".into(), egui::FontData::from_owned(bytes).into());
        fonts
            .families
            .get_mut(&egui::FontFamily::Name("Heading".into()))
            .unwrap()
            .push("georgia-bold".into());
    }
    ctx.set_fonts(fonts);
}

/// Wordmark del logo (dibujado, no imagen).
pub fn wordmark(ui: &mut egui::Ui) -> egui::Response {
    let t = Theme::default();
    let size = egui::vec2(150.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    // cuadrado de acento (marca "avant")
    let sq = egui::Rect::from_min_size(rect.left_center() - egui::vec2(0.0, 9.0), egui::vec2(10.0, 18.0));
    painter.rect_filled(sq, 1.0, t.accent);
    let galce = egui::Rect::from_min_size(sq.right_bottom() - egui::vec2(0.0, 4.0), egui::vec2(6.0, 4.0));
    painter.rect_filled(galce, 1.0, t.bg);
    painter.text(
        rect.left_center() + egui::vec2(18.0, 0.0),
        egui::Align2::LEFT_CENTER,
        "SMART MIX",
        egui::FontId::new(17.0, egui::FontFamily::Name("Heading".into())),
        t.text,
    );
    resp
}
