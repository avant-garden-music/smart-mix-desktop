//! Identidad visual Smart Mix (universo Avant Garden).
//!
//! Tipografías reales de `assets/`:
//!   Display   → Dela Gothic One (wordmark, titulares — estilo Publisher)
//!   Heading   → Termina 800 (títulos de sección)
//!   Funcional → Tusker Grotesk 1500 Medium (UI, tabla, botones, metadata)
//! Acento lavanda sobre negro tinta, referencia publisher.avant---garden.com.
//! Sistema visual separado de la lógica: todos los colores/tipos viven aquí.

use egui::Color32;

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
            bg: Color32::from_rgb(9, 9, 12),
            surface: Color32::from_rgb(16, 16, 21),
            surface2: Color32::from_rgb(24, 24, 31),
            border: Color32::from_rgb(38, 38, 48),
            text: Color32::from_rgb(238, 236, 245),
            text_dim: Color32::from_rgb(168, 165, 182),
            accent: Color32::from_rgb(185, 167, 255),
            accent_dim: Color32::from_rgb(112, 98, 172),
            danger: Color32::from_rgb(255, 108, 118),
            deck_a: Color32::from_rgb(185, 167, 255),
            deck_b: Color32::from_rgb(127, 212, 193),
        }
    }
}

/// Familia Display (Dela Gothic One).
pub const FONT_DISPLAY: &str = "ag_display";
/// Familia Heading (Termina 800).
pub const FONT_HEADING: &str = "ag_heading";
/// Familia funcional (Tusker Grotesk 1500).
pub const FONT_BODY: &str = "ag_body";

/// Ids tipográficos centralizados (fábrica para sustituir el diseño luego).
pub fn font_display(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(FONT_DISPLAY.into()))
}
pub fn font_heading(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(FONT_HEADING.into()))
}
pub fn font_body(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(FONT_BODY.into()))
}

/// Instala fuentes y visuales. Busca assets/ junto al ejecutable y en el CWD
/// (repo en desarrollo). Si falta un archivo, ese slot queda con la fuente
/// por defecto (documentado en docs/PROVENANCE.md).
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
    v.widgets.hovered.bg_fill = Color32::from_rgb(30, 30, 40);
    v.widgets.active.bg_fill = t.border;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, t.text_dim);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, t.text);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, t.text);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, t.accent);
    v.selection.bg_fill = Color32::from_rgba_premultiplied(185, 167, 255, 40);
    v.selection.stroke = egui::Stroke::new(1.0, t.accent);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, t.border);
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, t.border);
    v.hyperlink_color = t.accent;
    ctx.set_visuals(v);

    let mut fonts = egui::FontDefinitions::default();
    let mut install = |name: &str, rel_path: &str| -> bool {
        for base in asset_bases() {
            let p = base.join(rel_path);
            if let Ok(bytes) = std::fs::read(&p) {
                fonts
                    .font_data
                    .insert(name.to_string(), egui::FontData::from_owned(bytes).into());
                return true;
            }
        }
        false
    };
    let display = install(FONT_DISPLAY, "assets/DelaGothicOne-Regular.ttf");
    let heading = install(FONT_HEADING, "assets/termina/termina-800.ttf");
    let body = install(FONT_BODY, "assets/tusker/TuskerGrotesk-1500Medium.ttf");
    if body {
        let fams = fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap();
        fams.insert(0, FONT_BODY.to_string());
    }
    if display {
        fonts
            .families
            .entry(egui::FontFamily::Name(FONT_DISPLAY.into()))
            .or_default()
            .push(FONT_DISPLAY.to_string());
    }
    if heading {
        fonts
            .families
            .entry(egui::FontFamily::Name(FONT_HEADING.into()))
            .or_default()
            .push(FONT_HEADING.to_string());
    }
    ctx.set_fonts(fonts);
    if !display || !heading || !body {
        eprintln!(
            "[fonts] AV: display={display} heading={heading} body={body} (assets/ no encontrado en algunas bases)"
        );
    }
}

fn asset_bases() -> Vec<std::path::PathBuf> {
    let mut bases = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            bases.push(d.to_path_buf());
            if let Some(repo) = d.ancestors().nth(2) {
                bases.push(repo.to_path_buf());
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        bases.push(cwd);
    }
    bases
}

/// Color por nivel de energía 1..=10 (frío → cálido).
pub fn energy_color(level: f64) -> Color32 {
    let l = level.clamp(1.0, 10.0) / 10.0;
    let r = (60.0 + 195.0 * l.powf(1.3)) as u8;
    let g = (140.0 + 70.0 * (1.0 - l)) as u8;
    let b = (185.0 * (1.0 - l).powf(1.3) + 70.0 * l) as u8;
    Color32::from_rgb(r, g, b)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let w = v * (1.0 - (1.0 - f) * s);
    match (i as i32) % 6 {
        0 => (v, w, p),
        1 => (q, v, p),
        2 => (p, v, w),
        3 => (p, q, v),
        4 => (w, p, v),
        _ => (v, p, q),
    }
}

/// Color de la celda de clave Camelot (misma familia de matiz por vecindad).
pub fn key_color(code: &str) -> Color32 {
    let hue = match sm_model::camelot::CamelotKey::parse(code) {
        Some(k) => (k.num as f32 - 1.0) / 12.0,
        None => 0.62,
    };
    let (r, g, b) = hsv_to_rgb(hue, 0.30, 0.96);
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// Wordmark con tipografía Display propia.
pub fn wordmark(ui: &mut egui::Ui) -> egui::Response {
    let t = Theme::default();
    let size = egui::vec2(180.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    let sq = egui::Rect::from_min_size(
        rect.left_center() - egui::vec2(0.0, 11.0),
        egui::vec2(8.0, 22.0),
    );
    painter.rect_filled(sq, 2.0, t.accent);
    let notch =
        egui::Rect::from_min_size(sq.right_top() + egui::vec2(0.0, 5.0), egui::vec2(4.0, 4.0));
    painter.rect_filled(notch, 1.0, t.bg);
    painter.text(
        rect.left_center() + egui::vec2(16.0, -3.0),
        egui::Align2::LEFT_CENTER,
        "SMART MIX",
        font_display(15.0),
        t.text,
    );
    painter.text(
        rect.left_bottom() + egui::vec2(17.0, -2.0),
        egui::Align2::LEFT_BOTTOM,
        "AVANT GARDEN MUSIC",
        font_heading(8.5),
        t.text_dim,
    );
    resp
}
