//! Exportadores de texto plano: M3U y CSV estilo Mixed In Key.

use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::ExportEntry;

/// Escribe una playlist M3U simple: `#EXTM3U` y una ruta por línea.
pub fn write_m3u(paths: &[impl AsRef<str>], out: impl AsRef<Path>) -> Result<()> {
    let mut s = String::from("#EXTM3U\n");
    for p in paths {
        s.push_str(p.as_ref());
        s.push('\n');
    }
    fs::write(out.as_ref(), s)?;
    Ok(())
}

/// Escapa un campo CSV (comillas dobles si contiene coma, comillas o salto).
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Duración en segundos → `mm:ss`.
pub fn mmss(duration_s: f64) -> String {
    let total = duration_s.max(0.0).floor() as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Escribe un CSV estilo MIK con cabecera
/// `Filename,Key,BPM,Energy,Title,Artist,Album,Time` (Time en `mm:ss`).
///
/// BPM y Energy van redondeados a entero (el estilo de los CSV de MIK);
/// los campos ausentes quedan vacíos.
pub fn write_csv(entries: &[ExportEntry], out: impl AsRef<Path>) -> Result<()> {
    let mut s = String::from("Filename,Key,BPM,Energy,Title,Artist,Album,Time\n");
    for e in entries {
        let row = [
            e.path,
            e.key_camelot.unwrap_or(""),
            &e.bpm.map(|b| format!("{}", b.round() as i64)).unwrap_or_default(),
            &e.energy.map(|x| format!("{}", x.round() as i64)).unwrap_or_default(),
            e.title,
            e.artist,
            e.album,
            &mmss(e.duration_s),
        ]
        .iter()
        .map(|f| csv_field(f))
        .collect::<Vec<_>>()
        .join(",");
        s.push_str(&row);
        s.push('\n');
    }
    fs::write(out.as_ref(), s)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mmss_formato() {
        assert_eq!(mmss(0.0), "0:00");
        assert_eq!(mmss(59.9), "0:59");
        assert_eq!(mmss(215.0), "3:35");
        assert_eq!(mmss(3661.0), "61:01");
    }

    #[test]
    fn csv_field_escapes() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
