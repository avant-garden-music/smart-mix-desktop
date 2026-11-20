//! F-31: `read_junk_state` / `remove_junk` — detección y limpieza
//! conservadora de comments basura y frames iTunes (iTunSMPB/TCMP)
//! preservando TODO lo demás (título/artista/álbum/género/año).
//!
//! El seeding del fixture escribe un tag ID3v2.3 A MANO (bytes crudos,
//! chunk `id3 ` de RIFF) con TXXX iTunSMPB + TCMP + COMM basura y
//! metadatos válidos: así el test ejercita la representación EXACTA que
//! dejan iTunes/herramientas en archivos reales, sin depender del writer
//! bajo prueba (medido sobre lofty 0.18.2 con examples/diag_txxx.rs y
//! fijado en tests/txxx_preserve.rs: el guardado genérico de lofty SÍ
//! preserva los frames Unknown de id largo — EnergyLevel/iTunSMPB
//! sobreviven al re-guardado en WAV/MP3/FLAC; la creencia contraria que
//! documentaba este comentario era incorrecta).

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

/// Frame ID3v2.3: id + tamaño big-endian (NO syncsafe en v2.3) + flags.
fn frame(id: &str, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(10 + payload.len());
    f.extend_from_slice(id.as_bytes());
    f.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    f.extend_from_slice(&[0u8, 0]);
    f.extend_from_slice(payload);
    f
}

/// Frame de texto latin-1 (TIT2/TPE1/…/TCMP): encoding 0 + valor.
fn text_frame(id: &str, value: &str) -> Vec<u8> {
    let mut p = vec![0u8];
    p.extend_from_slice(value.as_bytes());
    frame(id, &p)
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

/// WAV PCM mono 16-bit 44.1kHz de 1 s con el tag ID3 en chunk `id3 `
/// (el canal RIFF que lofty lee/escribe, igual que Serato).
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
    // chunk id3 al final (como lo escribe Mixxx/Serato en RIFF); los
    // chunks impares llevan pad a par
    b.extend_from_slice(b"id3 ");
    b.extend_from_slice(&(tag.len() as u32).to_le_bytes());
    b.extend_from_slice(tag);
    if tag.len() % 2 == 1 {
        b.push(0);
    }
    fs::write(path, b).unwrap();
}

/// WAV con metadatos válidos + basura: TIT2/TPE1/TALB/TCON/TYER +
/// TXXX iTunSMPB + TCMP + COMM de tienda.
fn seed_junk_wav(path: &Path, comment: &str) {
    // TXXX: encoding 0 + descripción NUL + valor
    let mut txxx = vec![0u8];
    txxx.extend_from_slice(b"iTunSMPB\0");
    txxx.extend_from_slice(b"00000232");
    // COMM: encoding 0 + idioma "eng" + descripción NUL + texto
    let mut comm = vec![0u8];
    comm.extend_from_slice(b"eng\0");
    comm.extend_from_slice(comment.as_bytes());
    let tag = id3v23_tag(&[
        text_frame("TIT2", "Titulo Real"),
        text_frame("TPE1", "Artista Real"),
        text_frame("TALB", "Album Real"),
        text_frame("TCON", "House"),
        text_frame("TYER", "2020"),
        frame("TXXX", &txxx),
        text_frame("TCMP", "1"),
        frame("COMM", &comm),
    ]);
    write_wav_with_id3(path, &tag);
}

#[test]
fn detecta_y_limpia_basura_preservando_el_resto() {
    let dir = temp_dir("junk_tags");
    let wav = dir.join("t.wav");
    seed_junk_wav(&wav, "Purchased at Beatport");

    // detección sobre la representación real (TXXX→Unknown("iTunSMPB"))
    let st = sm_export::id3::read_junk_state(&wav).unwrap();
    assert_eq!(st.comment.as_deref(), Some("Purchased at Beatport"));
    assert!(st.itun_smpb, "iTunSMPB detectado");
    assert!(st.tcmp, "TCMP detectado");

    // limpia SOLO comments: comment fuera, frames gapless quedan,
    // metadatos intactos
    assert!(sm_export::id3::remove_junk(&wav, true, false).unwrap());
    let st2 = sm_export::id3::read_junk_state(&wav).unwrap();
    assert_eq!(st2.comment, None);
    assert!(st2.itun_smpb, "iTunSMPB conservado (política sin gapless)");
    assert!(st2.tcmp);
    {
        let tf = Probe::open(&wav).unwrap().read().unwrap();
        let tag = tf.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("Titulo Real"));
        assert_eq!(tag.artist().as_deref(), Some("Artista Real"));
        assert_eq!(tag.album().as_deref(), Some("Album Real"));
        assert_eq!(tag.genre().as_deref(), Some("House"));
        assert_eq!(tag.year(), Some(2020));
    }

    // ahora también gapless
    assert!(sm_export::id3::remove_junk(&wav, false, true).unwrap());
    let st3 = sm_export::id3::read_junk_state(&wav).unwrap();
    assert_eq!(st3.comment, None);
    assert!(!st3.itun_smpb);
    assert!(!st3.tcmp);
    {
        let tf = Probe::open(&wav).unwrap().read().unwrap();
        let tag = tf.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("Titulo Real"), "título intacto");
        assert_eq!(tag.artist().as_deref(), Some("Artista Real"), "artista intacto");
    }

    // segunda pasada: nada que borrar → Ok(false)
    assert!(!sm_export::id3::remove_junk(&wav, true, true).unwrap());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn comment_inocente_no_da_que_limpiar() {
    let dir = temp_dir("junk_ok");
    let wav = dir.join("ok.wav");
    seed_junk_wav(&wav, "mi nota");
    // sin opt-in de gapless y comment inocente: remove_junk no toca nada
    assert!(!sm_export::id3::remove_junk(&wav, true, false).unwrap());
    let st = sm_export::id3::read_junk_state(&wav).unwrap();
    assert_eq!(st.comment.as_deref(), Some("mi nota"), "el comment inocente sobrevive");
    assert!(st.itun_smpb);

    // con gapless: quita los frames itunes pero NO el comment inocente
    assert!(sm_export::id3::remove_junk(&wav, true, true).unwrap());
    let st2 = sm_export::id3::read_junk_state(&wav).unwrap();
    assert_eq!(st2.comment.as_deref(), Some("mi nota"), "comment inocente intacto");
    assert!(!st2.itun_smpb);
    assert!(!st2.tcmp);

    let _ = fs::remove_dir_all(&dir);
}
