//! DIAGNÓSTICO del bug de integridad reportado en QA: ¿el re-guardado
//! genérico de lofty (`write_meta_tags` / `write_id3_tags`) DROPEA frames
//! TXXX con descripción ajena no mapeada (p. ej. `EnergyLevel` de herramienta de referencia)?
//!
//! Siembra A MANO (patrón de tests/junk_tags.rs: el writer bajo prueba no
//! puede sembrar su propio caso) un WAV RIFF con chunk `id3 ` ID3v2.3 que
//! contiene:
//!   * TXXX EnergyLevel=8          (frame ajeno herramienta de referencia, el sospechoso)
//!   * TXXX iTunSMPB=00000232      (control: F-31 afirma que round-trip)
//!   * TIT2/TPE1/TALB/TCON/TYER    (metadatos propios)
//! y pasa el archivo por los write paths públicos, dumpeando TODOS los
//! items del tag genérico antes/después de cada escritura.
//!
//! La parte FLAC (Vorbis comments) es OPCIONAL: siembra con ffmpeg del
//! PATH (solo diagnóstico; los tests commitados no dependen de ffmpeg).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use lofty::{Accessor, ItemKey, Probe, TaggedFileExt};

fn temp_dir(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sm-diag-txxx-{}-{}", name, nanos));
    fs::create_dir_all(&d).unwrap();
    d
}

/// Frame ID3v2.3: id + tamaño big-endian (NO syncsafe en v2.3) + flags.
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

/// TXXX latin-1: encoding 0 + descripción NUL + valor.
fn txxx_frame(desc: &str, value: &str) -> Vec<u8> {
    let mut p = vec![0u8];
    p.extend_from_slice(desc.as_bytes());
    p.push(0);
    p.extend_from_slice(value.as_bytes());
    frame("TXXX", &p)
}

/// TXXX UTF-16 (encoding 1, BOM LE): variante de encoding de herramienta de referencia.
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
    p.extend_from_slice(&[0, 0]); // NUL UTF-16
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

/// Dumpea todos los items del tag primario (clave genérica + valor).
fn dump(label: &str, path: &Path) {
    let tf = Probe::open(path).unwrap().read().unwrap();
    let tag = tf.primary_tag().unwrap();
    println!("=== {label} ===");
    for item in tag.items() {
        println!("  {:?} = {:?}", item.key(), item.value());
    }
    let has = |k: ItemKey| tag.get_string(&k).map(String::from);
    println!(
        "  >> EnergyLevel present: {:?} | iTunSMPB present: {:?} | title: {:?}",
        has(ItemKey::Unknown("EnergyLevel".into())),
        has(ItemKey::Unknown("iTunSMPB".into())),
        tag.title()
    );
}

fn main() {
    let dir = temp_dir("run");
    println!("tmp dir: {}", dir.display());

    // --- Caso 1: WAV + ID3v2.3 sembrado a mano (latin-1) ---
    let wav = dir.join("diag.wav");
    let tag = id3v23_tag(&[
        text_frame("TIT2", "Titulo Real"),
        text_frame("TPE1", "Artista Real"),
        text_frame("TALB", "Album Real"),
        text_frame("TCON", "House"),
        text_frame("TYER", "2020"),
        txxx_frame("EnergyLevel", "8"),
        txxx_frame("iTunSMPB", "00000232"),
    ]);
    write_wav_with_id3(&wav, &tag);
    dump("WAV: BEFORE (sembrado a mano, ID3v2.3 latin-1)", &wav);

    sm_export::write_meta_tags(&wav, Some("Nuevo Titulo"), None, None, None, None).unwrap();
    dump("WAV: AFTER write_meta_tags(title)", &wav);

    sm_export::write_id3_tags(&wav, "7A", 127.5, Some(6.4), "{key} | {bpm}").unwrap();
    dump("WAV: AFTER write_id3_tags(key/bpm/comment)", &wav);

    // --- Caso 2: WAV + TXXX EnergyLevel en UTF-16 (variante de encoding) ---
    let wav16 = dir.join("diag16.wav");
    let tag16 = id3v23_tag(&[
        text_frame("TIT2", "Titulo Real"),
        txxx_frame_utf16("EnergyLevel", "8"),
    ]);
    write_wav_with_id3(&wav16, &tag16);
    dump("WAV16: BEFORE (TXXX EnergyLevel UTF-16)", &wav16);
    sm_export::write_meta_tags(&wav16, Some("Nuevo Titulo"), None, None, None, None).unwrap();
    dump("WAV16: AFTER write_meta_tags(title)", &wav16);

    // --- Caso 3 (opcional): FLAC Vorbis comments sembrado con ffmpeg ---
    let flac = dir.join("diag.flac");
    let st = Command::new("ffmpeg")
        .args([
            "-y", "-hide_banner", "-loglevel", "error",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1:sample_rate=44100",
            "-ac", "1", "-c:a", "flac",
            "-metadata", "TITLE=Titulo Real",
            "-metadata", "ARTIST=Artista Real",
            "-metadata", "EnergyLevel=8",
        ])
        .arg(&flac)
        .status();
    match st {
        Ok(s) if s.success() => {
            dump("FLAC: BEFORE (ffmpeg, Vorbis comments)", &flac);
            sm_export::write_meta_tags(&flac, Some("Nuevo Titulo"), None, None, None, None).unwrap();
            dump("FLAC: AFTER write_meta_tags(title)", &flac);
        }
        _ => println!("=== FLAC: SKIP (ffmpeg no disponible) ==="),
    }

    // --- Caso 4: variantes hostiles (valor vacío, desc con espacio,
    //     TXXX duplicada con la MISMA descripción) ---
    let wavv = dir.join("diag_variants.wav");
    let tagv = id3v23_tag(&[
        text_frame("TIT2", "Titulo Real"),
        txxx_frame("EnergyLevel", ""),           // valor vacío
        txxx_frame("Energy Level", "7"),         // desc con espacio
        txxx_frame("CuePoints", "v1=0.5"),       // dup desc #1
        txxx_frame("CuePoints", "v2=1.5"),       // dup desc #2 (mismo desc)
    ]);
    write_wav_with_id3(&wavv, &tagv);
    dump("WAV-VARIANTS: BEFORE (vacío/espacio/dup)", &wavv);
    sm_export::write_meta_tags(&wavv, Some("Nuevo Titulo"), None, None, None, None).unwrap();
    dump("WAV-VARIANTS: AFTER write_meta_tags(title)", &wavv);

    // --- Caso 5: MP3 — audio real de ffmpeg + tag ID3v2.3 crudo delante ---
    let mp3 = dir.join("diag.mp3");
    let st = Command::new("ffmpeg")
        .args([
            "-y", "-hide_banner", "-loglevel", "error",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1:sample_rate=44100",
            "-ac", "1", "-b:a", "64k", "-map_metadata", "-1",
        ])
        .arg(&mp3)
        .status();
    match st {
        Ok(s) if s.success() => {
            let audio = fs::read(&mp3).unwrap();
            let starts_id3 = audio.starts_with(b"ID3");
            println!("MP3 audio bytes: {} (empieza con ID3 propio: {starts_id3})", audio.len());
            let tag = id3v23_tag(&[
                text_frame("TIT2", "Titulo Real"),
                txxx_frame("EnergyLevel", "8"),
                txxx_frame("iTunSMPB", "00000232"),
            ]);
            let mut f = tag;
            f.extend_from_slice(&audio);
            fs::write(&mp3, f).unwrap();
            dump("MP3: BEFORE (ID3v2.3 crudo + frames MPEG)", &mp3);
            sm_export::write_meta_tags(&mp3, Some("Nuevo Titulo"), None, None, None, None).unwrap();
            dump("MP3: AFTER write_meta_tags(title)", &mp3);
            sm_export::write_id3_tags(&mp3, "7A", 127.5, Some(6.4), "{key} | {bpm}").unwrap();
            dump("MP3: AFTER write_id3_tags(key/bpm/comment)", &mp3);
        }
        _ => println!("=== MP3: SKIP (ffmpeg no disponible) ==="),
    }

    // --- Caso 6: archivos REALES del corpus del usuario (copias en tmp;
    //     los originales jamás se escriben) pasados como args ---
    for arg in std::env::args().skip(1) {
        let src = PathBuf::from(&arg);
        let dst = dir.join(format!("corpus_{}", src.file_name().unwrap().to_string_lossy()));
        fs::copy(&src, &dst).unwrap();
        dump(&format!("CORPUS {}: BEFORE", src.file_name().unwrap().to_string_lossy()), &dst);
        sm_export::write_meta_tags(&dst, Some("Nuevo Titulo"), None, None, None, None).unwrap();
        dump(&format!("CORPUS {}: AFTER write_meta_tags", src.file_name().unwrap().to_string_lossy()), &dst);
        sm_export::write_id3_tags(&dst, "7A", 127.5, Some(6.4), "{key} | {bpm}").unwrap();
        dump(&format!("CORPUS {}: AFTER write_id3_tags", src.file_name().unwrap().to_string_lossy()), &dst);
    }

    println!("dir con artefactos: {}", dir.display());
}
