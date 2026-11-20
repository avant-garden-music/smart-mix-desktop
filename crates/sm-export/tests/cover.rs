//! Cierre (portadas): `embedded_cover` lee el arte embebido del tag del
//! archivo (APIC en ID3v2 vía lofty) sin escribir jamás en el archivo.
//!
//! Todo se escribe en un temp dir ÚNICO bajo `std::env::temp_dir()`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use lofty::{AudioFile, MimeType, Picture, PictureType, Probe, Tag, TagType, TaggedFileExt};
use sm_export::id3::embedded_cover;

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!("sm-cover-{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&d).unwrap();
    d
}

/// WAV PCM mono 16-bit 44.1kHz de 1 s — header RIFF que lofty lee/reescribe.
fn write_test_wav(path: &Path) {
    const SR: u32 = 44_100;
    let data_len: u32 = SR * 2;
    let mut b: Vec<u8> = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&SR.to_le_bytes());
    b.extend_from_slice(&(SR * 2).to_le_bytes());
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

/// Firma PNG sintética válida para lofty (mínimo 8 bytes con firma).
fn png(bytes: &[u8]) -> Vec<u8> {
    let mut v = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend_from_slice(bytes);
    v
}

fn push_picture(path: &Path, pic_type: PictureType, data: Vec<u8>) {
    let mut tf = Probe::open(path).unwrap().read().unwrap();
    // mismo patrón que sm_export::id3 (tag_or_create): si el archivo no
    // tiene tag primario se INSERTA uno ID3v2 en el TaggedFile
    if tf.primary_tag().is_none() {
        tf.insert_tag(Tag::new(TagType::Id3v2));
    }
    let tag = tf.primary_tag_mut().expect("tag primario presente");
    tag.push_picture(Picture::new_unchecked(
        pic_type,
        Some(MimeType::Png),
        None,
        data,
    ));
    tf.save_to_path(path).unwrap();
}

#[test]
fn embedded_cover_prefiere_cover_front_sobre_otras_fotos() {
    let dir = temp_dir("front");
    let wav = dir.join("con_front.wav");
    write_test_wav(&wav);
    let front = png(b"FRONT");
    let other = png(b"OTRA");
    push_picture(&wav, PictureType::Other, other.clone());
    push_picture(&wav, PictureType::CoverFront, front.clone());
    assert_eq!(embedded_cover(&wav), Some(front));
    // y si SOLO hay una foto "otra", esa es la portada
    let wav2 = dir.join("solo_other.wav");
    write_test_wav(&wav2);
    push_picture(&wav2, PictureType::Other, other.clone());
    assert_eq!(embedded_cover(&wav2), Some(other));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn embedded_cover_sin_arte_o_archivo_ilegible_da_none() {
    let dir = temp_dir("none");
    // WAV válido sin APIC → None (la pista queda sin cover, no en error)
    let wav = dir.join("sin_arte.wav");
    write_test_wav(&wav);
    assert_eq!(embedded_cover(&wav), None);
    // archivo inexistente → None (best-effort, sin pánico)
    assert_eq!(embedded_cover(dir.join("no_existe.wav")), None);
    // bytes basura (no es audio) → None
    let junk = dir.join("junk.wav");
    fs::write(&junk, b"esto no es audio").unwrap();
    assert_eq!(embedded_cover(&junk), None);
    let _ = fs::remove_dir_all(&dir);
}
