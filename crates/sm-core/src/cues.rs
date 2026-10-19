//! Cue points — port de la regla empírica de `harmonic-engine/engine/mik_cues.py`
//! (inferida de 5843 cues / 765 pistas de la DB de MIK; el recorte exacto a 8
//! del servidor es UNKNOWN — [APROX] documentado).
//!
//! Candidatos: inicio del primer segmento (forzado, invariante DB) + límites
//! de segmento + relleno interior cada 32 beats. Prioridad: límites > relleno,
//! |Δnivel| − antigüedad; separación dura ≥ 32 beats; snap a beat.

pub const MAX_CUES: usize = 8;

/// segs: (start, end, level). Devuelve (time, level) por cue.
pub fn cue_points(
    segs: &[(f64, f64, i32)],
    beats: &[f64],
    max_cues: usize,
    min_gap_beats: usize,
    fill_every: usize,
) -> Vec<(f64, i32)> {
    if segs.is_empty() || beats.is_empty() {
        return Vec::new();
    }
    let bt = if beats.len() > 64 {
        let mut ds: Vec<f64> = beats[..64].windows(2).map(|w| w[1] - w[0]).collect();
        ds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        ds[ds.len() / 2]
    } else {
        0.5
    };
    let dur = beats[beats.len() - 1];
    if dur <= 0.0 {
        return Vec::new();
    }
    // candidatos: (tiempo, is_boundary, |Δnivel|)
    let mut cands: Vec<(f64, bool, f64)> = vec![(segs[0].0, true, 0.0)];
    for i in 1..segs.len() {
        let dlev = (segs[i].2 - segs[i - 1].2).abs() as f64;
        cands.push((segs[i].0, true, dlev));
    }
    for &(ss, se, _) in segs {
        let mut k = fill_every;
        while ss + k as f64 * bt < se - 0.05 {
            cands.push((ss + k as f64 * bt, false, 0.0));
            k += fill_every;
        }
    }
    // prioridad (sin scores): límites 2+Δnivel, rellenos 1.0, penalización por antigüedad
    let mut scored: Vec<(f64, f64)> = cands[1..]
        .iter()
        .map(|&(ct, isb, dlev)| {
            let pr = if isb { 2.0 + dlev } else { 1.0 } - 0.001 * (ct / dur);
            (pr, ct)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut chosen: Vec<f64> = vec![segs[0].0]; // invariante DB: primer cue = 1er segmento
    for (_, ct) in scored {
        if chosen.len() >= max_cues {
            break;
        }
        if chosen.iter().all(|c| (ct - c).abs() >= min_gap_beats as f64 * bt - 1e-9) {
            chosen.push(ct);
        }
    }
    chosen.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    chosen
        .into_iter()
        .map(|t| {
            // snap al beat más cercano
            let bi = beats
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (**a - t).abs().partial_cmp(&((**b - t).abs())).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0);
            let lvl = segs
                .iter()
                .find(|&&(ss, se, _)| ss - 1e-6 <= t && t < se)
                .map(|&(_, _, l)| l)
                .unwrap_or(5);
            (beats[bi], lvl)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_rule() {
        // 4 segmentos de 16 s (32 beats a 120), niveles 3 5 7 6
        let period = 0.5;
        let beats: Vec<f64> = (0..160).map(|k| k as f64 * period).collect();
        let segs = vec![
            (0.0, 16.0, 3),
            (16.0, 32.0, 5),
            (32.0, 48.0, 7),
            (48.0, 80.0, 6),
        ];
        let cues = cue_points(&segs, &beats, 8, 32, 32);
        assert!(!cues.is_empty() && cues.len() <= 8);
        // primer cue = inicio del primer segmento
        assert_eq!(cues[0].0, 0.0);
        // gap ≥ 32 beats = 16 s
        for w in cues.windows(2) {
            assert!(w[1].0 - w[0].0 >= 16.0 - 1e-6, "gap {} s", w[1].0 - w[0].0);
        }
        // todos los cues caen en beats (múltiplos de 0.5)
        for (t, _) in &cues {
            assert!((t / 0.5).fract().abs() < 1e-6, "t={t} no está en beat");
        }
    }

    #[test]
    fn max_cues_respected() {
        let beats: Vec<f64> = (0..600).map(|k| k as f64 * 0.5).collect();
        let segs: Vec<(f64, f64, i32)> = (0..20)
            .map(|i| (i as f64 * 16.0, (i + 1) as f64 * 16.0, (i % 8) as i32 + 1))
            .collect();
        let cues = cue_points(&segs, &beats, 8, 32, 32);
        assert!(cues.len() <= 8, "cues = {}", cues.len());
        assert_eq!(cues.len(), 8);
    }
}
