//! Caracterización del bloque de cues (antes de las correcciones de
//! anclaje/fase): 3 señales sintéticas FIJAS atraviesan el mismo camino que
//! pipeline.rs (beats → energía/segmentos → envelope_exact → scores →
//! ranker → clamp) y se pinean los tiempos exactos de los cues.
//!
//! Estos valores SON el comportamiento medido del motor en HEAD; si un
//! cambio de geometría los mueve, el test se actualiza DE FORMA DELIBERADA
//! documentando qué corrección los movió (ver commits fix(cues-*)).

fn synth_four_on_floor(bpm: f64, secs: f64, offbeat_strong: bool) -> Vec<f32> {
    let sr = 44_100.0f64;
    let n = (secs * sr) as usize;
    let mut x = vec![0.0f32; n];
    let period = 60.0 / bpm;
    // niveles de energia por seccion de 32 beats (0.4 / 0.75 / 1.0 / 0.55 / 0.9 / 0.7)
    let levels = [0.40f64, 0.75, 1.00, 0.55, 0.90, 0.70];
    let mut t = 0.0f64;
    let mut k = 0usize;
    while t < secs {
        let lvl = levels[(k / 32) % levels.len()];
        // kick: 90 ms de seno 70 Hz con caida exponencial
        let start = (t * sr) as usize;
        let len = (0.09 * sr) as usize;
        for i in 0..len.min(n - start) {
            let tt = i as f64 / sr;
            let envl = (-tt * 28.0).exp();
            let v = (2.0 * std::f64::consts::PI * 70.0 * tt).sin() * envl * 0.9 * lvl;
            x[start + i] += v as f32;
        }
        // hat a mitad de beat (fuerte en la variante offbeat)
        let hs = (0.02 * sr) as usize;
        let hstart = ((t + 0.5 * period) * sr) as usize;
        let amp = if offbeat_strong { 0.5 } else { 0.06 };
        for i in 0..hs.min(n.saturating_sub(hstart)) {
            let tt = i as f64 / sr;
            let envl = (-tt * 220.0).exp();
            let v = (2.0 * std::f64::consts::PI * 8000.0 * tt).sin() * envl * amp * lvl;
            x[hstart + i] += v as f32;
        }
        t += period;
        k += 1;
    }
    x
}

fn synth_intro_silence(bpm: f64, secs: f64) -> Vec<f32> {
    let silence = 8.0f64;
    let mut x = vec![0.0f32; (silence * 44_100.0) as usize];
    x.extend(synth_four_on_floor(bpm, secs - silence, false));
    x
}

/// Espejo EXACTO del bloque de cues de pipeline.rs (analyze_track sin decode
/// ni tonalidad): devuelve los cues (tiempo, nivel) ya clampleados.
fn run_cues(pcm: &[f32]) -> Vec<(f64, i32)> {
    use sm_core::{beats, cue_ranker, energy, cue_engine, models, pipeline};
    let mono: Vec<f32> = pcm.iter().map(|v| v / 2.0).collect(); // decode.mono() = suma/2
    let br = beats::analyze_pcm(&mono);
    assert!(br.valid(), "beatgrid invalido para la señal sintetica");
    let bt: Vec<f64> = (0..br.n_beats)
        .map(|k| br.t0_s + k as f64 * br.period_s)
        .collect();
    let ea = energy::EnergyAnalysis::new(&mono).expect("energia");
    let segs_full = if br.n_beats > 3 {
        ea.beats_anchored_segments(&bt)
    } else {
        ea.own_segments()
    };
    let models = models::Models::load(&pipeline::default_models_dir());
    let mut seg_tuples: Vec<(f64, f64, i32)> = Vec::new();
    for (t0, t1, _tempo, feats168, _vol) in &segs_full {
        let lvl = models.energy_segment(feats168).map(|v| v.round() as i32);
        seg_tuples.push((*t0, *t1, lvl.unwrap_or(5)));
    }
    let seg_tuples = energy::merge_same_level(seg_tuples);
    let mono_sum: Vec<f32> = mono.iter().map(|v| v * 2.0).collect();
    let (ch1, ch2) = cue_engine::envelope_exact(&mono_sum);
    let scores = cue_engine::detect_pre_analysis(&bt, &ch1, &ch2);
    let sc = scores.as_ref().expect("scores");
    let rk = cue_ranker::RankerModel::load(&pipeline::default_models_dir().join("cue_ranker.json"))
        .expect("ranker");
    let picked = cue_ranker::cue_points_ranked(&rk, &seg_tuples, &bt, sc, Some(br.tempo));
    cue_engine::clamp_first_cue_nonneg(&picked, &bt)
}

fn cue_times(cues: &[(f64, i32)]) -> Vec<String> {
    cues.iter().map(|(t, _)| format!("{:.3}", t)).collect()
}

#[test]
fn characterization_four_on_floor_124() {
    let pcm = synth_four_on_floor(124.0, 105.0, false);
    let cues = run_cues(&pcm);
    assert!(!cues.is_empty() && cues.len() <= 8, "cues = {cues:?}");
    assert!(cues[0].0 >= 0.0, "cue[0] negativo: {cues:?}");
    // VALORES PINED EN HEAD 9607f16 (caracterizacion pre fix(cues-anchor)):
    assert_eq!(
        cue_times(&cues),
        ["0.462", "30.946", "46.914", "62.398", "77.881", "93.365"]
    );
}

#[test]
fn characterization_intro_silence_118() {
    let pcm = synth_intro_silence(118.0, 108.0);
    let cues = run_cues(&pcm);
    assert!(!cues.is_empty() && cues.len() <= 8, "cues = {cues:?}");
    assert!(cues[0].0 >= 0.0, "cue[0] negativo: {cues:?}");
    assert_eq!(
        cue_times(&cues),
        ["0.351", "16.622", "41.029", "57.300", "73.571", "89.843", "106.114"]
    );
}

#[test]
fn characterization_offbeat_hats_121() {
    let pcm = synth_four_on_floor(121.0, 105.0, true);
    let cues = run_cues(&pcm);
    assert!(!cues.is_empty() && cues.len() <= 8, "cues = {cues:?}");
    assert_eq!(
        cue_times(&cues),
        ["0.474", "15.846", "32.210", "48.077", "63.945", "79.813", "95.681"]
    );
}
