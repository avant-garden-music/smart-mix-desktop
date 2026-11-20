//! Dump del perfil de tonalidad (88 teclas, agregado A) para reentrenamiento.
//! Uso: cargo run -p sm-core --release --example dump_key_profile -- lista.txt > perfiles.jsonl
//! lista.txt = una ruta absoluta por línea. Salida: JSON Lines {"path":..., "a88":[...]}.

use rayon::prelude::*;
use std::io::Write;

fn main() {
    let list = std::env::args().nth(1).expect("uso: dump_key_profile <lista.txt>");
    let txt = std::fs::read_to_string(&list).unwrap();
    let files: Vec<String> = txt
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    eprintln!("volcando {} pistas…", files.len());
    let ok = std::sync::atomic::AtomicUsize::new(0);
    let err = std::sync::atomic::AtomicUsize::new(0);
    let recs: Vec<String> = files
        .par_iter()
        .map(|f| {
        let path = std::path::Path::new(f);
        let rec = match sm_core::decode::decode_file(path) {
            Ok(d) => match sm_core::key::detect_key_frames(&d.mono()) {
                Some(kr) => {
                    let j = serde_json::json!({"path": f, "a88": kr.features88});
                    ok.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    j.to_string()
                }
                None => {
                    err.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    serde_json::json!({"path": f, "error": "sin frames de key"}).to_string()
                }
            },
            Err(e) => {
                err.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                serde_json::json!({"path": f, "error": e.to_string()}).to_string()
            }
        };
        rec
        })
        .collect();
    let mut w = std::io::BufWriter::new(std::io::stdout().lock());
    for r in &recs {
        writeln!(w, "{}", r).unwrap();
    }
    eprintln!(
        "ok={} err={}",
        ok.load(std::sync::atomic::Ordering::Relaxed),
        err.load(std::sync::atomic::Ordering::Relaxed)
    );
}
