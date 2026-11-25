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

/// Selección de filas de la tabla del studio: el conjunto de CHECKBOXES y
/// el ANCLA de rango viven juntos (shift+click integra al MISMO conjunto,
/// no una selección paralela). Funciones puras: la vista las llama en sus
/// puntos de integración (checkbox, shift+click de fila, ctrl+A, Escape) y
/// los tests fijan el contrato sin egui.
pub mod selection {
    use std::collections::BTreeSet;

    /// Checkbox simple: alterna la pista y la marca como ANCLA del rango
    /// (marcar o desmarcar: la última fila tocada por el usuario manda).
    pub fn toggle(checked: &mut BTreeSet<i64>, anchor: &mut Option<i64>, id: i64) {
        if !checked.insert(id) {
            checked.remove(&id);
        }
        *anchor = Some(id);
    }

    /// shift+click: añade a la selección el rango [ancla..target] según el
    /// ORDEN VISIBLE (ascendente o descendente, da igual). El ancla NO se
    /// mueve — shift+clicks sucesivos extienden o encogen el rango desde
    /// ella. Sin ancla (o ancla fuera de la vista actual) la fila queda
    /// sola y pasa a ser el ancla nueva: nunca se inventa un rango.
    pub fn range_add(
        checked: &mut BTreeSet<i64>,
        anchor: &mut Option<i64>,
        target: i64,
        visible: &[i64],
    ) {
        let range = anchor
            .and_then(|a| visible.iter().position(|&id| id == a))
            .and_then(|from| {
                visible
                    .iter()
                    .position(|&id| id == target)
                    .map(|to| (from, to))
            })
            .map(|(from, to)| {
                let (lo, hi) = (from.min(to), from.max(to));
                &visible[lo..=hi]
            });
        match range {
            Some(ids) => checked.extend(ids.iter().copied()),
            None => {
                // sin ancla válida: fila sola + ancla nueva
                checked.insert(target);
                *anchor = Some(target);
            }
        }
    }

    /// ctrl+A: TODA la vista visible entra a la selección (acumula sobre
    /// lo que ya había; el ancla no se toca).
    pub fn select_all(checked: &mut BTreeSet<i64>, visible: &[i64]) {
        checked.extend(visible.iter().copied());
    }

    /// Escape: selección y ancla vacías.
    pub fn clear(checked: &mut BTreeSet<i64>, anchor: &mut Option<i64>) {
        checked.clear();
        *anchor = None;
    }
}

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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagPatch {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub key: Option<Key>,
    pub bpm: Option<f32>,
    pub energy: Option<u8>,
    pub rating: Option<u8>,
}

#[derive(Clone, Debug, PartialEq)]
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
    /// Menú contextual de fila — Cargar pista: al reproductor SIN
    /// reproducir (el deck queda listo; nada suena hasta Play/Preview).
    LoadTrack(TrackId),
    /// Menú contextual de fila — Ver detalles: abre la vista Track Detail.
    ViewDetails(TrackId),
    /// Menú contextual de fila — Revelar en Explorer: SOLO abre la ventana
    /// del Explorer con el archivo seleccionado. Jamás toca archivos.
    RevealInExplorer(TrackId),
    /// Menú contextual de fila — Copiar título/artista/ruta al portapapeles.
    CopyTrackField {
        id: TrackId,
        field: TrackField,
    },
    /// Menú contextual de fila — Quitar de la biblioteca: SOLO base de
    /// datos. La app pide confirmación con conteo ANTES de tocar nada y
    /// NUNCA borra archivos de disco.
    RemoveFromLibrary(Vec<TrackId>),
}

/// Campo de pista copiable al portapapeles desde el menú contextual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackField {
    Title,
    Artist,
    Path,
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

/// Item del menú contextual de fila (click derecho). La VISIBILIDAD es pura
/// y depende SOLO de cuántas pistas abren el menú: con N>1 quedan
/// exclusivamente las acciones seguras en lote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackMenuItem {
    /// Cargar pista en el reproductor (sin reproducir) — single.
    Load,
    /// Preview: cargar y reproducir (mismo ruteo que el doble clic) — single.
    Preview,
    /// Filtrar compatibles: activa la rueda en el tono de la pista — single.
    FilterCompatible,
    EditTags,
    /// Editar energía (submenú 1..=10) — lote vía ApplyTags.
    Energy,
    /// Valoración (submenú 0..=5) — lote vía Rate por pista.
    Rating,
    /// Añadir a playlist (submenú) — lote vía AddToPlaylist.
    AddToPlaylist,
    Reanalyze,
    /// Ver detalles (Track Detail) — single.
    ViewDetails,
    /// Revelar en Explorer — single.
    Reveal,
    /// Copiar título/artista/ruta — single.
    CopyTitle,
    CopyArtist,
    CopyPath,
    /// Quitar de la biblioteca (SOLO BD, con confirmación y conteo).
    Remove,
    /// Añadir a cola de export (mismo ruteo que el '···').
    ExportQueue,
}

/// Menú de UNA pista: ítems single-only + los seguros en lote.
pub const CONTEXT_MENU_SINGLE: [TrackMenuItem; 15] = [
    TrackMenuItem::Load,
    TrackMenuItem::Preview,
    TrackMenuItem::FilterCompatible,
    TrackMenuItem::EditTags,
    TrackMenuItem::Energy,
    TrackMenuItem::Rating,
    TrackMenuItem::AddToPlaylist,
    TrackMenuItem::Reanalyze,
    TrackMenuItem::ViewDetails,
    TrackMenuItem::Reveal,
    TrackMenuItem::CopyTitle,
    TrackMenuItem::CopyArtist,
    TrackMenuItem::CopyPath,
    TrackMenuItem::Remove,
    TrackMenuItem::ExportQueue,
];

/// Menú de N>1 pistas: SOLO acciones seguras en lote — sin preview,
/// filtrar, cargar, revelar, detalles ni copiar.
pub const CONTEXT_MENU_MULTI: [TrackMenuItem; 7] = [
    TrackMenuItem::EditTags,
    TrackMenuItem::Energy,
    TrackMenuItem::Rating,
    TrackMenuItem::AddToPlaylist,
    TrackMenuItem::Reanalyze,
    TrackMenuItem::Remove,
    TrackMenuItem::ExportQueue,
];

/// Ítems VISIBLES según cuántas pistas abren el menú.
pub fn context_menu_items(target_count: usize) -> &'static [TrackMenuItem] {
    if target_count == 1 {
        &CONTEXT_MENU_SINGLE
    } else {
        &CONTEXT_MENU_MULTI
    }
}

/// Parámetro del ítem elegido (submenús de energía/valoración/playlist y
/// el tono de "filtrar compatibles").
#[derive(Clone, Copy, Debug)]
pub enum MenuParam {
    None,
    Energy(u8),
    Rating(u8),
    Playlist(i64),
    Key(Key),
}

/// Destino del ruteo COMPARTIDO de los menús de fila: el click derecho y
/// el '···' emiten EXACTAMENTE los mismos `Action` (mismo dispatcher) o
/// abren el MISMO editor de tags de la vista.
#[derive(Clone, Debug)]
pub enum MenuRoute {
    Actions(Vec<Action>),
    TagEditor(Vec<TrackId>),
}

/// Resuelve ítem + parámetros → ruteo. Devuelve `None` si el ítem es
/// single-only con N>1 (defensivo: el menú ya no lo muestra) o si el
/// parámetro no corresponde al ítem (nunca inventa rutas).
pub fn menu_route(item: TrackMenuItem, targets: &[TrackId], param: MenuParam) -> Option<MenuRoute> {
    // single-only EXIGE exactamente una pista: con N>1 el ruteo se niega
    // (el menú ya no los muestra; esto es la segunda barrera).
    let single = || targets.first().filter(|_| targets.len() == 1).copied();
    let batch = || targets.to_vec();
    let one_action = |action: Action| Some(MenuRoute::Actions(vec![action]));
    match item {
        TrackMenuItem::Load => single().map(|id| MenuRoute::Actions(vec![Action::LoadTrack(id)])),
        TrackMenuItem::Preview => {
            single().map(|id| MenuRoute::Actions(vec![Action::LoadAndPlay(id)]))
        }
        TrackMenuItem::FilterCompatible => match param {
            MenuParam::Key(key) => one_action(Action::SetCompatibility(Some(key))),
            _ => None,
        },
        TrackMenuItem::EditTags => Some(MenuRoute::TagEditor(batch())),
        TrackMenuItem::Energy => match param {
            MenuParam::Energy(level) => Some(MenuRoute::Actions(vec![Action::ApplyTags {
                track_ids: batch(),
                patch: TagPatch {
                    energy: Some(level),
                    ..TagPatch::default()
                },
            }])),
            _ => None,
        },
        TrackMenuItem::Rating => match param {
            MenuParam::Rating(rating) => {
                let actions = targets
                    .iter()
                    .map(|&track_id| Action::Rate { track_id, rating })
                    .collect();
                Some(MenuRoute::Actions(actions))
            }
            _ => None,
        },
        TrackMenuItem::AddToPlaylist => match param {
            MenuParam::Playlist(playlist_id) => one_action(Action::AddToPlaylist {
                playlist_id,
                track_ids: batch(),
            }),
            _ => None,
        },
        TrackMenuItem::Reanalyze => one_action(Action::Analyze(batch())),
        TrackMenuItem::ViewDetails => {
            single().map(|id| MenuRoute::Actions(vec![Action::ViewDetails(id)]))
        }
        TrackMenuItem::Reveal => {
            single().map(|id| MenuRoute::Actions(vec![Action::RevealInExplorer(id)]))
        }
        TrackMenuItem::CopyTitle => single().map(|id| {
            MenuRoute::Actions(vec![Action::CopyTrackField {
                id,
                field: TrackField::Title,
            }])
        }),
        TrackMenuItem::CopyArtist => single().map(|id| {
            MenuRoute::Actions(vec![Action::CopyTrackField {
                id,
                field: TrackField::Artist,
            }])
        }),
        TrackMenuItem::CopyPath => single().map(|id| {
            MenuRoute::Actions(vec![Action::CopyTrackField {
                id,
                field: TrackField::Path,
            }])
        }),
        TrackMenuItem::Remove => one_action(Action::RemoveFromLibrary(batch())),
        TrackMenuItem::ExportQueue => one_action(Action::Export(batch())),
    }
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

    // -- selección por rango (shift+click / ctrl+A / Escape): modelo puro --
    //
    // El conjunto de checkboxes y el ancla de rango viven JUNTOS: las
    // funciones son puras para poder fijar el contrato (ascendente,
    // descendente, mixto, total, vacío) antes de tocar la vista.

    fn sel() -> (std::collections::BTreeSet<TrackId>, Option<TrackId>) {
        (std::collections::BTreeSet::new(), None)
    }

    fn ids(set: &std::collections::BTreeSet<TrackId>) -> Vec<TrackId> {
        set.iter().copied().collect()
    }

    #[test]
    fn range_from_anchor_covers_asc_and_desc() {
        let visible: Vec<TrackId> = vec![1, 2, 3, 4, 5];
        // ascendente: checkbox en 2, shift+click en 5 → 2..5
        let (mut c, mut a) = sel();
        selection::toggle(&mut c, &mut a, 2);
        selection::range_add(&mut c, &mut a, 5, &visible);
        assert_eq!(ids(&c), vec![2, 3, 4, 5]);
        // descendente: ancla 5, shift+click en 2 → el MISMO rango
        let (mut c, mut a) = sel();
        selection::toggle(&mut c, &mut a, 5);
        selection::range_add(&mut c, &mut a, 2, &visible);
        assert_eq!(ids(&c), vec![2, 3, 4, 5]);
    }

    #[test]
    fn anchor_survives_mixed_checkbox_and_shift_selections() {
        // selecciones MIXTAS: checkbox 1, shift→3, checkbox 7, shift→9.
        // El rango se integra a la MISMA selección (unión, nunca la vacía).
        let visible: Vec<TrackId> = (1..=10).collect();
        let (mut c, mut a) = sel();
        selection::toggle(&mut c, &mut a, 1);
        selection::range_add(&mut c, &mut a, 3, &visible);
        assert_eq!(ids(&c), vec![1, 2, 3]);
        selection::toggle(&mut c, &mut a, 7);
        selection::range_add(&mut c, &mut a, 9, &visible);
        assert_eq!(ids(&c), vec![1, 2, 3, 7, 8, 9]);
        // el último checkbox movió el ancla: un shift posterior parte de ahí
        assert_eq!(a, Some(7));
        selection::range_add(&mut c, &mut a, 10, &visible);
        assert_eq!(ids(&c), vec![1, 2, 3, 7, 8, 9, 10]);
    }

    #[test]
    fn ctrl_a_selects_the_whole_visible_view() {
        let visible: Vec<TrackId> = vec![4, 8, 15, 16];
        let (mut c, mut a) = sel();
        selection::toggle(&mut c, &mut a, 8);
        // ctrl+A acumula la vista COMPLETA sobre lo que ya había
        selection::select_all(&mut c, &visible);
        assert_eq!(ids(&c), vec![4, 8, 15, 16]);
        // el ancla no se toca: sigue apuntando a la última fila tocada
        assert_eq!(a, Some(8));
    }

    #[test]
    fn escape_empties_selection_and_anchor() {
        let visible: Vec<TrackId> = (1..=6).collect();
        let (mut c, mut a) = sel();
        selection::toggle(&mut c, &mut a, 2);
        selection::range_add(&mut c, &mut a, 6, &visible);
        assert!(!c.is_empty());
        selection::clear(&mut c, &mut a);
        assert!(c.is_empty());
        assert_eq!(a, None);
    }

    #[test]
    fn range_without_anchor_selects_just_that_row_and_becomes_anchor() {
        let visible: Vec<TrackId> = vec![10, 20, 30];
        let (mut c, mut a) = sel();
        selection::range_add(&mut c, &mut a, 20, &visible);
        assert_eq!(ids(&c), vec![20]);
        assert_eq!(a, Some(20));
        // y un shift posterior ya tiene de dónde partir
        selection::range_add(&mut c, &mut a, 30, &visible);
        assert_eq!(ids(&c), vec![20, 30]);
    }

    #[test]
    fn stale_anchor_outside_the_view_reanchors_to_the_clicked_row() {
        // el ancla cayó fuera de la vista (filtro/colección cambió):
        // el shift+click no puede inventar un rango — ancla nueva.
        let visible: Vec<TrackId> = vec![100, 200, 300];
        let (mut c, mut a) = sel();
        a = Some(7); // 7 ya no está en la vista
        selection::range_add(&mut c, &mut a, 200, &visible);
        assert_eq!(ids(&c), vec![200]);
        assert_eq!(a, Some(200));
    }

    #[test]
    fn unchecking_keeps_the_row_as_anchor() {
        // desmarcar un checkbox también marca el ancla: el siguiente
        // shift parte de la fila que el usuario acaba de tocar.
        let visible: Vec<TrackId> = vec![1, 2, 3, 4];
        let (mut c, mut a) = sel();
        selection::toggle(&mut c, &mut a, 3);
        selection::toggle(&mut c, &mut a, 3); // desmarcar
        assert!(c.is_empty());
        assert_eq!(a, Some(3));
        selection::range_add(&mut c, &mut a, 1, &visible);
        assert_eq!(ids(&c), vec![1, 2, 3]);
    }

    // -- menú contextual de fila: visibilidad single/multi + ruteo compartido --

    #[test]
    fn single_context_menu_shows_the_full_item_list() {
        let items = context_menu_items(1);
        assert_eq!(items, &CONTEXT_MENU_SINGLE);
        assert_eq!(items.len(), 15);
        // el orden del spec: cargar, preview, filtrar… y quitar/export al final
        assert_eq!(items[0], TrackMenuItem::Load);
        assert_eq!(items[1], TrackMenuItem::Preview);
        assert_eq!(items[2], TrackMenuItem::FilterCompatible);
        assert_eq!(items[13], TrackMenuItem::Remove);
        assert_eq!(items[14], TrackMenuItem::ExportQueue);
    }

    #[test]
    fn multi_context_menu_hides_single_only_and_keeps_batch_safe() {
        let items = context_menu_items(2);
        assert_eq!(items, &CONTEXT_MENU_MULTI);
        for single_only in [
            TrackMenuItem::Load,
            TrackMenuItem::Preview,
            TrackMenuItem::FilterCompatible,
            TrackMenuItem::ViewDetails,
            TrackMenuItem::Reveal,
            TrackMenuItem::CopyTitle,
            TrackMenuItem::CopyArtist,
            TrackMenuItem::CopyPath,
        ] {
            assert!(
                !items.contains(&single_only),
                "con N>1 NO se muestra {single_only:?}"
            );
        }
        // y el lote seguro queda completo
        for batch_safe in [
            TrackMenuItem::EditTags,
            TrackMenuItem::Energy,
            TrackMenuItem::Rating,
            TrackMenuItem::AddToPlaylist,
            TrackMenuItem::Reanalyze,
            TrackMenuItem::Remove,
            TrackMenuItem::ExportQueue,
        ] {
            assert!(items.contains(&batch_safe), "falta en lote {batch_safe:?}");
        }
    }

    /// El ruteo del click derecho emite EXACTAMENTE los mismos `Action`
    /// que ya usan el menú '···' de la fila, el doble clic, la rueda y el
    /// popover de playlists — cero lógica duplicada.
    #[test]
    fn menu_routes_reuse_the_same_actions_as_the_row_menu() {
        let id = 7;
        // '···' → Re-analizar: Action::Analyze(vec![id])
        let MenuRoute::Actions(a) = menu_route(TrackMenuItem::Reanalyze, &[id], MenuParam::None)
            .expect("reanalyze debe rutear")
        else {
            panic!();
        };
        assert_eq!(a, vec![Action::Analyze(vec![id])]);
        // '···' → Exportar: Action::Export(vec![id])
        let MenuRoute::Actions(a) =
            menu_route(TrackMenuItem::ExportQueue, &[id], MenuParam::None).unwrap()
        else {
            panic!();
        };
        assert_eq!(a, vec![Action::Export(vec![id])]);
        // doble clic en fila: Action::LoadAndPlay(id)
        let MenuRoute::Actions(a) =
            menu_route(TrackMenuItem::Preview, &[id], MenuParam::None).unwrap()
        else {
            panic!();
        };
        assert_eq!(a, vec![Action::LoadAndPlay(id)]);
        // rueda armónica: Action::SetCompatibility(Some(key))
        let key: Key = "7A".parse().unwrap();
        let MenuRoute::Actions(a) =
            menu_route(TrackMenuItem::FilterCompatible, &[id], MenuParam::Key(key)).unwrap()
        else {
            panic!();
        };
        assert_eq!(a, vec![Action::SetCompatibility(Some(key))]);
        // '···' → Editar tags: abre el MISMO editor de la vista
        let MenuRoute::TagEditor(ids) =
            menu_route(TrackMenuItem::EditTags, &[id], MenuParam::None).unwrap()
        else {
            panic!();
        };
        assert_eq!(ids, vec![id]);
        // editor de tags (cierre energía editable): ApplyTags con SOLO energy
        let MenuRoute::Actions(a) =
            menu_route(TrackMenuItem::Energy, &[id], MenuParam::Energy(9)).unwrap()
        else {
            panic!();
        };
        assert_eq!(
            a,
            vec![Action::ApplyTags {
                track_ids: vec![id],
                patch: TagPatch {
                    energy: Some(9),
                    ..TagPatch::default()
                }
            }]
        );
        // columna de rating: Action::Rate por pista
        let MenuRoute::Actions(a) =
            menu_route(TrackMenuItem::Rating, &[id, 8], MenuParam::Rating(4)).unwrap()
        else {
            panic!();
        };
        assert_eq!(
            a,
            vec![
                Action::Rate {
                    track_id: id,
                    rating: 4
                },
                Action::Rate {
                    track_id: 8,
                    rating: 4
                }
            ]
        );
        // popover de playlists: AddToPlaylist con el lote completo
        let MenuRoute::Actions(a) = menu_route(
            TrackMenuItem::AddToPlaylist,
            &[id, 8],
            MenuParam::Playlist(3),
        )
        .unwrap() else {
            panic!();
        };
        assert_eq!(
            a,
            vec![Action::AddToPlaylist {
                playlist_id: 3,
                track_ids: vec![id, 8]
            }]
        );
    }

    /// Single-only con N>1: el resolver se niega (defensivo — el menú ya
    /// no los muestra, y nadie puede forzar el ruteo por detrás).
    #[test]
    fn single_only_items_refuse_to_route_for_multi_selections() {
        let targets = [5, 6, 7];
        for (item, param) in [
            (TrackMenuItem::Load, MenuParam::None),
            (TrackMenuItem::Preview, MenuParam::None),
            (TrackMenuItem::ViewDetails, MenuParam::None),
            (TrackMenuItem::Reveal, MenuParam::None),
            (TrackMenuItem::CopyTitle, MenuParam::None),
            (TrackMenuItem::CopyArtist, MenuParam::None),
            (TrackMenuItem::CopyPath, MenuParam::None),
        ] {
            assert!(
                menu_route(item, &targets, param).is_none(),
                "{item:?} no debe rutear con N>1"
            );
        }
    }

    /// Quitar de la biblioteca viaja como INTENCIÓN con los ids: la acción
    /// no borra nada por sí misma — la app pide confirmación con conteo y
    /// solo entonces toca la BD (jamás los archivos).
    #[test]
    fn remove_from_library_is_a_confirmation_request_not_a_delete() {
        let MenuRoute::Actions(a) =
            menu_route(TrackMenuItem::Remove, &[11, 22], MenuParam::None).unwrap()
        else {
            panic!();
        };
        assert_eq!(a, vec![Action::RemoveFromLibrary(vec![11, 22])]);
    }

    /// Ítems de submenú sin su parámetro (o con el parámetro equivocado):
    /// el ruteo devuelve None — nunca inventa un valor por defecto.
    #[test]
    fn submenu_items_without_their_param_do_not_route() {
        let id = 7;
        assert!(menu_route(TrackMenuItem::Energy, &[id], MenuParam::None).is_none());
        assert!(menu_route(TrackMenuItem::Rating, &[id], MenuParam::None).is_none());
        assert!(menu_route(TrackMenuItem::AddToPlaylist, &[id], MenuParam::None).is_none());
        assert!(menu_route(TrackMenuItem::FilterCompatible, &[id], MenuParam::None).is_none());
        // y un parámetro equivocado tampoco
        assert!(menu_route(TrackMenuItem::Energy, &[id], MenuParam::Rating(5)).is_none());
    }

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
