//! Exportación real de la biblioteca de la app a todos los formatos
//! (prueba de sistema, no unit test). Uso:
//!   cargo run -p sm-app --release --example export_all -- "C:/salida"

use sm_db::{LibraryDb, LibraryQuery};
use sm_export::ExportEntry;
use sm_model::CuePoint;

fn main() -> anyhow::Result<()> {
    let out = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("uso: export_all <carpeta_salida>"),
    );
    std::fs::create_dir_all(&out)?;
    let db_path = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("smart-mix")
        .join("library.db");
    let db = LibraryDb::open(&db_path)?;
    let q = LibraryQuery {
        limit: 0,
        ..Default::default()
    };
    let rows = db.query(&q)?;
    println!("exportando {} pistas → {}", rows.len(), out.display());

    struct Own {
        path: String,
        title: String,
        artist: String,
        album: String,
        genre: String,
        bpm: Option<f64>,
        key_camelot: Option<String>,
        key_name: Option<String>,
        energy: Option<f64>,
        duration_s: f64,
        cues: Vec<CuePoint>,
        beatgrid: Option<sm_model::BeatGrid>,
    }
    let mut own = Vec::new();
    for r in &rows {
        if let Some(f) = db.track_full(r.id)? {
            own.push(Own {
                path: f.meta.path.clone(),
                title: f.meta.title.clone(),
                artist: f.meta.artist.clone(),
                album: f.meta.album.clone(),
                genre: f.meta.genre.clone(),
                bpm: f.analysis.tempo,
                key_camelot: f.analysis.key_camelot.clone(),
                key_name: f.analysis.key_name.clone(),
                energy: f.analysis.energy,
                duration_s: f.analysis.duration_s,
                cues: f.analysis.cues.clone(),
                beatgrid: f.analysis.beatgrid.clone(),
            });
        }
    }
    let entries: Vec<ExportEntry> = own
        .iter()
        .map(|f| ExportEntry {
            path: &f.path,
            title: &f.title,
            artist: &f.artist,
            album: &f.album,
            genre: &f.genre,
            bpm: f.bpm,
            key_camelot: f.key_camelot.as_deref(),
            key_name: f.key_name.as_deref(),
            energy: f.energy,
            duration_s: f.duration_s,
            cues: &f.cues,
            beatgrid: f.beatgrid.as_ref(),
        })
        .collect();
    let n = entries.len();
    let paths: Vec<String> = entries.iter().map(|e| e.path.to_string()).collect();

    sm_export::write_traktor_nml(&entries, out.join("smart-mix.nml"))?;
    println!("✓ smart-mix.nml");
    sm_export::write_rekordbox_xml(&entries, out.join("rekordbox.xml"))?;
    println!("✓ rekordbox.xml");
    sm_export::write_serato_crates(&[("Smart Mix".to_string(), paths.clone())], &out)?;
    println!("✓ _Serato_/Subcrates/Smart Mix.crate");
    sm_export::write_engine_dj_library(&entries, &out.join("EngineExport"))?;
    println!("✓ EngineExport/ (EngineLibrary)");
    sm_export::write_m3u(&paths, out.join("smart-mix.m3u"))?;
    println!("✓ smart-mix.m3u");
    sm_export::write_csv(&entries, out.join("smart-mix.csv"))?;
    println!("✓ smart-mix.csv");
    println!("OK: {n} pistas exportadas a 6 formatos");
    Ok(())
}
