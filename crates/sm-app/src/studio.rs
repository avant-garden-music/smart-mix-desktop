//! Diseño "Smart Mix Studio" — ESTRUCTURA del kit GPT (2026-11-07) con la
//! PIEL Avant Garden (decisión del dueño, 2026-11-09 — docs/DESIGN.md).
//!
//! La forma (componentes estandarizados, appbar/drawer/rueda/tabla) viene
//! del kit; los COLORES y la escala tipográfica son la identidad AG
//! (theme.rs, paleta handoff-v1 §3) recalibrada a la escala herramienta de referencia (base
//! ~15px, filas 36px, botones ~33px). El waveform NO vive aquí: sigue sobre
//! su lienzo propio #0B0B0D en widgets.rs (intocable).
//!
//! Adaptación egui 0.33 → 0.27: CornerRadius→Rounding, text_edit_bg_color
//! sin Option, FontData sin Arc, UiBuilder→allocate_ui_at_rect,
//! rect_stroke sin StrokeKind.

use egui::{pos2, vec2, Color32, Pos2, Rect, Rounding, Sense, Shape, Stroke, Vec2};

// ---- tokens de color (piel Avant Garden §3; los light viven en Theme::light)
pub const BG: Color32 = Color32::from_rgb(11, 11, 13); // Brand/Background #0B0B0D
pub const CHROME: Color32 = Color32::from_rgb(14, 14, 16); // Surface/Lowest #0E0E10
pub const SIDE: Color32 = Color32::from_rgb(19, 19, 21); // Base #131315
pub const RAISED: Color32 = Color32::from_rgb(28, 27, 29); // Container Low #1C1B1D
pub const HOVER: Color32 = Color32::from_rgb(42, 42, 44); // Container High #2A2A2C
/// Selección: cian #1FD3FA al 10% sobre bg (§8), pre-mezclado porque
/// `from_rgba_unmultiplied` no es const.
pub const SELECTED: Color32 = Color32::from_rgb(13, 31, 37);
pub const TEXT: Color32 = Color32::from_rgb(229, 226, 225); // Content/Primary #E5E2E1
pub const SECONDARY: Color32 = Color32::from_rgb(203, 196, 210); // #CBC4D2
pub const SUBTLE: Color32 = Color32::from_rgb(143, 139, 130); // Muted #8F8B82
pub const LINE: Color32 = Color32::from_rgb(74, 69, 80); // Border/Variant #4A4550
#[allow(dead_code)]
pub const FAINT: Color32 = Color32::from_rgb(32, 31, 33); // Container #201F21
/// Acento AG: lavanda #DDC9FF (acción primaria; antes "lime" del kit).
pub const LIME: Color32 = Color32::from_rgb(221, 201, 255);
pub const LIME_TEXT: Color32 = Color32::from_rgb(11, 11, 13); // tinta sobre lavanda
pub const LILAC: Color32 = Color32::from_rgb(221, 201, 255); // lavanda AG
/// Chip KEY: SIEMPRE Container #201F21 (PARITY §8); el label distingue A/B.
pub const KEY_BG: Color32 = Color32::from_rgb(32, 31, 33);
pub const KEY_BG_MAJOR: Color32 = Color32::from_rgb(32, 31, 33);
/// Label B (mayor) → lavanda #DDC9FF · label A (menor) → cian #1FD3FA (§8).
pub const KEY_MAJOR: Color32 = Color32::from_rgb(221, 201, 255);
pub const KEY_MINOR: Color32 = Color32::from_rgb(31, 211, 250);
pub const WARNING: Color32 = Color32::from_rgb(251, 115, 47); // Orange #FB732F

// ---- geometría (tokens.json) -------------------------------------------
pub const APPBAR_H: f32 = 54.0; // botones 34 (fix table-rb) + margen vertical 10×2
pub const TRACKBAR_H: f32 = 104.0; // aire bajo la linea de artista (pedido por el dueño)
#[allow(dead_code)]
pub const WAVE_H: f32 = 101.0;
#[allow(dead_code)] // tokens del kit (valores usados inline en el port)
pub const TIMELINE_H: f32 = 16.0;
#[allow(dead_code)]
pub const PLAY_D: f32 = 39.0;
#[allow(dead_code)]
pub const CUE_H: f32 = 39.0;
pub const SIDEBAR_W: f32 = 190.0;
pub const ROW_H: f32 = 36.0;
pub const HEADER_H: f32 = 36.0;
#[allow(dead_code)]
pub const LIB_HEADER_H: f32 = 57.0;
pub const DRAWER_W: f32 = 304.0;
/// Reducción global ~-1.5pt (fix table-rb): 32 → 30 — "todo respira" sin
/// volver a lo chico.
pub const STATUS_H: f32 = 30.0;
pub const BTN_RADIUS: f32 = 6.0;
#[allow(dead_code)]
pub const SURFACE_RADIUS: f32 = 12.0;
/// Reducción global ~-1.5pt (fix table-rb, reversa parcial del bump +2pt
/// que dejó todo apretado): cuerpo 17→15.5, pequeño 15.5→14. Mismas
/// familias (Barlow SC/Sora) — el objetivo es que respire, no lo chico.
pub const FONT_BODY: f32 = 15.5;
pub const FONT_SMALL: f32 = 14.0;

// ---- tipografía ---------------------------------------------------------
#[allow(dead_code)]
pub fn title_font(sz: f32) -> egui::FontId {
    egui::FontId::proportional(sz)
}

/// Texto pequeño (14px) con color.
pub fn small(text: impl Into<String>, color: Color32) -> egui::RichText {
    egui::RichText::new(text).size(FONT_SMALL).color(color)
}

/// Cierre (elipsis): texto de UNA línea truncado con "…" para que quepa en
/// `max_width` con la fuente dada. Las celdas de la tabla pintan su texto
/// con `Button::wrap(false)` — ancho natural, corte en seco sin elipsis —
/// así que el recorte con elipsis se calcula AQUÍ contra el ancho real de
/// la celda. Medición con los glifos instalados (layout sin wrap).
pub fn ellipsize(
    fonts: &egui::text::Fonts,
    text: &str,
    font_id: egui::FontId,
    max_width: f32,
) -> String {
    let width = |s: &str| {
        fonts
            .layout_delayed_color(s.to_owned(), font_id.clone(), f32::INFINITY)
            .size()
            .x
    };
    if text.is_empty() || width(text) <= max_width {
        return text.to_owned();
    }
    const ELLIPSIS: &str = "…";
    let limit = (max_width - width(ELLIPSIS)).max(0.0);
    let chars: Vec<char> = text.chars().collect();
    // búsqueda binaria del prefijo más largo que cabe en `limit`
    let (mut lo, mut hi) = (0usize, chars.len()); // lo cabe, hi no
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if width(&chars[..mid].iter().collect::<String>()) <= limit {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let mut prefix: String = chars[..lo].iter().collect();
    while prefix.ends_with(char::is_whitespace) {
        prefix.pop();
    }
    prefix.push_str(ELLIPSIS);
    prefix
}

/// Etiqueta de sección (SUBTLE).
pub fn section_label(ui: &mut egui::Ui, label: &str) {
    ui.label(small(label, SUBTLE));
    ui.add_space(3.0);
}

/// Ejecuta contenido UI dentro de un rect exacto SIN avanzar el cursor del
/// padre (equivalente al `UiBuilder` no-allocante del kit en egui 0.33; en
/// 0.27 `allocate_ui_at_rect` AVANZA y desplaza los hermanos — bug medido).
pub fn at(ui: &mut egui::Ui, id: impl std::hash::Hash, rect: Rect, f: impl FnOnce(&mut egui::Ui)) {
    let clip = rect.intersect(ui.clip_rect());
    let mut child = ui.child_ui_with_id_source(rect, egui::Layout::top_down(egui::Align::Min), id);
    child.set_clip_rect(clip);
    f(&mut child);
}

// ---- tonos de botón -----------------------------------------------------
#[derive(Clone, Copy)]
pub enum Tone {
    Quiet,
    Raised,
    Selected,
    Primary,
}

pub fn button(
    ui: &mut egui::Ui,
    label: &str,
    icon: Option<Icon>,
    tone: Tone,
    width: f32,
) -> egui::Response {
    button_size(ui, label, icon, tone, vec2(width, 34.0))
}

pub fn button_size(
    ui: &mut egui::Ui,
    label: &str,
    icon: Option<Icon>,
    tone: Tone,
    size: Vec2,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let (fill, fg) = match tone {
        Tone::Quiet => (
            if response.hovered() {
                HOVER
            } else {
                Color32::TRANSPARENT
            },
            if response.hovered() { TEXT } else { SECONDARY },
        ),
        Tone::Raised => (if response.hovered() { HOVER } else { RAISED }, TEXT),
        Tone::Selected => (HOVER, TEXT),
        Tone::Primary => (LIME, LIME_TEXT),
    };
    ui.painter().rect_filled(rect, BTN_RADIUS, fill);
    let fg = if enabled { fg } else { fg.gamma_multiply(0.45) };
    let font = egui::FontId::proportional(if size.y <= 26.0 {
        FONT_SMALL
    } else {
        FONT_BODY
    });
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, fg);
    let content_w = galley.size().x + if icon.is_some() { 22.0 } else { 0.0 };
    let left = rect.center().x - content_w * 0.5;
    if let Some(icon) = icon {
        draw_icon(
            ui.painter(),
            pos2(left + 7.0, rect.center().y),
            14.0,
            icon,
            fg,
        );
    }
    let text_pos = pos2(
        left + if icon.is_some() { 22.0 } else { 0.0 },
        rect.center().y - galley.size().y * 0.5,
    );
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.galley(text_pos, galley, fg);
    response
}

pub fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str) -> egui::Response {
    icon_button_size(ui, icon, label, 36.0)
}

pub fn icon_button_size(ui: &mut egui::Ui, icon: Icon, label: &str, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 5.0, HOVER);
    }
    draw_icon(
        ui.painter(),
        rect.center(),
        if size < 25.0 { 13.0 } else { 16.0 },
        icon,
        if ui.is_enabled() {
            SECONDARY
        } else {
            SUBTLE.gamma_multiply(0.45)
        },
    );
    response.on_hover_text(label)
}

/// Item de navegación del sidebar (34px, barra lavanda si seleccionado).
pub fn nav_item(
    ui: &mut egui::Ui,
    label: &str,
    icon: Icon,
    count: Option<usize>,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    let fill = if selected {
        SELECTED
    } else if response.hovered() {
        HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 5.0, fill);
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
            0.0,
            LIME,
        );
    }
    draw_icon(
        ui.painter(),
        pos2(rect.left() + 14.0, rect.center().y),
        14.0,
        icon,
        if selected { LIME } else { SECONDARY },
    );
    let text_clip = Rect::from_min_max(
        pos2(rect.left() + 28.0, rect.top()),
        pos2(
            rect.right() - if count.is_some() { 27.0 } else { 5.0 },
            rect.bottom(),
        ),
    );
    ui.painter()
        .with_clip_rect(text_clip.intersect(ui.clip_rect()))
        .text(
            pos2(rect.left() + 28.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(FONT_BODY),
            if selected { TEXT } else { SECONDARY },
        );
    if let Some(count) = count {
        ui.painter().text(
            pos2(rect.right() - 7.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            count.to_string(),
            egui::FontId::proportional(FONT_SMALL),
            SUBTLE,
        );
    }
    response.on_hover_text(label)
}

/// Métrica (label sutil 11 + valor grande debajo).
pub fn metric(
    painter: &egui::Painter,
    at: Pos2,
    label: &str,
    value: &str,
    size: f32,
    color: Color32,
) {
    painter.text(
        at,
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::proportional(FONT_SMALL),
        SUBTLE,
    );
    painter.text(
        at + vec2(0.0, 22.0),
        egui::Align2::LEFT_TOP,
        value,
        egui::FontId::proportional(size),
        color,
    );
}

/// BPM con decimales pequeños (26/18px tras la reducción -1pt, tabulares).
pub fn draw_bpm(painter: &egui::Painter, at: Pos2, bpm: Option<f64>) {
    painter.text(
        at,
        egui::Align2::LEFT_TOP,
        "BPM",
        egui::FontId::proportional(FONT_SMALL),
        SUBTLE,
    );
    let text = bpm.map(|v| format!("{v:.2}")).unwrap_or_else(|| "—".into());
    let (whole, decimal) = text
        .split_once('.')
        .map(|(w, d)| (w, format!(".{d}")))
        .unwrap_or((&text, String::new()));
    let result = painter.text(
        at + vec2(0.0, 22.0),
        egui::Align2::LEFT_TOP,
        whole,
        egui::FontId::proportional(26.0),
        TEXT,
    );
    painter.text(
        pos2(result.right(), at.y + 31.0),
        egui::Align2::LEFT_TOP,
        decimal,
        egui::FontId::proportional(18.0),
        SECONDARY,
    );
}

/// Regla temporal proporcional a la duración REAL (nada de ticks equidistantes falsos).
pub fn draw_timeline(painter: &egui::Painter, rect: Rect, duration: Option<f64>) {
    let Some(duration) = duration.filter(|d| d.is_finite() && *d > 0.0) else {
        return;
    };
    let step = ((duration / 4.0 / 30.0).floor().max(1.0)) * 30.0;
    painter.text(
        pos2(rect.left(), rect.top() + 5.0),
        egui::Align2::LEFT_TOP,
        "0:00",
        egui::FontId::proportional(FONT_SMALL),
        SUBTLE,
    );
    for index in 1..=3 {
        let time = step * index as f64;
        if time >= duration - 1.0 {
            continue;
        }
        painter.text(
            pos2(
                rect.left() + rect.width() * (time / duration) as f32,
                rect.top() + 5.0,
            ),
            egui::Align2::CENTER_TOP,
            format_time(time as f32),
            egui::FontId::proportional(FONT_SMALL),
            SUBTLE,
        );
    }
    painter.text(
        pos2(rect.right(), rect.top() + 5.0),
        egui::Align2::RIGHT_TOP,
        format_time(duration as f32),
        egui::FontId::proportional(FONT_SMALL),
        SUBTLE,
    );
}

/// Badge de tonalidad AG (PARITY §8): chip Container #201F21, label
/// A (menor) cian #1FD3FA · B (mayor) lavanda #DDC9FF.
pub fn key_badge(ui: &mut egui::Ui, key: Option<&str>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(36.0, 25.0), Sense::click());
    let major = key.is_some_and(|k| k.ends_with('B'));
    ui.painter()
        .rect_filled(rect, 4.0, if major { KEY_BG_MAJOR } else { KEY_BG });
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        key.unwrap_or("—"),
        egui::FontId::proportional(FONT_SMALL),
        if major { KEY_MAJOR } else { KEY_MINOR },
    );
    resp
}

/// Paleta de 10 energías — leyenda oficial del dueño (dashboard 2026-11,
/// tira de 10 swatches redondeados LOW→HIGH): gradiente suave
/// azul→teal→verde→verde-amarillo→amarillo→naranja→rojo. Muestreada
/// píxel a píxel del centro de cada swatch de la leyenda de referencia.
pub const ENERGY_PALETTE: [Color32; 10] = [
    Color32::from_rgb(69, 153, 246), /* 1 azul */
    Color32::from_rgb(53, 181, 246), /* 2 azul claro */
    Color32::from_rgb(36, 209, 190), /* 3 teal */
    Color32::from_rgb(36, 197, 140), /* 4 verde-teal */
    Color32::from_rgb(111, 214, 93), /* 5 verde */
    Color32::from_rgb(196, 219, 66), /* 6 verde-amarillo */
    Color32::from_rgb(243, 209, 63), /* 7 amarillo */
    Color32::from_rgb(255, 159, 63), /* 8 naranja claro */
    Color32::from_rgb(243, 100, 55), /* 9 naranja rojizo */
    Color32::from_rgb(235, 66, 78),  /* 10 rojo */
];

/// Estado de los 10 swatches para un nivel de energía: los swatches
/// 1..=level quedan ENCENDIDOS (color pleno del gradiente) y el resto
/// apagados (dim/outline del MISMO color). `None` o 0 apagan todos; el
/// nivel se clampa a 10. Lógica pura — la comparten tabla y player.
pub fn energy_swatches(level: Option<u8>) -> [(Color32, bool); 10] {
    let lit = level.unwrap_or(0).min(10) as usize;
    let mut out = [(Color32::BLACK, false); 10];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = (ENERGY_PALETTE[i], i < lit);
    }
    out
}

/// Versión translúcida (alpha recta, NO premult) de un color opaco.
fn at_alpha(color: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        (alpha.clamp(0.0, 1.0) * 255.0) as u8,
    )
}

/// ESCALA DE ENERGÍA UNIFICADA (fix ui-rb2): 10 swatches redondeados con
/// el gradiente azul→teal→verde→amarillo→naranja→rojo de la leyenda del
/// dueño; encendidos hasta `level` con su color del gradiente, el resto
/// apagados (relleno tenue + contorno del mismo color). Painter puro —
/// la usan la celda ENERGÍA de la tabla y la métrica ENERGÍA del player:
/// MISMO componente, MISMA paleta (consistencia exacta).
pub fn energy_scale(painter: &egui::Painter, rect: Rect, level: Option<u8>) {
    const GAP: f32 = 2.0;
    let swatch = ((rect.width() - GAP * 9.0) / 10.0).max(1.0);
    let rounding = Rounding::same((swatch * 0.5).min(3.0));
    for (i, (color, lit)) in energy_swatches(level).iter().enumerate() {
        let x = rect.left() + i as f32 * (swatch + GAP);
        let swatch_rect = Rect::from_min_size(pos2(x, rect.top()), vec2(swatch, rect.height()));
        let (fill, stroke) = if *lit {
            (*color, Stroke::NONE)
        } else {
            // apagado: versión dim del MISMO color (outline tenue)
            (
                at_alpha(*color, 0.16),
                Stroke::new(1.0, at_alpha(*color, 0.42)),
            )
        };
        painter.add(Shape::Rect(egui::epaint::RectShape::new(
            swatch_rect,
            rounding,
            fill,
            stroke,
        )));
    }
}

/// Celda de energía de la tabla (fix ui-rb2): la escala unificada de 10
/// swatches (60px — el MISMO componente/paleta de la métrica del player).
pub fn energy_cell(ui: &mut egui::Ui, energy: Option<u8>) {
    let (rect, _) = ui.allocate_exact_size(vec2(60.0, 14.0), Sense::hover());
    energy_scale(ui.painter(), rect, energy);
}

// ---- tiempo -------------------------------------------------------------
pub fn format_time(seconds: f32) -> String {
    let seconds = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub fn format_cue_time(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).round() as u64;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

/// Botón de menú del popover (icono + label, fila completa de 36px).
pub fn menu_button(ui: &mut egui::Ui, label: &str, icon: Icon) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 6.0, HOVER);
    }
    let color = if ui.is_enabled() {
        SECONDARY
    } else {
        SUBTLE.gamma_multiply(0.45)
    };
    draw_icon(
        ui.painter(),
        pos2(rect.left() + 14.0, rect.center().y),
        16.0,
        icon,
        color,
    );
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.text(
        pos2(rect.left() + 32.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(FONT_BODY),
        color,
    );
    response.on_hover_text(label)
}

/// Colores del checkbox de fila de la tabla del studio (vector propio).
///
/// Bug del dueño: contra el fondo de fila SELECCIONADA (azul, cian 10% =
/// `SELECTED`) el relleno del checkbox checked se perdía — el checkbox de
/// egui pinta el mismo relleno translúcido en ambos estados y solo cambia
/// el trazo fino del check. Contrato (tests en este módulo):
/// - checked: relleno LIME (lavanda opaco) + glyph de tinta oscura
///   (LIME_TEXT) + stroke LIME — delta ≥150 de canal sobre fondo normal
///   (BG) Y sobre la fila seleccionada (SELECTED).
/// - unchecked: EXACTAMENTE el look de hoy (caja tenue translúcida +
///   borde LINE) — discreto sobre ambos fondos.
pub fn row_checkbox_colors(checked: bool) -> (Color32, Color32, Color32) {
    if checked {
        (LIME, LIME, LIME_TEXT)
    } else {
        // el checkbox de egui de hoy: blanco 15% + borde Variant
        (
            Color32::from_rgba_unmultiplied(255, 255, 255, 38),
            LINE,
            TEXT,
        )
    }
}

/// Checkbox de fila VECTORIAL de 18px: pintado con `row_checkbox_colors`
/// (relleno lavanda + tinta oscura cuando está checked) para que el estado
/// marcado se vea CLARO sobre la fila seleccionada y sobre el fondo
/// normal. Devuelve la respuesta; el caller decide qué hace con el clic
/// (mismo contrato de `ui.checkbox` pero sin mutar el bool aquí).
pub fn row_checkbox(ui: &mut egui::Ui, checked: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    let (fill, stroke, glyph) = row_checkbox_colors(checked);
    // hover (solo unchecked): la caja se aviva un paso, sigue discreta
    let fill = if !checked && response.hovered() {
        Color32::from_rgba_unmultiplied(255, 255, 255, 64)
    } else {
        fill
    };
    let box_rect = Rect::from_center_size(rect.center(), vec2(15.0, 15.0));
    ui.painter()
        .rect(box_rect, 4.0, fill, Stroke::new(1.5, stroke));
    if checked {
        // check mark vectorial (trazo grueso de tinta oscura)
        let (l, r, t, b) = (
            box_rect.left(),
            box_rect.right(),
            box_rect.top(),
            box_rect.bottom(),
        );
        let c = box_rect.center();
        ui.painter().add(Shape::line(
            vec![
                pos2(l + (r - l) * 0.20, c.y + 0.5),
                pos2(c.x - (r - l) * 0.06, b - (b - t) * 0.22),
                pos2(r - (r - l) * 0.18, t + (b - t) * 0.24),
            ],
            Stroke::new(2.2, glyph),
        ));
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, checked, ""));
    response
}

/// Centros x de las 5 estrellas del rating_button (paso 13px).
fn rating_star_centers(rect: Rect) -> [f32; 5] {
    let mut centers = [0.0; 5];
    for (star, c) in centers.iter_mut().enumerate() {
        *c = rect.left() + 5.5 + star as f32 * 13.0;
    }
    centers
}

/// Estrella 1..=5 bajo el puntero (None fuera de la tira de estrellas).
pub fn rating_star_hit(rect: Rect, pos: egui::Pos2) -> Option<u8> {
    if !rect.contains(pos) {
        return None;
    }
    let centers = rating_star_centers(rect);
    centers
        .iter()
        .position(|&cx| (pos.x - cx).abs() <= 6.5)
        .map(|i| i as u8 + 1)
}

/// Botón de valoración: 5 estrellas del kit (malla rellena / contorno) con
/// la semántica REAL de rating.rs (clic fija n; clic en la tope quita).
pub fn rating_button(ui: &mut egui::Ui, rating: u8, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(65.0, 26.0), Sense::click());
    let centers = rating_star_centers(rect);
    for (star, &cx) in centers.iter().enumerate() {
        let center = pos2(cx, rect.center().y);
        let on = star < rating as usize;
        let color = if on { LILAC } else { SUBTLE };
        let points: Vec<Pos2> = (0..10)
            .map(|index| {
                let angle =
                    index as f32 * std::f32::consts::TAU / 10.0 - std::f32::consts::TAU / 4.0;
                center + vec2(angle.cos(), angle.sin()) * if index % 2 == 0 { 5.0 } else { 2.2 }
            })
            .collect();
        if on {
            let mut mesh = egui::epaint::Mesh::default();
            mesh.colored_vertex(center, color);
            for point in &points {
                mesh.colored_vertex(*point, color);
            }
            for index in 0..10_u32 {
                mesh.add_triangle(0, index + 1, (index + 1) % 10 + 1);
            }
            ui.painter().add(Shape::mesh(mesh));
        } else {
            ui.painter()
                .add(Shape::closed_line(points, Stroke::new(0.9, color)));
        }
    }
    response.on_hover_text(label)
}

// ---- vocabulario de iconos geométricos (sin glifos de fuente) ----------
#[derive(Clone, Copy)]
pub enum Icon {
    Wave,
    Plus,
    Scan,
    More,
    Upload,
    ChevronDown,
    ChevronRight,
    Disc,
    Play,
    Pause,
    Previous,
    Next,
    Volume,
    Sliders,
    Library,
    Copy,
    List,
    Usb,
    Refresh,
    Tag,
    Close,
}

/// Iconos de líneas geométricas: cero dependencia de glifos (nada de tofu).
pub fn draw_icon(painter: &egui::Painter, center: Pos2, size: f32, icon: Icon, color: Color32) {
    let left = center - vec2(size, size) * 0.5;
    let p = |x: f32, y: f32| left + vec2(x, y) * (size / 24.0);
    let stroke = Stroke::new((size / 16.0).max(0.85) * 1.35, color);
    let line = |points: &[(f32, f32)]| {
        painter.add(Shape::line(
            points.iter().map(|&(x, y)| p(x, y)).collect(),
            stroke,
        ));
    };
    match icon {
        Icon::Wave => {
            for (x, top, bottom) in [
                (3.0, 9.0, 15.0),
                (7.0, 5.0, 19.0),
                (12.0, 2.0, 22.0),
                (17.0, 6.0, 18.0),
                (21.0, 9.0, 15.0),
            ] {
                line(&[(x, top), (x, bottom)]);
            }
        }
        Icon::Plus => {
            line(&[(12.0, 5.0), (12.0, 19.0)]);
            line(&[(5.0, 12.0), (19.0, 12.0)]);
        }
        Icon::Scan => {
            line(&[(8.0, 3.0), (3.0, 3.0), (3.0, 8.0)]);
            line(&[(16.0, 3.0), (21.0, 3.0), (21.0, 8.0)]);
            line(&[(3.0, 16.0), (3.0, 21.0), (8.0, 21.0)]);
            line(&[(21.0, 16.0), (21.0, 21.0), (16.0, 21.0)]);
            line(&[(4.0, 12.0), (20.0, 12.0)]);
        }
        Icon::More => {
            for x in [5.0, 12.0, 19.0] {
                painter.circle_filled(p(x, 12.0), size * 0.065, color);
            }
        }
        Icon::Upload => {
            line(&[(12.0, 15.0), (12.0, 3.0)]);
            line(&[(7.0, 8.0), (12.0, 3.0), (17.0, 8.0)]);
            line(&[(4.0, 15.0), (4.0, 21.0), (20.0, 21.0), (20.0, 15.0)]);
        }
        Icon::ChevronDown => line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
        Icon::ChevronRight => line(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)]),
        Icon::Disc => {
            painter.circle_stroke(center, size * 0.43, stroke);
            painter.circle_stroke(center, size * 0.11, stroke);
            line(&[(8.0, 5.5), (6.0, 7.0), (4.5, 10.0)]);
            line(&[(16.0, 18.5), (18.0, 17.0), (19.5, 14.0)]);
        }
        Icon::Play => {
            painter.add(Shape::convex_polygon(
                vec![p(7.0, 4.0), p(20.0, 12.0), p(7.0, 20.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for x in [6.0, 14.0] {
                painter.rect_filled(Rect::from_min_max(p(x, 4.0), p(x + 4.0, 20.0)), 1.0, color);
            }
        }
        Icon::Previous => {
            line(&[(5.0, 5.0), (5.0, 19.0)]);
            painter.add(Shape::convex_polygon(
                vec![p(18.0, 5.0), p(8.0, 12.0), p(18.0, 19.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Next => {
            line(&[(19.0, 5.0), (19.0, 19.0)]);
            painter.add(Shape::convex_polygon(
                vec![p(6.0, 5.0), p(16.0, 12.0), p(6.0, 19.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Volume => {
            line(&[
                (3.0, 9.0),
                (7.0, 9.0),
                (12.0, 5.0),
                (12.0, 19.0),
                (7.0, 15.0),
                (3.0, 15.0),
                (3.0, 9.0),
            ]);
            line(&[(16.0, 8.0), (18.0, 10.0), (18.0, 14.0), (16.0, 16.0)]);
            line(&[(19.0, 5.0), (22.0, 9.0), (22.0, 15.0), (19.0, 19.0)]);
        }
        Icon::Sliders => {
            line(&[(3.0, 6.0), (21.0, 6.0)]);
            line(&[(3.0, 12.0), (21.0, 12.0)]);
            line(&[(3.0, 18.0), (21.0, 18.0)]);
            for (x, y) in [(8.0, 6.0), (16.0, 12.0), (10.0, 18.0)] {
                painter.circle_filled(p(x, y), size * 0.085, RAISED);
                painter.circle_stroke(p(x, y), size * 0.085, stroke);
            }
        }
        Icon::Library => {
            line(&[(4.0, 3.0), (4.0, 21.0)]);
            line(&[(9.0, 3.0), (9.0, 21.0)]);
            line(&[(14.0, 4.0), (19.0, 21.0)]);
            line(&[(3.0, 21.0), (10.0, 21.0)]);
        }
        Icon::Copy => {
            painter.rect_stroke(Rect::from_min_max(p(8.0, 8.0), p(21.0, 21.0)), 2.0, stroke);
            line(&[
                (16.0, 5.0),
                (16.0, 3.0),
                (3.0, 3.0),
                (3.0, 16.0),
                (5.0, 16.0),
            ]);
        }
        Icon::List => {
            for y in [5.0, 11.0, 17.0] {
                painter.circle_filled(p(4.0, y), size * 0.045, color);
                line(&[(8.0, y), (20.0, y)]);
            }
        }
        Icon::Usb => {
            line(&[(12.0, 21.0), (12.0, 3.0)]);
            line(&[(8.0, 7.0), (12.0, 3.0), (16.0, 7.0)]);
            line(&[(12.0, 17.0), (5.0, 12.0), (5.0, 8.0)]);
            line(&[(12.0, 13.0), (19.0, 10.0), (19.0, 6.0)]);
            painter.circle_stroke(p(12.0, 21.0), size * 0.065, stroke);
        }
        Icon::Refresh => {
            line(&[
                (20.0, 10.0),
                (18.0, 5.0),
                (13.0, 3.0),
                (7.0, 5.0),
                (4.0, 10.0),
            ]);
            line(&[(20.0, 4.0), (20.0, 10.0), (14.0, 10.0)]);
            line(&[
                (4.0, 14.0),
                (6.0, 19.0),
                (11.0, 21.0),
                (17.0, 19.0),
                (20.0, 14.0),
            ]);
            line(&[(4.0, 20.0), (4.0, 14.0), (10.0, 14.0)]);
        }
        Icon::Tag => {
            line(&[
                (3.0, 3.0),
                (12.0, 3.0),
                (22.0, 13.0),
                (13.0, 22.0),
                (3.0, 12.0),
                (3.0, 3.0),
            ]);
            painter.circle_stroke(p(8.0, 8.0), size * 0.065, stroke);
        }
        Icon::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- checkbox de fila: visible sobre el fondo de fila seleccionada --
    //
    // Bug del dueño: con la fila seleccionada (fondo azul, cian 10%) el
    // relleno del checkbox checked se PERDÍA. El contrato cromático se
    // fija con la paleta Studio REAL y una distancia por canales mínima.

    /// Distancia cromática (suma de deltas R+G+B) — delta mínimo medible.
    fn channel_distance(a: Color32, b: Color32) -> i32 {
        (a.r() as i32 - b.r() as i32).abs()
            + (a.g() as i32 - b.g() as i32).abs()
            + (a.b() as i32 - b.b() as i32).abs()
    }

    /// Compone un relleno con alpha sobre un fondo de fila (como pinta egui).
    fn over(bg: Color32, fill: Color32) -> Color32 {
        let a = fill.a() as f32 / 255.0;
        let mix = |f: u8, b: u8| (f as f32 * a + b as f32 * (1.0 - a)) as u8;
        Color32::from_rgb(
            mix(fill.r(), bg.r()),
            mix(fill.g(), bg.g()),
            mix(fill.b(), bg.b()),
        )
    }

    /// El checkbox CHECKED tiene que verse CLARO sobre el fondo normal Y
    /// sobre el fondo azul de la fila seleccionada, distinguirse del estado
    /// unchecked y llevar un glyph de tinta oscura sobre el relleno.
    #[test]
    fn checked_row_checkbox_is_visible_over_selected_and_normal_bg() {
        let (fill_on, stroke_on, glyph_on) = row_checkbox_colors(true);
        // sobre la fila SELECCIONADA (cian 10% premezclado = SELECTED)
        let painted = over(SELECTED, fill_on);
        assert!(
            channel_distance(painted, SELECTED) >= 150,
            "checked {painted:?} se pierde contra la selección {SELECTED:?}"
        );
        // y sobre el fondo normal de la tabla (BG)
        let painted = over(BG, fill_on);
        assert!(
            channel_distance(painted, BG) >= 150,
            "checked {painted:?} se pierde contra el fondo {BG:?}"
        );
        // glyph de tinta oscura sobre el relleno lavanda
        assert!(
            channel_distance(glyph_on, fill_on) >= 150,
            "glyph invisible"
        );
        // el stroke refuerza el borde sobre la selección
        assert!(channel_distance(stroke_on, SELECTED) >= 150, "borde tenue");
        // y el estado checked DIFIERE del unchecked (el bug: hoy no cambia)
        let (fill_off, _, _) = row_checkbox_colors(false);
        for bg in [BG, SELECTED] {
            let delta = channel_distance(over(bg, fill_on), over(bg, fill_off));
            assert!(
                delta >= 120,
                "checked y unchecked se confunden sobre {bg:?} (delta {delta})"
            );
        }
    }

    /// El estado UNCHECKED queda discreto como hoy: tenue sobre ambos
    /// fondos (el cambio de look es SOLO para el checked).
    #[test]
    fn unchecked_row_checkbox_stays_discreet_on_the_row() {
        let (fill_off, stroke_off, _) = row_checkbox_colors(false);
        for bg in [BG, SELECTED] {
            let painted = over(bg, fill_off);
            assert!(
                channel_distance(painted, bg) <= 120,
                "unchecked {painted:?} deja de ser discreto sobre {bg:?}"
            );
        }
        // borde tenue pero presente (LINE sobre BG/SELECTED)
        assert!(channel_distance(stroke_off, BG) >= 60);
        assert!(channel_distance(stroke_off, SELECTED) >= 60);
    }

    /// fix ui-rb2: la paleta de energía son EXACTAMENTE los 10 colores de
    /// la leyenda del dueño (muestreados píxel a píxel del PNG adjunto),
    /// azul→teal→verde→verde-amarillo→amarillo→naranja→rojo (LOW→HIGH).
    #[test]
    fn energy_palette_es_la_leyenda_de_10_azul_a_rojo() {
        assert_eq!(ENERGY_PALETTE.len(), 10);
        assert_eq!(ENERGY_PALETTE[0], Color32::from_rgb(69, 153, 246), "1 azul");
        assert_eq!(ENERGY_PALETTE[2], Color32::from_rgb(36, 209, 190), "3 teal");
        assert_eq!(
            ENERGY_PALETTE[5],
            Color32::from_rgb(196, 219, 66),
            "6 verde-amarillo"
        );
        assert_eq!(
            ENERGY_PALETTE[6],
            Color32::from_rgb(243, 209, 63),
            "7 amarillo"
        );
        assert_eq!(
            ENERGY_PALETTE[8],
            Color32::from_rgb(243, 100, 55),
            "9 naranja"
        );
        assert_eq!(ENERGY_PALETTE[9], Color32::from_rgb(235, 66, 78), "10 rojo");
        // el gradiente arranca azul (canal azul dominante) y termina rojo
        assert!(ENERGY_PALETTE[0].b() > ENERGY_PALETTE[0].r());
        assert!(ENERGY_PALETTE[9].r() > ENERGY_PALETTE[9].b());
        // sin repetidos: 10 pasos distinguibles
        let mut sorted: Vec<_> = ENERGY_PALETTE.into_iter().collect();
        sorted.sort_by_key(|c| (c.r(), c.g(), c.b()));
        sorted.dedup_by_key(|c| (c.r(), c.g(), c.b()));
        assert_eq!(sorted.len(), 10);
    }

    /// fix ui-rb2: `energy_swatches` enciende 1..=level y clampa; None y 0
    /// apagan TODOS (la versión dim del mismo color).
    #[test]
    fn energy_swatches_encienden_hasta_el_nivel() {
        let lit = |level: Option<u8>| energy_swatches(level).iter().filter(|(_, on)| *on).count();
        assert_eq!(lit(None), 0, "None apaga todos");
        assert_eq!(lit(Some(0)), 0);
        assert_eq!(lit(Some(1)), 1);
        assert_eq!(lit(Some(5)), 5);
        assert_eq!(lit(Some(10)), 10);
        assert_eq!(lit(Some(200)), 10, "se clampa a 10");
        // apagado y encendido comparten el MISMO color del gradiente
        let sw = energy_swatches(Some(3));
        assert_eq!(sw[2], (ENERGY_PALETTE[2], true));
        assert_eq!(sw[3], (ENERGY_PALETTE[3], false));
        assert_eq!(sw[9], (ENERGY_PALETTE[9], false));
    }
}
