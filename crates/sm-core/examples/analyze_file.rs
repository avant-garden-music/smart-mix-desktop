//! Analiza un archivo y vuelca el resultado completo (validación e2e).
//! Uso: cargo run -p sm-core --release --example analyze_file -- "pista.mp3"

use sm_core::pipeline::{analyze_track, AnalyzeOptions};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("uso: analyze_file <audio>");
        std::process::exit(1);
    }
    let t0 = std::time::Instant::now();
    let models = sm_core::models::Models::load(&sm_core::pipeline::default_models_dir());
    println!("modelos ONNX: {}", if models.available() { "sí" } else { "no (fallback)" });
    let path = Path::new(&args[1]);
    match analyze_track(path, &AnalyzeOptions::default(), &models) {
        Ok(a) => {
            println!("archivo      : {}", path.display());
            println!("tonalidad    : {} ({}) conf={:.3}  2ª={} ({:.3})",
                a.key_camelot.as_deref().unwrap_or("?"),
                a.key_name.as_deref().unwrap_or("?"),
                a.key_confidence.unwrap_or(0.0),
                a.second_key.as_deref().unwrap_or("?"),
                a.second_confidence.unwrap_or(0.0));
            println!("BPM          : {:.4}  beats={}  t0={:.4}s",
                a.tempo.unwrap_or(0.0),
                a.beatgrid.as_ref().map(|b| b.n_beats).unwrap_or(0),
                a.beatgrid.as_ref().map(|b| b.t0_s).unwrap_or(0.0));
            println!("energía      : {:?}", a.energy);
            println!("volumen      : {:+.2} dB   LUFS {:+.2}   clips {}",
                a.volume_db.unwrap_or(0.0), a.lufs.unwrap_or(0.0),
                a.clipped_peak_count.unwrap_or(0));
            println!("segmentos    : {}", a.segments.len());
            println!("cues         : {}  {:?}", a.cues.len(),
                a.cues.iter().map(|c| format!("{:.2}", c.time_s)).collect::<Vec<_>>());
            println!("waveform     : {} buckets", a.waveform.len() / 3);
            println!("análisis     : {} ms (total proceso {} ms)",
                a.analysis_ms.unwrap_or(0), t0.elapsed().as_millis());
        }
        Err(e) => {
            eprintln!("ERROR: {e:#}");
            std::process::exit(2);
        }
    }
}
