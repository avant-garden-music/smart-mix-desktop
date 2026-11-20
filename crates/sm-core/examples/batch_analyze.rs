//! Batch de análisis sobre una carpeta — validación real + benchmark.
//! Uso: cargo run -p sm-core --release --example batch_analyze -- "C:/ruta" [workers]

use rayon::prelude::*;
use sm_core::pipeline::{analyze_track, AnalyzeOptions};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("uso: batch_analyze <carpeta> [workers]");
        std::process::exit(1);
    }
    let root = PathBuf::from(&args[1]);
    let workers: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let exts = ["mp3", "flac", "wav", "m4a", "aiff", "aif", "aac", "ogg"];
    let mut files: Vec<PathBuf> = Vec::new();
    for e in walkdir::WalkDir::new(&root).max_depth(6).into_iter().flatten() {
        if e.file_type().is_file() {
            if let Some(ext) = e.path().extension().and_then(|x| x.to_str()) {
                if exts.contains(&ext.to_ascii_lowercase().as_str()) {
                    files.push(e.path().to_path_buf());
                }
            }
        }
    }
    files.sort();
    println!("corpus: {} archivos, {} workers", files.len(), workers);
    let models = std::sync::Arc::new(sm_core::models::Models::load(
        &sm_core::pipeline::default_models_dir(),
    ));
    let t0 = Instant::now();
    let results: Vec<(String, String, f64, Option<f64>, Option<f64>, u64, Option<String>)> =
        files
            .par_iter()
            .enumerate()
            .map(|(i, f)| {
                let r = analyze_track(f, &AnalyzeOptions::default(), &models);
                match r {
                    Ok(a) => (
                        f.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(),
                        a.key_camelot.clone().unwrap_or_else(|| "?".into()),
                        a.tempo.unwrap_or(0.0),
                        a.energy,
                        a.lufs,
                        a.analysis_ms.unwrap_or(0),
                        None,
                    ),
                    Err(e) => (
                        f.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(),
                        "ERROR".into(),
                        0.0,
                        None,
                        None,
                        0,
                        Some(format!("{e}")),
                    ),
                }
            })
            .collect();
    // con progreso por archivo sería ideal; rayon map es eager con collect
    let total = t0.elapsed();
    let ok = results.iter().filter(|r| r.6.is_none()).count();
    let errs = results.len() - ok;
    let mut key_dist: BTreeMap<String, usize> = BTreeMap::new();
    let mut bpm_min = f64::MAX;
    let mut bpm_max = 0.0f64;
    let mut sum_ms = 0u64;
    let mut max_ms = 0u64;
    for r in &results {
        if r.6.is_some() {
            continue;
        }
        *key_dist.entry(r.1.clone()).or_default() += 1;
        if r.2 > 0.0 {
            bpm_min = bpm_min.min(r.2);
            bpm_max = bpm_max.max(r.2);
        }
        sum_ms += r.5;
        max_ms = max_ms.max(r.5);
    }
    println!("\n=== RESUMEN ===");
    println!("analizadas OK : {ok} / {}", results.len());
    println!("errores       : {errs}");
    if ok > 0 {
        println!("tiempo total  : {:.1} s ({:.2} s/pista promedio, peor {} ms)",
            total.as_secs_f64(), sum_ms as f64 / ok as f64 / 1000.0, max_ms);
        println!("throughput    : {:.1} pistas/min con {workers} workers",
            ok as f64 / total.as_secs_f64() * 60.0);
        println!("BPM rango     : {bpm_min:.1} – {bpm_max:.1}");
        println!("distribución de tonalidades:");
        let mut keys: Vec<_> = key_dist.into_iter().collect();
        keys.sort_by(|a, b| b.1.cmp(&a.1));
        for (k, n) in keys.iter().take(24) {
            println!("  {k:>4} : {n}");
        }
    }
    let mut n_err = 0;
    for r in &results {
        if let Some(e) = &r.6 {
            if n_err < 10 {
                println!("ERROR {}: {}", r.0, e);
            }
            n_err += 1;
        }
    }
    // dump JSON para análisis posterior
    let dump: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "file": r.0, "key": r.1, "bpm": r.2,
                "energy": r.3, "lufs": r.4, "ms": r.5, "error": r.6,
            })
        })
        .collect();
    let out = std::path::Path::new("batch_results.json");
    let _ = std::fs::write(out, serde_json::to_string_pretty(&dump).unwrap_or_default());
    println!("\n→ batch_results.json");
}
