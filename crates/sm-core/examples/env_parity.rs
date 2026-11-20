//! Paridad numérica del port cue_engine vs harmonic-engine (Python).
//! Lee los vuelcos de dump_ref.py y compara envolvente + scores + cues.
//! Uso: cargo run --release -p sm-core --example env_parity

use sm_core::cue_engine;

const DIR: &str = r"C:/Users/Crist/AppData/Local/hermes/profiles/smartmix-build/cache/scratch/cuepar";

fn read_f32(name: &str) -> Vec<f32> {
    let bytes = std::fs::read(format!("{DIR}/{name}")).expect(name);
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn read_meta(name: &str) -> serde_json::Value {
    let s = std::fs::read_to_string(format!("{DIR}/{name}")).unwrap();
    serde_json::from_str(&s).unwrap()
}

fn stats(a: &[f32], b: &[f32]) -> (f64, f64, f64) {
    // (max abs diff, corr, max rel diff en valores > 1e-6)
    let n = a.len().min(b.len());
    let mut maxd = 0.0f64;
    let mut maxrel = 0.0f64;
    let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..n {
        let (x, y) = (a[i] as f64, b[i] as f64);
        let d = (x - y).abs();
        if d > maxd { maxd = d; }
        if y.abs() > 1e-6 {
            let r = d / y.abs();
            if r > maxrel { maxrel = r; }
        }
        sa += x; sb += y; saa += x * x; sbb += y * y; sab += x * y;
    }
    let cov = sab / n as f64 - (sa / n as f64) * (sb / n as f64);
    let va = saa / n as f64 - (sa / n as f64).powi(2);
    let vb = sbb / n as f64 - (sb / n as f64).powi(2);
    let corr = cov / (va.sqrt() * vb.sqrt());
    (maxd, corr, maxrel)
}

fn main() {
    for meta_name in [
        "ref_0_1___Alex_Gaudino___Destination_Calabria__meta.json",
        "ref_1_19___Shiba_San___OKAY__Original_Mix__mp3_meta.json",
    ] {
        let stem = meta_name.trim_end_matches("_meta.json");
        let meta = read_meta(meta_name);
        let mono = read_f32(&format!("{stem}_mono.f32"));
        let py_ch1 = read_f32(&format!("{stem}_ch1.f32"));
        let py_ch2 = read_f32(&format!("{stem}_ch2.f32"));

        let (ch1, ch2) = cue_engine::envelope_exact(&mono);
        let rf = format!("{stem}_rust_ch1.f32");
        let bytes: Vec<u8> = ch1.iter().flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(format!("{DIR}/{rf}"), bytes).unwrap();
        let (d1, c1, r1) = stats(&ch1, &py_ch1);
        let (d2, c2, r2) = stats(&ch2, &py_ch2);
        println!("== {stem}");
        println!(
            "  env  rust={} py={} | ch1 maxd={:.3e} corr={:.6} rel={:.3e} | ch2 maxd={:.3e} corr={:.6} rel={:.3e}",
            ch1.len(), py_ch1.len(), d1, c1, r1, d2, c2, r2
        );

        // rejilla de periodo constante desde los primeros 64 beats
        let b64: Vec<f64> = meta["beats_first64"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let n_beats = meta["n_beats"].as_u64().unwrap() as usize;
        let period = (b64[63] - b64[0]) / 63.0;
        let bt: Vec<f64> = (0..n_beats).map(|i| b64[0] + i as f64 * period).collect();

        let scores = cue_engine::detect_pre_analysis(&bt, &ch1, &ch2);
        if let Some(sc) = &scores {
            let smax = sc.iter().cloned().fold(0.0f64, f64::max);
            let argmax = sc
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(0);
            let py = &meta["scores_stats"];
            println!(
                "  scor rust len={} max={:.4}@{} | py len={} max={:.4}@{}",
                sc.len(), smax, argmax,
                py["len"].as_u64().unwrap(),
                py["max"].as_f64().unwrap(),
                py["argmax"].as_u64().unwrap()
            );
            // primeros 128 (comparacion gruesa)
            let py128: Vec<f64> = py["first128"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let sc32: Vec<f32> = sc.iter().map(|v| *v as f32).collect();
            let py32: Vec<f32> = py128.iter().map(|v| *v as f32).collect();
            let (ds, cs, _) = stats(&sc32[..128], &py32);
            println!("  scor[0..128] maxd={:.3e} corr={:.6}", ds, cs);

            // seleccion con segs sinteticos (0, dur, 5) — igual que dump_ref
            let dur = bt[n_beats - 1];
            let cues = cue_engine::cue_points(
                &[(0.0, dur, 5)], &bt, Some(sc), 8, 32, 32,
            );
            let py_cues: Vec<(f64, i64)> = meta["cues"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    let a = v.as_array().unwrap();
                    (a[0].as_f64().unwrap(), a[1].as_i64().unwrap())
                })
                .collect();
            println!("  cues rust={:?}", cues);
            println!("  cues py  ={:?}", py_cues);
        }
    }
}
