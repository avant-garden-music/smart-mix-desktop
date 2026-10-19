//! Tests de integración de sm-export.
//!
//! Todo se escribe en un temp dir ÚNICO bajo `std::env::temp_dir()`;
//! nada fuera de él se toca.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sm_export::serato::parse_crate_paths;
use sm_export::{
    preview_id3, write_csv, write_engine_dj_library, write_id3_tags, write_m3u,
    write_rekordbox_xml, write_serato_crates, write_traktor_nml, ExportEntry,
};
use sm_model::CuePoint;
use lofty::{Accessor, AudioFile, TaggedFileExt};

// ------------------------------------------------------------- utilidades

/// Temp dir único por test (pid + nanos) — nunca colisiona entre tests.
fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!(
        "sm-export-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&d).unwrap();
    d
}

/// Valida manualmente el balance de tags XML (pila de aperturas/cierres;
/// ignora la declaración y los tags self-closing).
fn assert_balanced(xml: &str) {
    let body = xml.split_once("?>").map(|(_, r)| r).unwrap_or(xml);
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = body[i..].find('<') {
        let abs = i + rel;
        let end = abs + body[abs..].find('>').expect("tag sin '>' de cierre");
        let tag = &body[abs + 1..end];
        if !(tag.starts_with('?') || tag.starts_with('!')) {
            if let Some(name) = tag.strip_prefix('/') {
                let top = stack
                    .pop()
                    .unwrap_or_else(|| panic!("cierre </{name}> sin apertura"));
                assert_eq!(top, name.trim(), "tag desbalanceado: </{}>", name.trim());
            } else if !tag.ends_with('/') {
                stack.push(tag.split_whitespace().next().unwrap().to_string());
            }
        }
        i = end + 1;
    }
    assert!(stack.is_empty(), "tags sin cerrar: {stack:?}");
}

/// WAV PCM mono 16-bit 44.1kHz de 1 s, generado a mano (header RIFF
/// correcto que lofty puede leer y reescribir).
fn write_test_wav(path: &Path) {
    const SR: u32 = 44_100;
    let data_len: u32 = SR * 2; // 1 s * 2 bytes/muestra
    let mut b: Vec<u8> = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&SR.to_le_bytes());
    b.extend_from_slice(&(SR * 2).to_le_bytes()); // byte rate
    b.extend_from_slice(&2u16.to_le_bytes()); // block align
    b.extend_from_slice(&16u16.to_le_bytes()); // bits
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..SR {
        let t = i as f32 / SR as f32;
        let s = (t * 2.0 * std::f32::consts::PI * 440.0).sin() * 8000.0;
        b.extend_from_slice(&(s as i16).to_le_bytes());
    }
    fs::write(path, b).unwrap();
}

struct Fixture {
    dir: PathBuf,
    p1: String,
    p2: String,
    cues: Vec<CuePoint>,
}

/// Dos pistas de ejemplo con datos completos (el path 1 lleva espacios y
/// un '&' para probar escaping de XML y %20 de URL).
fn fixture(name: &str) -> Fixture {
    let dir = temp_dir(name);
    let music = dir.join("music");
    fs::create_dir_all(&music).unwrap();
    let f1 = music.join("Track One.wav");
    let f2 = music.join("backing.mp3");
    fs::write(&f1, b"dummy wav").unwrap();
    fs::write(&f2, b"dummy mp3").unwrap();
    let cues = vec![
        CuePoint {
            time_s: 12.345,
            energy_level: Some(6),
            origin: "auto".into(),
            label: None,
        },
        CuePoint {
            time_s: 45.678,
            energy_level: Some(7),
            origin: "auto".into(),
            label: None,
        },
    ];
    Fixture {
        p1: f1.to_string_lossy().into_owned(),
        p2: f2.to_string_lossy().into_owned(),
        dir,
        cues,
    }
}

impl Fixture {
    fn entries(&self) -> Vec<ExportEntry<'_>> {
        vec![
            ExportEntry {
                path: &self.p1,
                title: "Track One",
                artist: "Artist & Co",
                album: "First Album",
                genre: "House",
                bpm: Some(127.5),
                key_camelot: Some("7A"),
                key_name: Some("D Minor"),
                energy: Some(6.0),
                duration_s: 215.4,
                cues: &self.cues,
                beatgrid: None,
            },
            ExportEntry {
                path: &self.p2,
                title: "Backing",
                artist: "Other Artist",
                album: "",
                genre: "Techno",
                bpm: None,
                key_camelot: None,
                key_name: None,
                energy: None,
                duration_s: 61.0,
                cues: &[],
                beatgrid: None,
            },
        ]
    }
}

// ---------------------------------------------------------------- Traktor

#[test]
fn traktor_nml_atributos_y_balance() {
    let fx = fixture("nml");
    let out = fx.dir.join("collection.nml");
    write_traktor_nml(&fx.entries(), &out).unwrap();
    let xml = fs::read_to_string(&out).unwrap();

    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(xml.contains("<NML VERSION=\"19\">"));
    assert!(xml.contains("<COLLECTION ENTRIES=\"2\">"));
    // ENTRY + LOCATION (DIR con /: y barra final, FILE con nombre+ext)
    assert!(xml.contains("TITLE=\"Track One\""));
    assert!(xml.contains("ARTIST=\"Artist &amp; Co\"")); // escaping de &
    assert!(xml.contains("FILE=\"Track One.wav\""));
    let dir_attr = xml.split("DIR=\"").nth(1).unwrap().split('"').next().unwrap();
    assert!(dir_attr.ends_with("/:"), "DIR debe acabar en /: ({dir_attr})");
    assert!(dir_attr.contains("/:"), "DIR debe separar con /: ({dir_attr})");
    // TEMPO
    assert!(xml.contains("<TEMPO BPM=\"127.500\" BPM_QUALITY=\"100\"/>"));
    // CUE_V2: n1..n8, TYPE=4, START ms con decimales, LEN, REPEATS, HOT_CUE
    assert!(xml.contains("NAME=\"n1\""));
    assert!(xml.contains("TYPE=\"4\""));
    assert!(xml.contains("START=\"12345.000\""));
    assert!(xml.contains("LEN=\"0.000000\""));
    assert!(xml.contains("REPEATS=\"-1\""));
    assert!(xml.contains("HOT_CUE=\"1\""));
    assert!(xml.contains("NAME=\"n2\""));
    assert!(xml.contains("HOT_CUE=\"2\""));
    assert!(!xml.contains("NAME=\"n3\""));
    // XML balanceado (validación manual)
    assert_balanced(&xml);
}

// -------------------------------------------------------------- rekordbox

#[test]
fn rekordbox_xml_track_bpm_y_position_marks() {
    let fx = fixture("rekordbox");
    let out = fx.dir.join("rekordbox.xml");
    write_rekordbox_xml(&fx.entries(), &out).unwrap();
    let xml = fs::read_to_string(&out).unwrap();

    assert!(xml.contains("<DJ_PLAYLISTS VERSION=\"1.0.0\">"));
    assert!(xml.contains("<PRODUCT Name=\"Smart Mix\" Version=\"0.1.0\" Company=\"Avant Garden Music\"/>"));
    assert!(xml.contains("<COLLECTION Count=\"2\">"));
    // TrackID incremental + atributos de la pista 1
    assert!(xml.contains("TrackID=\"1\""));
    assert!(xml.contains("TrackID=\"2\""));
    assert!(xml.contains("Name=\"Track One\""));
    assert!(xml.contains("Album=\"First Album\""));
    assert!(xml.contains("Genre=\"House\""));
    assert!(xml.contains("Length=\"215\"")); // segundos enteros
    // Location: file:// con %20 para espacios
    assert!(xml.contains("Location=\"file:///"));
    assert!(xml.contains("Track%20One.wav"));
    // BPM 2 decimales y KeyName camelot
    assert!(xml.contains("BPM=\"127.50\""));
    assert!(xml.contains("AverageBpm=\"127.50\""));
    assert!(xml.contains("KeyName=\"7A\""));
    // TEMPO hijo
    assert!(xml.contains("<TEMPO Inizio=\"0.000\" Bpm=\"127.50\" Metro=\"4/4\" Battito=\"128\"/>"));
    // POSITION_MARK por cue
    assert!(xml.contains("<POSITION_MARK Name=\"CUE1\" Type=\"Cue\" Start=\"12.345\" End=\"0.000\" Num=\"1\"/>"));
    assert!(xml.contains("<POSITION_MARK Name=\"CUE2\" Type=\"Cue\" Start=\"45.678\" End=\"0.000\" Num=\"2\"/>"));
    // la pista 2 (sin BPM) no lleva KeyName/BPM
    let track2 = xml.split("<TRACK TrackID=\"2\"").nth(1).unwrap().split("</TRACK>").next().unwrap();
    assert!(!track2.contains("BPM="));
    assert!(!track2.contains("KeyName="));
    assert_balanced(&xml);
}

// ----------------------------------------------------------------- serato

#[test]
fn serato_crates_formato_tlv() {
    let fx = fixture("serato");
    let root = fx.dir.join("SeratoUSB");
    let p1_fwd = fx.p1.replace('\\', "/");
    let p2_fwd = fx.p2.replace('\\', "/");
    write_serato_crates(
        &[
            ("Warmup".into(), vec![fx.p1.clone(), fx.p2.clone()]),
            ("Deep".into(), vec![fx.p2.clone()]),
        ],
        &root,
    )
    .unwrap();

    let warmup = fs::read(root.join("_Serato_").join("Subcrates").join("Warmup.crate")).unwrap();
    let deep = fs::read(root.join("_Serato_").join("Subcrates").join("Deep.crate")).unwrap();

    // vrsn primero, versión "2.0" en UTF-16BE
    assert_eq!(&warmup[0..4], b"vrsn");
    let vlen = u32::from_be_bytes([warmup[4], warmup[5], warmup[6], warmup[7]]) as usize;
    assert_eq!(vlen, 6); // "2.0" → 3 unidades UTF-16
    assert_eq!(&warmup[8..8 + vlen], &[0x00, b'2', 0x00, b'.', 0x00, b'0']);

    // un otrk por pista con ptrk anidado (roundtrip con el parser)
    assert_eq!(parse_crate_paths(&warmup), Some(vec![p1_fwd.clone(), p2_fwd.clone()]));
    assert_eq!(parse_crate_paths(&deep), Some(vec![p2_fwd]));
    // cabecera de versión exacta re-parseada
    assert!(warmup.starts_with(b"vrsn") && deep.starts_with(b"vrsn"));
}

// ------------------------------------------------------------- engine dj

#[test]
fn engine_dj_library_sqlite_v2() {
    let fx = fixture("engine");
    // la música vive bajo la raíz (fx.dir/music) → paths relativos ../music/…
    let root = fx.dir.clone();
    write_engine_dj_library(&fx.entries(), &root).unwrap();

    let db = root.join("EngineLibrary").join("m.db");
    let db2 = root.join("Database2").join("m.db");
    assert!(db.is_file(), "falta EngineLibrary/m.db");
    assert!(db2.is_file(), "falta Database2/m.db");
    assert_eq!(fs::read(&db).unwrap(), fs::read(&db2).unwrap(), "Database2 debe ser copia");

    let conn = rusqlite::Connection::open(&db).unwrap();
    // Information declara el schema v2.20.1 con uuid
    let (maj, min, patch, uuid): (i64, i64, i64, String) = conn
        .query_row(
            "SELECT schemaVersionMajor, schemaVersionMinor, schemaVersionPatch, uuid \
             FROM Information",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!((maj, min, patch), (2, 20, 1));
    assert_eq!(uuid.len(), 36);
    assert_eq!(uuid.as_bytes()[14], b'4', "uuid debe ser v4");

    // pistas insertadas con path relativo a EngineLibrary/
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM Track", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 2);
    let (path, filename, bpm, bpm_analyzed, key, rating, title, comment, length): (
        String, String, i64, f64, i64, i64, String, String, i64,
    ) = conn
        .query_row(
            "SELECT path, filename, bpm, bpmAnalyzed, key, rating, title, comment, length \
             FROM Track WHERE filename = 'Track One.wav'",
            [],
            |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
                    r.get(6)?, r.get(7)?, r.get(8)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(path, "../music/Track One.wav");
    assert_eq!(filename, "Track One.wav");
    assert_eq!(bpm, 128);
    assert!((bpm_analyzed - 127.5).abs() < 1e-9);
    assert_eq!(key, 23); // 7A = D Minor → último del círculo Engine
    assert_eq!(rating, 60); // energy 6 × 10
    assert_eq!(title, "Track One");
    assert!(comment.contains("Camelot 7A"), "comment: {comment}");
    assert!(comment.contains("D Minor"));
    assert!(comment.contains("Energy 6"));
    assert!(comment.contains("Smart Mix"));
    assert_eq!(length, 215);

    // el trigger del schema rellenó origin*
    let (origin_uuid, origin_id): (String, i64) = conn
        .query_row(
            "SELECT originDatabaseUuid, originTrackId FROM Track \
             WHERE filename = 'Track One.wav'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(origin_uuid, uuid);
    assert!(origin_id > 0);

    // destino ya usado → error (no hay merge en v1)
    assert!(write_engine_dj_library(&fx.entries(), &root).is_err());
}

// -------------------------------------------------------------- m3u / csv

#[test]
fn m3u_y_csv() {
    let fx = fixture("m3ucsv");
    let entries = fx.entries();
    let m3u = fx.dir.join("playlist.m3u");
    let csv = fx.dir.join("collection.csv");

    write_m3u(&[fx.p1.clone(), fx.p2.clone()], &m3u).unwrap();
    let m3u_txt = fs::read_to_string(&m3u).unwrap();
    assert!(m3u_txt.starts_with("#EXTM3U\n"));
    assert_eq!(m3u_txt.lines().count(), 3);
    assert!(m3u_txt.contains(&fx.p1));
    assert!(m3u_txt.contains(&fx.p2));

    write_csv(&entries, &csv).unwrap();
    let csv_txt = fs::read_to_string(&csv).unwrap();
    let mut lines = csv_txt.lines();
    assert_eq!(
        lines.next().unwrap(),
        "Filename,Key,BPM,Energy,Title,Artist,Album,Time"
    );
    let row1 = lines.next().unwrap();
    assert!(row1.starts_with(&fx.p1), "row1 debe empezar por el path: {row1}");
    let fields: Vec<&str> = row1.split(',').collect();
    assert_eq!(fields[1], "7A");
    assert_eq!(fields[2], "128");
    assert_eq!(fields[3], "6");
    assert_eq!(fields[4], "Track One");
    assert_eq!(fields[5], "Artist & Co");
    assert_eq!(fields[6], "First Album");
    assert_eq!(fields[7], "3:35"); // 215.4 s → mm:ss
    let row2 = lines.next().unwrap();
    // sin key/bpm/energy → campos vacíos consecutivos
    assert!(row2.ends_with(",,,Backing,Other Artist,,1:01"), "row2: {row2}");
}

// -------------------------------------------------------------------- id3

#[test]
fn id3_tags_en_wav_lectura_y_reescritura() {
    let dir = temp_dir("id3");
    let wav = dir.join("tone.wav");
    write_test_wav(&wav);
    let bytes_before = fs::read(&wav).unwrap();
    assert_eq!(bytes_before.len(), 44 + 88_200);

    // preview SIN escribir: antes vacío → después con valores; no toca el archivo
    let preview = preview_id3(&wav, "7A", 127.5, Some(6.4), "{key} | {bpm} | Energy {energy}")
        .unwrap();
    assert!(preview.contains("InitialKey: <ninguno> → 7A"), "{preview}");
    assert!(preview.contains("BPM (TBPM): <ninguno> → 128"), "{preview}");
    assert!(preview.contains("Comment: <ninguno> → 7A | 128 | Energy 6"), "{preview}");
    assert_eq!(fs::read(&wav).unwrap(), bytes_before, "preview no debe escribir");

    // escritura + verificación releyendo con lofty
    write_id3_tags(&wav, "7A", 127.5, Some(6.4), "{key} | {bpm} | Energy {energy}").unwrap();
    let tf = lofty::Probe::open(&wav).unwrap().read().unwrap();
    let tag = tf.primary_tag().expect("el WAV debe tener tag ID3v2 tras escribir");
    assert_eq!(tag.get_string(&lofty::ItemKey::InitialKey), Some("7A"));
    assert_eq!(tag.get_string(&lofty::ItemKey::IntegerBpm), Some("128"));
    assert_eq!(tag.comment().unwrap().as_ref(), "7A | 128 | Energy 6");

    // el audio sigue siendo legible y con la duración original (1 s)
    let props = tf.properties();
    assert_eq!(props.duration().as_secs(), 1);

    // preview ahora muestra los valores previos
    let preview2 = preview_id3(&wav, "8B", 100.0, None, "{key} {bpm}").unwrap();
    assert!(preview2.contains("InitialKey: 7A → 8B"), "{preview2}");
    assert!(preview2.contains("BPM (TBPM): 128 → 100"), "{preview2}");
    assert!(preview2.contains("Comment: 7A | 128 | Energy 6 → 8B 100"), "{preview2}");
}
