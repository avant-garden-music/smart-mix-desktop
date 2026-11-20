//! Rueda Camelot (F-21, spec CAMELOT-WHEEL-SPEC-v1 de design).
//!
//! 24 celdas = 12 sectores de 30° × anillo interior A / exterior B.
//! 12 arriba, sentido horario. Clic en celda filtra la tabla por
//! compatibilidad armónica (mismo motor que el chip KEY: SmartFilter
//! `key_compatible_with`); clic en el centro limpia el filtro.
//! Teclado: ←/→ recorren sectores, Tab alterna anillo, Enter aplica, Esc cierra.

use crate::theme::{key_label_color, Theme};
use egui::{pos2, Pos2, Sense, Vec2};

/// Estado de la rueda (cursor de teclado + última interacción).
#[derive(Default)]
pub struct WheelState {
    pub cursor: (u8, bool), // (número 1-12, es_anillo_B)
}

/// Geometría de la spec (rueda 800px, escalable).
const R_CENTER: f32 = 200.0;
const R_A_IN: f32 = 215.0;
const R_A_OUT: f32 = 292.0;
const R_B_IN: f32 = 300.0;
const R_B_OUT: f32 = 378.0;

fn polar(c: Pos2, r: f32, ang_deg: f32) -> Pos2 {
    let a = ang_deg.to_radians();
    pos2(c.x + r * a.sin(), c.y - r * a.cos())
}

/// Wedge anular como polígono cerrado (arcos aproximados a segmentos).
fn wedge_shape(
    c: Pos2,
    r_in: f32,
    r_out: f32,
    a0: f32,
    a1: f32,
    fill: egui::Color32,
) -> egui::Shape {
    let steps = 10;
    let mut pts = Vec::with_capacity(2 * steps + 2);
    for i in 0..=steps {
        let a = a0 + (a1 - a0) * i as f32 / steps as f32;
        pts.push(polar(c, r_out, a));
    }
    for i in (0..=steps).rev() {
        let a = a0 + (a1 - a0) * i as f32 / steps as f32;
        pts.push(polar(c, r_in, a));
    }
    egui::epaint::PathShape {
        points: pts,
        closed: true,
        fill,
        stroke: egui::Stroke::NONE,
    }
    .into()
}

/// Compatibilidad Camelot estricta: mismo número ±1 con misma letra +
/// mismo número con la otra letra (incluye la propia celda).
pub fn compatible_set(num: u8, is_b: bool) -> Vec<(u8, bool)> {
    let prev = if num == 1 { 12 } else { num - 1 };
    let next = if num == 12 { 1 } else { num + 1 };
    vec![(num, is_b), (prev, is_b), (next, is_b), (num, !is_b)]
}

/// Celda bajo el puntero (None si está en el gap o fuera).
fn hit(c: Pos2, p: Pos2, s: f32) -> Option<(u8, bool)> {
    let dx = p.x - c.x;
    let dy = p.y - c.y;
    let r = dx.hypot(dy) / s;
    let is_b = if (R_B_IN..=R_B_OUT).contains(&r) {
        true
    } else if (R_A_IN..=R_A_OUT).contains(&r) {
        false
    } else {
        return None;
    };
    let mut ang = dx.atan2(-dy).to_degrees(); // rumbo de brujula: 0 = arriba, horario (casa con polar())
    if ang < 0.0 {
        ang += 360.0;
    }
    let n = (((ang + 15.0) / 30.0).floor() as i64 % 12) as u8;
    let num = if n == 0 { 12 } else { n };
    Some((num, is_b))
}

/// Dibuja la rueda. `filter` = compat_key activo; `track_key` = tonalidad
/// de la pista seleccionada (para el centro); `count` = pistas de la vista.
/// `on_filter(Option<String>)` aplica/quita el filtro de tabla.
pub fn wheel_ui(
    ui: &mut egui::Ui,
    filter: &Option<String>,
    track_key: Option<&str>,
    count: i64,
    state: &mut WheelState,
    on_filter: &mut dyn FnMut(Option<String>),
    compact: bool,
) {
    let t = Theme::default();
    let avail = ui.available_size();
    let size = avail.x.min(avail.y).min(760.0);
    let s = size / 800.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let c = rect.center();
    let painter = ui.painter();

    // ángulo central del sector n (12 arriba, horario)
    let sector = |n: u8| (n as f32 % 12.0) * 30.0;

    // ESTADO NEUTRAL: solo el FILTRO explicito resalta gajos y compatibles.
    // La tonalidad de la pista es informativa (centro, tenue) pero NO filtra
    // ni pinta compatibles mientras no haya selección (pedido del usuario).
    let sel = filter.as_deref().and_then(parse_key);
    let hovered = resp
        .hovered()
        .then(|| resp.hover_pos())
        .flatten()
        .and_then(|p| hit(c, p, s));

    // teclado: en compacto SOLO con hover (no robar flechas a la app)
    if !compact || resp.hovered() {
        let mut nav: Option<(u8, bool)> = None;
        let mut apply = false;
        ui.input(|i| {
            if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowUp) {
                let (n, b) = state.cursor;
                nav = Some((if n == 12 { 1 } else { n + 1 }, b));
            }
            if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowDown) {
                let (n, b) = state.cursor;
                nav = Some((if n == 1 { 12 } else { n - 1 }, b));
            }
            if i.key_pressed(egui::Key::Tab) {
                let (n, b) = state.cursor;
                nav = Some((n, !b));
            }
            if i.key_pressed(egui::Key::Enter) {
                apply = true;
            }
        });
        if let Some(cell) = nav {
            state.cursor = cell;
        }
        if apply {
            let (n, b) = state.cursor;
            on_filter(Some(format!("{n}{}", if b { 'B' } else { 'A' })));
        }
    }

    for n in 1u8..=12 {
        for is_b in [false, true] {
            let mid = sector(n);
            let (r_in, r_out, r_lab) = if is_b {
                (R_B_IN, R_B_OUT, (R_B_IN + R_B_OUT) / 2.0)
            } else {
                (R_A_IN, R_A_OUT, (R_A_IN + R_A_OUT) / 2.0)
            };
            let cell = (n, is_b);
            let is_sel = sel == Some(cell);
            let is_compat = sel
                .map(|s| compatible_set(s.0, s.1).contains(&cell) && !is_sel)
                .unwrap_or(false);
            let is_hover = hovered == Some(cell);
            let is_cursor = state.cursor == cell;

            let family = if is_b {
                t.accent // lavanda #DDC9FF
            } else {
                t.cyan // cian #1FD3FA
            };
            // fills por estado (spec §Colores)
            let fill = if is_sel {
                egui::Color32::from_rgba_unmultiplied(family.r(), family.g(), family.b(), 26)
            } else if is_compat {
                egui::Color32::from_rgba_unmultiplied(family.r(), family.g(), family.b(), 13)
            } else if is_hover || is_cursor {
                t.container_high
            } else {
                t.container
            };
            let stroke = if is_sel {
                egui::Stroke::new(2.5, family)
            } else if is_compat {
                egui::Stroke::new(1.5, family)
            } else {
                egui::Stroke::new(1.0, t.container_highest)
            };

            let shape = wedge_shape(c, r_in * s, r_out * s, mid - 15.0, mid + 15.0, fill);
            painter.add(shape);
            // borde redibujado como segments del polígono exterior+interior
            for &(rr, a0, a1) in &[
                (r_out, mid - 15.0, mid + 15.0),
                (r_in, mid + 15.0, mid - 15.0),
            ] {
                let steps = 10;
                for i in 0..steps {
                    let aa = a0 + (a1 - a0) * i as f32 / steps as f32;
                    let ab = a0 + (a1 - a0) * (i + 1) as f32 / steps as f32;
                    painter.line_segment([polar(c, rr * s, aa), polar(c, rr * s, ab)], stroke);
                }
            }

            // etiqueta
            let label = format!("{n}{}", if is_b { 'B' } else { 'A' });
            let txt_col = if is_sel || is_compat {
                family
            } else {
                t.text_secondary
            };
            let size = if is_sel { 26.0 } else { 20.0 };
            painter.text(
                polar(c, r_lab * s, mid),
                egui::Align2::CENTER_CENTER,
                label,
                crate::theme::font_body(size * s.max(0.55)),
                txt_col,
            );
        }
    }

    // centro (spec: tonalidad activa 84px + contador + compatibles)
    let center_r = R_CENTER * s;
    painter.circle_filled(c, center_r, t.container);
    painter.circle_stroke(c, center_r, egui::Stroke::new(1.0, t.container_highest));
    match sel {
        Some((n, b)) => {
            let key = format!("{n}{}", if b { 'B' } else { 'A' });
            painter.text(
                pos2(c.x, c.y - center_r * 0.35),
                egui::Align2::CENTER_CENTER,
                &key,
                crate::theme::font_body(54.0 * s.max(0.5)),
                key_label_color(&key),
            );
            painter.text(
                pos2(c.x, c.y + center_r * 0.18),
                egui::Align2::CENTER_CENTER,
                format!("{count} pistas"),
                crate::theme::font_body(16.0 * s.max(0.5)),
                t.text_secondary,
            );
            let comps: Vec<String> = compatible_set(n, b)
                .iter()
                .skip(1)
                .map(|(m, bb)| format!("{m}{}", if *bb { 'B' } else { 'A' }))
                .collect();
            if !compact {
                painter.text(
                    pos2(c.x, c.y + center_r * 0.55),
                    egui::Align2::CENTER_CENTER,
                    format!("compatibles: {}", comps.join(" · ")),
                    crate::theme::font_body(14.0 * s.max(0.5)),
                    t.text_dim,
                );
            }
        }
        None => {
            // estado NEUTRAL: sin filtro activo. La tonalidad de la pista
            // se muestra tenue (informativa); el hint explica la salida.
            if let Some(tk) = track_key.and_then(parse_key) {
                let key = format!("{}{}", tk.0, if tk.1 { "B" } else { "A" });
                painter.text(
                    pos2(c.x, c.y - center_r * 0.3),
                    egui::Align2::CENTER_CENTER,
                    &key,
                    crate::theme::font_body(40.0 * s.max(0.5)),
                    t.text_dim,
                );
                painter.text(
                    pos2(c.x, c.y + center_r * 0.3),
                    egui::Align2::CENTER_CENTER,
                    "sin filtro - clic en gajo",
                    crate::theme::font_body(13.0 * s.max(0.5)),
                    t.text_dim,
                );
            } else {
                painter.text(
                    c,
                    egui::Align2::CENTER_CENTER,
                    "SIN TONALIDAD",
                    crate::theme::font_body(20.0 * s.max(0.5)),
                    t.text_dim,
                );
            }
        }
    }

    // interacción de puntero
    if resp.clicked() {
        match hovered.or(Some(state.cursor)) {
            Some(cell) if hovered.is_some() => {
                state.cursor = cell;
                let (n, b) = cell;
                let key = format!("{n}{}", if b { 'B' } else { 'A' });
                // toggle: clic en la activa limpia
                if filter.as_deref() == Some(key.as_str()) {
                    on_filter(None);
                } else {
                    on_filter(Some(key));
                }
            }
            _ => {
                // clic centro/ninguna celda → limpiar
                on_filter(None);
            }
        }
    }
}

fn parse_key(code: &str) -> Option<(u8, bool)> {
    let code = code.trim();
    let num: u8 = code.get(..code.len() - 1)?.parse().ok()?;
    let is_b = code.ends_with('B') || code.ends_with('b');
    if (1..=12).contains(&num) {
        Some((num, is_b))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_casa_con_el_dibujo_en_cada_gajo() {
        // el punto medio de cada gajo dibujado con polar() debe devolver
        // EXACTAMENTE esa celda (regresion del bug de rotacion reportado)
        let c = pos2(400.0, 400.0);
        let s = 1.0;
        for n in 1u8..=12 {
            let ang = (n % 12) as f32 * 30.0;
            for (r_mid, is_b) in [(253.5f32, false), (339.0f32, true)] {
                let p = polar(c, r_mid, ang);
                let got = hit(c, p, s).unwrap_or_else(|| panic!("gajo {n} B={is_b}: None"));
                assert_eq!(
                    got,
                    (n, is_b),
                    "polar({ang}) cayo en {got:?}, esperaba ({n}, {is_b})"
                );
            }
        }
    }

    #[test]
    fn hit_fuera_de_los_anillos_es_none() {
        let c = pos2(0.0, 0.0);
        assert!(hit(c, pos2(50.0, 50.0), 1.0).is_none()); // centro
        assert!(hit(c, pos2(500.0, 500.0), 1.0).is_none()); // fuera
        assert!(hit(c, pos2(410.0, 400.0), 1.0).is_none()); // gap entre anillos
    }

    use super::*;

    #[test]
    fn compatibilidad_camelot_estricata() {
        // 8A → 7A, 9A, 8B (+ sí misma)
        assert_eq!(
            compatible_set(8, false),
            vec![(8, false), (7, false), (9, false), (8, true)]
        );
        // wrap: 1A → 12A, 2A, 1B
        assert_eq!(
            compatible_set(1, false),
            vec![(1, false), (12, false), (2, false), (1, true)]
        );
        // wrap: 12B → 11B, 1B, 12A
        assert_eq!(
            compatible_set(12, true),
            vec![(12, true), (11, true), (1, true), (12, false)]
        );
    }

    #[test]
    fn parse_claves() {
        assert_eq!(parse_key("8A"), Some((8, false)));
        assert_eq!(parse_key("12B"), Some((12, true)));
        assert_eq!(parse_key("X"), None);
        assert_eq!(parse_key("13A"), None);
    }
}
