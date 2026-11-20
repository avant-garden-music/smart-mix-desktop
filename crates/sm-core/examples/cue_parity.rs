//! Medición de paridad de cues vs ground truth herramienta de referencia.
//! Lee una lista de rutas (un archivo por línea) desde argv[1], corre el
//! pipeline completo por pista e imprime TSV: path <TAB> cue_times_csv.
//! Uso: cargo run --release -p sm-core --example cue_parity -- lista.txt
//!
//! Modo debug (argv[2] == "debug"): columnas extra para diagnóstico —
//!   path, bpm, cues, engine, t0, period, n_beats, segs, cues_lr, cues_emp
//! El bloque reproducido aquí espeja pipeline.rs (decode→beats→energy→
//! envelope→scores→selección); cues debe coincidir con analyze_track.

use sm_core::pipeline::AnalyzeOptions;

fn main() {
    let list = std::env::args().nth(1).expect("lista de pistas");
    let debug = std::env::args().nth(2).as_deref() == Some("debug");
    let txt = std::fs::read_to_string(&list).unwrap();
    let models = sm_core::models::Models::load(&sm_core::pipeline::default_models_dir());
    let ranker = sm_core::cue_ranker::RankerModel::load(
        &sm_core::pipeline::default_models_dir().join("cue_ranker.json"),
    );
    for line in txt.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !debug {
            match sm_core::pipeline::analyze_track(
                std::path::Path::new(line),
                &AnalyzeOptions::default(),
                &models,
            ) {
                Ok(ta) => {
                    let cues: Vec<String> =
                        ta.cues.iter().map(|c| format!("{:.3}", c.time_s)).collect();
                    let bpm = ta.tempo.map(|t| format!("{:.2}", t)).unwrap_or_default();
                    println!("{}\t{}\t{}", line, bpm, cues.join(","));
                }
                Err(e) => eprintln!("ERR {}\t{}", line, e),
            }
            continue;
        }
        // ---- modo debug: espejo del bloque de cues de pipeline.rs ----
        let decoded = match sm_core::decode::decode_file(std::path::Path::new(line)) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("ERR {}\t{}", line, e);
                continue;
            }
        };
        let mono = decoded.mono();
        let (br, onset_env) = sm_core::beats::analyze_pcm_with_env(&mono);
        let bt: Vec<f64> = (0..br.n_beats)
            .map(|k| br.t0_s + k as f64 * br.period_s)
            .collect();
        let ea = match sm_core::energy::EnergyAnalysis::new(&mono) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("ERR {}\t{}", line, e);
                continue;
            }
        };
        let segs_full = if br.n_beats > 3 {
            ea.beats_anchored_segments(&bt)
        } else {
            ea.own_segments()
        };
        let mut seg_tuples: Vec<(f64, f64, i32)> = Vec::new();
        for (t0, t1, _tempo, feats168, _vol) in &segs_full {
            let lvl = models.energy_segment(feats168).map(|v| v.round() as i32);
            seg_tuples.push((*t0, *t1, lvl.unwrap_or(5)));
        }
        let merged = sm_core::energy::merge_same_level(seg_tuples);
        // mono = L+R SIN 1/2 (AddAudio del binario; mono ⇒ 2x)
        let mono_sum: Vec<f32> = if decoded.channels <= 1 {
            decoded.interleaved.iter().map(|v| v * 2.0).collect()
        } else {
            decoded
                .interleaved
                .chunks(decoded.channels)
                .map(|c| c.iter().sum::<f32>())
                .collect()
        };
        let (ch1, ch2) = sm_core::cue_engine::envelope_exact(&mono_sum);
        let scores = sm_core::cue_engine::detect_pre_analysis(&bt, &ch1, &ch2);
        let scores_csv = scores
            .as_ref()
            .map(|s| {
                s.iter()
                    .map(|v| format!("{:.5}", v))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let (mut cues, cues_lr, cues_emp, engine) = match &scores {
            Some(sc) => {
                let lr = sm_core::cue_engine::cue_points(&merged, &bt, Some(sc), 8, 32, 32);
                match &ranker {
                    Some(rk) => (
                        sm_core::cue_ranker::cue_points_ranked(
                            rk,
                            &merged,
                            &bt,
                            sc,
                            Some(br.tempo),
                        ),
                        lr,
                        Vec::new(),
                        "ranker",
                    ),
                    None => {
                        let emp = sm_core::cues::cue_points(&merged, &bt, 8, 32, 32);
                        (lr.clone(), lr, emp, "lr")
                    }
                }
            }
            None => {
                let emp = sm_core::cues::cue_points(&merged, &bt, 8, 32, 32);
                (emp.clone(), Vec::new(), emp, "emp")
            }
        };
        // espejo del pipeline: clamp del primer cue al primer beat >= 0
        cues = sm_core::cue_engine::clamp_first_cue_nonneg(&cues, &bt);
        // espejo del pipeline: re-anclaje al beat previo (fix cues-anchor)
        let mut phase_debug = String::new();
        if scores.is_some() {
            let phase = onset_env
                .as_ref()
                .map(|(env, hop_t)| sm_core::beats::onset_phase_offset(&bt, env, *hop_t))
                .unwrap_or(0.0);
            phase_debug = format!(
                "{:.3}\t{}",
                phase,
                sm_core::cue_engine::anchor_shift_beats(bt[0], bt[1] - bt[0], phase)
            );
            cues = sm_core::cue_engine::anchor_prev_cues(&cues, &bt, phase);
            cues = sm_core::cue_engine::phase_snap_cues(&cues, &bt, phase);
        }
        let cues_lr = sm_core::cue_engine::clamp_first_cue_nonneg(&cues_lr, &bt);
        let csv = |v: &[(f64, i32)]| -> String {
            v.iter()
                .map(|(t, _)| format!("{:.3}", t))
                .collect::<Vec<_>>()
                .join(",")
        };
        let segs_csv = merged
            .iter()
            .map(|(s, e, l)| format!("{:.2}:{:.2}:{}", s, e, l))
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{}\t{}\t{}\t{}\t{:.4}\t{:.5}\t{}\t{}\t{}\t{}\t{}\t{}",
            line,
            format!("{:.2}", br.tempo),
            csv(&cues),
            engine,
            br.t0_s,
            br.period_s,
            br.n_beats,
            segs_csv,
            csv(&cues_lr),
            csv(&cues_emp),
            scores_csv,
            phase_debug,
        );
    }
}
