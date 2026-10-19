//! sm-model — tipos de dominio compartidos de Smart Mix.
//!
//! Sin dependencias externas de I/O: solo datos puros que cruzan las
//! fronteras entre crates (motor, base de datos, exportadores, UI).

use serde::{Deserialize, Serialize};

pub mod camelot;

pub use camelot::{camelot_code, camelot_display_name, CamelotKey};

/// Metadatos de archivo/etiquetas (leídos con lofty en sm-library).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct TrackMeta {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub year: Option<i32>,
    pub track_no: Option<i32>,
    pub duration_s: f64,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub format: String,
    pub file_size: u64,
    pub file_modified_unix: i64,
}

/// Punto de cue (automático o del usuario).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CuePoint {
    pub time_s: f64,
    pub energy_level: Option<i32>,
    /// "auto" | "user"
    pub origin: String,
    pub label: Option<String>,
}

/// Segmento de energía (32 beats re-anclados a 16).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnergySegment {
    pub start_s: f64,
    pub end_s: f64,
    pub level: Option<i32>,
}

/// Beatgrid de periodo constante t_k = t0 + k·P (estructura exacta que
/// persiste MIK; ver harmonic-engine docs/mik_beats_spec.md §3.5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BeatGrid {
    pub period_s: f64,
    /// Primer beat, pertenece a (-P, 0].
    pub t0_s: f64,
    pub n_beats: usize,
    pub tempo: f64,
}

impl BeatGrid {
    pub fn beat_time(&self, k: usize) -> f64 {
        self.t0_s + k as f64 * self.period_s
    }
    pub fn beats(&self) -> impl Iterator<Item = f64> + '_ {
        (0..self.n_beats).map(move |k| self.beat_time(k))
    }
    pub fn beat_index_near(&self, t: f64) -> usize {
        let k = ((t - self.t0_s) / self.period_s).round();
        k.clamp(0.0, self.n_beats.saturating_sub(1) as f64) as usize
    }
}

/// Resultado completo del análisis de una pista (motor sm-core).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TrackAnalysis {
    // tonalidad
    pub key_camelot: Option<String>,
    pub key_name: Option<String>,
    pub key_confidence: Option<f64>,
    pub second_key: Option<String>,
    pub second_confidence: Option<f64>,
    // ritmo
    pub tempo: Option<f64>,
    pub beatgrid: Option<BeatGrid>,
    // energía
    pub energy: Option<f64>,
    pub segments: Vec<EnergySegment>,
    pub cues: Vec<CuePoint>,
    // volumen
    pub volume_db: Option<f64>,
    pub lufs: Option<f64>,
    pub clipped_peak_count: Option<i64>,
    /// Waveform de visualización: tripletes (min, max, rms) por bucket.
    pub waveform: Vec<f32>,
    pub duration_s: f64,
    /// Milisegundos que tardó el análisis completo.
    pub analysis_ms: Option<u64>,
}

impl TrackAnalysis {
    pub fn is_analyzed(&self) -> bool {
        self.key_camelot.is_some() || self.tempo.is_some()
    }
}

/// Estado de análisis persistido en la biblioteca.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStatus {
    Pending,
    Queued,
    Analyzing,
    Done,
    Failed,
}

/// Fila mínima que la tabla virtualizada pinta (sm-db la produce).
#[derive(Debug, Clone, Default)]
pub struct TrackRow {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub key_camelot: Option<String>,
    pub bpm: Option<f64>,
    pub energy: Option<f64>,
    pub duration_s: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    /// "manual" | "smart"
    pub kind: String,
    /// Para smart: filtro serializado. Para manual: None.
    pub smart_filter: Option<SmartFilter>,
}

/// Filtro de playlist inteligente (y de la vista "Compatible con").
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SmartFilter {
    pub text: Option<String>,
    pub key_compatible_with: Option<String>,
    pub energy_min: Option<f64>,
    pub energy_max: Option<f64>,
    pub bpm_min: Option<f64>,
    pub bpm_max: Option<f64>,
    pub genre: Option<String>,
}

/// Preferencias de la aplicación (persistidas en settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub language: String,
    pub analyze_on_import: bool,
    pub write_tags_id3: bool,
    pub id3_comment_format: String,
    pub export_traktor: bool,
    pub export_serato: bool,
    pub export_rekordbox: bool,
    pub analysis_workers: usize,
    pub ui_theme_dark: bool,
    pub target_device: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: "es".into(),
            analyze_on_import: true,
            write_tags_id3: false,
            id3_comment_format: "{key} | {bpm} | Energy {energy}".into(),
            export_traktor: true,
            export_serato: true,
            export_rekordbox: true,
            analysis_workers: 4,
            ui_theme_dark: true,
            target_device: String::new(),
        }
    }
}
