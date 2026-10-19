//! Diagnóstico: compara duración/waveform del análisis (sm-core) vs el deck
//! (decode_full nativo) vs lo persistido en BD, para archivos reales.

use sm_db::{LibraryDb, LibraryQuery, SortField};

fn main() -> anyhow::Result<()> {
    let db_path = std::env::var("LOCALAPPDATA")
        .map(|p| std::path::PathBuf::from(p).join("smart-mix").join("library.db"))
        .unwrap();
    let db = LibraryDb::open(&db_path)?;

    let q = LibraryQuery {
        sort: SortField::Duration,
        desc: true,
        limit: 400,
        ..Default::default()
    };
    let rows = db.query(&q)?;
    println!("pistas consultadas: {}", rows.len());

    // 3 más largas + primeros 2 mp3 distintos
    let mut picks: Vec<i64> = rows.iter().take(3).map(|r| r.id).collect();
    for r in &rows {
        if let Some(tf) = db.track_full(r.id)? {
            if tf.meta.path.to_lowercase().ends_with(".mp3") && !picks.contains(&r.id) {
                picks.push(r.id);
                if picks.len() >= 5 {
                    break;
                }
            }
        }
    }

    println!(
        "{:<6} {:<6} {:>9} {:>9} {:>9} {:>8} {:>8} {:>8}",
        "id", "ext", "db_s", "core_s", "deck_s", "waveB", "d_core%", "d_deck%"
    );
    for id in picks {
        let tf = db.track_full(id)?.expect("track_full");
        let p = std::path::PathBuf::from(&tf.meta.path);
        let db_s = tf.analysis.duration_s;
        let core = sm_core::decode::decode_file(&p);
        let deck = sm_audio::deck::decode_full(&p);
        let core_s = core.as_ref().map(|d| d.duration_s).unwrap_or(0.0);
        let (deck_s, deck_rate, deck_ch) = deck
            .as_ref()
            .map(|(s, r, c)| ((s.len() / (*c).max(1)) as f64 / *r as f64, *r, *c))
            .unwrap_or((0.0, 0, 0));
        let buckets = db.waveform(id).ok().flatten().map(|w| w.len()).unwrap_or(0);
        let dc = if deck_s > 0.0 { ((core_s - deck_s) / deck_s) * 100.0 } else { 0.0 };
        let dd = if deck_s > 0.0 { ((db_s - deck_s) / deck_s) * 100.0 } else { 0.0 };
        println!(
            "{:<6} {:<6} {:>9.2} {:>9.2} {:>9.2} {:>8} {:>8.2} {:>8.2}  sr={} ch={}{}{}",
            id,
            p.extension().and_then(|e| e.to_str()).unwrap_or("?"),
            db_s,
            core_s,
            deck_s,
            buckets,
            dc,
            dd,
            deck_rate,
            deck_ch,
            core.err().map(|e| format!(" CORE_ERR={e}")).unwrap_or_default(),
            deck.err().map(|e| format!(" DECK_ERR={e}")).unwrap_or_default(),
        );
    }
    println!("\nesperado: waveB=2048, d_core%≈0, d_deck%≈0");
    Ok(())
}
