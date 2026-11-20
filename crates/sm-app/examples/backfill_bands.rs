//! Backfill del envelope espectral de 3 bandas (waveform_bands) para las
//! pistas ya analizadas — sin re-analizar key/BPM/energy.
//! Uso: cargo run -p sm-app --release --example backfill_bands

use sm_core::decode::decode_file;
use sm_core::volume;
use sm_db::{LibraryDb, LibraryQuery};

fn main() -> anyhow::Result<()> {
    let db_path = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("smart-mix")
        .join("library.db");
    let db = LibraryDb::open(&db_path)?;
    let rows = db.query(&LibraryQuery {
        limit: 0,
        ..Default::default()
    })?;
    let mut done = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    for r in &rows {
        let already = db
            .waveform_bands(r.id)?
            .map(|b| !b.is_empty())
            .unwrap_or(false);
        if already {
            skipped += 1;
            continue;
        }
        match db.track_full(r.id) {
            Ok(Some(f)) => match decode_file(std::path::Path::new(&f.meta.path)) {
                Ok(dec) => {
                    let bands = volume::band_envelope(&dec.interleaved, dec.channels, 2048, 44_100);
                    db.set_waveform_bands(r.id, &bands)?;
                    done += 1;
                    if done % 50 == 0 {
                        eprintln!("[backfill] {done} pistas…");
                    }
                }
                Err(e) => {
                    failed += 1;
                    eprintln!("[backfill] ✗ {}: {e}", f.meta.path);
                }
            },
            Ok(None) => skipped += 1,
            Err(_) => skipped += 1,
        }
    }
    println!("backfill bandas: ok={done} ya_tenian={skipped} fallidos={failed}");
    Ok(())
}
