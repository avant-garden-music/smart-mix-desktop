//! Import headless: escanea una carpeta, llena la BD de la app (misma ruta
//! que usa sm-app) y analiza con el pool real — prueba de sistema completa
//! sin UI. Uso:
//!   cargo run -p sm-app --release --example headless_import -- "C:/ruta" [workers]

use sm_core::models::Models;
use sm_core::pipeline::{analyze_track, AnalyzeOptions};
use sm_db::LibraryDb;
use sm_library::{read_metadata, scan_root};
use sm_model::AnalysisStatus;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(
        args.get(1).expect("uso: headless_import <carpeta> [workers]"),
    );
    let workers: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6);
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("smart-mix");
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("library.db");
    println!("BD: {}", db_path.display());
    let db = Arc::new(Mutex::new(LibraryDb::open(&db_path)?));

    let t0 = Instant::now();
    let files = scan_root(&root);
    println!("escaneo: {} archivos en {:.1} s", files.len(), t0.elapsed().as_secs_f64());

    // 1) upsert de metadatos (paralelo con rayon, DB en serie por Mutex)
    let metas: Vec<_> = {
        let db = db.clone();
        files
            .par_iter()
            .filter_map(|f| {
                read_metadata(f)
                    .map_err(|e| {
                        eprintln!("meta {}: {e}", f.display());
                        e
                    })
                    .ok()
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|sf| {
                let mut db = db.lock().unwrap();
                let id = db.upsert_track(&sf.meta).expect("upsert");
                if let Some(cov) = sf.cover {
                    let _ = db.set_cover(id, Some(cov));
                }
                (id, sf.meta.path.clone())
            })
            .collect()
    };
    println!("metadatos: {} pistas en {:.1} s", metas.len(), t0.elapsed().as_secs_f64());
    let stats = db.lock().unwrap().stats()?;
    println!("BD: total={} pendientes={}", stats.total, stats.pending);

    // 2) análisis paralelo
    let models = Arc::new(Models::load(&sm_core::pipeline::default_models_dir()));
    println!("modelos: {}", if models.available() { "activos" } else { "fallback" });
    let queue = Arc::new(Mutex::new(metas.clone().into_iter().collect::<std::collections::VecDeque<_>>()));
    let done = Arc::new(AtomicU64::new(0));
    let failed = Arc::new(AtomicU64::new(0));
    let total = metas.len() as u64;
    let t1 = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..workers {
            let db = db.clone();
            let models = models.clone();
            let queue = queue.clone();
            let done = done.clone();
            let failed = failed.clone();
            s.spawn(move || loop {
                let item = queue.lock().unwrap().pop_front();
                let Some((id, path)) = item else { break };
                match analyze_track(std::path::Path::new(&path), &AnalyzeOptions::default(), &models) {
                    Ok(a) => {
                        if let Ok(mut db) = db.lock() {
                            if let Err(e) = db.save_analysis(id, &a) {
                                eprintln!("save {path}: {e}");
                            }
                        }
                        done.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        eprintln!("analyze {path}: {e}");
                        if let Ok(mut db) = db.lock() {
                            let _ = db.set_analysis_status(id, AnalysisStatus::Failed);
                        }
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
                let d = done.load(Ordering::Relaxed) + failed.load(Ordering::Relaxed);
                if d % 50 == 0 {
                    println!("  progreso {d}/{total} ({:.1} pistas/min)",
                        d as f64 / t1.elapsed().as_secs_f64() * 60.0);
                }
            });
        }
    });
    println!("análisis: {} OK, {} errores, {:.1} s ({:.1} pistas/min)",
        done.load(Ordering::Relaxed), failed.load(Ordering::Relaxed),
        t1.elapsed().as_secs_f64(),
        (done.load(Ordering::Relaxed) + failed.load(Ordering::Relaxed)) as f64
            / t1.elapsed().as_secs_f64() * 60.0);

    // 3) verificación: consulta ordenada por BPM + una búsqueda FTS
    let db = db.lock().unwrap();
    let q = sm_db::LibraryQuery {
        sort: sm_db::SortField::Bpm,
        desc: true,
        limit: 5,
        ..Default::default()
    };
    for r in db.query(&q)? {
        println!("  top BPM: {:>40} {:>4} {:>7.2} E{:>3}", r.title.chars().take(40).collect::<String>(),
            r.key_camelot.clone().unwrap_or_default(), r.bpm.unwrap_or(0.0),
            r.energy.map(|e| format!("{}", e.round())).unwrap_or_default());
    }
    let qf = sm_db::LibraryQuery {
        text: Some("love".into()),
        limit: 3,
        ..Default::default()
    };
    println!("FTS 'love': {} coincidencias (top 3 pedidas)", db.count(&qf)?);
    let stats = db.stats()?;
    println!("BD final: total={} analizadas={} pendientes={} fallidas={}",
        stats.total, stats.analyzed, stats.pending, stats.failed);
    Ok(())
}

use rayon::prelude::*;
