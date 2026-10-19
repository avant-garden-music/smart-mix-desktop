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
/// * `{bpm}` → BPM redondeado a entero (mismo valor que TBPM, estilo MIK).
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
