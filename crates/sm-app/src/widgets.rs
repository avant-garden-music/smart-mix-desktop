//! Widgets custom: waveform con cues, segmentos de energía y playhead.

use crate::theme::{energy_color, Theme};
use sm_model::EnergySegment;

pub struct WaveView<'a> {
    pub wave: &'a [f32],
    pub duration_s: f64,
    pub position_s: Option<f64>,
    pub cues: &'a [f64],
    pub segments: &'a [EnergySegment],
    pub height: f32,
    pub on_seek: &'a mut dyn FnMut(f64),
}

impl<'a> egui::Widget for WaveView<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let t = Theme::default();
        let size = egui::vec2(ui.available_width(), self.height);
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
        let painter = ui.painter();
        // fondo
        painter.rect_filled(rect, 4.0, t.bg);
        let n = self.wave.len() / 3;
        if n > 0 {
            let w = rect.width();
            let mid = rect.center().y;
            let amp = rect.height() * 0.5 * 0.92;
            let buckets_per_px = n as f32 / w;
            for px in 0..w as i32 {
                let x = rect.left() + px as f32 + 0.5;
                let b0 = (px as f32 * buckets_per_px) as usize;
                let b1 = ((px as f32 + 1.0) * buckets_per_px).ceil() as usize;
                let b1 = b1.clamp(b0 + 1, n);
                let mut mn = 0.0f32;
                let mut mx = 0.0f32;
                let mut rms = 0.0f32;
                for b in b0..b1 {
                    mn = mn.min(self.wave[b * 3]);
                    mx = mx.max(self.wave[b * 3 + 1]);
                    rms += self.wave[b * 3 + 2];
                }
                rms /= (b1 - b0) as f32;
                let y0 = mid - mx * amp;
                let y1 = mid - mn * amp;
                painter.line_segment(
                    [egui::pos2(x, y0), egui::pos2(x, y1)],
                    egui::Stroke::new(1.0, t.accent_dim),
                );
                let ry0 = mid - rms * amp;
                let ry1 = mid + rms * amp;
                painter.line_segment(
                    [egui::pos2(x, ry0), egui::pos2(x, ry1)],
                    egui::Stroke::new(1.0, t.accent),
                );
            }
        }
        // ribbon de segmentos de energía
        let ribbon = egui::Rect::from_min_size(
            rect.left_top(),
            egui::vec2(rect.width(), 3.0),
        );
        painter.rect_filled(ribbon, 0.0, t.surface2);
        for seg in self.segments {
            let a = ((seg.start_s / self.duration_s).clamp(0.0, 1.0)) as f32;
            let b = ((seg.end_s / self.duration_s).clamp(0.0, 1.0)) as f32;
            let lvl = seg.level.unwrap_or(5) as f64;
            let r = egui::Rect::from_min_max(
                egui::pos2(ribbon.left() + a * ribbon.width(), ribbon.top()),
                egui::pos2(ribbon.left() + b * ribbon.width(), ribbon.bottom()),
            );
            painter.rect_filled(r, 0.0, energy_color(lvl));
        }
        // cues
        for (i, &c) in self.cues.iter().enumerate() {
            let x = rect.left() + (c / self.duration_s).clamp(0.0, 1.0) as f32 * rect.width();
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.4, egui::Color32::from_rgba_premultiplied(236, 231, 221, 200)),
            );
            painter.text(
                egui::pos2(x + 2.0, rect.top() + 6.0),
                egui::Align2::LEFT_TOP,
                format!("{}", i + 1),
                egui::FontId::monospace(9.0),
                egui::Color32::from_rgba_premultiplied(236, 231, 221, 220),
            );
        }
        // playhead
        if let Some(p) = self.position_s {
            let x = rect.left() + (p / self.duration_s).clamp(0.0, 1.0) as f32 * rect.width();
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.6, egui::Color32::WHITE),
            );
        }
        // interacción: click → seek
        if resp.clicked() || resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let frac = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
                (self.on_seek)(frac * self.duration_s);
            }
        }
        resp
    }
}
