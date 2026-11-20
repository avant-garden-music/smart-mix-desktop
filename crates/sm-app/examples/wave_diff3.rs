//! Comparación frame-exacta: sm-core (análisis) vs deck (nativo) en archivos
//! específicos. Muestra frames, duración y delta por archivo.

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths = if args.is_empty() {
        vec![
            "C:/Users/Crist/Music/all/Tigerblind - Cosmic Urges.mp3".into(),
            "C:/Users/Crist/Music/all/The Blaze - Virile.mp3".into(),
            "C:/Users/Crist/Music/all/Tiff Cornish - Repeat At The Dancehall.mp3".into(),
        ]
    } else {
        args
    };
    println!(
        "{:>28} {:>10} {:>10} {:>10} {:>10} {:>8} {:>8} {:>8}",
        "archivo", "core_s", "deck_s", "delta_s", "delta%", "coreSR", "deckSR", "deckCH"
    );
    for p in paths {
        let path = std::path::PathBuf::from(&p);
        let core = sm_core::decode::decode_file(&path);
        let deck = sm_audio::deck::decode_full(&path);
        match (core, deck) {
            (Ok(c), Ok((ds, dr, dc))) => {
                let core_frames = c.interleaved.len() / c.channels.max(1);
                let deck_frames = ds.len() / dc.max(1);
                let deck_s = deck_frames as f64 / dr as f64;
                let delta = deck_s - c.duration_s;
                let pct = delta / deck_s * 100.0;
                println!(
                    "{:>28} {:>10.3} {:>10.3} {:>10.3} {:>9.2}% {:>8} {:>8} {:>8}",
                    path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                    c.duration_s,
                    deck_s,
                    delta,
                    pct,
                    c.sample_rate,
                    dr,
                    dc
                );
                eprintln!(
                    "  {}: core_frames={} ({} ch), deck_frames={} ({} ch)",
                    path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                    core_frames,
                    c.channels,
                    deck_frames,
                    dc
                );
            }
            (Err(e), _) => println!("{p}: CORE ERROR {e}"),
            (_, Err(e)) => println!("{p}: DECK ERROR {e}"),
        }
    }
    Ok(())
}
