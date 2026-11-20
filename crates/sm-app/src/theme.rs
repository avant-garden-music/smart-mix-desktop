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
    // —— Paleta canónica handoff-v1 §3 (20 colores exactos de biblioteca) ——
    /// Brand/Background #0B0B0D.
    pub bg: Color32,
    /// Surface/Base #131315 (alias legacy `surface`).
    pub surface: Color32,
    /// Surface/Container Low #1C1B1D (alias legacy `surface2`).
    pub surface2: Color32,
    /// Border/Variant #4A4550.
    pub border: Color32,
    /// Content/Primary #E5E2E1.
    pub text: Color32,
    /// Content/Muted #8F8B82.
    pub text_dim: Color32,
    /// Brand/Primary (lavanda) #DDC9FF — acción primaria, loaded deck.
    pub accent: Color32,
    /// Lavanda atenuada (derivada, para fondos tenues de acento).
    #[allow(dead_code)] // tokens del spec sin consumir aun
    pub accent_dim: Color32,
    /// Semantic/Error #FFB4AB.
    #[allow(dead_code)] // tokens spec sin consumir
    pub danger: Color32,
    /// Deck A cargado: lavanda canónico (§8 "loaded deck").
    pub deck_a: Color32,
    /// Deck B cargado: cian de marca.
    pub deck_b: Color32,
    /// Hover de fila: blanco 7% (hairline §5).
    pub row_hover: Color32,
    /// Selección: cian #1FD3FA al 10% (§8).
    pub row_selected: Color32,
    // —— Colores canónicos extendidos (§3) ——
    /// Brand/Magenta #F50077 — playing, playhead, tiempos.
    pub magenta: Color32,
    /// Brand/Cyan #1FD3FA — selección, focus ring, progreso.
    pub cyan: Color32,
    /// Brand/Orange #FB732F — unanalyzed, avisos.
    pub orange: Color32,
    /// Brand/Acid #DDFF00 — logo, "compatible".
    pub acid: Color32,
    /// Semantic/Success #A8DAB5.
    pub success: Color32,
    /// Semantic/Warning #F8E3A1.
    pub warning: Color32,
    /// Border/Outline #958E9B.
    pub outline: Color32,
    /// Content/Secondary #CBC4D2.
    pub text_secondary: Color32,
    /// Surface/Lowest #0E0E10.
    pub lowest: Color32,
    /// Surface/Container #201F21.
    pub container: Color32,
    /// Surface/Container High #2A2A2C.
    pub container_high: Color32,
    /// Surface/Container Highest #353437.
    pub container_highest: Color32,
    /// F-25: true cuando esta paleta es la clara (deriva del modo global).
    pub light: bool,
}

/// Modo global de paleta (F-25): lo fija el arranque desde Preferences
/// y el toggle de Preferencias en vivo. `Theme::default()` lo lee, asi los
/// ~40 call-sites existentes cambian de paleta sin tocarse.
static LIGHT_MODE: std::sync::OnceLock<std::sync::RwLock<bool>> = std::sync::OnceLock::new();

fn light_cell() -> &'static std::sync::RwLock<bool> {
    LIGHT_MODE.get_or_init(|| std::sync::RwLock::new(false))
}

/// Fija el modo de paleta (true = claro). Idempotente y barato.
pub fn set_light_mode(on: bool) {
    if let Ok(mut g) = light_cell().write() {
        *g = on;
    }
}

/// Modo actual (false = oscuro, default).
pub fn light_mode() -> bool {
    light_cell().read().map(|g| *g).unwrap_or(false)
}

impl Theme {
    /// Paleta oscura canonica penpot #0B0B0D (handoff-v1 §3) — Avant Garden.
    /// Re-skin 2026-11-09: estructura del kit Studio conservada, piel AG.
    pub fn dark() -> Self {
        let accent = Color32::from_rgb(221, 201, 255); // #DDC9FF
        let bg = Color32::from_rgb(11, 11, 13); // #0B0B0D
                                                // lavanda atenuada: mezcla 45% accent / 55% bg
        let accent_dim = Color32::from_rgb(
            (221.0 * 0.45 + 11.0 * 0.55) as u8,
            (201.0 * 0.45 + 11.0 * 0.55) as u8,
            (255.0 * 0.45 + 13.0 * 0.55) as u8,
        );
        Theme {
            light: false,
            bg,
            surface: Color32::from_rgb(19, 19, 21), // Base #131315
            surface2: Color32::from_rgb(28, 27, 29), // Container Low #1C1B1D
            border: Color32::from_rgb(74, 69, 80),  // Variant #4A4550
            text: Color32::from_rgb(229, 226, 225), // Content Primary #E5E2E1
            text_dim: Color32::from_rgb(143, 139, 130), // Muted #8F8B82
            text_secondary: Color32::from_rgb(203, 196, 210), // #CBC4D2
            lowest: Color32::from_rgb(14, 14, 16),  // #0E0E10
            container: Color32::from_rgb(32, 31, 33), // #201F21
            container_high: Color32::from_rgb(42, 42, 44), // #2A2A2C
            container_highest: Color32::from_rgb(53, 52, 55), // #353437
            accent,
            accent_dim,
            danger: Color32::from_rgb(255, 180, 171), // Error #FFB4AB
            deck_a: accent,
            deck_b: Color32::from_rgb(31, 211, 250), // Cyan #1FD3FA
            row_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 18),
            row_selected: Color32::from_rgba_unmultiplied(31, 211, 250, 26),
            magenta: Color32::from_rgb(245, 0, 119), // #F50077
            cyan: Color32::from_rgb(31, 211, 250),   // #1FD3FA
            orange: Color32::from_rgb(251, 115, 47), // #FB732F (waveform MEDIOS)
            acid: Color32::from_rgb(221, 255, 0),    // #DDFF00
            success: Color32::from_rgb(168, 218, 181), // #A8DAB5
            warning: Color32::from_rgb(248, 227, 161), // #F8E3A1
            outline: Color32::from_rgb(149, 142, 155), // #958E9B
        }
    }

    /// Paleta clara F-25: fondos #F7F6F3/#FFFFFF, tinta #1A1A1E; los
    /// acentos de marca se oscurecen para contraste AA (≥4.5:1) sobre
    /// claro — verificado por test con la formula WCAG. `text_dim` baja a
    /// #66666D (el #6B6B72 de spec queda en 4.44:1 sobre surface2).
    pub fn light() -> Self {
        let accent = Color32::from_rgb(0x6B, 0x4F, 0xBB); // lavanda oscurecida
        let bg = Color32::from_rgb(0xF7, 0xF6, 0xF3);
        let accent_dim = Color32::from_rgb(0xB8, 0xAB, 0xDA); // 45/55 con bg
        Theme {
            light: true,
            bg,
            surface: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            surface2: Color32::from_rgb(0xED, 0xEB, 0xE6),
            border: Color32::from_rgb(0xE2, 0xE0, 0xDC),
            text: Color32::from_rgb(0x1A, 0x1A, 0x1E),
            text_dim: Color32::from_rgb(0x66, 0x66, 0x6D),
            text_secondary: Color32::from_rgb(0x4A, 0x4A, 0x52),
            lowest: Color32::from_rgb(0xED, 0xEB, 0xE6),
            container: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            container_high: Color32::from_rgb(0xE2, 0xE0, 0xDC),
            container_highest: Color32::from_rgb(0xD8, 0xD5, 0xCF),
            accent,
            accent_dim,
            danger: Color32::from_rgb(0xB3, 0x26, 0x1E),
            deck_a: accent,
            deck_b: Color32::from_rgb(0x0E, 0x74, 0x90),
            row_hover: Color32::from_rgba_unmultiplied(0, 0, 0, 13),
            row_selected: Color32::from_rgba_unmultiplied(14, 116, 144, 30),
            magenta: Color32::from_rgb(0xAD, 0x00, 0x54),
            cyan: Color32::from_rgb(0x0E, 0x74, 0x90),
            orange: Color32::from_rgb(0xB5, 0x47, 0x08),
            acid: Color32::from_rgb(0x5F, 0x6C, 0x00),
            success: Color32::from_rgb(0x1B, 0x6B, 0x33),
            warning: Color32::from_rgb(0x80, 0x63, 0x00),
            outline: Color32::from_rgb(0xB9, 0xB6, 0xAF),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        if light_mode() {
            Theme::light()
        } else {
            Theme::dark()
        }
    }
}

/// Contraste WCAG 2.x entre dos colores (1.0..=21.0). Formula pura, misma
/// definicion que la spec: luminancia relativa sRGB y (L1+0.05)/(L2+0.05).
#[cfg_attr(not(test), allow(dead_code))] // usado por los tests WCAG del tema
pub fn contrast_ratio(a: Color32, b: Color32) -> f64 {
    let lin = |c: u8| -> f64 {
        let c = c as f64 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let lum =
        |c: Color32| -> f64 { 0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b()) };
    let (l1, l2) = (lum(a), lum(b));
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

/// Lienzo propio del waveform (F-25): SIEMPRE oscuro canonico #0B0B0D —
/// las 3 bandas espectrales de widgets.rs traen colores claros hardcodeados
/// (altos #F2F2F4) que sobre fondo claro serian invisibles; recalibrar las
/// bandas para claro queda fuera de alcance (widgets.rs intocable).
pub const WAVE_BG: Color32 = Color32::from_rgb(11, 11, 13);

/// Familia Display — acentos de marca (Dela Gothic One, OFL).
pub const FONT_DISPLAY: &str = "ag_display";
/// Familia Heading — encabezados (Sora, OFL).
pub const FONT_HEADING: &str = "ag_heading";
/// Familia funcional — interfaz (Barlow Semi Condensed, OFL).
pub const FONT_BODY: &str = "ag_body";
/// Editorial — títulos grandes (Big Shoulders Display, OFL).
pub const FONT_EDITORIAL: &str = "ag_editorial";
/// Datos numéricos — BPM/LUFS/duración (JetBrains Mono, OFL).
pub const FONT_MONO: &str = "ag_mono";

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
#[allow(dead_code)] // tipografia Display del spec, sin consumir
pub fn font_editorial(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(FONT_EDITORIAL.into()))
}
#[allow(dead_code)]
pub fn font_mono(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name(FONT_MONO.into()))
}

/// Instala fuentes y visuales. Busca assets/ junto al ejecutable y en el CWD
/// (repo en desarrollo). Si falta un archivo, ese slot queda con la fuente
/// por defecto (documentado en docs/PROVENANCE.md).
pub fn install(ctx: &egui::Context) {
    // Re-skin AG: spacing del kit recalibrado a la escala herramienta de referencia +2pt (base
    // ~17px, botones ~36px de alto) — la estructura del kit se conserva.
    {
        let mut st = (*ctx.style()).clone();
        st.spacing.item_spacing = egui::vec2(8.0, 6.0);
        st.spacing.button_padding = egui::vec2(12.0, 7.0);
        st.spacing.interact_size = egui::vec2(36.0, 36.0);
        st.spacing.window_margin = egui::Margin::same(16.0);
        ctx.set_style(st);
    }
    let t = Theme::default();
    // F-25: la base sigue el modo (el resto de fills ya derivan de t)
    let mut v = if t.light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    // Superficies canónicas §3: app = Background, ventanas = Container,
    // campos débiles = Lowest → jerarquía por elevación de superficie.
    v.panel_fill = t.bg;
    v.window_fill = t.container;
    v.extreme_bg_color = t.bg;
    v.faint_bg_color = t.lowest;
    v.code_bg_color = t.container_high;
    v.widgets.noninteractive.bg_fill = t.lowest;
    v.widgets.inactive.bg_fill = t.surface2;
    v.widgets.hovered.bg_fill = t.container_high;
    v.widgets.active.bg_fill = t.container_highest;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, t.text_dim);
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, t.text);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, t.text);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, t.cyan);
    // Selección canónica §8: cian al 10% (alpha recta, NO premult) —
    // F-25: derivada del tema (oscuro = cian #1FD3FA al 10%, idéntico a hoy)
    v.selection.bg_fill = t.row_selected;
    v.selection.stroke = egui::Stroke::new(1.0, t.cyan);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, t.border);
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, t.border);
    // fix ui-rb2 — SCROLLBARS ESTILO CHROMIUM: delgadas (~5px), thumb
    // redondeado semitransparente y SIN track visible. El scrollbar de
    // egui pinta el thumb con widgets.{inactive,hovered,active}.bg_fill
    // cuando foreground_color=false (los botones usan weak_bg_fill, así
    // que no se ven afectados), y el track con extreme_bg_color == el
    // fondo del panel → invisible. El rounding de píldora (bar_width/2)
    // va en el rounding de esos mismos widget-visuals. Lo heredan TODOS
    // los ScrollArea: la tabla (egui_extras usa el ScrollStyle global
    // también para descontar su carril del ancho disponible — ver
    // table_available_width), el sidebar y la rueda.
    let (thumb_idle, thumb_hover, thumb_base) = if t.light {
        // en claro el thumb es tinta translúcida (blanco no se vería)
        (
            Color32::from_rgba_unmultiplied(0, 0, 0, 46), // ~18%
            Color32::from_rgba_unmultiplied(0, 0, 0, 90), // ~35%
            Color32::from_rgba_unmultiplied(0, 0, 0, 128),
        )
    } else {
        (
            Color32::from_rgba_unmultiplied(255, 255, 255, 38), // ~15%
            Color32::from_rgba_unmultiplied(255, 255, 255, 77), // ~30%
            Color32::from_rgba_unmultiplied(255, 255, 255, 115),
        )
    };
    v.widgets.inactive.bg_fill = thumb_idle;
    v.widgets.hovered.bg_fill = thumb_hover;
    v.widgets.active.bg_fill = thumb_base;
    v.widgets.inactive.rounding = egui::Rounding::same(2.5);
    v.widgets.hovered.rounding = egui::Rounding::same(2.5);
    v.widgets.active.rounding = egui::Rounding::same(2.5);
    v.hyperlink_color = t.cyan;
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
    let heading = install(FONT_HEADING, "assets/fonts/sora/sora-v17-latin-700.ttf");
    let body = install(
        FONT_BODY,
        "assets/fonts/barlow-semi-condensed/barlow-semi-condensed-v16-latin-500.ttf",
    );
    let editorial = install(
        FONT_EDITORIAL,
        "assets/fonts/big-shoulders-display/bsd-700.ttf",
    );
    let mono = install(
        FONT_MONO,
        "assets/fonts/jetbrains-mono/jetbrains-mono-v24-latin-400.ttf",
    );
    // Re-skin AG: SIN inserción de Segoe UI del sistema — la interfaz vuelve
    // a Barlow Semi Condensed (ag_body) como proportional primaria y Sora
    // para headings, según la identidad Avant Garden.
    if body {
        let fams = fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap();
        fams.insert(0, FONT_BODY.to_string());
        // además registrar la familia NOMBRADA ag_body: sin esto, cualquier
        // FontId con FontFamily::Name(FONT_BODY) pániquea en runtime
        fonts
            .families
            .entry(egui::FontFamily::Name(FONT_BODY.into()))
            .or_default()
            .push(FONT_BODY.to_string());
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
    if editorial {
        fonts
            .families
            .entry(egui::FontFamily::Name(FONT_EDITORIAL.into()))
            .or_default()
            .push(FONT_EDITORIAL.to_string());
    }
    if mono {
        // Registra la familia NOMBRADA ag_mono (la usan font_mono() y el
        // estilo Monospace) y además redirige la familia Monospace nativa de
        // egui hacia JetBrains Mono (todos los .monospace() la heredan).
        fonts
            .families
            .entry(egui::FontFamily::Name(FONT_MONO.into()))
            .or_default()
            .push(FONT_MONO.to_string());
        fonts
            .families
            .insert(egui::FontFamily::Monospace, vec![FONT_MONO.to_string()]);
    }
    ctx.set_fonts(fonts);

    // Tipografía global más legible — DESPUÉS de registrar fuentes, y el
    // override Monospace→JetBrains Mono solo si el archivo cargó (si no,
    // unbind de familia = panic en runtime).
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(15.5, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(14.5, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(14.5, egui::FontFamily::Proportional), // herramienta de referencia chrome ~14.5
    );
    if mono {
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(14.0, egui::FontFamily::Name(FONT_MONO.into())),
        );
    }
    // fix ui-rb2 — Scrollbar estilo Chromium: barra delgada de 5px PEGADA
    // al borde del contenedor (margins 0), thumb mínimo de 24px. No
    // flotante: reserva sus 5px, así la tabla (egui_extras descuenta
    // exactamente inner+bar+outer del ancho disponible) nunca queda con
    // contenido bajo el thumb y el scrollbar vertical vive fijo al borde
    // derecho de la VENTANA (la suma de columnas es siempre el ancho
    // disponible — ver table_layout).
    let mut sc = egui::style::ScrollStyle::default();
    sc.floating = false;
    sc.bar_width = 5.0;
    sc.bar_inner_margin = 0.0;
    sc.bar_outer_margin = 0.0;
    sc.handle_min_length = 24.0;
    sc.foreground_color = false; // thumb = widgets.*.bg_fill (translúcido)
    style.spacing.scroll = sc;
    ctx.set_style(style);

    if !display || !heading || !body || !editorial || !mono {
        eprintln!(
            "[fonts] AV: display={display} heading={heading} body={body} editorial={editorial} mono={mono} (assets/ no encontrado en algunas bases)"
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

/// Color por nivel de energía 1..=10 — tira espectral canónica de la maestra
/// (§9 waveform/progreso): cian → amarillo → naranja → magenta.
pub fn energy_color(level: f64) -> Color32 {
    // F-25: en claro, las 4 paradas se oscurecen (AA sobre surface, test
    // theme::colores_de_dato_legibles_en_claro); en oscuro quedan canonicas.
    if light_mode() {
        return match level.clamp(1.0, 10.0) as i32 {
            1..=3 => Color32::from_rgb(0x0E, 0x74, 0x90), // cian oscuro
            4..=6 => Color32::from_rgb(0x5F, 0x6C, 0x00), // acid oscuro
            7..=8 => Color32::from_rgb(0xB5, 0x47, 0x08), // naranja oscuro
            _ => Color32::from_rgb(0xAD, 0x00, 0x54),     // magenta oscuro
        };
    }
    let _l = level.clamp(1.0, 10.0) / 10.0;
    // paradas canónicas: Cyan #1FD3FA · Warning #F8E3A1 · Orange #FB732F · Magenta #F50077
    // dump nativo (§7): el ribbon usa EXACTAMENTE 4 colores por segmento,
    // sin gradiente — la interpolación no tocaba los hexes exactos
    match level.clamp(1.0, 10.0) as i32 {
        1..=3 => Color32::from_rgb(31, 211, 250), // Cyan
        4..=6 => Color32::from_rgb(221, 255, 0),  // Acid
        7..=8 => Color32::from_rgb(251, 115, 47), // Orange
        _ => Color32::from_rgb(245, 0, 119),      // Magenta
    }
}

/// Color del LABEL de tonalidad — regla A/B canónica (PARITY-SPEC §8,
/// dumps nativos: 24/24 en A → cian #1FD3FA · 15/15 en B → lavanda #DDC9FF;
/// aplica a tabla, deck y tarjeta de análisis). El chip es SIEMPRE
/// Container #201F21 (38/38 nativo).
pub fn key_label_color(code: &str) -> Color32 {
    // F-25: en claro las familias se oscurecen (lavanda/cian de marca son
    // ilegibles sobre chip claro); en oscuro canonicas.
    if light_mode() {
        if code.ends_with('B') {
            return Color32::from_rgb(0x6B, 0x4F, 0xBB); // lavanda oscurecida
        }
        return Color32::from_rgb(0x0E, 0x74, 0x90); // cian oscuro
    }
    if code.ends_with('B') {
        Color32::from_rgb(221, 201, 255) // Primary #DDC9FF
    } else {
        Color32::from_rgb(31, 211, 250) // Cyan #1FD3FA
    }
}

/// Lockup horizontal oficial de Penpot (symbol #DDC9FF + wordmark Sora 700
/// vectorizado) rasterizado a PNG y embebido. Reemplaza al wordmark dibujado
/// a mano en código. La textura se cachea en ctx.data (1 sola subida a GPU).
/// Re-skin AG: vuelve al appbar de la vista Studio (dentro de la zona de
/// logo del kit, 128x30).
pub fn brand_lockup(
    ui: &mut egui::Ui,
    height: f32,
    tex: &mut Option<egui::TextureHandle>,
) -> egui::Response {
    let ctx = ui.ctx().clone();
    let tex = match tex {
        Some(t) => t,
        None => {
            let img =
                image::load_from_memory(include_bytes!("../assets/brand/logo_horizontal_128.png"))
                    .expect("logo embebido valido")
                    .to_rgba8();
            let (w, h) = (img.width(), img.height());
            let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &img);
            let t = ctx.load_texture("brand-lockup", color, egui::TextureOptions::default());
            *tex = Some(t.clone());
            tex.as_ref().unwrap()
        }
    };
    let sz = tex.size_vec2();
    let size = egui::vec2(height * (sz.x / sz.y), height);
    ui.add(egui::Image::new(egui::ImageSource::Texture(
        egui::load::SizedTexture::new(tex.id(), size),
    )))
}

/// Wordmark con tipografía Display propia.
#[allow(dead_code)] // wordmark dibujado a mano; reemplazado por brand_lockup Penpot
pub fn wordmark(ui: &mut egui::Ui) -> egui::Response {
    let t = Theme::default();
    let size = egui::vec2(180.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    let sq = egui::Rect::from_min_size(
        rect.left_center() - egui::vec2(0.0, 11.0),
        egui::vec2(8.0, 22.0),
    );
    painter.rect_filled(sq, 2.0, t.acid);
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

/// Paleta de hot cues estilo CDJ-3000/rekordbox — 8 pads A..H.
pub const HOTCUE_COLORS: [egui::Color32; 8] = [
    egui::Color32::from_rgb(0xE5, 0x3E, 0x3E), // A rojo
    egui::Color32::from_rgb(0xF2, 0x94, 0x2E), // B naranja
    egui::Color32::from_rgb(0xE7, 0xC5, 0x3A), // C amarillo
    egui::Color32::from_rgb(0x53, 0xB6, 0x6B), // D verde
    egui::Color32::from_rgb(0x3F, 0xC1, 0xC9), // E aqua
    egui::Color32::from_rgb(0x4F, 0x8D, 0xE5), // F azul
    egui::Color32::from_rgb(0x9B, 0x6D, 0xE5), // G violeta
    egui::Color32::from_rgb(0xE0, 0x6B, 0xC8), // H rosa
];

pub fn hotcue_color(i: usize) -> egui::Color32 {
    HOTCUE_COLORS[i % 8]
}

pub fn hotcue_letter(i: usize) -> char {
    (b'A' + (i as u8 % 8)) as char
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Restaura el modo global al salir del test (estado compartido).
    struct LightGuard;
    impl Default for LightGuard {
        fn default() -> Self {
            set_light_mode(false);
            LightGuard
        }
    }
    impl Drop for LightGuard {
        fn drop(&mut self) {
            set_light_mode(false);
        }
    }

    #[test]
    fn contraste_wcag_blanco_negro_21_simetrico() {
        let w = Color32::from_rgb(255, 255, 255);
        let b = Color32::from_rgb(0, 0, 0);
        assert!((contrast_ratio(w, b) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(b, w) - 21.0).abs() < 0.01);
        let c = Color32::from_rgb(11, 11, 13);
        assert!((contrast_ratio(c, c) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn paleta_oscura_canonica_intacta() {
        // CONTRATO Avant Garden (handoff-v1 §3): restaurada tras el kit.
        let t = Theme::dark();
        assert_eq!(t.bg, Color32::from_rgb(11, 11, 13));
        assert_eq!(t.surface, Color32::from_rgb(19, 19, 21));
        assert_eq!(t.container, Color32::from_rgb(32, 31, 33));
        assert_eq!(t.text, Color32::from_rgb(229, 226, 225));
        assert_eq!(t.text_dim, Color32::from_rgb(143, 139, 130));
        assert_eq!(t.accent, Color32::from_rgb(221, 201, 255));
        assert_eq!(t.acid, Color32::from_rgb(221, 255, 0));
        assert_eq!(t.cyan, Color32::from_rgb(31, 211, 250));
        assert_eq!(t.magenta, Color32::from_rgb(245, 0, 119));
        assert!(!t.light);
    }

    #[test]
    fn paleta_clara_tokens_de_spec() {
        let t = Theme::light();
        assert_eq!(t.bg, Color32::from_rgb(0xF7, 0xF6, 0xF3));
        assert_eq!(t.surface, Color32::from_rgb(0xFF, 0xFF, 0xFF));
        assert_eq!(t.surface2, Color32::from_rgb(0xED, 0xEB, 0xE6));
        assert_eq!(t.border, Color32::from_rgb(0xE2, 0xE0, 0xDC));
        assert_eq!(t.text, Color32::from_rgb(0x1A, 0x1A, 0x1E));
        assert!(t.light);
    }

    #[test]
    fn contraste_claro_titulo_7_secundario_45() {
        let t = Theme::light();
        for fondo in [t.bg, t.surface, t.surface2, t.container] {
            assert!(contrast_ratio(t.text, fondo) >= 7.0, "titulo {fondo:?}");
        }
        for fondo in [t.bg, t.surface, t.surface2, t.container] {
            assert!(contrast_ratio(t.text_dim, fondo) >= 4.5, "dim {fondo:?}");
            assert!(
                contrast_ratio(t.text_secondary, fondo) >= 4.5,
                "sec {fondo:?}"
            );
        }
    }

    #[test]
    fn acentos_claros_AA_sobre_fondos_claros() {
        let t = Theme::light();
        for c in [
            t.acid, t.cyan, t.magenta, t.accent, t.orange, t.danger, t.warning, t.success,
        ] {
            assert!(contrast_ratio(c, t.bg) >= 4.5, "acento {c:?} sobre bg");
            assert!(
                contrast_ratio(c, t.surface) >= 4.5,
                "acento {c:?} sobre surface"
            );
        }
    }

    #[test]
    fn default_sigue_el_modo_global() {
        let _g = LightGuard::default();
        set_light_mode(false);
        assert!(!Theme::default().light);
        assert_eq!(Theme::default().bg, Theme::dark().bg);
        set_light_mode(true);
        assert!(Theme::default().light);
        assert_eq!(Theme::default().bg, Theme::light().bg);
    }

    #[test]
    fn install_aplica_visuals_segun_modo() {
        let _g = LightGuard::default();
        let ctx = egui::Context::default();
        set_light_mode(true);
        install(&ctx);
        assert_eq!(ctx.style().visuals.panel_fill, Theme::light().bg);
        assert_eq!(ctx.style().visuals.window_fill, Theme::light().container);
        set_light_mode(false);
        install(&ctx);
        assert_eq!(ctx.style().visuals.panel_fill, Theme::dark().bg);
        assert_eq!(ctx.style().visuals.window_fill, Theme::dark().container);
    }

    #[test]
    fn colores_de_dato_legibles_en_claro_intactos_en_oscuro() {
        let _g = LightGuard::default();
        set_light_mode(true);
        let t = Theme::light();
        // labels de tonalidad sobre el chip container
        assert!(contrast_ratio(key_label_color("8A"), t.container) >= 4.5);
        assert!(contrast_ratio(key_label_color("8B"), t.container) >= 4.5);
        // tira espectral completa sobre surface
        for lvl in 1..=10 {
            assert!(
                contrast_ratio(energy_color(lvl as f64), t.surface) >= 4.5,
                "energy {lvl}"
            );
        }
        // en oscuro quedan EXACTAMENTE como hoy (sin regresion)
        set_light_mode(false);
        assert_eq!(key_label_color("8A"), Color32::from_rgb(31, 211, 250));
        assert_eq!(key_label_color("8B"), Color32::from_rgb(221, 201, 255));
        assert_eq!(energy_color(5.0), Color32::from_rgb(221, 255, 0));
        assert_eq!(energy_color(9.0), Color32::from_rgb(245, 0, 119));
    }
    #[test]
    fn wave_bg_siempre_oscuro_canonico() {
        // decision F-25: el waveform conserva lienzo propio #0B0B0D
        // (bandas hardcodeadas claras en widgets.rs, intocable).
        assert_eq!(WAVE_BG, Color32::from_rgb(11, 11, 13));
    }
}
