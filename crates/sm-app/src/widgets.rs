//! Widgets: waveform con zoom, cues, segmentos de energía y playhead.
//!
//! Las conversiones coordenada↔tiempo son funciones puras (testeables);
//! el estado de zoom (inicio de ventana + ancho visible) vive en el widget.

use crate::theme::{energy_color, Theme};
use sm_model::EnergySegment;

// ------------------------------------------------- conversiones puras ----

/// x (px) → tiempo (s). `t0`/`span` definen la ventana visible.
#[inline]
pub fn x_to_time(x: f32, width: f32, t0: f64, span: f64, duration: f64) -> f64 {
    if width <= 0.0 || span <= 0.0 {
        return 0.0;
    }
    let frac = ((x / width) as f64).clamp(0.0, 1.0);
    (t0 + frac * span).clamp(0.0, duration)
}

/// tiempo (s) → x (px).
#[inline]
pub fn time_to_x(t: f64, width: f32, t0: f64, span: f64) -> f32 {
    if span <= 0.0 {
        return 0.0;
    }
    (((t - t0) / span) as f32 * width).clamp(0.0, width)
}

/// Nueva ventana tras zoom por rueda: mantiene `anchor_t` fija.
#[inline]
pub fn zoom_span(
    old_span: f64,
    factor: f64,
    anchor_t: f64,
    old_t0: f64,
    duration: f64,
) -> (f64, f64) {
    let min_span = (duration / 2000.0).max(0.5);
    let max_span = duration.max(0.001);
    let new_span = (old_span * factor).clamp(min_span, max_span);
    // proporción del anchor dentro de la ventana antigua
    let frac = ((anchor_t - old_t0) / old_span).clamp(0.0, 1.0);
    let mut t0 = anchor_t - frac * new_span;
    if t0 < 0.0 {
        t0 = 0.0;
    }
    if t0 + new_span > duration {
        t0 = (duration - new_span).max(0.0);
    }
    (t0, new_span)
}

// ------------------------------------------------------------- widget ----

pub struct WaveView<'a> {
    /// Ventana de zoom (t0, span) — propiedad del caller (persistente).
    pub zoom: &'a mut (f64, f64),
    pub wave: &'a [f32],
    pub duration_s: f64,
    pub position_s: Option<f64>,
    pub cues: &'a [f64],
    pub segments: &'a [EnergySegment],
    pub height: f32,
    pub on_seek: &'a mut dyn FnMut(f64),
}

impl<'a> WaveView<'a> {
    // el zoom vive en self.zoom (caller), no en egui memory
}

impl<'a> egui::Widget for &mut WaveView<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let t = Theme::default();
        let size = egui::vec2(ui.available_width(), self.height);
        let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        let painter = ui.painter();
        painter.rect_filled(rect, 4.0, t.bg);
        let duration = self.duration_s.max(0.001);
        // normalizar zoom si la pista cambió o es inválido
        if self.zoom.1 <= 0.0
            || self.zoom.1 > duration
            || self.zoom.0 < 0.0
            || self.zoom.0 >= duration.max(0.001)
        {
            *self.zoom = (0.0, duration);
        }
        let (t0, span) = *self.zoom;

        let n = self.wave.len() / 3;
        let w = rect.width();
        let mid = rect.center().y;
        let amp = rect.height() * 0.5 * 0.92;
        if n > 0 {
            // rango de buckets visibles según ventana
            let b0 = ((t0 / duration) * n as f64) as usize;
            let b1 = (((t0 + span) / duration) * n as f64).ceil() as usize;
            let b1 = b1.clamp(b0 + 1, n);
            let buckets_per_px = (b1 - b0) as f32 / w;
            for px in 0..w as i32 {
                let x = rect.left() + px as f32 + 0.5;
                let i0 = b0 + (px as f32 * buckets_per_px) as usize;
                let i1 = ((px as f32 + 1.0) * buckets_per_px).ceil() as usize;
                let i1 = i1.clamp(i0 + 1, b1).min(n);
                let mut mn = 0.0f32;
                let mut mx = 0.0f32;
                let mut rms = 0.0f32;
                for b in i0..i1 {
                    mn = mn.min(self.wave[b * 3]);
                    mx = mx.max(self.wave[b * 3 + 1]);
                    rms += self.wave[b * 3 + 2];
                }
                rms /= (i1 - i0) as f32;
                painter.line_segment(
                    [egui::pos2(x, mid - mx * amp), egui::pos2(x, mid - mn * amp)],
                    egui::Stroke::new(1.0, t.accent_dim),
                );
                painter.line_segment(
                    [
                        egui::pos2(x, mid - rms * amp),
                        egui::pos2(x, mid + rms * amp),
                    ],
                    egui::Stroke::new(1.0, t.accent),
                );
            }
        }
        // ribbon de energía
        let ribbon = egui::Rect::from_min_size(rect.left_top(), egui::vec2(w, 3.0));
        painter.rect_filled(ribbon, 0.0, t.surface2);
        for seg in self.segments {
            let a = ((seg.start_s / duration).clamp(0.0, 1.0)) as f32;
            let b = ((seg.end_s / duration).clamp(0.0, 1.0)) as f32;
            let lvl = seg.level.unwrap_or(5) as f64;
            let r = egui::Rect::from_min_max(
                egui::pos2(ribbon.left() + a * w, ribbon.top()),
                egui::pos2(ribbon.left() + b * w, ribbon.bottom()),
            );
            painter.rect_filled(r, 0.0, energy_color(lvl));
        }
        // mini-mapa cuando hay zoom: pista completa + ventana resaltada,
        // para que nunca parezca que la waveform "termina" antes del audio
        if span < duration * 0.999 {
            let mm_h = 4.0;
            let mm = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - mm_h),
                egui::pos2(rect.right(), rect.bottom()),
            );
            painter.rect_filled(mm, 0.0, t.surface2);
            let wa = (t0 / duration) as f32 * w;
            let wb = ((t0 + span) / duration) as f32 * w;
            let win = egui::Rect::from_min_max(
                egui::pos2(rect.left() + wa, mm.top()),
                egui::pos2(rect.left() + wb, mm.bottom()),
            );
            painter.rect_filled(win, 0.0, t.accent);
        }
        // cues
        for (i, &c) in self.cues.iter().enumerate() {
            if c < t0 || c > t0 + span {
                continue;
            }
            let x = rect.left() + time_to_x(c, w, t0, span);
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(
                    1.4,
                    egui::Color32::from_rgba_premultiplied(238, 236, 245, 210),
                ),
            );
            painter.text(
                egui::pos2(x + 2.0, rect.top() + 6.0),
                egui::Align2::LEFT_TOP,
                format!("{}", i + 1),
                egui::FontId::monospace(9.0),
                egui::Color32::from_rgba_premultiplied(238, 236, 245, 230),
            );
        }
        // playhead
        if let Some(p) = self.position_s {
            let x = rect.left() + time_to_x(p, w, t0, span);
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.6, egui::Color32::WHITE),
            );
        }
        // interacción
        if resp.clicked() || resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let frac = ((pos.x - rect.left()) / w).clamp(0.0, 1.0) as f64;
                (self.on_seek)((t0 + frac * span).clamp(0.0, duration));
            }
        }
        // zoom por rueda (ancla en el puntero o playhead)
        let anchor = resp
            .hover_pos()
            .map(|p| x_to_time(p.x - rect.left(), w, t0, span, duration))
            .or(self.position_s)
            .unwrap_or(t0 + span / 2.0);
        resp = resp.on_hover_cursor(egui::CursorIcon::Crosshair);
        let scroll = resp.ctx.input(|i| i.raw_scroll_delta.y);
        if resp.hovered() && scroll.abs() > 0.0 {
            let factor = if scroll > 0.0 { 1.0 / 1.3 } else { 1.3 };
            let win = zoom_span(span, factor, anchor, t0, duration);
            *self.zoom = win;
        }
        // doble clic → reset zoom
        if resp.double_clicked() {
            *self.zoom = (0.0, duration);
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_roundtrip() {
        // ventana 30–90 s sobre 180 s de pista, ancho 600 px
        let (t0, span, w, dur) = (30.0, 60.0, 600.0, 180.0);
        for &t in &[30.0, 45.5, 89.99, 90.0] {
            let x = time_to_x(t, w, t0, span);
            let back = x_to_time(x, w, t0, span, dur);
            assert!((back - t).abs() < 0.01, "t={t} → x={x} → {back}");
        }
    }

    #[test]
    fn seek_click_extremes() {
        let (t0, span, w, dur) = (0.0, 120.0, 400.0, 120.0);
        assert!(x_to_time(0.0, w, t0, span, dur).abs() < 1e-9);
        assert!((x_to_time(w, w, t0, span, dur) - 120.0).abs() < 1e-9);
        // fuera de rango se clampea
        assert!(x_to_time(-50.0, w, t0, span, dur).abs() < 1e-9);
        assert!((x_to_time(w * 9.0, w, t0, span, dur) - 120.0).abs() < 1e-9);
    }

    #[test]
    fn zoom_keeps_anchor_inside() {
        let dur = 100.0;
        // zoom in ×4 con ancla en 50 s
        let (t0, span) = zoom_span(100.0, 0.25, 50.0, 0.0, dur);
        assert!((span - 25.0).abs() < 1e-9);
        assert!(t0 <= 50.0 && 50.0 <= t0 + span, "ancla fuera: {t0}+{span}");
        // zoom out desde ventana pequeña no excede duración
        let (t0, span) = zoom_span(10.0, 20.0, 5.0, 0.0, dur);
        assert_eq!((span, t0), (100.0, 0.0));
        // mínimo de span
        let (t0, span) = zoom_span(100.0, 1e-6, 10.0, 0.0, dur);
        assert!(span >= dur / 2000.0);
        assert!(t0 >= 0.0 && t0 + span <= dur + 1e-9);
    }

    #[test]
    fn cobertura_total_duracion() {
        // El intervalo [0, T] debe mapear al ancho completo del widget:
        // t=0 → x=0 y t=T → x=width (borde derecho = final REAL del audio).
        for &dur in &[30.0, 182.0, 425.8, 563.45] {
            let w = 1200.0;
            assert!(time_to_x(0.0, w, 0.0, dur).abs() < 1e-6);
            let x_end = time_to_x(dur, w, 0.0, dur);
            assert!(
                (x_end - w).abs() < 0.51,
                "t=T no llega al borde: {x_end} vs {w}"
            );
            // el playhead nunca excede el ancho aunque pos > dur (clamp)
            assert_eq!(time_to_x(dur * 1.5, w, 0.0, dur), w);
        }
    }

    #[test]
    fn ventana_zoom_no_deja_zona_muerta() {
        // Cualquier ventana de zoom válida (t0, span) mantiene la relación
        // proporcional: el extremo derecho de la ventana es t0+span y el
        // mapeo inverso lo recupera exactamente.
        let (dur, w) = (420.0, 900.0);
        for &(t0, span) in &[(0.0, 420.0), (100.0, 150.0), (300.0, 120.0), (415.0, 5.0)] {
            let t_right = x_to_time(w, w, t0, span, dur);
            assert!(
                (t_right - (t0 + span)).abs() < 0.01,
                "ventana ({t0},{span}): derecha={t_right}"
            );
            let t_left = x_to_time(0.0, w, t0, span, dur);
            assert!((t_left - t0).abs() < 0.01);
        }
    }

    #[test]
    fn buckets_cubren_duracion_completa() {
        // La waveform persistida son n buckets × 3 (mn,mx,rms); el último
        // bucket debe corresponder a t ≈ duración. Simula el mapeo de render.
        let dur = 240.0;
        let n = 2048usize;
        let wave: Vec<f32> = vec![0.5; n * 3]; // energía uniforme
        let wn = wave.len() / 3;
        // t del primer y último bucket según el mapeo del widget
        let t_first = 0.0 / wn as f64 * dur;
        let t_last = (wn - 1) as f64 / wn as f64 * dur;
        assert!(t_first.abs() < 1e-9);
        // el último bucket está a un bucket del final (resolución de dur/n)
        let res = dur / wn as f64;
        assert!(
            (dur - t_last) <= res * 1.001,
            "último bucket en {t_last}s, resolución {res}s"
        );
        // ventana completa: b0=0 y b1=n (todos los buckets visibles)
        let b0 = (0.0 / dur * wn as f64) as usize;
        let b1 = ((0.0 + dur) / dur * wn as f64).ceil() as usize;
        assert_eq!((b0, b1), (0, wn));
    }
}
