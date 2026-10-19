//! Orquestador del análisis completo — espejo de `mik_full.py::analyze_full`.

use crate::{beats, cues, decode, energy, key, models, volume};
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
        Self { waveform_buckets: 2048, max_cues: 8 }
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
    let br = beats::analyze_pcm(&mono);
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

    // --- energía (segmentación sobre NUESTRO grid) ---
    let mut segments_out: Vec<EnergySegment> = Vec::new();
    let mut energy_global: Option<f64> = None;
    let mut cue_out: Vec<CuePoint> = Vec::new();
    if let Ok(ea) = energy::EnergyAnalysis::new(&mono) {
        let segs_full = match &beatgrid {
            Some(bg) if bg.n_beats > 3 => {
                let bt: Vec<f64> = bg.beats().collect();
                ea.beats_anchored_segments(&bt)
            }
            _ => ea.own_segments(),
        };
        let mut seg_tuples: Vec<(f64, f64, i32)> = Vec::new();
        for (t0, t1, _tempo, feats168, _vol) in &segs_full {
            let lvl = models.energy_segment(feats168).map(|v| v.round() as i32);
            seg_tuples.push((*t0, *t1, lvl.unwrap_or(5)));
            segments_out.push(EnergySegment { start_s: *t0, end_s: *t1, level: lvl });
        }
        let seg_tuples = energy::merge_same_level(seg_tuples);
        energy_global = models.energy_global(&ea.global_features());
        // cues sobre segmentos (con nivel clasificado si hay modelo)
        if let Some(bg) = &beatgrid {
            let bt: Vec<f64> = bg.beats().collect();
            cue_out = cues::cue_points(&seg_tuples, &bt, opts.max_cues, 32, 32)
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
                key_name = Some(sm_model::camelot::camelot_display_name(key_camelot.as_deref().unwrap()));
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
        duration_s: decoded.duration_s,
        analysis_ms: Some(t0.elapsed().as_millis() as u64),
    })
}

/// Resuelve el directorio de modelos ONNX: exe_dir/models, CWD/models.
pub fn default_models_dir() -> std::path::PathBuf {
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
