//! Bench de sm-db en memoria: inserción masiva (30k y 100k pistas
//! sintéticas) y latencia p50/p95 de tres consultas típicas de la UI.
//!
//! Ejecutar en release: `cargo run --release -p sm-db --example bench_30k`.

use std::time::Instant;

use sm_db::{LibraryDb, LibraryQuery, SortField};
use sm_model::{SmartFilter, TrackAnalysis, TrackMeta};

const WORDS: [&str; 8] = ["Love", "Neon", "Rhythm", "Ocean", "Fire", "Midnight", "Echo", "Velvet"];
const GENRES: [&str; 5] = ["House", "Techno", "Ambient", "Drum & Bass", "Pop"];

struct Lcg(u64);
impl Lcg {
    fn next_f(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn synth(i: usize, rng: &mut Lcg) -> TrackMeta {
    TrackMeta {
        path: format!("C:/bench/lib/{:06}.mp3", i),
        title: format!("{} {}", WORDS[i % WORDS.len()], i),
        artist: format!("Artist {:03}", i % 400),
        album: format!("Album {:03}", i % 800),
        genre: GENRES[i % GENRES.len()].to_string(),
        year: Some(1990 + (i % 35) as i32),
        track_no: Some((i % 20) as i32 + 1),
        duration_s: 120.0 + rng.next_f() * 300.0,
        bitrate_kbps: Some(320),
        sample_rate: Some(44_100),
        channels: Some(2),
        format: "mp3".into(),
        file_size: 8_000_000 + (i % 1000) as u64 * 1000,
        file_modified_unix: 1_700_000_000 + (i % 100_000) as i64,
    }
}

fn synth_analysis(i: usize, meta: &TrackMeta) -> TrackAnalysis {
    TrackAnalysis {
        key_camelot: Some(format!("{}{}", 1 + (i % 12), if i % 2 == 0 { 'A' } else { 'B' })),
        key_name: None,
        key_confidence: Some(0.5 + (i % 50) as f64 / 100.0),
        tempo: Some(90.0 + (i % 80) as f64),
        energy: Some(1.0 + (i % 10) as f64),
        duration_s: meta.duration_s,
        ..Default::default()
    }
}

fn pct(samples: &mut Vec<f64>, p: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((p / 100.0) * (samples.len() - 1) as f64).round() as usize;
    samples[idx.min(samples.len() - 1)]
}

fn timed(iters: usize, mut f: impl FnMut()) -> Vec<f64> {
    (0..iters).map(|_| {
        let t = Instant::now();
        f();
        t.elapsed().as_secs_f64() * 1e3
    })
    .collect()
}

fn bench_queries(db: &LibraryDb) -> anyhow::Result<()> {
    const ITERS: usize = 100;

    let q_bpm = LibraryQuery {
        sort: SortField::Bpm,
        desc: true,
        limit: 200,
        ..Default::default()
    };
    let q_fts = LibraryQuery {
        text: Some("love".into()),
        sort: SortField::Title,
        limit: 200,
        ..Default::default()
    };
    let q_energy = LibraryQuery {
        smart: Some(SmartFilter {
            energy_min: Some(5.0),
            energy_max: Some(7.0),
            ..Default::default()
        }),
        sort: SortField::Energy,
        desc: true,
        limit: 200,
        ..Default::default()
    };

    // resultados sanity + warmup
    let n_bpm = db.query(&q_bpm)?.len();
    let n_fts = db.query(&q_fts)?.len();
    let n_energy = db.query(&q_energy)?.len();

    let mut t_bpm = timed(ITERS, || {
        db.query(&q_bpm).unwrap();
    });
    let mut t_fts = timed(ITERS, || {
        db.query(&q_fts).unwrap();
    });
    let mut t_energy = timed(ITERS, || {
        db.query(&q_energy).unwrap();
    });

    println!("{:<26}{:>10}{:>10}{:>10}", "operación", "filas", "p50 (ms)", "p95 (ms)");
    println!("{:-<56}", "");
    println!(
        "{:<26}{:>10}{:>10.3}{:>10.3}",
        "sort por bpm DESC",
        n_bpm,
        pct(&mut t_bpm, 50.0),
        pct(&mut t_bpm, 95.0)
    );
    println!(
        "{:<26}{:>10}{:>10.3}{:>10.3}",
        "búsqueda FTS 'love'",
        n_fts,
        pct(&mut t_fts, 50.0),
        pct(&mut t_fts, 95.0)
    );
    println!(
        "{:<26}{:>10}{:>10.3}{:>10.3}",
        "filtro energy 5..7",
        n_energy,
        pct(&mut t_energy, 50.0),
        pct(&mut t_energy, 95.0)
    );
    Ok(())
}

fn run(n: usize) -> anyhow::Result<()> {
    let db = LibraryDb::open_in_memory()?;
    println!();
    println!("==== sm-db bench in-memory — {n} pistas ====");

    let mut rng = Lcg(0x5EED_1234);

    let t0 = Instant::now();
    let tx = db.begin()?;
    for i in 0..n {
        let m = synth(i, &mut rng);
        db.upsert_track(&m)?;
    }
    tx.commit()?;
    let t_ins = t0.elapsed();

    let t0 = Instant::now();
    let tx = db.begin()?;
    for i in 0..n {
        let m = synth(i, &mut rng);
        let id = db.upsert_track(&m)?; // upsert devuelve el mismo id
        let _ = id;
    }
    tx.commit()?;
    let t_reup = t0.elapsed();

    let t0 = Instant::now();
    let tx = db.begin()?;
    for i in 0..n {
        let m = synth(i, &mut rng);
        let id = db.upsert_track(&m)?;
        db.save_analysis(id, &synth_analysis(i, &m))?;
    }
    tx.commit()?;
    let t_an = t0.elapsed();

    println!(
        "insert  (upsert_track)   : {:>9.1} ms  ({:>7.0}/s)",
        t_ins.as_secs_f64() * 1e3,
        n as f64 / t_ins.as_secs_f64()
    );
    println!(
        "re-upsert (path existente): {:>8.1} ms  ({:>7.0}/s)",
        t_reup.as_secs_f64() * 1e3,
        n as f64 / t_reup.as_secs_f64()
    );
    println!(
        "analyze (save_analysis)  : {:>9.1} ms  ({:>7.0}/s)",
        t_an.as_secs_f64() * 1e3,
        n as f64 / t_an.as_secs_f64()
    );

    bench_queries(&db)?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    println!("sm-db benchmark (in-memory SQLite, FTS5 + índices)");
    run(30_000)?;
    run(100_000)?;
    Ok(())
}
