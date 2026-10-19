//! Cobertura temporal real de la waveform persistida: último bucket con
//! energía y % de la duración declarada que cubre.

use sm_db::{LibraryDb, LibraryQuery, SortField};

fn main() -> anyhow::Result<()> {
    let db_path = std::env::var("LOCALAPPDATA")
        .map(|p| std::path::PathBuf::from(p).join("smart-mix").join("library.db"))
        .unwrap();
    let db = LibraryDb::open(&db_path)?;
    let q = LibraryQuery { limit: 0, ..Default::default() };
    let rows = db.query(&q)?;
    println!("pistas: {}", rows.len());

    let mut peor: Vec<(f64, i64, String, f64, f64)> = Vec::new(); // (cobertura%, id, path, dur, t_ultimo)
    let mut sin_wave = 0usize;
    for r in &rows {
        let wv = match db.waveform(r.id).ok().flatten() {
            Some(w) => w,
            None => {
                sin_wave += 1;
                continue;
            }
        };
        let n = wv.len() / 3;
        if n == 0 || r.duration_s <= 0.0 {
            continue;
        }
        // último bucket con energía (>1% de amplitud máxima global)
        let max_amp = wv.chunks(3).fold(0.0f32, |m, c| m.max(c[1].abs()));
        let thr = max_amp * 0.01;
        let mut last = 0usize;
        for (i, c) in wv.chunks(3).enumerate() {
            if c[1].abs() > thr || c[0].abs() > thr {
                last = i;
            }
        }
        let t_last = last as f64 / n as f64 * r.duration_s;
        let cover = t_last / r.duration_s * 100.0;
        peor.push((cover, r.id, String::new(), r.duration_s, t_last));
        let e = peor.len() - 1;
        peor[e].2 = db
            .track_full(r.id)?
            .map(|f| f.meta.path)
            .unwrap_or_default();
    }
    peor.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    println!("sin waveform: {sin_wave}");
    println!(
        "{:>8} {:>6} {:>9} {:>9}  {}",
        "cover%", "id", "dur_s", "t_last", "path"
    );
    for (cover, id, path, dur, t_last) in peor.iter().take(12) {
        println!("{:>7.1}% {:>6} {:>9.1} {:>9.1}  {}", cover, id, dur, t_last, path);
    }
    let full = peor.iter().filter(|(c, ..)| *c > 99.0).count();
    println!(
        "\nresumen: {} pistas con waveform, {} cubren >99%, {} menos de 99%",
        peor.len(),
        full,
        peor.len() - full
    );
    Ok(())
}
