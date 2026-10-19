//! Valores compartidos por los exportadores: marcas de la app, mapa
//! Camelot→notación musical (port de `core/tagger/values.py`), codificación
//! Engine DJ de la tonalidad (port de `core/tagger/engine_dj.py`) y helpers
//! de escaping XML / URL.

use crate::ExportEntry;

/// Nombre del producto en los XML (rekordbox `PRODUCT@Name`, NML `HEAD`).
pub const APP_NAME: &str = "Smart Mix";
/// Versión del producto (workspace 0.1.0).
pub const APP_VERSION: &str = "0.1.0";
/// Compañía declarada en el XML de rekordbox.
pub const APP_COMPANY: &str = "Avant Garden Music";
/// Marca de software: se añade al comment del digest (como
/// `SOFTWARE_MARK` de values.py).
pub const SOFTWARE_MARK: &str = "Smart Mix 0.1.0";

/// Rueda Camelot → notación musical con bemoles (estilo Mixed In Key),
/// verbatim de `CAMELOT_TO_MUSICAL` en values.py.
const CAMELOT_TO_MUSICAL: [(&str, &str); 24] = [
    ("1A", "Ab Minor"),
    ("2A", "Eb Minor"),
    ("3A", "Bb Minor"),
    ("4A", "F Minor"),
    ("5A", "C Minor"),
    ("6A", "G Minor"),
    ("7A", "D Minor"),
    ("8A", "A Minor"),
    ("9A", "E Minor"),
    ("10A", "B Minor"),
    ("11A", "F# Minor"),
    ("12A", "Db Minor"),
    ("1B", "B Major"),
    ("2B", "F# Major"),
    ("3B", "Db Major"),
    ("4B", "Ab Major"),
    ("5B", "Eb Major"),
    ("6B", "Bb Major"),
    ("7B", "F Major"),
    ("8B", "C Major"),
    ("9B", "G Major"),
    ("10B", "D Major"),
    ("11B", "A Major"),
    ("12B", "E Major"),
];

/// `'7A'` → `'D Minor'` (None si el código no es un Camelot válido).
pub fn camelot_to_musical(code: &str) -> Option<&'static str> {
    let upper = code.trim().to_ascii_uppercase();
    CAMELOT_TO_MUSICAL
        .iter()
        .find(|(k, _)| *k == upper)
        .map(|(_, v)| *v)
}

/// Orden Engine DJ de la tonalidad (enum `musical_key` de libdjinterop):
/// círculo de quintas alternando relativo mayor/menor, 0..23.
/// Verbatim de `_ENGINE_KEY_ORDER` en engine_dj.py.
const ENGINE_KEY_ORDER: [&str; 24] = [
    "C Major",
    "A Minor",
    "G Major",
    "E Minor",
    "D Major",
    "B Minor",
    "A Major",
    "F# Minor",
    "E Major",
    "Db Minor",
    "B Major",
    "Ab Minor",
    "F# Major",
    "Eb Minor",
    "Db Major",
    "Bb Minor",
    "Ab Major",
    "F Minor",
    "Eb Major",
    "C Minor",
    "Bb Major",
    "G Minor",
    "F Major",
    "D Minor",
];

/// `'D Minor'` → índice Engine 0-23 (None si no se reconoce).
pub fn musical_to_engine_key(musical: &str) -> Option<i64> {
    let m = musical.trim().replace('\u{266f}', "#").replace('\u{266d}', "b");
    ENGINE_KEY_ORDER.iter().position(|n| *n == m).map(|i| i as i64)
}

/// camelot ('7A') o notación musical ('D Minor') → índice Engine 0-23.
///
/// Camelot tiene prioridad (misma regla que `engine_key` de engine_dj.py);
/// None cuando falta o no se reconoce (la columna queda NULL).
pub fn engine_key(camelot: Option<&str>, musical: Option<&str>) -> Option<i64> {
    if let Some(code) = camelot {
        let upper = code.trim().to_ascii_uppercase();
        if let Some((_, musical)) = CAMELOT_TO_MUSICAL.iter().find(|(k, _)| *k == upper) {
            return musical_to_engine_key(musical);
        }
    }
    musical.and_then(musical_to_engine_key)
}

/// Raíces de tonalidad → semitono (0-11 desde C), verbatim de `_ROOTS` en
/// export.py (incluye bemoles estilo 'DB'/'EB').
const ROOTS: [(&str, u32); 17] = [
    ("C", 0),
    ("C#", 1),
    ("DB", 1),
    ("D", 2),
    ("D#", 3),
    ("EB", 3),
    ("E", 4),
    ("F", 5),
    ("F#", 6),
    ("GB", 6),
    ("G", 7),
    ("G#", 8),
    ("AB", 8),
    ("A", 9),
    ("A#", 10),
    ("BB", 10),
    ("B", 11),
];

/// `'D Minor'` → 14 (MUSICAL_KEY de Traktor: 0-11 mayor, 12-23 menor, C=0).
///
/// Port de `_traktor_key_value` en export.py.
pub fn traktor_key_value(musical: &str) -> u32 {
    let upper = musical.trim().to_ascii_uppercase();
    let mut parts = upper.split_whitespace();
    let root = parts.next().unwrap_or("C");
    let semitone = ROOTS
        .iter()
        .find(|(k, _)| *k == root)
        .map(|(_, v)| *v)
        .unwrap_or(0);
    let minor = parts.any(|p| p.starts_with("MIN"));
    if minor { semitone + 12 } else { semitone }
}

/// Digest de una línea con todos los valores analizados (va donde se leen
/// comentarios). Port de `data_comment` en values.py con los campos que
/// viajan en [`ExportEntry`] (camelot, tonalidad musical, energía).
pub fn data_comment(camelot: Option<&str>, musical: Option<&str>, energy: Option<i64>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(code) = camelot.filter(|s| !s.is_empty()) {
        parts.push(format!("Camelot {code}"));
    }
    if let Some(m) = musical.filter(|s| !s.is_empty()) {
        parts.push(m.to_string());
    }
    if let Some(e) = energy {
        parts.push(format!("Energy {e}"));
    }
    parts.join(" | ")
}

/// Digest completo de una entrada (lo que va en Comments/COMMENT).
pub fn entry_comment(entry: &ExportEntry) -> String {
    let digest = data_comment(
        entry.key_camelot,
        entry.key_name,
        entry.energy.map(|e| e.round() as i64),
    );
    if digest.is_empty() {
        String::new()
    } else {
        format!("{digest} | {SOFTWARE_MARK}")
    }
}

/// Escaping XML de atributos: `& < > " '`.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Percent-encoding de un path para URL `file://`: seguros los no
/// reservados (`A-Za-z0-9-_.~`), `/` y `:` (unidad de Windows); el resto
/// como `%XX` (espacio → `%20`).
pub fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        let safe = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/' | b':');
        if safe {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Path → URL `file:///` con `%20` para espacios.
pub fn file_url(path: &str) -> String {
    let norm = path.replace('\\', "/");
    format!("file:///{}", percent_encode_path(&norm))
}

/// Extensión lower-case sin punto ("mp3", "wav").
pub fn ext_lower(path: &str) -> String {
    path.rsplit(['.', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Extensión → atributo Kind del XML de rekordbox (port de `_KIND` en
/// export.py).
pub fn app_kind(path: &str) -> &'static str {
    match ext_lower(path).as_str() {
        "mp3" => "MPEG audio",
        "aiff" | "aif" => "AIFF audio",
        "wav" => "WAVE audio",
        "flac" => "FLAC audio",
        _ => "",
    }
}

/// Último segmento del path (nombre+extensión), separadores `/` y `\`.
pub fn file_name_of(path: &str) -> String {
    let norm = path.replace('\\', "/");
    norm.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// Nombre sin extensión (stem).
pub fn file_stem_of(path: &str) -> String {
    let name = file_name_of(path);
    match name.rfind('.') {
        Some(0) | None => name,
        Some(i) => name[..i].to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camelot_maps() {
        assert_eq!(camelot_to_musical("7A"), Some("D Minor"));
        assert_eq!(camelot_to_musical("7a"), Some("D Minor"));
        assert_eq!(camelot_to_musical("8B"), Some("C Major"));
        assert_eq!(camelot_to_musical("99Z"), None);
    }

    #[test]
    fn engine_key_roundtrip() {
        // Círculo de quintas alternando: C=0, A Minor=1, D Major=4,
        // Db Minor=9, D Minor=23 (último del enum).
        assert_eq!(musical_to_engine_key("C Major"), Some(0));
        assert_eq!(musical_to_engine_key("A Minor"), Some(1));
        assert_eq!(musical_to_engine_key("D Major"), Some(4));
        assert_eq!(musical_to_engine_key("D Minor"), Some(23));
        assert_eq!(musical_to_engine_key("D♭ Minor"), Some(9)); // ♭ → b
        assert_eq!(engine_key(Some("7A"), None), Some(23)); // 7A = D Minor
        assert_eq!(engine_key(Some("8A"), None), Some(1)); // 8A = A Minor
        assert_eq!(engine_key(None, Some("A Minor")), Some(1));
        assert_eq!(engine_key(None, None), None);
    }

    #[test]
    fn traktor_key() {
        assert_eq!(traktor_key_value("C Major"), 0);
        assert_eq!(traktor_key_value("D Minor"), 14);
        assert_eq!(traktor_key_value("Db Minor"), 13);
        assert_eq!(traktor_key_value(""), 0);
    }

    #[test]
    fn escaping_y_urls() {
        assert_eq!(xml_escape("a&b<c>\"d\"'e"), "a&amp;b&lt;c&gt;&quot;d&quot;&apos;e");
        assert_eq!(file_url("C:/My Music/a b.mp3"), "file:///C:/My%20Music/a%20b.mp3");
        assert_eq!(file_name_of("C:/x/y/track.mp3"), "track.mp3");
        assert_eq!(file_stem_of("C:/x/y/track.mp3"), "track");
        assert_eq!(ext_lower("C:/X/Y.WAV"), "wav");
    }
}
