//! Re-análisis COMPLETO de la biblioteca (key v5 + cues motor herramienta de referencia) — actualiza
//! la BD de la app para todas las pistas. Uso:
//!   cargo run -p sm-app --release --example reanalyze_library [-- umbral_ms]
//! La app debe estar CERRADA (bloqueo SQLite). Análisis en paralelo (rayon),
//! escrituras seriales.

use rayon::prelude::*;
use sm_core::pipeline::{analyze_track, AnalyzeOptions};
use sm_db::{LibraryDb, LibraryQuery};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let db_path = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("smart-mix")
        .join("library.db");
    let mut db = LibraryDb::open(&db_path)?;
    let rows = db.query(&LibraryQuery {
        limit: 0,
        ..Default::default()
    })?;
    println!("re-analizando {} pistas…", rows.len());
    // fase 1: rutas serial (Connection no es Sync); fase 2 paralela sin db
    let ids_paths: Vec<(i64, std::path::PathBuf)> = rows
        .iter()
        .filter_map(|r| {
            let p = db
                .track_full(r.id)
                .ok()
                .flatten()
                .map(|f| std::path::PathBuf::from(f.meta.path))?;
            if p.as_os_str().is_empty() {
                None
            } else {
                Some((r.id, p))
            }
        })
        .collect();
    println!("re-analizando {} pistas…", ids_paths.len());
    let models = Arc::new(sm_core::models::Models::load(
        &sm_core::pipeline::default_models_dir(),
    ));
    let opts = AnalyzeOptions::default();
    let results: Vec<(i64, anyhow::Result<sm_model::TrackAnalysis>)> = ids_paths
        .par_iter()
        .map(|(id, p)| (*id, analyze_track(p, &opts, &models)))
        .collect();
    let mut ok = 0usize;
    let mut err = 0usize;
    for (id, res) in results {
        match res {
            Ok(a) => {
                db.save_analysis(id, &a)?;
                ok += 1;
            }
            Err(_) => err += 1,
        }
    }
    println!("re-análisis: ok={ok} err={err}");
    Ok(())
}
