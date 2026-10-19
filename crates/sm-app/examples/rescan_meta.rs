//! Re-escaneo SOLO de metadatos (sin análisis): actualiza title/artist de
//! pistas ya en la BD (p. ej. tras mejorar el fallback de nombre de archivo).
//! Uso: cargo run -p sm-app --release --example rescan_meta

use sm_db::{LibraryDb, LibraryQuery};
use sm_library::read_metadata;

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
    let mut updated = 0;
    for r in &rows {
        let p = std::path::PathBuf::from(
            db.track_full(r.id)?
                .map(|f| f.meta.path)
                .unwrap_or_default(),
        );
        if p.as_os_str().is_empty() {
            continue;
        }
        if let Ok(sf) = read_metadata(&p) {
            db.upsert_track(&sf.meta)?;
            updated += 1;
        }
    }
    println!("metadatos actualizados: {updated}/{}", rows.len());
    Ok(())
}
