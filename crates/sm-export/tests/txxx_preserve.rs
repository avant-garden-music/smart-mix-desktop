//! Integridad de tags AJENAS al re-guardado genérico de lofty: las TXXX
//! con descripción no mapeada (`Unknown("EnergyLevel")` de herramienta de referencia,
//! `Unknown("iTunSMPB")` de iTunes) deben SOBREVIVIR a
//! `write_meta_tags` / `write_id3_tags` (F-24 Tag Editor con
//! `tag_write_files`), junto con los campos que el editor sí escribe.
//!
//! ORIGEN: QA reportó pérdida de la TXXX `EnergyLevel` de herramienta de referencia al
//! re-guardar. Diagnosticado con `examples/diag_txxx.rs` sobre lofty
//! 0.18.2 (pineado desde el scaffold, jamás bumpeado): **NO REPRODUCE** —
//! la TXXX ajena sobrevive en WAV (latin-1 y UTF-16), MP3, FLAC y en
//! archivos reales del corpus con tags herramienta de referencia/Beatport/Serato. Estos tests
//! son el PIN de regresión de ese diagnóstico: si un bump de lofty o un
//! refactor del write path reintroduce la pérdida, falla aquí y no en
//! los archivos del usuario.
//!
//! El seeding escribe los tags A MANO (patrón de `junk_tags.rs`: el
//! writer bajo prueba no puede sembrar su propio caso):
//!   * WAV  — ID3v2.3 crudo en chunk `id3 ` de RIFF (como herramienta de referencia/Serato).
//!   * MP3  — ID3v2.3 crudo delante de un stream MPEG desnudo
//!            (`fixtures/mpeg_bare.mp3`, sin ID3 propio).
//!   * FLAC — bloque VORBIS_COMMENT crudo reemplazando el del fixture
//!            (`fixtures/sine05.flac`).

use std::fs;
use std::path::{Path, PathBuf};

use lofty::{Accessor, AudioFile, ItemKey, Probe, TaggedFileExt};

fn temp_dir(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!(
        "sm-export-{}-{}-{}",
        name,
        std::process::id(),
        nanos
    ));
    fs::create_dir_all(&d).unwrap();
    d
}

// ---------------------------------------------------------------------------
// Seeding ID3v2.3 crudo (mismo formato que deja herramienta de referencia: tamaños big-endian
// NO syncsafe en v2.3, pad a par en RIFF)
// ---------------------------------------------------------------------------

/// Frame ID3v2.3: id + tamaño big-endian + flags.
fn frame(id: &str, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(10 + payload.len());
    f.extend_from_slice(id.as_bytes());
    f.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    f.extend_from_slice(&[0u8, 0]);
    f.extend_from_slice(payload);
    f
}

/// Frame de texto latin-1: encoding 0 + valor.
fn text_frame(id: &str, value: &str) -> Vec<u8> {
    let mut p = vec![0u8];
    p.extend_from_slice(value.as_bytes());
    frame(id, &p)
}

/// TXXX latin-1 (la representación de herramienta de referencia en AIFF/WAV): encoding 0 +
/// descripción NUL + valor.
fn txxx_frame(desc: &str, value: &str) -> Vec<u8> {
    let mut p = vec![0u8];
    p.extend_from_slice(desc.as_bytes());
    p.push(0);
    p.extend_from_slice(value.as_bytes());
    frame("TXXX", &p)
}

/// TXXX UTF-16 con BOM LE (la representación de herramienta de referencia en MP3): encoding 1.
fn txxx_frame_utf16(desc: &str, value: &str) -> Vec<u8> {
    let enc = |s: &str| -> Vec<u8> {
        let mut v = vec![0xFF, 0xFE];
        for u in s.encode_utf16() {
            v.extend_from_slice(&u.to_le_bytes());
        }
        v
    };
    let mut p = vec![1u8];
    p.extend(enc(desc));
    p.extend_from_slice(&[0, 0]); // NUL terminador UTF-16
    p.extend(enc(value));
    frame("TXXX", &p)
}

/// Tag ID3v2.3 completo: cabecera (tamaño syncsafe) + frames.
fn id3v23_tag(frames: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = frames.concat();
    let mut tag = vec![b'I', b'D', b'3', 3, 0, 0];
    let sz = body.len() as u32;
    tag.extend_from_slice(&[
        (sz >> 21) as u8,
        ((sz >> 14) & 0x7f) as u8,
        ((sz >> 7) & 0x7f) as u8,
        (sz & 0x7f) as u8,
    ]);
    tag.extend_from_slice(&body);
    tag
}

/// WAV PCM mono 16-bit 44.1kHz de 1 s con el tag ID3 en chunk `id3 `.
fn write_wav_with_id3(path: &Path, tag: &[u8]) {
    const SR: u32 = 44_100;
    let data_len: u32 = SR * 2;
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len + 8 + tag.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&SR.to_le_bytes());
    b.extend_from_slice(&(SR * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..SR {
        let t = i as f32 / SR as f32;
        let s = (t * 2.0 * std::f32::consts::PI * 440.0).sin() * 8000.0;
        b.extend_from_slice(&(s as i16).to_le_bytes());
    }
    b.extend_from_slice(b"id3 ");
    b.extend_from_slice(&(tag.len() as u32).to_le_bytes());
    b.extend_from_slice(tag);
    if tag.len() % 2 == 1 {
        b.push(0);
    }
    fs::write(path, b).unwrap();
}

/// MP3 = ID3v2.3 crudo + stream MPEG desnudo del fixture (sin ID3 propio).
fn write_mp3_with_id3(path: &Path, tag: &[u8]) {
    let mut b = tag.to_vec();
    b.extend_from_slice(include_bytes!("fixtures/mpeg_bare.mp3"));
    fs::write(path, b).unwrap();
}

/// FLAC con el bloque VORBIS_COMMENT del fixture REEMPLAZADO por uno
/// crudo con `entries` (KEY=value) — seeding sin depender del writer
/// bajo prueba. Respeta los flags last-block del fixture.
fn write_flac_with_comments(path: &Path, entries: &[(&str, &str)]) {
    let fixture = include_bytes!("fixtures/sine05.flac");
    assert_eq!(&fixture[..4], b"fLaC", "fixture flac válida");
    let mut off = 4usize;
    let mut out = fixture[..off].to_vec();
    let mut replaced = false;
    loop {
        let hdr = fixture[off];
        let last = hdr & 0x80 != 0;
        let btype = hdr & 0x7f;
        let blen = ((fixture[off + 1] as usize) << 16)
            | ((fixture[off + 2] as usize) << 8)
            | fixture[off + 3] as usize;
        if btype == 4 {
            let mut payload = Vec::new();
            let vendor = b"sm-test";
            payload.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
            payload.extend_from_slice(vendor);
            payload.extend_from_slice(&(entries.len() as u32).to_le_bytes());
            for (k, v) in entries {
                let e = format!("{k}={v}");
                payload.extend_from_slice(&(e.len() as u32).to_le_bytes());
                payload.extend_from_slice(e.as_bytes());
            }
            let l = payload.len() as u32;
            out.push(btype | if last { 0x80 } else { 0 });
            out.extend_from_slice(&[(l >> 16) as u8, (l >> 8) as u8, l as u8]);
            out.extend_from_slice(&payload);
            replaced = true;
        } else {
            out.extend_from_slice(&fixture[off..off + 4 + blen]);
        }
        off += 4 + blen;
        if last {
            break;
        }
    }
    assert!(replaced, "el fixture flac debe traer bloque VORBIS_COMMENT");
    out.extend_from_slice(&fixture[off..]); // audio intacto
    fs::write(path, out).unwrap();
}

/// Tag ajeno típico de archivo del corpus: metadatos propios + TXXX
/// EnergyLevel de herramienta de referencia + TXXX iTunSMPB de iTunes.
fn seed_tag_own_plus_foreign() -> Vec<u8> {
    id3v23_tag(&[
        text_frame("TIT2", "Titulo Real"),
        text_frame("TPE1", "Artista Real"),
        text_frame("TALB", "Album Real"),
        text_frame("TCON", "House"),
        text_frame("TYER", "2020"),
        txxx_frame("EnergyLevel", "8"),
        txxx_frame("iTunSMPB", "00000232"),
    ])
}

fn read_primary(path: &Path) -> lofty::Tag {
    let tf = Probe::open(path).unwrap().read().unwrap();
    tf.primary_tag().unwrap().clone()
}

fn foreign(tag: &lofty::Tag, desc: &str) -> Option<String> {
    tag.get_string(&ItemKey::Unknown(desc.into()))
        .map(String::from)
}

// ---------------------------------------------------------------------------
// WAV (ID3v2.3 en RIFF) — write_meta_tags
// ---------------------------------------------------------------------------

#[test]
fn wav_write_meta_tags_preserva_txxx_ajenas() {
    let dir = temp_dir("txxx_wav_meta");
    let wav = dir.join("t.wav");
    write_wav_with_id3(&wav, &seed_tag_own_plus_foreign());

    sm_export::write_meta_tags(
        &wav,
        Some("Nuevo Titulo"),
        None,
        None,
        Some("Techno"),
        None,
    )
    .unwrap();

    let tag = read_primary(&wav);
    // el editor aplicó SUS campos (no un no-op silencioso)
    assert_eq!(tag.title().as_deref(), Some("Nuevo Titulo"));
    assert_eq!(tag.genre().as_deref(), Some("Techno"));
    // y las TXXX ajenas SOBREVIVEN al re-guardado
    assert_eq!(foreign(&tag, "EnergyLevel").as_deref(), Some("8"), "TXXX EnergyLevel de herramienta de referencia debe sobrevivir");
    assert_eq!(foreign(&tag, "iTunSMPB").as_deref(), Some("00000232"), "TXXX iTunSMPB debe sobrevivir");
    // el resto de metadatos propios también
    assert_eq!(tag.artist().as_deref(), Some("Artista Real"));
    assert_eq!(tag.album().as_deref(), Some("Album Real"));
    assert_eq!(tag.year(), Some(2020));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn wav_write_id3_tags_preserva_txxx_ajenas() {
    let dir = temp_dir("txxx_wav_id3");
    let wav = dir.join("t.wav");
    write_wav_with_id3(&wav, &seed_tag_own_plus_foreign());

    sm_export::write_id3_tags(&wav, "7A", 127.5, Some(6.4), "{key} | {bpm} | Energy {energy}")
        .unwrap();

    let tag = read_primary(&wav);
    // los campos del exportador se escribieron
    assert_eq!(
        tag.get_string(&ItemKey::InitialKey),
        Some("7A")
    );
    assert_eq!(tag.get_string(&ItemKey::IntegerBpm), Some("128"));
    assert_eq!(tag.comment().as_deref(), Some("7A | 128 | Energy 6"));
    // y las TXXX ajenas siguen
    assert_eq!(foreign(&tag, "EnergyLevel").as_deref(), Some("8"), "TXXX EnergyLevel de herramienta de referencia debe sobrevivir");
    assert_eq!(foreign(&tag, "iTunSMPB").as_deref(), Some("00000232"), "TXXX iTunSMPB debe sobrevivir");
    assert_eq!(tag.title().as_deref(), Some("Titulo Real"));

    fs::remove_dir_all(&dir).ok();
}

/// La TXXX de herramienta de referencia en MP3 llega en UTF-16: misma garantía de preservación.
#[test]
fn wav_write_meta_tags_preserva_txxx_utf16() {
    let dir = temp_dir("txxx_wav_utf16");
    let wav = dir.join("t.wav");
    let tag = id3v23_tag(&[
        text_frame("TIT2", "Titulo Real"),
        txxx_frame_utf16("EnergyLevel", "8"),
    ]);
    write_wav_with_id3(&wav, &tag);

    sm_export::write_meta_tags(&wav, Some("Nuevo Titulo"), None, None, None, None).unwrap();

    let tag = read_primary(&wav);
    assert_eq!(tag.title().as_deref(), Some("Nuevo Titulo"));
    assert_eq!(
        foreign(&tag, "EnergyLevel").as_deref(),
        Some("8"),
        "TXXX EnergyLevel UTF-16 debe sobrevivir"
    );

    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// MP3 (ID3v2.3 delante de MPEG real)
// ---------------------------------------------------------------------------

#[test]
fn mp3_write_paths_preservan_txxx_ajenas() {
    let dir = temp_dir("txxx_mp3");
    let mp3 = dir.join("t.mp3");
    write_mp3_with_id3(&mp3, &seed_tag_own_plus_foreign());

    sm_export::write_meta_tags(&mp3, Some("Nuevo Titulo"), None, None, None, None).unwrap();
    let tag = read_primary(&mp3);
    assert_eq!(tag.title().as_deref(), Some("Nuevo Titulo"));
    assert_eq!(foreign(&tag, "EnergyLevel").as_deref(), Some("8"), "TXXX EnergyLevel de herramienta de referencia debe sobrevivir en MP3");
    assert_eq!(foreign(&tag, "iTunSMPB").as_deref(), Some("00000232"));

    sm_export::write_id3_tags(&mp3, "7A", 127.5, Some(6.4), "{key} | {bpm}").unwrap();
    let tag = read_primary(&mp3);
    assert_eq!(tag.get_string(&ItemKey::InitialKey), Some("7A"));
    assert_eq!(tag.comment().as_deref(), Some("7A | 128"));
    assert_eq!(foreign(&tag, "EnergyLevel").as_deref(), Some("8"), "TXXX EnergyLevel sobrevive también al segundo write path");

    fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// FLAC (Vorbis comments)
// ---------------------------------------------------------------------------

#[test]
fn flac_write_meta_tags_preserva_campo_ajeno() {
    let dir = temp_dir("txxx_flac");
    let flac = dir.join("t.flac");
    write_flac_with_comments(
        &flac,
        &[
            ("TITLE", "Titulo Real"),
            ("ARTIST", "Artista Real"),
            ("EnergyLevel", "8"), // campo ajeno herramienta de referencia
        ],
    );

    sm_export::write_meta_tags(&flac, Some("Nuevo Titulo"), None, None, None, None).unwrap();

    let tag = read_primary(&flac);
    assert_eq!(tag.title().as_deref(), Some("Nuevo Titulo"));
    assert_eq!(
        foreign(&tag, "EnergyLevel").as_deref(),
        Some("8"),
        "campo vorbis ajeno EnergyLevel debe sobrevivir"
    );
    assert_eq!(tag.artist().as_deref(), Some("Artista Real"));

    fs::remove_dir_all(&dir).ok();
}
