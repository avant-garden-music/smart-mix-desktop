//! sm-export — exportadores a formatos DJ.
//!
//! Port de los exportadores de `harmonic-engine/core/tagger/` a Rust:
//!
//! * [`write_traktor_nml`] — colección Traktor NML (port de export.py).
//! * [`write_rekordbox_xml`] — DJ_PLAYLISTS de rekordbox (port de export.py).
//! * [`write_serato_crates`] — crates `_Serato_/Subcrates/*.crate` (formato
//!   TLV documentado en el proyecto; ver módulo `serato`).
//! * [`write_serato_markers`] — hot cues GEOB `Serato Markers2` DENTRO del
//!   audio mp3/aiff/wav (port de `core/tagger/serato.py`; módulo
//!   `serato_markers`, con paridad de bytes contra la referencia Python).
//! * [`write_engine_dj_library`] — EngineLibrary SQLite v2.20.1 (port fiel
//!   de engine_dj.py: `EngineLibrary/m.db` + `Database2/m.db`).
//! * [`write_m3u`] / [`write_csv`] — texto plano (CSV estilo herramienta de referencia).
//! * [`write_id3_tags`] / [`preview_id3`] — comment/TKEY/TBPM con lofty.
//! * [`plan_renames`] / [`preview_renames`] / [`apply_renames`] —
//!   renombrado de archivos estilo herramienta de referencia (F-30) con plan puro y apply
//!   explícito (port de rename.py; nunca sobrescribe).
//!
//! Todo XML se genera manualmente con `String` y escaping de
//! `& < > " '`; toda función devuelve `anyhow::Result`.

pub mod engine;
pub mod id3;
pub mod rename;
pub mod serato;
pub mod serato_markers;
pub mod text;
pub mod values;
pub mod xml;

pub use engine::write_engine_dj_library;
pub use id3::{
    preview_id3, write_id3_tags, write_meta_tags, is_junk_comment, read_junk_state, remove_junk,
    JunkTagState,
};
pub use rename::{
    apply_renames, plan_renames, preview_renames, RenameFailure, RenameInfo, RenamePattern,
    RenamePlan, RenameReport,
};
pub use serato::write_serato_crates;
pub use serato_markers::{
    build_markers2, cues_from_points, parse_markers2, read_serato_markers, write_serato_markers,
};
pub use text::{write_csv, write_m3u};
pub use xml::{write_rekordbox_xml, write_traktor_nml};

use sm_model::{BeatGrid, CuePoint};

/// Pista a exportar: datos de análisis + metadatos que viajan a cada
/// formato. Los campos `Option` ausentes simplemente no se escriben
/// (nada se inventa, misma regla que el tagger Python).
#[derive(Debug, Clone)]
pub struct ExportEntry<'a> {
    pub path: &'a str,
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    pub genre: &'a str,
    pub bpm: Option<f64>,
    /// Código Camelot (`7A`).
    pub key_camelot: Option<&'a str>,
    /// Notación musical (`D Minor`).
    pub key_name: Option<&'a str>,
    pub energy: Option<f64>,
    pub duration_s: f64,
    pub cues: &'a [CuePoint],
    pub beatgrid: Option<&'a BeatGrid>,
}

impl<'a> ExportEntry<'a> {
    /// Constructor vacío con solo path (todos los campos opcionales en
    /// `None`/`""`, duración 0, sin cues).
    pub fn new(path: &'a str) -> Self {
        Self {
            path,
            title: "",
            artist: "",
            album: "",
            genre: "",
            bpm: None,
            key_camelot: None,
            key_name: None,
            energy: None,
            duration_s: 0.0,
            cues: &[],
            beatgrid: None,
        }
    }
}
