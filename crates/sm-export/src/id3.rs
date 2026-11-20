//! Etiquetado ID3 con lofty: comment con formato, initial key (TKEY) y
//! TBPM.
//!
//! Port del *resultado* de `core/tagger/values.py` (TKEY con notación
//! dada, TBPM entero redondeado) usando lofty como escritor de tags en
//! lugar del tagger ID3 manual del .py.

use std::path::Path;

use anyhow::Result;
use lofty::{Accessor, AudioFile, ItemKey, Probe, Tag, TagType, TaggedFileExt};

/// Expande los placeholders `{key}` `{bpm}` `{energy}` de
/// `comment_format`.
///
/// * `{key}` → la tonalidad tal cual (p. ej. `7A`).
/// * `{bpm}` → BPM redondeado a entero (mismo valor que TBPM, estilo herramienta de referencia).
/// * `{energy}` → energía redondeada a entero; cadena vacía si es `None`.
///
/// Los placeholders desconocidos se dejan tal cual. Formato por defecto de
/// la app: `"{key} | {bpm} | Energy {energy}"`.
pub fn build_comment(comment_format: &str, key: &str, bpm: f64, energy: Option<f64>) -> String {
    comment_format
        .replace("{key}", key)
        .replace("{bpm}", &format!("{}", bpm.round() as i64))
        .replace(
            "{energy}",
            &energy.map(|e| format!("{}", e.round() as i64)).unwrap_or_default(),
        )
}

/// Tag primario del archivo, creando uno ID3v2 si aún no existe.
fn tag_or_create(tf: &mut lofty::TaggedFile) -> &mut Tag {
    if tf.primary_tag().is_some() {
        tf.primary_tag_mut().expect("primary tag presente")
    } else {
        tf.insert_tag(Tag::new(TagType::Id3v2));
        tf.primary_tag_mut().expect("tag recién insertado")
    }
}

/// Lee el estado actual (antes) de los campos que escribe el exportador.
fn read_current(path: &Path) -> Result<(Option<String>, Option<String>, Option<String>)> {
    let tf = Probe::open(path)?.read()?;
    let tag = tf.primary_tag();
    Ok((
        tag.and_then(|t| t.get_string(&ItemKey::InitialKey)).map(String::from),
        tag.and_then(|t| t.get_string(&ItemKey::IntegerBpm)).map(String::from),
        tag.and_then(|t| t.comment().map(|c| c.into_owned())),
    ))
}

/// Escribe en `path`: comment según `comment_format`, initial key (TKEY) y
/// TBPM (entero redondeado, como `build_id3_frames` en values.py).
///
/// Usa lofty, que preserva el resto del archivo (para WAV escribe el chunk
/// `id3 ` de RIFF que lee Serato). Pisa los valores anteriores de esos
/// tres campos; no toca título/artista.
pub fn write_id3_tags(
    path: impl AsRef<Path>,
    key: &str,
    bpm: f64,
    energy: Option<f64>,
    comment_format: &str,
) -> Result<()> {
    let path = path.as_ref();
    let comment = build_comment(comment_format, key, bpm, energy);
    let mut tf = Probe::open(path)?.read()?;
    let tag = tag_or_create(&mut tf);
    tag.set_comment(comment);
    tag.insert_text(ItemKey::InitialKey, key.to_string());
    tag.insert_text(ItemKey::IntegerBpm, format!("{}", bpm.round() as i64));
    tf.save_to_path(path)?;
    Ok(())
}

/// Escribe SOLO los campos `Some(...)` en el tag primario del archivo
/// (título/artista/álbum/género/año) y preserva todo lo demás.
///
/// F-24 Tag Editor: misma maquinaria de escritura que [`write_id3_tags`]
/// (lofty) pero para los metadatos editables en vez de key/BPM/comment.
/// Formatos de archivo soportados aquí: mp3/aiff/wav (ID3v2) y flac
/// (Vorbis comments) — el llamador filtra por extensión.
pub fn write_meta_tags(
    path: impl AsRef<Path>,
    title: Option<&str>,
    artist: Option<&str>,
    album: Option<&str>,
    genre: Option<&str>,
    year: Option<i32>,
) -> Result<()> {
    let path = path.as_ref();
    let mut tf = Probe::open(path)?.read()?;
    let tag = tag_or_create(&mut tf);
    if let Some(v) = title {
        tag.set_title(v.to_string());
    }
    if let Some(v) = artist {
        tag.set_artist(v.to_string());
    }
    if let Some(v) = album {
        tag.set_album(v.to_string());
    }
    if let Some(v) = genre {
        tag.set_genre(v.to_string());
    }
    if let Some(y) = year {
        tag.set_year(y.clamp(0, 9999) as u32);
    }
    tf.save_to_path(path)?;
    Ok(())
}

/// Vista previa antes→después de [`write_id3_tags`] **sin escribir**.
///
/// Devuelve una línea por campo con el formato
/// `Campo: antes → después` (`antes` vacío → `<ninguno>`).
pub fn preview_id3(
    path: impl AsRef<Path>,
    key: &str,
    bpm: f64,
    energy: Option<f64>,
    comment_format: &str,
) -> Result<String> {
    let (cur_key, cur_bpm, cur_comment) = read_current(path.as_ref())?;
    let new_comment = build_comment(comment_format, key, bpm, energy);
    let new_bpm = format!("{}", bpm.round() as i64);
    let fmt = |o: &Option<String>| o.clone().unwrap_or_else(|| "<ninguno>".into());
    Ok(format!(
        "InitialKey: {} → {}\nBPM (TBPM): {} → {}\nComment: {} → {}",
        fmt(&cur_key),
        key,
        fmt(&cur_bpm),
        new_bpm,
        fmt(&cur_comment),
        new_comment
    ))
}

// ---------------------------------------------------------------------------
// F-31: limpieza de tags basura (detección + IO conservadora)
// ---------------------------------------------------------------------------

/// Firmas típicas de comments basura (case-insensitive, por subcadena).
/// Fuente única de la detección: sm-app (tag_editor) la re-exporta.
const JUNK_SUBSTRINGS: &[&str] = &[
    // tiendas / urls
    "http://", "https://", "www.",
    "beatport", "junodownload", "juno download", "traxsource", "bandcamp",
    "audiojelly", "whatpeopleplay", "djdownload", "dj download",
    "itunes", "apple music", "amazon", "google play",
    // procedencia
    "purchased", "comprado en",
    // herramientas de análisis / DJ
    "traktor", "serato", "rekordbox", "keyfinder",
    "rapid evolution", "platinum notes", "beatunes", "bpm analyzer",
    "initial key",
    // codificador
    "lame3",
];

/// true si el comment es basura detectable (url de tienda, firma de
/// herramienta, clave camelot pelada o codificador). Los comments
/// inocentes (notas del usuario) NUNCA se marcan — [`remove_junk`] se
/// niega a borrarlos aunque el llamador lo pida.
pub fn is_junk_comment(c: &str) -> bool {
    let c = c.trim();
    if c.is_empty() {
        return false;
    }
    let low = c.to_lowercase();
    if JUNK_SUBSTRINGS.iter().any(|s| low.contains(s)) {
        return true;
    }
    // clave camelot pelada ("8A", "12B"): comment de key escrito por otra
    // herramienta (la tonalidad real vive en TKEY)
    let bytes = low.as_bytes();
    let (digits, last) = bytes.split_at(bytes.len() - 1);
    let letter_ok = last == b"a" || last == b"b";
    let num_ok = (1..=2).contains(&digits.len())
        && digits.iter().all(|b| b.is_ascii_digit())
        && digits
            .iter()
            .fold(0u32, |acc, b| acc * 10 + (b - b'0') as u32)
            .wrapping_sub(1) < 12; // 1..=12
    letter_ok && num_ok
}

/// Estado de tags del archivo relevante para la limpieza F-31.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct JunkTagState {
    pub comment: Option<String>,
    /// TXXX "iTunSMPB" (tabla de gapless de iTunes).
    pub itun_smpb: bool,
    /// TCMP (flag de compilación de iTunes).
    pub tcmp: bool,
}

fn itun_smpb_key() -> ItemKey {
    ItemKey::Unknown("iTunSMPB".into())
}

/// Lee el estado de los campos que la limpieza F-31 puede quitar.
/// Formatos: mp3/aiff/wav (ID3v2) y flac (Vorbis) — la abstracción de
/// lofty mapea COMMENT/iTunSMPB/TCMP a las claves nativas de cada uno.
pub fn read_junk_state(path: impl AsRef<Path>) -> Result<JunkTagState> {
    let tf = Probe::open(path.as_ref())?.read()?;
    let tag = tf.primary_tag();
    Ok(JunkTagState {
        comment: tag.and_then(|t| t.comment().map(|c| c.into_owned())),
        itun_smpb: tag
            .is_some_and(|t| t.get_string(&itun_smpb_key()).is_some()),
        tcmp: tag
            .is_some_and(|t| t.get_string(&ItemKey::FlagCompilation).is_some()),
    })
}

/// Borra el comment basura y/o los frames iTunes (iTunSMPB/TCMP) del tag
/// primario, preservando TODO lo demás (título/artista/álbum/género/año/
/// key/bpm/cues/GEOB de Serato).
///
/// * `rm_comment` solo se aplica si el comment actual ES basura según
///   [`is_junk_comment`] (un comment inocente sobrevive por contrato).
/// * `rm_itunes_gapless` quita iTunSMPB y TCMP incondicionalmente — es un
///   opt-in explícito del usuario (borrarlo puede romper gapless).
///
/// Devuelve true si borró al menos un campo (false = no había nada).
pub fn remove_junk(
    path: impl AsRef<Path>,
    rm_comment: bool,
    rm_itunes_gapless: bool,
) -> Result<bool> {
    let path = path.as_ref();
    let mut tf = Probe::open(path)?.read()?;
    let do_comment = rm_comment
        && tf
            .primary_tag()
            .and_then(|t| t.comment())
            .is_some_and(|c| is_junk_comment(&c));
    let do_smpb = rm_itunes_gapless
        && tf
            .primary_tag()
            .is_some_and(|t| t.get_string(&itun_smpb_key()).is_some());
    let do_tcmp = rm_itunes_gapless
        && tf
            .primary_tag()
            .is_some_and(|t| t.get_string(&ItemKey::FlagCompilation).is_some());
    if !(do_comment || do_smpb || do_tcmp) {
        return Ok(false);
    }
    let tag = tf.primary_tag_mut().expect("tag presente si hay algo que borrar");
    if do_comment {
        tag.remove_key(&ItemKey::Comment);
    }
    if do_smpb {
        tag.remove_key(&itun_smpb_key());
    }
    if do_tcmp {
        tag.remove_key(&ItemKey::FlagCompilation);
    }
    tf.save_to_path(path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_placeholders() {
        let c = build_comment("{key} | {bpm} | Energy {energy}", "7A", 127.6, Some(6.4));
        assert_eq!(c, "7A | 128 | Energy 6");
        assert_eq!(build_comment("{key}-{bpm}", "1A", 100.2, None), "1A-100");
        // placeholder desconocido se conserva; energy ausente → vacío
        assert_eq!(build_comment("{key} {lufs} {energy}", "8B", 90.0, None), "8B {lufs} ");
    }
}
