//! Tests de integración de sm-db: CRUD, FTS, orden, paginación, filtros
//! smart, roundtrip de análisis, playlists, duplicados, borrado en cascada,
//! ajustes y estadísticas.

use sm_db::{LibraryDb, LibraryQuery, SortField};
use sm_model::{
    AnalysisStatus, BeatGrid, CuePoint, EnergySegment, Preferences, SmartFilter, TrackAnalysis,
    TrackMeta,
};

fn meta(path: &str, title: &str, artist: &str) -> TrackMeta {
    TrackMeta {
        path: path.into(),
        title: title.into(),
        artist: artist.into(),
        album: format!("Album of {artist}"),
        genre: "House".into(),
        duration_s: 180.0,
        format: "mp3".into(),
        file_size: 1_000_000,
        file_modified_unix: 1_700_000_000,
        ..Default::default()
    }
}

fn an(key: Option<&str>, bpm: Option<f64>, energy: Option<f64>) -> TrackAnalysis {
    TrackAnalysis {
        key_camelot: key.map(String::from),
        // Nota: no usar sm_model::camelot_display_name aquí: su pitch_class
        // tiene un bug preexistente (resto negativo) con claves menores.
        key_name: key.map(|k| format!("Key {k}")),
        tempo: bpm,
        energy,
        duration_s: 200.0,
        ..Default::default()
    }
}

fn db() -> LibraryDb {
    LibraryDb::open_in_memory().expect("abriendo db en memoria")
}

#[test]
fn crud_upsert_y_roundtrip_de_metadatos() {
    let db = db();
    let mut m = meta("C:/lib/one.mp3", "One", "Artist A");
    let id1 = db.upsert_track(&m).unwrap();
    assert!(id1 > 0);

    // re-upsert por mismo path → mismo id, metadatos refrescados
    m.title = "One (Remaster)".into();
    m.file_size = 2_000_000;
    m.file_modified_unix = 1_700_000_999;
    let id2 = db.upsert_track(&m).unwrap();
    assert_eq!(id1, id2);

    let q = LibraryQuery { limit: 0, ..Default::default() };
    assert_eq!(db.count(&q).unwrap(), 1);

    let full = db.track_full(id1).unwrap().expect("track_full");
    assert_eq!(full.meta.title, "One (Remaster)");
    assert_eq!(full.meta.file_size, 2_000_000);
    assert_eq!(full.meta.file_modified_unix, 1_700_000_999);
    assert_eq!(full.status, AnalysisStatus::Pending);
    assert!(!full.meta.path.is_empty());

    // portada
    db.set_cover(id1, Some(vec![1, 2, 3, 4])).unwrap();
    assert_eq!(db.cover(id1).unwrap(), Some(vec![1, 2, 3, 4]));
    db.set_cover(id1, None).unwrap();
    assert_eq!(db.cover(id1).unwrap(), None);

    assert!(db.track_full(9999).unwrap().is_none());
}

#[test]
fn fts_busqueda_parcial_por_prefijo() {
    let db = db();
    db.upsert_track(&meta("C:/a.mp3", "Love Tonight", "Ava")).unwrap();
    db.upsert_track(&meta("C:/b.mp3", "Loveland", "Bo")).unwrap();
    db.upsert_track(&meta("C:/c.mp3", "Bohemian Rhapsody", "Queen")).unwrap();

    let q = |text: &str| LibraryQuery {
        text: Some(text.into()),
        limit: 0,
        ..Default::default()
    };
    assert_eq!(db.count(&q("love")).unwrap(), 2); // "love*" pega Love y Loveland
    let rows = db.query(&q("love")).unwrap();
    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    assert!(titles.contains(&"Love Tonight"));
    assert!(titles.contains(&"Loveland"));

    assert_eq!(db.count(&q("rhap")).unwrap(), 1); // prefijo parcial
    assert_eq!(db.count(&q("queen")).unwrap(), 1); // busca también en artista
    assert_eq!(db.count(&q("zzz")).unwrap(), 0);
    assert_eq!(db.count(&q("!!!")).unwrap(), 3); // sin tokens → sin filtro
}

#[test]
fn sort_por_bpm_descendente() {
    let db = db();
    let bpms = [120.5, 90.0, 174.25, 128.0, 100.0];
    for (i, bpm) in bpms.iter().enumerate() {
        let id = db.upsert_track(&meta(&format!("C:/t{i}.mp3"), &format!("T{i}"), "X")).unwrap();
        db.save_analysis(id, &an(Some("8A"), Some(*bpm), None)).unwrap();
    }
    let q = LibraryQuery {
        sort: SortField::Bpm,
        desc: true,
        limit: 0,
        ..Default::default()
    };
    let rows = db.query(&q).unwrap();
    let got: Vec<f64> = rows.iter().filter_map(|r| r.bpm).collect();
    let mut want = bpms.to_vec();
    want.sort_by(|a, b| b.partial_cmp(a).unwrap());
    assert_eq!(got, want);
}

#[test]
fn paginacion_coherente_con_count() {
    let db = db();
    for i in 0..25 {
        db.upsert_track(&meta(&format!("C:/p{i:02}.mp3"), &format!("Track {i:02}"), "X")).unwrap();
    }
    let base = LibraryQuery {
        sort: SortField::Title,
        desc: false,
        limit: 10,
        ..Default::default()
    };
    assert_eq!(db.count(&base).unwrap(), 25);

    let p1 = db.query(&base).unwrap();
    let p2 = db.query(&LibraryQuery { offset: 10, ..base.clone() }).unwrap();
    let p3 = db.query(&LibraryQuery { offset: 20, ..base.clone() }).unwrap();
    assert_eq!(p1.len(), 10);
    assert_eq!(p2.len(), 10);
    assert_eq!(p3.len(), 5);
    let total: u64 = (p1.len() + p2.len() + p3.len()) as u64;
    assert_eq!(total, db.count(&base).unwrap() as u64);

    let titles: Vec<String> = p1.iter().chain(&p2).chain(&p3).map(|r| r.title.clone()).collect();
    let mut sorted = titles.clone();
    sorted.sort();
    assert_eq!(titles, sorted); // sin repetidos ni huecos
    let uniq: std::collections::HashSet<&String> = titles.iter().collect();
    assert_eq!(uniq.len(), 25);
}

#[test]
fn smart_filter_energia_y_clave_compatible() {
    let db = db();
    // (clave, energía) — compatible con 8A: 8A, 8B, 7A, 9A
    let cases: [(&str, f64, bool); 8] = [
        ("8A", 5.0, true),
        ("8B", 6.0, true),
        ("7A", 7.0, true),
        ("9A", 5.5, true),
        ("9B", 6.0, false), // relativa de 9A: incompatible con 8A
        ("10A", 5.0, false),
        ("8A", 3.0, false), // energía fuera de 5..7
        ("8A", 8.0, false),
    ];
    for (i, (key, energy, _)) in cases.iter().enumerate() {
        let id = db.upsert_track(&meta(&format!("C:/k{i}.mp3"), &format!("K{i}"), "X")).unwrap();
        db.save_analysis(id, &an(Some(key), Some(120.0), Some(*energy))).unwrap();
    }
    let q = LibraryQuery {
        smart: Some(SmartFilter {
            energy_min: Some(5.0),
            energy_max: Some(7.0),
            key_compatible_with: Some("8A".into()),
            ..Default::default()
        }),
        limit: 0,
        ..Default::default()
    };
    let rows = db.query(&q).unwrap();
    assert_eq!(rows.len(), 4, "esperaba exactamente las 4 compatibles");
    assert_eq!(db.count(&q).unwrap(), 4);
    for r in &rows {
        let k = r.key_camelot.as_deref().unwrap();
        assert!(matches!(k, "8A" | "8B" | "7A" | "9A"), "clave inesperada {k}");
    }
}

#[test]
fn save_analysis_y_track_full_roundtrip() {
    let db = db();
    let id = db.upsert_track(&meta("C:/full.mp3", "Full Track", "Dj Full")).unwrap();

    let a = TrackAnalysis {
        key_camelot: Some("8A".into()),
        key_name: Some("A minor".into()),
        key_confidence: Some(0.8731),
        second_key: Some("8B".into()),
        second_confidence: Some(0.3122),
        tempo: Some(128.53),
        beatgrid: Some(BeatGrid { period_s: 0.468_218_5, t0_s: -0.21, n_beats: 456, tempo: 128.12 }),
        energy: Some(6.5),
        segments: vec![
            EnergySegment { start_s: 0.0, end_s: 16.0, level: Some(4) },
            EnergySegment { start_s: 16.0, end_s: 32.0, level: Some(6) },
        ],
        cues: vec![
            CuePoint { time_s: 1.25, energy_level: Some(5), origin: "auto".into(), label: None },
            CuePoint { time_s: 65.5, energy_level: None, origin: "user".into(), label: Some("Drop".into()) },
        ],
        volume_db: Some(-0.84),
        lufs: Some(-9.21),
        clipped_peak_count: Some(4),
        waveform: vec![0.0, -0.5, 0.25, 1.0, -0.999, 0.123_456],
        duration_s: 213.4,
        analysis_ms: Some(1234),
    };
    db.save_analysis(id, &a).unwrap();

    let full = db.track_full(id).unwrap().expect("track_full tras save_analysis");
    assert_eq!(full.status, AnalysisStatus::Done);
    assert_eq!(full.analysis, a, "roundtrip completo del análisis");
    // duration_s de la fila fue refinada por la del análisis
    assert!((full.meta.duration_s - 213.4).abs() < 1e-9);

    // waveform bincode idéntico bit a bit
    let wf = db.waveform(id).unwrap().expect("waveform");
    assert_eq!(wf, a.waveform);
    assert_eq!(wf.len(), 6);
}

#[test]
fn replace_cues_sobrescribe() {
    let db = db();
    let id = db.upsert_track(&meta("C:/cues.mp3", "Cues", "X")).unwrap();
    db.save_analysis(id, &an(Some("8A"), Some(120.0), Some(5.0))).unwrap();
    let before = db.track_full(id).unwrap().unwrap();
    assert_eq!(before.analysis.cues.len(), 0);

    let cues = vec![CuePoint {
        time_s: 32.0,
        energy_level: Some(7),
        origin: "user".into(),
        label: Some("Cue 1".into()),
    }];
    db.replace_cues(id, &cues).unwrap();
    let after = db.track_full(id).unwrap().unwrap();
    assert_eq!(after.analysis.cues, cues);
    // el resto del análisis sigue intacto
    assert_eq!(after.analysis.key_camelot.as_deref(), Some("8A"));
    assert_eq!(after.analysis.tempo, Some(120.0));
}

#[test]
fn playlists_manuales_y_smart() {
    let db = db();
    let a = db.upsert_track(&meta("C:/1.mp3", "A", "X")).unwrap();
    let b = db.upsert_track(&meta("C:/2.mp3", "B", "X")).unwrap();
    let c = db.upsert_track(&meta("C:/3.mp3", "C", "X")).unwrap();

    let pl = db.create_playlist("Warmup", "manual", None).unwrap();
    db.add_track_to_playlist(pl.id, a).unwrap();
    db.add_track_to_playlist(pl.id, b).unwrap();
    db.add_track_to_playlist(pl.id, c).unwrap();
    // duplicado → no-op
    db.add_track_to_playlist(pl.id, b).unwrap();
    assert_eq!(db.playlist_track_ids(pl.id).unwrap(), vec![a, b, c]);

    // query por playlist respeta el orden de position
    let q = LibraryQuery { playlist_id: Some(pl.id), limit: 0, ..Default::default() };
    let rows = db.query(&q).unwrap();
    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![a, b, c]);
    assert_eq!(db.count(&q).unwrap(), 3);

    // remove → el orden conserva la secuencia
    db.remove_track_from_playlist(pl.id, b).unwrap();
    assert_eq!(db.playlist_track_ids(pl.id).unwrap(), vec![a, c]);

    // rename
    db.rename_playlist(pl.id, "Warmup 2").unwrap();
    assert_eq!(db.playlists().unwrap()[0].name, "Warmup 2");

    // smart: el filtro hace roundtrip por JSON
    let filter = SmartFilter {
        energy_min: Some(5.0),
        genre: Some("House".into()),
        ..Default::default()
    };
    let smart = db.create_playlist("High Energy", "smart", Some(&filter)).unwrap();
    let all = db.playlists().unwrap();
    assert_eq!(all.len(), 2);
    let stored_smart = all.iter().find(|p| p.id == smart.id).unwrap();
    assert_eq!(stored_smart.kind, "smart");
    assert_eq!(stored_smart.smart_filter.as_ref().unwrap(), &filter);

    // delete playlist → sus pistas caen
    db.delete_playlist(pl.id).unwrap();
    assert!(db.playlist_track_ids(pl.id).unwrap().is_empty());
    assert_eq!(db.playlists().unwrap().len(), 1);
}

#[test]
fn duplicates_por_tags_y_por_titulo_duracion() {
    let db = db();
    // exact: mismo artist+title normalizado, distinta puntuación/case
    db.upsert_track(&meta("C:/dup1.mp3", "Summer Jam", "DJ Luna")).unwrap();
    db.upsert_track(&meta("C:/dup2.mp3", "summer  JAM!", "dj luna")).unwrap();
    // perceptual: título similar (1 edición) + duración ±2 s, artista distinto
    let mut m3 = meta("C:/dup3.mp3", "One More Time", "Da Pack");
    m3.duration_s = 200.0;
    db.upsert_track(&m3).unwrap();
    let mut m4 = meta("C:/dup4.mp3", "One More Times", "Da Pack II");
    m4.duration_s = 201.0;
    db.upsert_track(&m4).unwrap();
    // único
    db.upsert_track(&meta("C:/uniq.mp3", "Anthem", "Solo")).unwrap();

    let groups = db.duplicates().unwrap();
    assert_eq!(groups.len(), 2, "grupos: {groups:?}");
    let by_reason: std::collections::HashMap<&str, usize> = groups
        .iter()
        .map(|g| (g.reason.as_str(), g.tracks.len()))
        .collect();
    assert_eq!(by_reason.get("exact"), Some(&2));
    assert_eq!(by_reason.get("perceptual"), Some(&2));

    let exact = groups.iter().find(|g| g.reason == "exact").unwrap();
    let paths: Vec<&str> = exact.tracks.iter().map(|t| t.1.as_str()).collect();
    assert!(paths.contains(&"C:/dup1.mp3") && paths.contains(&"C:/dup2.mp3"));
}

#[test]
fn delete_track_limpia_todo() {
    let db = db();
    let victim = db.upsert_track(&meta("C:/gone.mp3", "Gone Love Song", "X")).unwrap();
    let other = db.upsert_track(&meta("C:/stay.mp3", "Love Remains", "Y")).unwrap();
    db.save_analysis(victim, &an(Some("8A"), Some(120.0), Some(5.0))).unwrap();
    db.set_cover(victim, Some(vec![9, 9])).unwrap();
    let pl = db.create_playlist("P", "manual", None).unwrap();
    db.add_track_to_playlist(pl.id, victim).unwrap();
    db.add_track_to_playlist(pl.id, other).unwrap();

    db.delete_track(victim).unwrap();

    assert!(db.track_full(victim).unwrap().is_none());
    assert!(db.waveform(victim).unwrap().is_none());
    assert!(db.cover(victim).unwrap().is_none());
    assert_eq!(db.playlist_track_ids(pl.id).unwrap(), vec![other]);
    let q = LibraryQuery { text: Some("love".into()), limit: 0, ..Default::default() };
    let rows = db.query(&q).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, other);
    let all = LibraryQuery { limit: 0, ..Default::default() };
    assert_eq!(db.count(&all).unwrap(), 1);
    // idempotencia de error: borrar de nuevo falla claro
    assert!(db.delete_track(victim).is_err());
}

#[test]
fn settings_y_preferences_json() {
    let db = db();
    assert_eq!(db.get_setting("nope").unwrap(), None);

    // Preferences bajo "preferences" como JSON (comparadas serializadas:
    // sm-model no deriva PartialEq para Preferences)
    let prefs = Preferences::default();
    db.set_preferences(&prefs).unwrap();
    assert_eq!(
        serde_json::to_string(&db.get_preferences().unwrap()).unwrap(),
        serde_json::to_string(&prefs).unwrap()
    );

    let mut p2 = prefs.clone();
    p2.analyze_on_import = false;
    p2.analysis_workers = 8;
    db.set_preferences(&p2).unwrap();
    let back = db.get_preferences().unwrap();
    assert!(!back.analyze_on_import);
    assert_eq!(back.analysis_workers, 8);
    assert_eq!(back.language, prefs.language);

    // set/get genérico
    db.set_setting("last_folder", "\"C:/Music\"").unwrap();
    assert_eq!(db.get_setting("last_folder").unwrap(), Some("\"C:/Music\"".into()));
    db.set_setting("last_folder", "\"C:/Media\"").unwrap(); // upsert
    assert_eq!(db.get_setting("last_folder").unwrap(), Some("\"C:/Media\"".into()));
}

#[test]
fn stats_estados_y_filtro_por_estado() {
    let db = db();
    let t1 = db.upsert_track(&meta("C:/s1.mp3", "S1", "X")).unwrap();
    let t2 = db.upsert_track(&meta("C:/s2.mp3", "S2", "X")).unwrap();
    let t3 = db.upsert_track(&meta("C:/s3.mp3", "S3", "X")).unwrap();
    let t4 = db.upsert_track(&meta("C:/s4.mp3", "S4", "X")).unwrap();

    db.save_analysis(t1, &an(Some("8A"), Some(120.0), None)).unwrap(); // done
    db.set_analysis_status(t2, AnalysisStatus::Failed).unwrap();
    db.set_analysis_status(t3, AnalysisStatus::Queued).unwrap();
    // t4 queda pending

    let s = db.stats().unwrap();
    assert_eq!(s.total, 4);
    assert_eq!(s.analyzed, 1);
    assert_eq!(s.pending, 1);
    assert_eq!(s.failed, 1);

    let failed = db.track_ids_by_status(AnalysisStatus::Failed, 100).unwrap();
    assert_eq!(failed, vec![(t2, "C:/s2.mp3".into())]);
    let pending = db.track_ids_by_status(AnalysisStatus::Pending, 100).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, t4);
    // limit
    let limited = db.track_ids_by_status(AnalysisStatus::Pending, 0).unwrap();
    assert!(limited.is_empty());

    // filtro por estado en query
    let q = LibraryQuery {
        status_filter: Some(AnalysisStatus::Done),
        limit: 0,
        ..Default::default()
    };
    let rows = db.query(&q).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, t1);
    assert_eq!(db.count(&q).unwrap(), 1);
}

#[test]
fn override_key_bpm_manual() {
    let db = db();
    let id = db.upsert_track(&meta("C:/ov.mp3", "Override", "X")).unwrap();
    db.save_analysis(id, &an(Some("8A"), Some(128.5), Some(5.0))).unwrap();

    db.override_key_bpm(id, Some("5A"), Some(100.0)).unwrap();
    let full = db.track_full(id).unwrap().unwrap();
    assert_eq!(full.analysis.key_camelot.as_deref(), Some("5A"));
    assert_eq!(full.analysis.tempo, Some(100.0));
    // lo demás intacto
    assert_eq!(full.analysis.energy, Some(5.0));

    // solo bpm
    db.override_key_bpm(id, None, Some(140.0)).unwrap();
    let full = db.track_full(id).unwrap().unwrap();
    assert_eq!(full.analysis.key_camelot.as_deref(), Some("5A"));
    assert_eq!(full.analysis.tempo, Some(140.0));

    // nada → no-op Ok
    db.override_key_bpm(id, None, None).unwrap();
    // pista inexistente → error
    assert!(db.override_key_bpm(999, Some("1A"), None).is_err());
}

#[test]
fn filtro_smart_bpm_genero_y_texto() {
    let db = db();
    let mk = |path: &str, title: &str, artist: &str, genre: &str| {
        let mut m = meta(path, title, artist);
        m.genre = genre.into();
        m
    };
    let i1 = db.upsert_track(&mk("C:/h1.mp3", "Sun", "A", "Deep House")).unwrap();
    let i2 = db.upsert_track(&mk("C:/h2.mp3", "Moon", "B", "Techno")).unwrap();
    let i3 = db.upsert_track(&mk("C:/h3.mp3", "Star", "C", "Ambient")).unwrap();
    db.save_analysis(i1, &an(Some("8A"), Some(124.0), Some(6.0))).unwrap();
    db.save_analysis(i2, &an(Some("9A"), Some(138.0), Some(7.0))).unwrap();
    db.save_analysis(i3, &an(Some("2B"), Some(90.0), Some(3.0))).unwrap();

    let q = LibraryQuery {
        smart: Some(SmartFilter {
            bpm_min: Some(120.0),
            bpm_max: Some(130.0),
            genre: Some("house".into()),
            ..Default::default()
        }),
        limit: 0,
        ..Default::default()
    };
    let rows = db.query(&q).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, i1);

    // texto dentro del smart filter
    let q2 = LibraryQuery {
        smart: Some(SmartFilter { text: Some("mo".into()), ..Default::default() }),
        limit: 0,
        ..Default::default()
    };
    assert_eq!(db.count(&q2).unwrap(), 1); // "mo*" → Moon

    // clave inválida → sin resultados
    let q3 = LibraryQuery {
        smart: Some(SmartFilter { key_compatible_with: Some("XX".into()), ..Default::default() }),
        limit: 0,
        ..Default::default()
    };
    assert_eq!(db.count(&q3).unwrap(), 0);

    let _ = (i2, i3);
}

#[test]
fn sort_por_clave_es_numerico() {
    let db = db();
    let keys = ["9A", "10A", "11A", "12A", "8A"];
    for (i, k) in keys.iter().enumerate() {
        let id = db.upsert_track(&meta(&format!("C:/key{i}.mp3"), &format!("K{i}"), "X")).unwrap();
        db.save_analysis(id, &an(Some(k), None, None)).unwrap();
    }
    let q = LibraryQuery { sort: SortField::Key, desc: false, limit: 0, ..Default::default() };
    let rows = db.query(&q).unwrap();
    let got: Vec<String> = rows.iter().filter_map(|r| r.key_camelot.clone()).collect();
    assert_eq!(got, vec!["8A", "9A", "10A", "11A", "12A"], "8 < 9 < 10 numérico");
}
