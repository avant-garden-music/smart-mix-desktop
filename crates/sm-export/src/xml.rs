//! Exportadores XML: colección Traktor NML y rekordbox DJ_PLAYLISTS.
//!
//! XML manual con `String` y escaping de `& < > " '` (sin deps de parser):
//! los archivos son pequeños y el formato se controla byte a byte.

use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::values::{
    app_kind, entry_comment, file_name_of, file_url, traktor_key_value, xml_escape, APP_COMPANY,
    APP_NAME, APP_VERSION,
};
use crate::ExportEntry;

/// Cabecera XML común (decl + raíz la añade cada exportador).
const XML_DECL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;

/// NML: partición LOCATION en (VOLUME, DIR, FILE).
///
/// DIR estilo Traktor: directorios separados por `/:` y barra final
/// (`C:/Users/x/Music/a.mp3` → VOLUME `C:`, DIR `Users/:x/:Music/:`).
fn location_parts(path: &str) -> (String, String, String) {
    let norm = path.replace('\\', "/");
    let segs: Vec<&str> = norm.split('/').filter(|s| !s.is_empty()).collect();
    let (volume, dirs) = match segs.split_first() {
        Some((first, rest))
            if first.len() == 2
                && first.as_bytes()[0].is_ascii_alphabetic()
                && first.as_bytes()[1] == b':' =>
        {
            (first.to_string(), rest.to_vec())
        }
        _ => (String::new(), segs.clone()),
    };
    let dir = format!("{}/:", dirs.join("/:"));
    let file = file_name_of(path);
    (volume, dir, file)
}

/// Escribe una colección Traktor NML (v2 layout, `NML VERSION="19"`).
///
/// Formato:
/// ```xml
/// <NML VERSION="19">
///  <HEAD COMPANY="Smart Mix" PROGRAM="Smart Mix" VERSION="0.1.0"/>
///  <COLLECTION ENTRIES="n">
///   <ENTRY TITLE="…" ARTIST="…">
///    <LOCATION VOLUME="C:" DIR="Users/:Crist/:Music/:" FILE="a.mp3"/>
///    <INFO GENRE="…" RANKING="0" PLAYCOUNT="0" COMMENT="digest"/>
///    <TEMPO BPM="128.000" BPM_QUALITY="100"/>
///    <MUSICAL_KEY VALUE="14"/>
///    <CUES><CUE_V2 NAME="n1" DISC_ORDER="0" TYPE="4" START="1234.000"
///           LEN="0.000000" REPEATS="-1" HOT_CUE="1"/>…</CUES>
///   </ENTRY>…
///  </COLLECTION>
///  <PLAYLISTS><NODE TYPE="FOLDER" NAME="ROOT"/></PLAYLISTS>
/// </NML>
/// ```
///
/// Por cue: `CUE_V2` con NAME `n1..n8`, TYPE=4 (cue), START en milisegundos
/// con decimales, LEN=0, REPEATS=-1 y HOT_CUE=1..8 (las pistas más allá de
/// la octava repiten hot cue 8, como hace el exportador Python con
/// `min(n + 1, 8)`).
pub fn write_traktor_nml(entries: &[ExportEntry], out: impl AsRef<Path>) -> Result<()> {
    let mut x = String::new();
    x.push_str(XML_DECL);
    x.push_str(&format!("\n<NML VERSION=\"19\">\n"));
    x.push_str(&format!(
        "  <HEAD COMPANY=\"{APP_NAME}\" PROGRAM=\"{APP_NAME}\" VERSION=\"{APP_VERSION}\"/>\n"
    ));
    x.push_str(&format!("  <COLLECTION ENTRIES=\"{}\">\n", entries.len()));
    for e in entries {
        let stem = crate::values::file_stem_of(e.path);
        let title: &str = if e.title.is_empty() { stem.as_str() } else { e.title };
        x.push_str(&format!(
            "    <ENTRY TITLE=\"{}\" ARTIST=\"{}\">\n",
            xml_escape(title),
            xml_escape(e.artist)
        ));
        let (volume, dir, file) = location_parts(e.path);
        x.push_str(&format!(
            "      <LOCATION VOLUME=\"{}\" DIR=\"{}\" FILE=\"{}\"/>\n",
            xml_escape(&volume),
            xml_escape(&dir),
            xml_escape(&file)
        ));
        let comment = entry_comment(e);
        let comment_attr = if comment.is_empty() {
            String::new()
        } else {
            format!(" COMMENT=\"{}\"", xml_escape(&comment))
        };
        x.push_str(&format!(
            "      <INFO GENRE=\"{}\" RANKING=\"0\" PLAYCOUNT=\"0\"{comment_attr}/>\n",
            xml_escape(e.genre)
        ));
        if let Some(bpm) = e.bpm {
            x.push_str(&format!(
                "      <TEMPO BPM=\"{bpm:.3}\" BPM_QUALITY=\"100\"/>\n"
            ));
        }
        // MUSICAL_KEY: camelot → musical → valor 0-23 (0 si no se reconoce).
        let musical = e
            .key_name
            .map(|s| s.to_string())
            .or_else(|| crate::values::camelot_to_musical(e.key_camelot.unwrap_or("")).map(|s| s.to_string()));
        if let Some(m) = musical {
            x.push_str(&format!(
                "      <MUSICAL_KEY VALUE=\"{}\"/>\n",
                traktor_key_value(&m)
            ));
        }
        if !e.cues.is_empty() {
            x.push_str("      <CUES>\n");
            for (n, cue) in e.cues.iter().enumerate() {
                let hot = (n + 1).min(8);
                x.push_str(&format!(
                    "        <CUE_V2 NAME=\"n{hot}\" DISC_ORDER=\"0\" TYPE=\"4\" START=\"{:.3}\" LEN=\"0.000000\" REPEATS=\"-1\" HOT_CUE=\"{hot}\"/>\n",
                    cue.time_s * 1000.0
                ));
            }
            x.push_str("      </CUES>\n");
        }
        x.push_str("    </ENTRY>\n");
    }
    x.push_str("  </COLLECTION>\n");
    x.push_str("  <PLAYLISTS>\n    <NODE TYPE=\"FOLDER\" NAME=\"ROOT\"/>\n  </PLAYLISTS>\n");
    x.push_str("</NML>\n");
    fs::write(out.as_ref(), x)?;
    Ok(())
}

/// Escribe un XML de rekordbox (DJ_PLAYLISTS v1.0.0, el subconjunto mínimo
/// que rekordbox 6 importa: Location + TEMPO + POSITION_MARK).
///
/// Formato:
/// ```xml
/// <DJ_PLAYLISTS VERSION="1.0.0">
///  <PRODUCT Name="Smart Mix" Version="0.1.0" Company="Avant Garden Music"/>
///  <COLLECTION Count="n">
///   <TRACK TrackID="1" Name="…" Artist="…" Album="…" Genre="…"
///          Kind="MPEG audio" Length="215"
///          Location="file:///C:/My%20Music/a.mp3"
///          BPM="128.00" AverageBpm="128.00" KeyName="7A">
///    <TEMPO Inizio="0.000" Bpm="128.00" Metro="4/4" Battito="128"/>
///    <POSITION_MARK Name="CUE1" Type="Cue" Start="12.345" End="0.000" Num="1"/>
///   </TRACK>…
///  </COLLECTION>
///  <PLAYLISTS><NODE TYPE="FOLDER" NAME="ROOT"/></PLAYLISTS>
/// </DJ_PLAYLISTS>
/// ```
///
/// * Location: URL `file:///` con `%20` para espacios (`:` de la unidad se
///   deja literal, la forma canónica de las URL file de Windows).
/// * Length en segundos enteros; BPM con 2 decimales.
/// * Por cue un POSITION_MARK Name=`CUE1…`, Type=`Cue`, Start en segundos
///   con fracción, Num incremental.
pub fn write_rekordbox_xml(entries: &[ExportEntry], out: impl AsRef<Path>) -> Result<()> {
    let mut x = String::new();
    x.push_str(XML_DECL);
    x.push_str("\n<DJ_PLAYLISTS VERSION=\"1.0.0\">\n");
    x.push_str(&format!(
        "  <PRODUCT Name=\"{APP_NAME}\" Version=\"{APP_VERSION}\" Company=\"{APP_COMPANY}\"/>\n"
    ));
    x.push_str(&format!("  <COLLECTION Count=\"{}\">\n", entries.len()));
    for (i, e) in entries.iter().enumerate() {
        let stem = crate::values::file_stem_of(e.path);
        let title: &str = if e.title.is_empty() { stem.as_str() } else { e.title };
        let mut attrs = format!(
            "TrackID=\"{}\" Name=\"{}\" Artist=\"{}\" Album=\"{}\" Genre=\"{}\" Kind=\"{}\" Length=\"{}\" Location=\"{}\"",
            i + 1,
            xml_escape(title),
            xml_escape(e.artist),
            xml_escape(e.album),
            xml_escape(e.genre),
            xml_escape(app_kind(e.path)),
            e.duration_s.floor() as i64,
            xml_escape(&file_url(e.path)),
        );
        if let Some(bpm) = e.bpm {
            attrs.push_str(&format!(" BPM=\"{bpm:.2}\" AverageBpm=\"{bpm:.2}\""));
        }
        if let Some(key) = e.key_camelot {
            attrs.push_str(&format!(" KeyName=\"{}\"", xml_escape(key)));
        }
        x.push_str(&format!("    <TRACK {attrs}>\n"));
        if let Some(bpm) = e.bpm {
            x.push_str(&format!(
                "      <TEMPO Inizio=\"0.000\" Bpm=\"{bpm:.2}\" Metro=\"4/4\" Battito=\"{}\"/>\n",
                bpm.round() as i64
            ));
        }
        for (n, cue) in e.cues.iter().enumerate() {
            x.push_str(&format!(
                "      <POSITION_MARK Name=\"CUE{}\" Type=\"Cue\" Start=\"{:.3}\" End=\"0.000\" Num=\"{}\"/>\n",
                n + 1,
                cue.time_s,
                n + 1
            ));
        }
        x.push_str("    </TRACK>\n");
    }
    x.push_str("  </COLLECTION>\n");
    x.push_str("  <PLAYLISTS>\n    <NODE TYPE=\"FOLDER\" NAME=\"ROOT\"/>\n  </PLAYLISTS>\n");
    x.push_str("</DJ_PLAYLISTS>\n");
    fs::write(out.as_ref(), x)?;
    Ok(())
}
