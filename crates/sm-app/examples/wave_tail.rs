//! Para las peores coberturas: decodificar y medir RMS de la zona que la
//! waveform muestra vacía. Si hay energía audible → análisis truncado.

use sm_db::{LibraryDb, LibraryQuery, SortField};

fn main() -> anyhow::Result<()> {
    let db_path = std::env::var("LOCALAPPDATA")
        .map(|p| std::path::PathBuf::from(p).join("smart-mix").join("library.db"))
        .unwrap();
    let db = LibraryDb::open(&db_path)?;

    let ids: Vec<i64> = std::env::args().skip(1).filter_map(|s| s.parse().ok()).collect();
    let ids = if ids.is_empty() { vec![373, 560, 609, 633, 107, 668] } else { ids };

    println!(
        "{:>6} {:>9} {:>9} {:>9} {:>10} {:>10} {:>8}",
        "id", "dur_s", "t_last", "tail_s", "rms_tail", "rms_body", "db_tail"
    );
    for id in ids {
        let tf = db.track_full(id)?.expect("track");
        let dur = tf.analysis.duration_s;
        let wv = db.waveform(id)?.expect("wave");
        let n = wv.len() / 3;
        let max_amp = wv.chunks(3).fold(0.0f32, |m, c| m.max(c[1].abs()));
        let thr = max_amp * 0.01;
        let mut last = 0usize;
        for (i, c) in wv.chunks(3).enumerate() {
            if c[1].abs() > thr || c[0].abs() > thr {
                last = i;
            }
        }
        let t_last = last as f64 / n as f64 * dur;

        let p = std::path::PathBuf::from(&tf.meta.path);
        let (samples, _rate, ch) = sm_audio::deck::decode_full(&p)?;
        let frames = samples.len() / ch.max(1);
        let i_start = ((t_last / dur) * frames as f64) as usize;
        // RMS del tail (desde t_last hasta el final) y del body (primer 60%)
        let mut acc = 0.0f64;
        let mut cnt = 0usize;
        for f in i_start..frames {
            for c in 0..ch {
                let v = samples[f * ch + c] as f64;
                acc += v * v;
                cnt += 1;
            }
        }
        let rms_tail = if cnt > 0 { (acc / cnt as f64).sqrt() } else { 0.0 };
        let b_end = (frames as f64 * 0.6) as usize;
        let mut acc = 0.0f64;
        let mut cnt = 0usize;
        for f in 0..b_end {
            for c in 0..ch {
                let v = samples[f * ch + c] as f64;
                acc += v * v;
                cnt += 1;
            }
        }
        let rms_body = if cnt > 0 { (acc / cnt as f64).sqrt() } else { 0.0 };
        let db_tail = 20.0 * rms_tail.log10().max(-120.0);
        println!(
            "{:>6} {:>9.1} {:>9.1} {:>9.1} {:>10.5} {:>10.5} {:>8.1}",
            id,
            dur,
            t_last,
            dur - t_last,
            rms_tail,
            rms_body,
            db_tail
        );
    }
    Ok(())
}
