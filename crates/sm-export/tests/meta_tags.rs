//! F-24: `write_meta_tags` escribe SOLO los campos `Some(...)` en el tag
//! del archivo y preserva el resto (round-trip sobre un WAV PCM válido).

use std::fs;
use std::path::{Path, PathBuf};

use lofty::{Accessor, AudioFile, Probe, TaggedFileExt};

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

/// WAV PCM mono 16-bit 44.1kHz de 1 s (mismo generador que export.rs).
fn write_test_wav(path: &Path) {
    const SR: u32 = 44_100;
    let data_len: u32 = SR * 2;
    let mut b: Vec<u8> = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
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
    fs::write(path, b).unwrap();
}

#[test]
fn escribe_solo_campos_some_y_preserva_el_resto() {
    let dir = temp_dir("meta_tags_some");
    let wav = dir.join("t.wav");
    write_test_wav(&wav);

    sm_export::write_meta_tags(
        &wav,
        Some("Nuevo título"),
        Some("Artista"),
        Some("Álbum"),
        Some("Techno"),
        Some(1999),
    )
    .unwrap();

    let tf = Probe::open(&wav).unwrap().read().unwrap();
    let tag = tf.primary_tag().unwrap();
    assert_eq!(tag.title().as_deref(), Some("Nuevo título"));
    assert_eq!(tag.artist().as_deref(), Some("Artista"));
    assert_eq!(tag.album().as_deref(), Some("Álbum"));
    assert_eq!(tag.genre().as_deref(), Some("Techno"));
    assert_eq!(tag.year(), Some(1999));

    // segunda pasada con SOLO genre: lo demás se conserva intacto
    sm_export::write_meta_tags(&wav, None, None, None, Some("House"), None).unwrap();
    let tf = Probe::open(&wav).unwrap().read().unwrap();
    let tag = tf.primary_tag().unwrap();
    assert_eq!(tag.title().as_deref(), Some("Nuevo título"));
    assert_eq!(tag.year(), Some(1999));
    assert_eq!(tag.genre().as_deref(), Some("House"));

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn archivo_inexistente_da_error_controlado() {
    let dir = temp_dir("meta_tags_err");
    let r = sm_export::write_meta_tags(dir.join("nope.wav"), Some("x"), None, None, None, None);
    assert!(r.is_err());
    fs::remove_dir_all(&dir).ok();
}
