//! F-26: lógica pura del rating 1-5 estrellas.
//!
//! El painter JAMÁS decide geometría (regla del proyecto para render):
//! la UI dibuja círculos con painter SOLO a partir de las salidas de estas
//! funciones — sin glifos unicode (evitan tofu en las fuentes embebidas).

/// Estrellas del rating (fijas por diseño herramienta de referencia/rekordbox).
pub const STAR_COUNT: usize = 5;

/// Estados de las 5 estrellas para un rating dado (`true` = llena).
/// `rating` se recorta a 0..=5.
pub fn star_states(rating: u8) -> [bool; STAR_COUNT] {
    let r = rating.min(STAR_COUNT as u8);
    let mut s = [false; STAR_COUNT];
    for (i, v) in s.iter_mut().enumerate() {
        *v = (i as u8) < r;
    }
    s
}

/// Rects (d×d) de las 5 estrellas, de izquierda a derecha, centrados como
/// grupo dentro de `area` vertical y horizontalmente.
pub fn star_rects(area: egui::Rect, diameter: f32, gap: f32) -> Vec<egui::Rect> {
    let n = STAR_COUNT as f32;
    let total_w = n * diameter + (n - 1.0) * gap;
    let x0 = area.center().x - total_w / 2.0;
    let y0 = area.center().y - diameter / 2.0;
    (0..STAR_COUNT)
        .map(|i| {
            let x = x0 + i as f32 * (diameter + gap);
            egui::Rect::from_min_size(egui::pos2(x, y0), egui::vec2(diameter, diameter))
        })
        .collect()
}

/// Índice 1-5 de la estrella que contiene `pos` (None fuera del grupo).
pub fn hit_star(rects: &[egui::Rect], pos: egui::Pos2) -> Option<u8> {
    rects
        .iter()
        .position(|r| r.contains(pos))
        .map(|i| i as u8 + 1)
}

/// Nuevo rating tras un clic en la estrella `clicked` (1-5):
/// clic en la estrella tope del rating actual QUITA el rating (0);
/// cualquier otra lo fija a `clicked`.
pub fn next_rating(current: u8, clicked: u8) -> u8 {
    let (cur, clk) = (current.min(5), clicked.clamp(1, 5));
    if cur == clk {
        0
    } else {
        clk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estados_de_estrellas_por_rating() {
        assert_eq!(star_states(0), [false; STAR_COUNT]);
        assert_eq!(star_states(3), [true, true, true, false, false]);
        assert_eq!(star_states(5), [true; STAR_COUNT]);
        // recorte defensivo
        assert_eq!(star_states(9), [true; STAR_COUNT]);
    }

    #[test]
    fn rects_cinco_iguales_ordenados_y_centrados() {
        let area = egui::Rect::from_min_size(egui::pos2(100.0, 10.0), egui::vec2(200.0, 36.0));
        let (d, g) = (10.0f32, 3.0f32);
        let rects = star_rects(area, d, g);
        assert_eq!(rects.len(), STAR_COUNT);
        // orden izquierda → derecha
        assert!(rects.windows(2).all(|w| w[0].center().x < w[1].center().x));
        // tamaño exacto d×d
        assert!(rects.iter().all(|r| r.size() == egui::vec2(d, d)));
        // paso entre centros = d + gap
        let step = rects[1].center().x - rects[0].center().x;
        assert!((step - (d + g)).abs() < 1e-4);
        // grupo centrado en el área
        let total_w = STAR_COUNT as f32 * d + (STAR_COUNT as f32 - 1.0) * g;
        assert!((rects[0].left() - (area.center().x - total_w / 2.0)).abs() < 1e-4);
        for r in &rects {
            assert!((r.center().y - area.center().y).abs() < 1e-4);
            assert!(area.contains(r.center()), "cada estrella dentro del área");
        }
    }

    #[test]
    fn hit_encuentra_estrella_correcta() {
        let rects = star_rects(
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(80.0, 20.0)),
            10.0,
            3.0,
        );
        // centro de cada estrella → índice 1-5
        for (i, r) in rects.iter().enumerate() {
            assert_eq!(hit_star(&rects, r.center()), Some(i as u8 + 1));
        }
        // fuera del grupo (arriba, abajo, lejos)
        assert_eq!(hit_star(&rects, egui::pos2(-50.0, 10.0)), None);
        assert_eq!(hit_star(&rects, egui::pos2(40.0, -30.0)), None);
        // entre dos estrellas (zona de gap) → la estrella más próxima NO: gap no contiene
        let between = egui::pos2(
            (rects[0].right() + rects[1].left()) / 2.0,
            rects[0].center().y,
        );
        assert_eq!(hit_star(&rects, between), None);
    }

    #[test]
    fn clic_tope_actual_quita_rating() {
        assert_eq!(next_rating(0, 3), 3);
        assert_eq!(next_rating(3, 3), 0, "clic en la actual (tope) → quita");
        assert_eq!(next_rating(4, 2), 2, "bajar de 4 a 2");
        assert_eq!(next_rating(2, 5), 5, "subir");
        assert_eq!(next_rating(5, 5), 0);
        assert_eq!(next_rating(1, 1), 0);
        assert_eq!(
            next_rating(3, 1),
            1,
            "clic en una estrella por debajo del tope → fija"
        );
    }
}
