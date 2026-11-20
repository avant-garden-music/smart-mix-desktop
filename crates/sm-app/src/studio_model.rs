//! Modelo de la vista Studio (port del kit GPT smart-mix-hermes-kit/rust/src/model.rs).
//!
//! Puerto fiel con TRES adaptaciones deliberadas para la app real:
//! - `TrackId = i64`: los IDs REALES de sm-db (no u64 de demo).
//! - `Key::compatible_keys` delega en `crate::wheel::compatible_set` (las
//!   reglas de producción son la autoridad; el kit solo es presentación).
//! - `Action::LoadAndPlay`/`Action::SetSearch`/`Action::CancelAnalysis`:
//!   intenciones que existen en la app real (doble clic en fila, búsqueda
//!   del appbar, cancelar análisis) y que el kit standalone no tenía.
//!
//! Sin efectos: audio/FS/BD/análisis viven en la app; todo sale como Action.

use std::{fmt, str::FromStr};

pub type TrackId = i64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum KeyMode {
    Minor,
    Major,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Key {
    pub number: u8,
    pub mode: KeyMode,
}

impl Key {
    pub fn new(number: u8, mode: KeyMode) -> Option<Self> {
        (1..=12).contains(&number).then_some(Self { number, mode })
    }

    /// Vecinos de rueda en orden de presentación. Delega en las reglas de
    /// producción (`wheel::compatible_set`): mismo número ±1 misma letra +
    /// mismo número letra contraria (incluye la propia).
    pub fn compatible_keys(&self) -> [Self; 4] {
        let is_b = self.mode == KeyMode::Major;
        crate::wheel::compatible_set(self.number, is_b)
            .into_iter()
            .map(|(number, b)| Self {
                number,
                mode: if b { KeyMode::Major } else { KeyMode::Minor },
            })
            .collect::<Vec<_>>()
            .try_into()
            .ok()
            .expect("compatible_set devuelve exactamente 4 tonalidades")
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            self.number,
            match self.mode {
                KeyMode::Minor => 'A',
                KeyMode::Major => 'B',
            }
        )
    }
}

impl FromStr for Key {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_uppercase();
        let (number, mode) = if let Some(number) = normalized.strip_suffix('A') {
            (number, KeyMode::Minor)
        } else if let Some(number) = normalized.strip_suffix('B') {
            (number, KeyMode::Major)
        } else {
            return Err("La tonalidad debe terminar en A o B.".into());
        };
        number
            .parse::<u8>()
            .ok()
            .and_then(|n| Self::new(n, mode))
            .ok_or_else(|| "Usa una tonalidad entre 1A–12A o 1B–12B.".into())
    }
}

#[derive(Clone, Debug)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artist: String,
    pub key: Option<Key>,
    pub bpm: Option<f32>,
    /// Escala de presentación 1..=10 del kit. La app real guarda energía
    /// 1..=10 como f64: el adaptador redondea SOLO al presentar; el valor
    /// interno de la BD jamás se reescribe por mostrarlo.
    pub energy: Option<u8>,
    pub rating: u8,
    pub duration_seconds: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct Cue {
    pub label: char,
    pub seconds: f32,
    pub color: egui::Color32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Deck {
    A,
    B,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PitchRange {
    Six,
    Sixteen,
    Wide,
}

impl fmt::Display for Deck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::A => "A",
            Self::B => "B",
        })
    }
}

#[derive(Clone, Debug)]
pub struct Player {
    pub track: Option<Track>,
    pub deck: Deck,
    pub position_seconds: f32,
    pub playing: bool,
    pub pitch_percent: f32,
    pub pitch_range: PitchRange,
    /// Límites exactos del rango W del reproductor existente (±50%).
    pub wide_pitch_bounds: Option<(f32, f32)>,
    /// Ganancia de salida normalizada 0..1 según el host.
    pub volume: f32,
    /// Confianza normalizada 0..1; no es un re-análisis.
    pub key_confidence: Option<f32>,
    pub lufs: Option<f32>,
    pub clips: usize,
    pub format: String,
    pub cues: Vec<Cue>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            track: None,
            deck: Deck::A,
            position_seconds: 0.0,
            playing: false,
            pitch_percent: 0.0,
            pitch_range: PitchRange::Sixteen,
            wide_pitch_bounds: None,
            volume: 0.66,
            key_confidence: None,
            lufs: None,
            clips: 0,
            format: String::new(),
            cues: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub count: usize,
    /// "manual" | "smart" — decide si la playlist admite orden manual (F-19).
    pub kind: String,
    /// F-18: playlist padre (None = raíz).
    pub parent: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Collection {
    All,
    Unanalyzed,
    Duplicates,
    Playlist(i64),
}

/// Progreso del análisis de fondo (None = cola vacía).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisProgress {
    pub done: u64,
    pub total: u64,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub workspace_name: String,
    pub library_count: usize,
    pub unanalyzed_count: usize,
    /// Colección actual suministrada por el host: la app real FILTRA en la
    /// BD (compat/búsqueda/estado); la vista solo ordena lo que recibe y
    /// jamás fabrica filas con `library_count`.
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,
    pub player: Player,
    pub queue_length: usize,
    pub analysis: Option<AnalysisProgress>,
    /// Título corto de la pista EN análisis (file_stem) para el status bar
    /// rotativo (analyze-live); None con la cola en reposo.
    pub current_analyzing: Option<String>,
    pub device_labels: Vec<String>,
    pub status: String,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            workspace_name: "Smart Mix".into(),
            library_count: 0,
            unanalyzed_count: 0,
            tracks: Vec::new(),
            playlists: Vec::new(),
            player: Player::default(),
            queue_length: 0,
            analysis: None,
            current_analyzing: None,
            device_labels: Vec::new(),
            status: String::new(),
        }
    }
}

/// `None` = sin cambio. La app es la dueña de la autorización por campo y
/// de las reglas de validación del editor de tags real.
#[derive(Clone, Debug, Default)]
pub struct TagPatch {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub key: Option<Key>,
    pub bpm: Option<f32>,
    pub energy: Option<u8>,
    pub rating: Option<u8>,
}

#[derive(Clone, Debug)]
pub enum Action {
    Import,
    Analyze(Vec<TrackId>),
    ReanalyzeAll,
    /// Lista vacía = exportar la biblioteca COMPLETA (solo desde el menú
    /// explícito); el host no debe reinterpretar la vista visible.
    Export(Vec<TrackId>),
    SelectTrack(TrackId),
    /// Doble clic en fila: seleccionar y reproducir en el deck activo.
    LoadAndPlay(TrackId),
    SetCollection(Collection),
    SetCompatibility(Option<Key>),
    SetSearch(String),
    TogglePlayback,
    Seek(f32),
    SetDeck(Deck),
    SetVolume(f32),
    SetPitch(f32),
    SetPitchRange(PitchRange),
    Cue(char),
    AddCue,
    EditCues,
    CancelAnalysis,
    Rate {
        track_id: TrackId,
        rating: u8,
    },
    ApplyTags {
        track_ids: Vec<TrackId>,
        patch: TagPatch,
    },
    #[allow(dead_code)] // el sidebar abre el dialogo existente de la app
    CreatePlaylist(String),
    AddToPlaylist {
        playlist_id: i64,
        track_ids: Vec<TrackId>,
    },
    RefreshDevices,
    /// Dev: wipe TOTAL del análisis + re-análisis inmediato
    ResetAnalysisFull,
    /// Abre el diálogo de nueva playlist EXISTENTE de la app (soporta padre
    /// para sub-playlists F-18 y kind smart).
    OpenNewPlaylistDialog {
        parent: Option<i64>,
    },
    RenamePlaylist(i64),
    DeletePlaylist(i64),
    /// F-19: alterna el modo de orden manual de la playlist activa.
    ToggleManualOrder,
    /// Operaciones USB reales de la app sobre el dispositivo `index`.
    Device {
        index: usize,
        op: DeviceOp,
    },
    /// Abre la vista de Preferencias de la app.
    OpenSettings,
    /// Abre la ventana "Acerca de".
    OpenAbout,
}

/// Campo del editor de tags que recibe el foco al abrir el drawer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditField {
    Title,
    Bpm,
    Key,
    Energy,
}

/// Origen del popover de acciones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popover {
    Library,
    Track(TrackId),
    Analysis,
    Pitch,
    AddToPlaylist,
}

/// Campo de ordenación de la tabla (solo presentación: la vista ordena lo
/// que el host le entrega; no re-filtra).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortField {
    /// '#' de fila: orden ORIGINAL por id real de la pista (asc/desc).
    Number,
    Title,
    Artist,
    Key,
    Bpm,
    Energy,
    Rating,
    Duration,
}

/// Frontera de integración con el renderer de waveform EXISTENTE de la app.
///
/// Se conservan sus bandas azul/naranja/blanco, contorno continuo, layout de
/// canales, agregación de picos, ganancia/normalización, zoom, scroll,
/// overlay de cues y mapeo de coordenadas — el adaptador SOLO reserva un
/// rect y llama al renderer dentro de él.
pub trait WaveformView {
    fn show(&mut self, ui: &mut egui::Ui, rect: egui::Rect, player: &Player) -> Option<Action>;
}

/// Frontera de miniaturas de portada (columna PORTADA de la tabla).
///
/// La vista pide el arte EMBEBIDO por pista; el host resuelve caché/decode
/// (la app real enchufa su `cover_pipeline`: LRU por capacidad + worker con
/// decodificación FUERA del hilo de UI). La vista solo pide filas visibles
/// con presupuesto por frame — jamás decodifica por su cuenta.
pub trait CoverArt {
    /// Textura lista para la pista (None = aún no disponible / sin arte).
    fn cover_texture(&mut self, track_id: TrackId) -> Option<egui::TextureHandle>;
    /// Encola la decodificación (idempotente; el host deduplica).
    fn request_cover(&mut self, track_id: TrackId);
}

/// Operación sobre una unidad USB (flujos existentes de la app).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceOp {
    /// Copia de seguridad verificada (SHA-256) hacia una carpeta local.
    Backup,
    /// Flujo Safe Format (backup → FAT32 → restore, doble confirmación).
    SafeFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harmonic_wheel_wraps_and_keeps_mode() {
        let key: Key = "1A".parse().unwrap();
        let labels = key.compatible_keys().map(|key| key.to_string());
        assert_eq!(labels, ["1A", "12A", "2A", "1B"]);
        let key: Key = "12B".parse().unwrap();
        assert_eq!(
            key.compatible_keys().map(|key| key.to_string()),
            ["12B", "11B", "1B", "12A"]
        );
    }

    #[test]
    fn rejects_invalid_keys_instead_of_inventing_values() {
        for invalid in ["", "0A", "13B", "9C", "AA", "-1A", "🤷"] {
            assert!(invalid.parse::<Key>().is_err(), "accepted {invalid}");
        }
        assert_eq!(" 7a ".parse::<Key>().unwrap().to_string(), "7A");
    }

    #[test]
    fn compatible_keys_matches_production_wheel_rules() {
        // Paridad literal con wheel::compatible_set para las 24 tonalidades:
        // si la regla de producción cambia, este test obliga a revisar el port.
        for number in 1..=12u8 {
            for mode in [KeyMode::Minor, KeyMode::Major] {
                let key = Key { number, mode };
                let is_b = mode == KeyMode::Major;
                let expected: Vec<String> = crate::wheel::compatible_set(number, is_b)
                    .into_iter()
                    .map(|(n, b)| format!("{n}{}", if b { 'B' } else { 'A' }))
                    .collect();
                let got: Vec<String> = key
                    .compatible_keys()
                    .iter()
                    .map(|k| k.to_string())
                    .collect();
                assert_eq!(got, expected, "desajuste para {key}");
            }
        }
    }
}
