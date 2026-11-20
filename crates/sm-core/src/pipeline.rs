//! Orquestador del análisis completo — espejo de `ref_full.py::analyze_full`.

use crate::{beats, cues, decode, energy, key, cue_engine, models, volume};
use anyhow::Result;
use sm_model::{BeatGrid, CuePoint, EnergySegment, TrackAnalysis};
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct AnalyzeOptions {
    pub waveform_buckets: usize,
    pub max_cues: usize,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self {
            waveform_buckets: 2048,
            max_cues: 8,
        }
    }
}

/// Analiza un archivo de audio completo.
/// `models` puede ser `Models::none()` → fallback Krumhansl + energía sin
/// clasificar (segmentos con nivel None).
pub fn analyze_track(
    path: &Path,
    opts: &AnalyzeOptions,
    models: &models::Models,
) -> Result<TrackAnalysis> {
    let t0 = Instant::now();
    let decoded = decode::decode_file(path)?;
    let mono = decoded.mono();

    // --- beats (base de la segmentación de energía) ---
    let (br, onset_env) = beats::analyze_pcm_with_env(&mono);
    let beatgrid = if br.valid() {
        Some(BeatGrid {
            period_s: br.period_s,
            t0_s: br.t0_s,
            n_beats: br.n_beats,
            tempo: br.tempo,
        })
    } else {
        None
    };
    // tiempos de beat derivados UNA vez (antes se recalculaban para
    // segmentos y para cues: bg.beats() es puro → mismo Vec)
    let beats_times: Option<Vec<f64>> = beatgrid.as_ref().map(|bg| bg.beats().collect());

    // --- energía (segmentación sobre NUESTRO grid) ---
    let mut segments_out: Vec<EnergySegment> = Vec::new();
    let mut energy_global: Option<f64> = None;
    let mut cue_out: Vec<CuePoint> = Vec::new();
    if let Ok(ea) = energy::EnergyAnalysis::new(&mono) {
        let segs_full = match (&beats_times, &beatgrid) {
            (Some(bt), Some(bg)) if bg.n_beats > 3 => ea.beats_anchored_segments(bt),
            _ => ea.own_segments(),
        };
        let mut seg_tuples: Vec<(f64, f64, i32)> = Vec::new();
        for (t0, t1, _tempo, feats168, _vol) in &segs_full {
            let lvl = models.energy_segment(feats168).map(|v| v.round() as i32);
            seg_tuples.push((*t0, *t1, lvl.unwrap_or(5)));
            segments_out.push(EnergySegment {
                start_s: *t0,
                end_s: *t1,
                level: lvl,
            });
        }
        let seg_tuples = energy::merge_same_level(seg_tuples);
        energy_global = models.energy_global(&ea.global_features());
        // cues: ruta herramienta de referencia exacta (envelope WaveformAnalyzerFast + scores
        // DetectLikelyCuePointsV3 + regla LR); fallback a la regla empírica
        // si no hay señal suficiente.
        if let (Some(bg), Some(bt)) = (&beatgrid, &beats_times) {
            // mono = L+R SIN 1/2 (AddAudio del binario; mono ⇒ 2x).
            // Con ≤2 canales mono·2 es la suma bit-exacta (÷2 y ×2 son
            // exactos en IEEE-754; test mono_por_dos_es_suma_bit_exacto),
            // así que se deriva del mono ya computado en vez de recorrer
            // el PCM interleaved de nuevo. >2 canales conserva la suma directa.
            let mono_sum: Vec<f32> = if decoded.channels <= 2 {
                mono.iter().map(|v| v * 2.0).collect()
            } else {
                decoded
                    .interleaved
                    .chunks(decoded.channels)
                    .map(|c| c.iter().sum::<f32>())
                    .collect()
            };
            let (ch1, ch2) = cue_engine::envelope_exact(&mono_sum);
            let scores = cue_engine::detect_pre_analysis(&bt, &ch1, &ch2);
            // ranker HGB entrenado (bit-exacto) si está en models/; cacheado
            // global (el JSON de 4.7 MB NO se re-parsea por pista)
            static RANKER: std::sync::OnceLock<Option<crate::cue_ranker::RankerModel>> =
                std::sync::OnceLock::new();
            let ranker = RANKER.get_or_init(|| {
                crate::cue_ranker::RankerModel::load(
                    &crate::pipeline::default_models_dir().join("cue_ranker.json"),
                )
            });
            let picked = match (&scores, &ranker) {
                (Some(sc), Some(rk)) => {
                    crate::cue_ranker::cue_points_ranked(rk, &seg_tuples, &bt, sc, Some(bg.tempo))
                }
                (Some(sc), None) => {
                    cue_engine::cue_points(&seg_tuples, &bt, Some(sc), opts.max_cues, 32, 32)
                }
                (None, _) => cues::cue_points(&seg_tuples, &bt, opts.max_cues, 32, 32),
            };
            let mut picked = cue_engine::clamp_first_cue_nonneg(&picked, &bt);
            // fix(cues-anchor): re-anclaje al beat previo. La fase de onsets
            // (ataques del audio dentro de la celda del beat, medida con ch1)
            // revela cuándo el "primer beat >= 0" saltó un beat entero y la
            // rejilla de fills quedó anclada un beat TARDE (48% de cues
            // cubiertas a +0.70 beats de herramienta de referencia — docs/CUE-PARITY-DIAGNOSIS §5).
            // Mueve SOLO índices de beat de los cues; el beatgrid exportado
            // (BPM/XML/serato) NO se toca.
            if scores.is_some() {
                // fase de onsets vs el grid propio (envolvente de beats.rs,
                // ya calculada en analyze_pcm_with_env — sin STFT extra)
                let phase = onset_env
                    .as_ref()
                    .map(|(env, hop_t)| beats::onset_phase_offset(&bt, env, *hop_t))
                    .unwrap_or(0.0);
                // 1) re-anclaje entero de la rejilla de fills (familia +0.75)
                picked = cue_engine::anchor_prev_cues(&picked, &bt, phase);
                // 2) snap sub-beat de TODOS los cues a la línea de beat
                //    fase-corregida (donde el audio tiene sus ataques)
                picked = cue_engine::phase_snap_cues(&picked, &bt, phase);
            }
            cue_out = picked
                .into_iter()
                .map(|(t, lvl)| CuePoint {
                    time_s: t,
                    energy_level: Some(lvl),
                    origin: "auto".into(),
                    label: None,
                })
                .collect();
        }
    }

    // --- tonalidad ---
    let (mut key_camelot, mut key_name, mut key_conf, mut second_key, mut second_conf) =
        (None, None, None, None, None);
    match key::detect_key_frames(&mono) {
        Some(kr) => {
            if let Some(ranked) = models.classify_key(&kr.feats112) {
                if !ranked.is_empty() {
                    key_camelot = Some(ranked[0].0.clone());
                    key_conf = Some(ranked[0].1);
                    if ranked.len() > 1 {
                        second_key = Some(ranked[1].0.clone());
                        second_conf = Some(ranked[1].1);
                    }
                    key_name = Some(sm_model::camelot::camelot_display_name(&ranked[0].0));
                }
            }
            if key_camelot.is_none() {
                // fallback Krumhansl (sin modelo)
                let mut a88 = [0.0f64; 88];
                a88.copy_from_slice(&kr.features88[..88]);
                let (c, n, s) = key::classify_fallback(&a88);
                key_camelot = Some(c);
                key_name = Some(sm_model::camelot::camelot_display_name(
                    key_camelot.as_deref().unwrap(),
                ));
                key_conf = Some(s.clamp(0.0, 1.0));
                let _ = n;
            }
        }
        None => tracing::warn!("tonalidad: audio insuficiente en {}", path.display()),
    }

    // --- volumen / LUFS / waveform ---
    let mut hist = [0u64; volume::BINS];
    let hist_v = volume::volume_histogram(&decoded.interleaved);
    hist.copy_from_slice(&hist_v);
    let vol_db = volume::volume_db(&hist);
    let lufs = volume::lufs_integrated(&decoded.interleaved, decoded.channels, decoded.sample_rate);
    let clipped = volume::clipped_peak_count(&decoded.interleaved);
    let recs = volume::waveform_records(&decoded.interleaved, decoded.channels, 1024);
    let wave = volume::display_waveform(&recs, opts.waveform_buckets);
    let bands = volume::band_envelope(
        &decoded.interleaved,
        decoded.channels,
        opts.waveform_buckets,
        crate::decode::ANALYSIS_SR,
    );

    Ok(TrackAnalysis {
        key_camelot,
        key_name,
        key_confidence: key_conf,
        second_key,
        second_confidence: second_conf,
        tempo: beatgrid.as_ref().map(|b| b.tempo),
        beatgrid,
        energy: energy_global,
        segments: segments_out,
        cues: cue_out,
        volume_db: Some(vol_db),
        lufs: Some(lufs),
        clipped_peak_count: Some(clipped as i64),
        waveform: wave,
        waveform_bands: Some(bands),
        duration_s: decoded.duration_s,
        analysis_ms: Some(t0.elapsed().as_millis() as u64),
    })
}

/// Resuelve el directorio de modelos ONNX: exe_dir/models, CWD/models.
pub fn default_models_dir() -> std::path::PathBuf {
    // Dev: el models/ del workspace (baked en compile time) manda sobre
    // cualquier copia vieja junto al exe (bug real: target/release/models/
    // tenía un key_linear v4 colapsado del 19-oct y la app lo cargaba en
    // vez del v5 del repo). En un build instalado la ruta del workspace no
    // existe → cae a exe_dir/models (comportamiento de distribución).
    let ws = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("models"))
        .filter(|p| p.is_dir());
    if let Some(d) = ws {
        return d;
    }
    if let Ok(exe) = std::env::current_exe() {
        let d = exe
            .parent()
            .map(|p| p.join("models"))
            .filter(|p| p.is_dir());
        if let Some(d) = d {
            return d;
        }
    }
    std::path::PathBuf::from("models")
}
