//! Vista Studio — port de smart-mix-hermes-kit/rust/src/view.rs a egui 0.27.
//!
//! La vista NO tiene efectos: toda intención del usuario sale como
//! `studio_model::Action` y la app real la despacha. El waveform lo dibuja
//! EXCLUSIVAMENTE el renderer existente (widgets::WaveView) a través del
//! adaptador `WaveformView`; aquí no hay waveform de reemplazo.
//!
//! Adaptaciones 0.33 → 0.27: UiBuilder→`child_ui_with_id_source` (el hijo
//! del waveform reserva el rect SIN avanzar el cursor), id_salt→id_source,
//! CornerRadius→f32, Frame::new→Frame::none(). Componentes del kit
//! reutilizados de `crate::studio` (no duplicados).

use std::{cmp::Ordering, collections::BTreeSet};

use egui::{
    pos2, vec2, Align, Align2, Area, CentralPanel, Color32, ComboBox, Context, CursorIcon, FontId,
    Frame, Id, Label, Layout, Margin, Order, Painter, Pos2, Rect, Response, RichText, Rounding,
    ScrollArea, Sense, Shape, SidePanel, Slider, Stroke, TextEdit, TopBottomPanel, Ui, Window,
};
use egui_extras::{Column, TableBuilder};

use crate::studio;
use crate::studio_model::{
    context_menu_items, menu_route, Action, AnalysisProgress, Collection, Deck, EditField, Key,
    KeyMode, MenuParam, MenuRoute, Model, PitchRange, Popover, SortField, Track, TrackId,
    TrackMenuItem, WaveformView,
};
use crate::texts;

const PLAYER_HEIGHT: f32 = 271.0;
/// Reducción global (fix table-rb): filas 40 → 38.
const TABLE_ROW_HEIGHT: f32 = 38.0;
/// Header de la tabla: 36 → 34 (fix table-rb).
const TABLE_HEADER_HEIGHT: f32 = 34.0;
/// Fuente FINAL de las celdas de la tabla (16 → 14.5, fix table-rb). La
/// comparten título/artista/BPM y el caption de la rueda (mismo tamaño).
const TABLE_CELL_FONT: f32 = 14.5;
const DRAWER_WIDTH: f32 = 304.0;

/// k de escala del sidebar: ancho disponible / diseño base 1420, con
/// clamp 0.9–1.5 (por debajo de ~1278px no encoge más; en ultrawide
/// crece). La rueda y las listas internas usan available_width y se
/// estiran solas.
fn sidebar_k(available_width: f32) -> f32 {
    (available_width / 1420.0).clamp(0.9, 1.5)
}

/// Rango de ancho ARRASTRABLE del sidebar (fix ui-freeze): mínimo = el
/// diseño al k más bajo (0.9 → nunca más angosto que en ventana chica),
/// máximo 360px (deja ensanchar más allá del k 1.5 de ultrawide). El
/// PanelState persistido de sesiones anteriores se clampa a este rango
/// en cada carga (egui 0.27 panel.rs).
fn sidebar_width_range() -> (f32, f32) {
    (studio::SIDEBAR_W * 0.9, 360.0)
}

/// Anchos base (diseño 1420px) de las columnas de la tabla del kit. El
/// índice `COL_TITLE` es placeholder: el título absorbe el resto, así que
/// su base no se usa.
const COL_BASES: [f32; 11] = [
    28.0,  /* 0 checkbox */
    27.0,  /* 1 '#' */
    44.0,  /* 2 portada (FIJA: COL_RANGES la clava en 44) */
    0.0,   /* 3 título (absorbe el resto) */
    118.0, /* 4 artista */
    56.0,  /* 5 tonalidad */
    68.0,  /* 6 bpm */
    68.0,  /* 7 energía */
    80.0,  /* 8 rating */
    86.0,  /* 9 duración ('Duración' completa: vive en [84, 88]) */
    28.0,  /* 10 menú de fila */
];
/// Índice de la columna del título.
const COL_TITLE: usize = 3;

/// Alineación de una columna de la tabla. Cada columna la define UNA vez
/// (COL_ALIGNS) y header + celdas la comparten — estilo rekordbox: texto a
/// la izquierda, numéricas a la derecha, simbólicas al centro (fix del
/// "header a la izquierda pero contenido centrado" que el dueño leía como
/// inconsistente y confuso).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ColAlign {
    Left,
    Center,
    Right,
}

const COL_ALIGNS: [ColAlign; 11] = [
    ColAlign::Center, /* 0 checkbox */
    ColAlign::Center, /* 1 '#' */
    ColAlign::Center, /* 2 portada */
    ColAlign::Left,   /* 3 título */
    ColAlign::Left,   /* 4 artista */
    ColAlign::Center, /* 5 tonalidad */
    ColAlign::Right,  /* 6 bpm (mono, decimales alineados) */
    ColAlign::Center, /* 7 energía */
    ColAlign::Center, /* 8 rating */
    ColAlign::Right,  /* 9 duración */
    ColAlign::Center, /* 10 menú de fila */
];

/// Rango de resize [min, max] en px por columna (fix ui-rb2): SOLO el
/// título y el artista son ARRASTRABLES; el resto (checkbox, #, portada,
/// KEY, BPM, ENERGÍA, RATING, Duración, menú) son FIJAS con ancho fijo
/// del mínimo (min == max → sin asa, inmóviles). La suma
/// título+artista+Σfijas es SIEMPRE el ancho disponible de la tabla
/// (ver `table_layout`): nunca scroll horizontal, nunca columnas
/// cortadas. Los MÍNIMOS alojan el header COMPLETO sin abreviar con la
/// tipografía final — medido con el layout real de Barlow SC 14px en
/// `column_minimums_fit_full_headers`.
const COL_RANGES: [(f32, f32); 11] = [
    (24.0, 24.0),    /* 0 checkbox FIJA (control 18px + aire) */
    (24.0, 24.0),    /* 1 '#' FIJA */
    (44.0, 44.0),    /* 2 portada FIJA */
    (180.0, 2400.0), /* 3 título FLEXIBLE (absorbe el resto) */
    (80.0, 600.0),   /* 4 artista FLEXIBLE (clamp dinámico) */
    (54.0, 54.0),    /* 5 tonalidad FIJA */
    (64.0, 64.0),    /* 6 bpm FIJA */
    (76.0, 76.0),    /* 7 energía FIJA (escala de 10 swatches: 60px) */
    (68.0, 68.0),    /* 8 rating FIJA */
    (84.0, 84.0),    /* 9 duración FIJA ('Duración'/'Duration' completos) */
    (26.0, 26.0),    /* 10 menú de fila FIJA (botón 23px + aire) */
];
/// Índice de la columna del artista.
const COL_ARTIST: usize = 4;
/// Número de columnas de la tabla.
const COL_COUNT: usize = 11;

/// k de escala de la tabla: ancho disponible / diseño base 1420, con
/// clamp 0.85–1.5.
fn table_k(available_width: f32) -> f32 {
    (available_width / 1420.0).clamp(0.85, 1.5)
}

/// Ancho efectivo de una columna fija: el drag del usuario (`Some`) manda
/// sobre el escalado de ventana; si no, base·k. El rango [min, max] de
/// `COL_RANGES` se aplica SIEMPRE (fix table-rb: resize acotado — también
/// a los valores por defecto, p. ej. en ventana chica las columnas de
/// texto viajan por su mínimo, que aloja el header completo).
fn col_width(base: f32, range: (f32, f32), k: f32, user: Option<f32>) -> f32 {
    user.unwrap_or(base * k).clamp(range.0, range.1)
}

/// Ancho de columnas que egui_extras da al CONTENIDO de la tabla: el
/// disponible del ui MENOS el carril del scrollbar vertical
/// (inner_margin + bar_width + outer_margin — egui_extras lo resta
/// SIEMPRE que hay vscroll). Replicar la misma cifra garantiza que la
/// suma de las columnas llene EXACTO el inner: sin scroll horizontal,
/// sin columnas cortadas y con el scrollbar vertical pegado al borde
/// derecho de la ventana (fix ui-rb2).
fn table_available_width(ui: &Ui) -> f32 {
    let scroll = &ui.spacing().scroll;
    ui.available_width() - (scroll.bar_inner_margin + scroll.bar_width + scroll.bar_outer_margin)
}

/// Layout de la tabla (fix ui-rb2, lógica pura): SOLO título y artista
/// son flexibles; las demás columnas son FIJAS (min == max en
/// COL_RANGES). La suma título+artista+Σfijas es SIEMPRE exactamente
/// `available` — la tabla NUNCA desborda. `title_user`/`artist_user`
/// son los anchos fijados por drag del header: a lo sumo gobierna UNO
/// (el drag de una columna flexible limpia el de la otra — el último
/// drag manda; si ambos llegan Some, manda el artista). El título
/// absorbe el resto: ensanchar el artista ENCOGE el título en vivo (y
/// viceversa), con mínimos que preservan las fijas.
fn table_layout(
    available: f32,
    k: f32,
    title_user: Option<f32>,
    artist_user: Option<f32>,
) -> [f32; COL_COUNT] {
    let mut widths = [0.0f32; COL_COUNT];
    for c in 0..COL_COUNT {
        if c != COL_TITLE && c != COL_ARTIST {
            // FIJA: min == max → inmóvil ante drag y escalado
            widths[c] = COL_RANGES[c].0;
        }
    }
    let fixed_sum: f32 = widths.iter().sum();
    let budget = (available - fixed_sum).max(0.0); // lo que sobra para t+a
    let title_min = COL_RANGES[COL_TITLE].0;
    let artist_min = COL_RANGES[COL_ARTIST].0;
    let artist_max = COL_RANGES[COL_ARTIST].1;
    // snap a 0.5px para que drags subpíxel no "bailen" el layout
    let snap = |w: f32| (w * 2.0).round() * 0.5;
    let (title_w, artist_w) = if budget >= title_min + artist_min {
        if let Some(artist) = artist_user {
            // drag de ARTISTA: clampa a (available - Σfijas - título_min)
            let artist = snap(artist.clamp(artist_min, (budget - title_min).min(artist_max)));
            (budget - artist, artist)
        } else if let Some(title) = title_user {
            // drag de TÍTULO: el artista cede lo que el título gana
            let title = snap(title.clamp(title_min, budget - artist_min));
            (title, budget - title)
        } else {
            // auto: artista a base·k; el título absorbe el resto
            let artist = snap(
                col_width(COL_BASES[COL_ARTIST], COL_RANGES[COL_ARTIST], k, None)
                    .min(budget - title_min),
            );
            (budget - artist, artist)
        }
    } else {
        // ventana raquítica (budget < mínimos juntos): las fijas se
        // preservan y título/artista reparten el presupuesto proporcional
        // a sus mínimos — la suma SIGUE siendo == available
        let total_min = title_min + artist_min;
        let artist = snap(budget * artist_min / total_min);
        (budget - artist, artist)
    };
    widths[COL_TITLE] = title_w;
    widths[COL_ARTIST] = artist_w;
    widths
}

/// UI-only state. Vive entre frames como campo de `SmartMixApp`.
pub struct StudioUi {
    selected: Option<TrackId>,
    checked: BTreeSet<TrackId>,
    /// Ancla del rango de shift+click: la última fila tocada (checkbox o
    /// clic simple). Vive JUNTO a `checked` — el rango se integra a la
    /// MISMA selección de checkboxes (modelo `studio_model::selection`).
    anchor: Option<TrackId>,
    collection: Collection,
    compatibility: Option<Key>,
    sort: Option<(SortField, bool)>,
    editor: Option<TagDraft>,
    popover: Option<Popover>,
    wheel_open: bool,
    search: String,
    /// F-18: carpetas de playlists colapsadas (id → colapsada).
    playlist_collapsed: std::collections::HashMap<i64, bool>,
    lang: String,
    /// Textura del lockup oficial Avant Garden (appbar); se cachea en ctx.
    brand_tex: Option<egui::TextureHandle>,
    /// Anchos de columna de la tabla fijados por drag del usuario en el
    /// asa del header (`None` = auto: base escalado por k de ventana).
    /// Índice = posición en `COL_BASES`. Anchos PROPIOS (no egui_extras):
    /// `.resizable(false)` mata la persistencia de TableState para que el
    /// escalado por ventana siga vivo en las columnas no arrastradas.
    col_widths: Vec<Option<f32>>,
}

impl Default for StudioUi {
    fn default() -> Self {
        Self {
            selected: None,
            checked: BTreeSet::new(),
            anchor: None,
            collection: Collection::All,
            compatibility: None,
            sort: None,
            editor: None,
            popover: None,
            wheel_open: true,
            search: String::new(),
            playlist_collapsed: std::collections::HashMap::new(),
            lang: "es".into(),
            brand_tex: None,
            col_widths: Vec::new(),
        }
    }
}

/// Fila aplanada del árbol de playlists (F-18, máx 3 niveles).
struct PlaylistRow<'a> {
    playlist: &'a crate::studio_model::Playlist,
    depth: usize,
    has_children: bool,
    expanded: bool,
}

/// Aplana el árbol de playlists respetando carpetas colapsadas.
fn flatten_playlists<'a>(
    playlists: &'a [crate::studio_model::Playlist],
    collapsed: &std::collections::HashMap<i64, bool>,
) -> Vec<PlaylistRow<'a>> {
    fn walk<'a>(
        playlists: &'a [crate::studio_model::Playlist],
        collapsed: &std::collections::HashMap<i64, bool>,
        parent: Option<i64>,
        depth: usize,
        out: &mut Vec<PlaylistRow<'a>>,
    ) {
        for playlist in playlists.iter().filter(|p| p.parent == parent) {
            let has_children = playlists.iter().any(|p| p.parent == Some(playlist.id));
            let expanded = !collapsed.get(&playlist.id).copied().unwrap_or(false);
            out.push(PlaylistRow {
                playlist,
                depth,
                has_children,
                expanded,
            });
            if has_children && expanded {
                walk(playlists, collapsed, Some(playlist.id), depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(playlists, collapsed, None, 1, &mut out);
    out
}

// ------------------------------------------------- analyze-live (puro) ----
// Lógica del status bar rotativo aislada de egui para testearla sin frame.

/// Período del toggle texto↔barra: 5 s de texto, 5 s de barra.
pub(crate) const STATUS_ROTATE_SECS: f64 = 5.0;

/// true = fase BARRA (porcentaje + pista actual); false = fase TEXTO.
/// Usa el reloj de egui (`input.time`): período 2×5 s, sin estado en la UI.
pub(crate) fn analysis_phase_shows_bar(time_s: f64) -> bool {
    time_s.rem_euclid(STATUS_ROTATE_SECS * 2.0) >= STATUS_ROTATE_SECS
}

/// Porcentaje 0..=100 del lote (clamp); total 0 → 0 (nunca NaN/división
/// por cero: la barra solo se pinta con la cola activa, pero el cálculo
/// debe ser total).
pub(crate) fn analysis_percent(done: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (done as f32 / total as f32 * 100.0).clamp(0.0, 100.0)
    }
}

/// Tope de caracteres del título corto de la pista en análisis.
pub(crate) const ANALYZING_TITLE_MAX: usize = 30;

/// file_stem del path, truncado a [`ANALYZING_TITLE_MAX`] chars con '…' —
/// el status bar es una franja de 32px: jamás un nombre largo la desborda.
pub(crate) fn short_analyzing_title(path: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if stem.chars().count() > ANALYZING_TITLE_MAX {
        let cut: String = stem.chars().take(ANALYZING_TITLE_MAX - 1).collect();
        format!("{cut}…")
    } else {
        stem.to_string()
    }
}

impl StudioUi {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)] // API del kit (usada por tests)
    pub fn selected_track(&self) -> Option<TrackId> {
        self.selected
    }

    pub fn checked_tracks(&self) -> BTreeSet<TrackId> {
        self.checked.clone()
    }

    /// Vacía la selección de checkboxes y el ancla del rango (Escape, o la
    /// app tras quitar pistas de la biblioteca). Único punto de entrada
    /// para el host: mantiene el contrato del modelo de selección.
    pub fn clear_selection(&mut self) {
        crate::studio_model::selection::clear(&mut self.checked, &mut self.anchor);
    }

    #[allow(dead_code)] // API del kit
    pub fn selected_collection(&self) -> Collection {
        self.collection
    }

    #[allow(dead_code)] // API del kit
    pub fn compatibility_reference(&self) -> Option<Key> {
        self.compatibility
    }

    /// Sincroniza colección/compatibilidad desde el estado de la app
    /// (arranque, hooks SM_PL, etc.).
    pub fn sync_from_app(&mut self, collection: Collection, compatibility: Option<Key>) {
        self.collection = collection;
        self.compatibility = compatibility;
    }

    /// Dibuja un frame y devuelve sus intenciones. `draw_library == false`
    /// deja el CentralPanel libre para las vistas alternativas de la app
    /// (Settings / Tag Editor / Track Detail).
    pub fn show(
        &mut self,
        ctx: &Context,
        lang: &str,
        model: &Model,
        waveform: &mut dyn WaveformView,
        covers: &mut dyn crate::studio_model::CoverArt,
        draw_library: bool,
    ) -> Vec<Action> {
        self.lang = lang.to_string();
        let lang = self.lang.clone();
        let mut actions = Vec::new();
        let visible = self.visible_indices(model);
        if self.editor.is_none()
            && !visible
                .iter()
                .any(|&index| Some(model.tracks[index].id) == self.selected)
        {
            self.selected = visible.first().map(|&index| model.tracks[index].id);
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            if self.editor.is_some() || self.popover.is_some() {
                self.editor = None;
                self.popover = None;
            } else if !ctx.wants_keyboard_input() {
                // Escape sin overlays: vacía la selección de checkboxes y el
                // ancla del rango (mismo modelo, sin selección paralela).
                crate::studio_model::selection::clear(&mut self.checked, &mut self.anchor);
            }
        }
        // ctrl+A: TODA la vista visible entra a la MISMA selección de
        // checkboxes. Con un campo de texto enfocado (búsqueda) egui pide
        // el teclado → el atajo le pertenece (seleccionar todo el texto).
        if !ctx.wants_keyboard_input()
            && ctx.input(|input| input.key_pressed(egui::Key::A) && input.modifiers.ctrl)
        {
            let visible_ids: Vec<TrackId> = visible
                .iter()
                .map(|&index| model.tracks[index].id)
                .collect();
            crate::studio_model::selection::select_all(&mut self.checked, &visible_ids);
            self.editor = None;
        }

        TopBottomPanel::top("smart_mix_studio_appbar")
            .exact_height(studio::APPBAR_H)
            .frame(
                Frame::none()
                    .fill(studio::CHROME)
                    .inner_margin(Margin::symmetric(18.0, 9.0)),
            )
            .show(ctx, |ui| self.appbar(ui, &lang, model, &mut actions));

        TopBottomPanel::top("smart_mix_studio_player")
            .exact_height(PLAYER_HEIGHT)
            .frame(
                Frame::none()
                    .fill(studio::BG)
                    .inner_margin(Margin::symmetric(20.0, 0.0)),
            )
            .show(ctx, |ui| {
                self.player(ui, &lang, model, waveform, covers, &mut actions)
            });

        TopBottomPanel::bottom("smart_mix_studio_status")
            .exact_height(studio::STATUS_H)
            .frame(
                Frame::none()
                    .fill(studio::CHROME)
                    .inner_margin(Margin::symmetric(15.0, 4.0)),
            )
            .show(ctx, |ui| self.status_line(ui, &lang, model, &mut actions));

        // RESPONSIVO: el sidebar escala con la ventana (k sobre el diseño
        // base 1420, clamp 0.9–1.5) — calculado ANTES del panel sobre el
        // rect que la ventana realmente ofrece. RESIZABLE (fix ui-freeze):
        // el borde derecho se arrastra (interacción nativa de egui sobre el
        // contenido, con cursor y línea guía); default = SIDEBAR_W·k y el
        // rango vive en `sidebar_width_range`. La rueda y el árbol usan
        // available_width → se adaptan solos (rueda con tope 202 centrada).
        let sidebar_w = studio::SIDEBAR_W * sidebar_k(ctx.available_rect().width());
        let (sidebar_min, sidebar_max) = sidebar_width_range();
        SidePanel::left("smart_mix_studio_sidebar")
            .resizable(true)
            .width_range(sidebar_min..=sidebar_max)
            .default_width(sidebar_w)
            .frame(
                Frame::none()
                    .fill(studio::SIDE)
                    .inner_margin(Margin::symmetric(12.0, 15.0)),
            )
            .show(ctx, |ui| {
                ScrollArea::vertical()
                    .id_source("studio_sidebar_scroll")
                    .show(ui, |ui| {
                        self.sidebar(ui, &lang, model, covers, &mut actions);
                    });
            });

        let mut library_rect = Rect::NOTHING;
        if draw_library {
            CentralPanel::default()
                .frame(
                    Frame::none()
                        .fill(studio::BG)
                        .inner_margin(Margin::same(0.0)),
                )
                .show(ctx, |ui| {
                    library_rect = ui.max_rect();
                    self.library(ui, &lang, model, &mut actions, covers);
                });
            // El editor de tags es un overlay sobre la tabla (kit): no
            // encoge el waveform ni re-fluja la tabla por frame.
            if self.editor.is_some() {
                self.tag_drawer(ctx, &lang, library_rect, model, &mut actions);
            }
        }
        self.popover(ctx, &lang, model, &mut actions);
        actions
    }

    fn appbar(&mut self, ui: &mut Ui, lang: &str, model: &Model, actions: &mut Vec<Action>) {
        ui.horizontal_centered(|ui| {
            // Lockup oficial Avant Garden (symbol + wordmark, theme.rs) DENTRO
            // de la misma zona 128x30 del kit — mismo layout de appbar.
            let (brand, _) = ui.allocate_exact_size(vec2(128.0, 30.0), Sense::hover());
            studio::at(ui, "studio_brand_lockup", brand, |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    crate::theme::brand_lockup(ui, 25.0, &mut self.brand_tex);
                });
            });
            ui.add_space(8.0);
            if studio::button(
                ui,
                &texts::t(lang, "studio.import"),
                Some(studio::Icon::Plus),
                studio::Tone::Quiet,
                94.0,
            )
            .clicked()
            {
                actions.push(Action::Import);
            }
            if studio::button(
                ui,
                &texts::t(lang, "studio.analyze"),
                Some(studio::Icon::Scan),
                studio::Tone::Raised,
                102.0,
            )
            .clicked()
            {
                actions.push(Action::Analyze(self.target_ids(model)));
            }
            // Exportar en el appbar, INMEDIATAMENTE a la derecha de Analizar
            // (decisión del dueño): exporta la selección actual.
            if studio::button(
                ui,
                &texts::t(lang, "studio.export"),
                Some(studio::Icon::Upload),
                studio::Tone::Raised,
                102.0,
            )
            .clicked()
            {
                actions.push(Action::Export(self.target_ids(model)));
            }
            if studio::icon_button_size(
                ui,
                studio::Icon::More,
                &texts::t(lang, "studio.tools"),
                34.0,
            )
            .clicked()
            {
                self.toggle_popover(Popover::Library);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let previous = self.compatibility;
                ComboBox::from_id_source("studio_compatible_key")
                    .width(64.0)
                    .selected_text(
                        self.compatibility
                            .map(|key| key.to_string())
                            .unwrap_or_else(|| "—".into()),
                    )
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.compatibility,
                            None,
                            texts::t(lang, "studio.all_keys"),
                        );
                        for number in 1..=12u8 {
                            for mode in [KeyMode::Minor, KeyMode::Major] {
                                let key = Key { number, mode };
                                ui.selectable_value(
                                    &mut self.compatibility,
                                    Some(key),
                                    key.to_string(),
                                );
                            }
                        }
                    });
                ui.label(studio::small(
                    texts::t(lang, "toolbar.compat"),
                    studio::SECONDARY,
                ));
                // búsqueda real de la app (no estaba en el kit; conservada)
                if ui.available_width() > 420.0 {
                    let before = self.search.clone();
                    let search = TextEdit::singleline(&mut self.search)
                        .hint_text(texts::t(lang, "toolbar.search"))
                        .desired_width(190.0);
                    let response = ui.add(search);
                    if (response.changed() || response.lost_focus()) && before != self.search {
                        actions.push(Action::SetSearch(self.search.clone()));
                    }
                }
                if ui.available_width() > 150.0 {
                    ui.label(studio::small(&model.workspace_name, studio::SUBTLE));
                }
                if self.compatibility != previous {
                    self.checked.clear();
                    self.selected = None;
                    self.editor = None;
                    actions.push(Action::SetCompatibility(self.compatibility));
                }
            });
        });
    }

    fn status_line(&mut self, ui: &mut Ui, lang: &str, model: &Model, actions: &mut Vec<Action>) {
        // Nota 0.27: sin add_sized+truncate aquí (el label no pintaba en la
        // franja de 28px); label simple como en el appbar.
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(6.0, 16.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 2.5, studio::LIME);
            // analyze-live: mientras corre el análisis el extremo izquierdo
            // alterna cada 5 s (reloj de egui, sin estado extra en StudioUi)
            // entre el texto de estado y una barra compacta con % + la pista
            // EN análisis.
            let rotating = model
                .analysis
                .filter(|_| analysis_phase_shows_bar(ui.ctx().input(|i| i.time)));
            match rotating {
                Some(AnalysisProgress { done, total }) => {
                    let pct = analysis_percent(done, total);
                    let (bar, _) = ui.allocate_exact_size(vec2(110.0, 6.0), Sense::hover());
                    ui.painter().rect_filled(bar, 3.0, studio::LINE);
                    let mut fill = bar;
                    fill.set_width(bar.width() * pct / 100.0);
                    ui.painter().rect_filled(fill, 3.0, studio::LILAC);
                    ui.label(studio::small(format!("{pct:.0}%"), studio::SECONDARY));
                    let track = model.current_analyzing.as_deref().unwrap_or("…");
                    let line = texts::t(lang, "studio.analyzing_now").replace("{track}", track);
                    ui.label(studio::small(line, studio::SUBTLE));
                }
                None => {
                    let base = if model.status.is_empty() {
                        texts::t(lang, "studio.ready")
                    } else {
                        model.status.clone()
                    };
                    ui.label(studio::small(base, studio::SECONDARY));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if let Some(AnalysisProgress { done, total }) = model.analysis {
                    let frac = if total > 0 {
                        (done as f32 / total as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    ui.label(studio::small(
                        format!("{} {}/{}", texts::t(lang, "studio.analyzing"), done, total),
                        studio::SUBTLE,
                    ));
                    let (bar, bar_resp) = ui.allocate_exact_size(vec2(90.0, 4.0), Sense::hover());
                    let _ = bar_resp;
                    ui.painter().rect_filled(bar, 2.0, studio::LINE);
                    let mut fill = bar;
                    fill.set_width(90.0 * frac);
                    ui.painter().rect_filled(fill, 2.0, studio::LIME);
                    if studio::button_size(
                        ui,
                        &texts::t(lang, "studio.cancel_analysis"),
                        None,
                        studio::Tone::Raised,
                        vec2(78.0, 26.0),
                    )
                    .clicked()
                    {
                        actions.push(Action::CancelAnalysis);
                    }
                }
                ui.label(studio::small(
                    format!("{} {}", texts::t(lang, "studio.queue"), model.queue_length),
                    studio::SUBTLE,
                ));
            });
        });
    }

    fn player(
        &mut self,
        ui: &mut Ui,
        lang: &str,
        model: &Model,
        waveform: &mut dyn WaveformView,
        covers: &mut dyn crate::studio_model::CoverArt,
        actions: &mut Vec<Action>,
    ) {
        ui.spacing_mut().item_spacing.y = 0.0;
        let width = ui.available_width();
        let (head, _) = ui.allocate_exact_size(vec2(width, studio::TRACKBAR_H), Sense::hover());
        let compact_actions = width < 1110.0;
        // Exportar vive ahora en el appbar junto a Analizar: la cabecera de
        // pista solo conserva el menú de acciones de la pista (…)
        let action_width = if compact_actions { 44.0 } else { 48.0 };
        let metric_width = 354.0;
        let art = Rect::from_min_size(head.min + vec2(0.0, 15.0), vec2(52.0, 52.0));
        // Cierre (portada en cabecera): el rect de la cabecera muestra la
        // portada de la pista CARGADA con el mismo pipeline de la tabla
        // (CoverArt + LRU + worker; request_cover idempotente). Fallback
        // vinilo cuando no hay pista o aún no llegó el arte.
        let loaded_id = model.player.track.as_ref().map(|track| track.id);
        let texture = loaded_id.and_then(|id| covers.cover_texture(id));
        if let (Some(id), None) = (loaded_id, &texture) {
            covers.request_cover(id);
        }
        match &texture {
            Some(texture) => {
                let uv = cover_uv(texture.size_vec2());
                ui.painter().add(Shape::Rect(egui::epaint::RectShape {
                    rect: art,
                    rounding: Rounding::same(9.0),
                    fill: Color32::WHITE,
                    stroke: Stroke::NONE,
                    fill_texture_id: texture.id(),
                    uv,
                }));
            }
            None => {
                ui.painter().rect_filled(art, 9.0, studio::RAISED);
                studio::draw_icon(
                    ui.painter(),
                    art.center(),
                    26.0,
                    studio::Icon::Disc,
                    studio::LILAC,
                );
            }
        }

        // La banda de identidad llega hasta top+86 (TRACKBAR_H=104): el
        // artista en caja de 24px — las descendentes ('p' de "upper
        // class") necesitan aire BAJO la línea base, no solo el alto de
        // las mayúsculas; la fila ya no queda pegada al borde inferior.
        let identity = Rect::from_min_max(
            pos2(art.right() + 15.0, head.top() + 12.0),
            pos2(
                head.right() - metric_width - action_width - 30.0,
                head.top() + 86.0,
            ),
        );
        studio::at(ui, "studio_player_identity", identity, |ui| {
            let no_track = texts::t(lang, "studio.no_track");
            let title = model
                .player
                .track
                .as_ref()
                .map(|track| track.title.as_str())
                .unwrap_or(no_track.as_str());
            ui.horizontal(|ui| {
                ui.add_sized(
                    vec2((identity.width() - 74.0).max(70.0), 30.0),
                    Label::new(RichText::new(title).size(26.0).color(studio::TEXT)).truncate(true),
                );
                ui.spacing_mut().item_spacing.x = 2.0;
                for deck in [Deck::A, Deck::B] {
                    let tone = if model.player.deck == deck {
                        studio::Tone::Selected
                    } else {
                        studio::Tone::Raised
                    };
                    if studio::button_size(ui, &deck.to_string(), None, tone, vec2(26.0, 25.0))
                        .on_hover_text(
                            texts::t(lang, "studio.deck_hover")
                                .replace("{deck}", &deck.to_string()),
                        )
                        .clicked()
                    {
                        actions.push(Action::SetDeck(deck));
                    }
                }
            });
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                let artist = model
                    .player
                    .track
                    .as_ref()
                    .map(|track| track.artist.as_str())
                    .filter(|name| !name.is_empty())
                    .unwrap_or("—");
                let artist_width = (identity.width() - 113.0).max(60.0);
                ui.add_sized(
                    vec2(artist_width.min(210.0), 24.0),
                    Label::new(RichText::new(artist).size(16.0).color(studio::SECONDARY))
                        .truncate(true),
                );
                ui.label(studio::small("·", studio::SUBTLE));
                ui.label(studio::small(&model.player.format, studio::SUBTLE));
                if model.player.clips > 0 {
                    ui.label(studio::small("·", studio::SUBTLE));
                    let clip_text = texts::t(lang, "studio.clips")
                        .replace("{n}", &model.player.clips.to_string());
                    let response = ui.add(
                        Label::new(studio::small(clip_text, studio::WARNING)).sense(Sense::click()),
                    );
                    if response.clicked() {
                        self.toggle_popover(Popover::Analysis);
                    }
                }
            });
        });

        let metrics_left = head.right() - metric_width - action_width - 15.0;
        let metrics_y = head.top() + 16.0;
        let painter = ui.painter();
        let track = model.player.track.as_ref();
        let bpm = track
            .and_then(|track| track.bpm)
            .map(|bpm| (bpm as f64) * (1.0 + model.player.pitch_percent as f64 / 100.0));
        studio::draw_bpm(painter, pos2(metrics_left, metrics_y), bpm);
        let key_value = track
            .and_then(|track| track.key)
            .map(|key| key.to_string())
            .unwrap_or_else(|| "—".into());
        studio::metric(
            painter,
            pos2(metrics_left + 96.0, metrics_y),
            &texts::t(lang, "studio.key"),
            &key_value,
            26.0,
            studio::LILAC,
        );
        // ENERGÍA (fix ui-rb2): escala unificada de 10 swatches con el
        // gradiente azul→teal→verde→amarillo→naranja→rojo de la leyenda —
        // el MISMO componente y paleta de la celda de la tabla (fuera el
        // texto "5 / 10" y los puntos lila). Se decidió SIN número: la
        // tira de swatches ya comunica el nivel y ambos sitios quedan
        // idénticos (verificado en captura).
        painter.text(
            pos2(metrics_left + 181.0, metrics_y),
            Align2::LEFT_TOP,
            &texts::t(lang, "studio.energy"),
            FontId::proportional(studio::FONT_SMALL),
            studio::SUBTLE,
        );
        studio::energy_scale(
            painter,
            Rect::from_min_size(
                pos2(metrics_left + 181.0, metrics_y + 21.0),
                vec2(68.0, 18.0),
            ),
            track.and_then(|track| track.energy),
        );
        studio::at(
            ui,
            "studio_player_lufs",
            Rect::from_min_size(
                pos2(metrics_left + 262.0, metrics_y + 10.0),
                vec2(92.0, 33.0),
            ),
            |ui| {
                let value = model
                    .player
                    .lufs
                    .map(|value| format!("{value:.1} LUFS"))
                    .unwrap_or_else(|| "LUFS —".into());
                if studio::button_size(
                    ui,
                    &value,
                    Some(studio::Icon::ChevronDown),
                    studio::Tone::Quiet,
                    vec2(90.0, 31.0),
                )
                .clicked()
                {
                    self.toggle_popover(Popover::Analysis);
                }
            },
        );

        let actions_rect = Rect::from_min_size(
            pos2(head.right() - action_width, head.top() + 27.0),
            vec2(action_width, 31.0),
        );
        ui.painter().line_segment(
            [
                pos2(actions_rect.left() - 10.0, head.top() + 25.0),
                pos2(actions_rect.left() - 10.0, head.top() + 61.0),
            ],
            Stroke::new(1.0, studio::LINE),
        );
        studio::at(ui, "studio_player_actions", actions_rect, |ui| {
            ui.horizontal(|ui| {
                if studio::icon_button(
                    ui,
                    studio::Icon::More,
                    &texts::t(lang, "studio.track_actions"),
                )
                .clicked()
                {
                    if let Some(track) = &model.player.track {
                        self.toggle_popover(Popover::Track(track.id));
                    }
                }
            });
        });

        let (wave_rect, _) = ui.allocate_exact_size(vec2(width, 101.0), Sense::hover());
        let clip = wave_rect.intersect(ui.clip_rect());
        // Hijo NO-allocante: el rect de 101px ya está reservado arriba; un
        // hijo que avance el cursor desplazaría timeline y transporte.
        let mut waveform_ui = ui.child_ui_with_id_source(
            wave_rect,
            Layout::top_down(Align::Min),
            "studio_waveform_host",
        );
        waveform_ui.set_clip_rect(clip);
        if let Some(action) = waveform.show(&mut waveform_ui, wave_rect, &model.player) {
            actions.push(action);
        }
        let (timeline, _) = ui.allocate_exact_size(vec2(width, 21.0), Sense::hover());
        studio::draw_timeline(
            ui.painter(),
            timeline,
            track
                .and_then(|track| track.duration_seconds)
                .map(f64::from),
        );
        let (bottom, _) = ui.allocate_exact_size(vec2(width, 64.0), Sense::hover());
        self.transport(ui, lang, bottom, model, actions);
    }

    fn transport(
        &mut self,
        ui: &mut Ui,
        lang: &str,
        rect: Rect,
        model: &Model,
        actions: &mut Vec<Action>,
    ) {
        let top = rect.top() + 11.0;
        // píldora izquierda REPRODUCTOR + BPM: el kit la omitía y el usuario
        // pide conservar el detalle (tempo a la vista) — pintada con
        // painter para no perturbar el layout del transporte. REGLA del
        // dueño: solo hay UN reproductor — antes decía "DECK A"
        // (format! dinámico); ahora es i18n sin la palabra Deck. 'DECK A'
        // era corto; 'REPRODUCTOR' es más largo → 13px y BPM al borde
        // derecho de una píldora más ancha (sin solapar el texto).
        {
            let deck_name = texts::t(lang, "studio.player_pill");
            let bpm_txt = model
                .player
                .track
                .as_ref()
                .and_then(|t| t.bpm)
                .map(|b| format!("{b:.2}"));
            let pill = Rect::from_min_size(pos2(rect.left(), top), vec2(148.0, 42.0));
            ui.painter().rect_filled(pill, 8.0, studio::RAISED);
            ui.painter().text(
                pos2(pill.left() + 10.0, pill.center().y),
                egui::Align2::LEFT_CENTER,
                deck_name,
                FontId::proportional(13.0),
                studio::TEXT,
            );
            if let Some(b) = &bpm_txt {
                ui.painter().text(
                    pos2(pill.right() - 8.0, pill.center().y),
                    egui::Align2::RIGHT_CENTER,
                    b,
                    FontId::proportional(13.0),
                    studio::LILAC,
                );
            }
        }
        studio::at(
            ui,
            "studio_transport",
            Rect::from_min_size(pos2(rect.left() + 160.0, top), vec2(195.0, 42.0)),
            |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    if studio::icon_button(
                        ui,
                        studio::Icon::Previous,
                        &texts::t(lang, "studio.prev_cue"),
                    )
                    .clicked()
                    {
                        let position = model.player.position_seconds;
                        match model
                            .player
                            .cues
                            .iter()
                            .filter(|cue| cue.seconds < position - 0.5)
                            .max_by(|a, b| a.seconds.total_cmp(&b.seconds))
                        {
                            Some(cue) => actions.push(Action::Cue(cue.label)),
                            None => actions.push(Action::Seek(0.0)),
                        }
                    }
                    let (play_rect, response) =
                        ui.allocate_exact_size(vec2(42.0, 42.0), Sense::click());
                    ui.painter()
                        .circle_filled(play_rect.center(), 21.0, studio::LIME);
                    studio::draw_icon(
                        ui.painter(),
                        play_rect.center(),
                        19.0,
                        if model.player.playing {
                            studio::Icon::Pause
                        } else {
                            studio::Icon::Play
                        },
                        studio::LIME_TEXT,
                    );
                    let label = if model.player.playing {
                        texts::t(lang, "studio.pause")
                    } else {
                        texts::t(lang, "studio.play")
                    };
                    if response.on_hover_text(&label).clicked() {
                        actions.push(Action::TogglePlayback);
                    }
                    if studio::icon_button(
                        ui,
                        studio::Icon::Next,
                        &texts::t(lang, "studio.next_cue"),
                    )
                    .clicked()
                    {
                        let position = model.player.position_seconds;
                        match model
                            .player
                            .cues
                            .iter()
                            .filter(|cue| cue.seconds > position + 0.5)
                            .min_by(|a, b| a.seconds.total_cmp(&b.seconds))
                        {
                            Some(cue) => actions.push(Action::Cue(cue.label)),
                            None => {
                                if let Some(seconds) =
                                    model.player.track.as_ref().and_then(|t| t.duration_seconds)
                                {
                                    actions.push(Action::Seek(seconds));
                                }
                            }
                        }
                    }
                    ui.add_space(5.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(
                            RichText::new(studio::format_time(model.player.position_seconds))
                                .size(19.0)
                                .color(studio::TEXT),
                        );
                        let total = model
                            .player
                            .track
                            .as_ref()
                            .and_then(|t| t.duration_seconds)
                            .map(studio::format_time)
                            .unwrap_or_else(|| "—".into());
                        ui.label(studio::small(format!("/ {total}"), studio::SUBTLE));
                    });
                });
            },
        );

        let utilities_width = 139.0;
        // píldora 148 + aire 12 + transporte 195 + aire 20 (pill más ancha:
        // 'REPRODUCTOR' no cabe en los 122 antiguos de 'DECK A')
        let cues_left = rect.left() + 375.0;
        let cues_right = rect.right() - utilities_width - 22.0;
        let count = model.player.cues.len();
        if count > 0 {
            let available = (cues_right - cues_left).max(0.0);
            let cue_width =
                ((available - (count.saturating_sub(1) as f32 * 5.0)) / count as f32).max(56.0);
            let cues_rect = Rect::from_min_size(pos2(cues_left, top), vec2(available, 53.0));
            studio::at(ui, "studio_cues_strip", cues_rect, |ui| {
                // Con más de ocho cues reales quedan alcanzables por scroll
                // sin pintar sobre volumen/pitch ni encoger las etiquetas.
                ScrollArea::horizontal()
                    .id_source("studio_cues_scroll")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            for cue in &model.player.cues {
                                let (cue_rect, response) =
                                    ui.allocate_exact_size(vec2(cue_width, 42.0), Sense::click());
                                ui.painter().rect_filled(
                                    cue_rect,
                                    5.0,
                                    if response.hovered() {
                                        studio::HOVER
                                    } else {
                                        studio::RAISED
                                    },
                                );
                                ui.painter().text(
                                    pos2(cue_rect.center().x, cue_rect.top() + 12.0),
                                    Align2::CENTER_CENTER,
                                    cue.label.to_string(),
                                    FontId::proportional(14.0),
                                    cue.color,
                                );
                                ui.painter().text(
                                    pos2(cue_rect.center().x, cue_rect.top() + 30.0),
                                    Align2::CENTER_CENTER,
                                    studio::format_cue_time(cue.seconds as f64),
                                    FontId::proportional(14.0),
                                    studio::SECONDARY,
                                );
                                let hover = texts::t(lang, "studio.cue_hover")
                                    .replace("{label}", &cue.label.to_string())
                                    .replace(
                                        "{time}",
                                        &studio::format_cue_time(cue.seconds as f64),
                                    );
                                if response.on_hover_text(hover).clicked() {
                                    actions.push(Action::Cue(cue.label));
                                }
                            }
                        });
                    });
            });
        }

        studio::at(
            ui,
            "studio_player_utilities",
            Rect::from_min_size(
                pos2(rect.right() - utilities_width, top),
                vec2(utilities_width, 39.0),
            ),
            |ui| {
                ui.horizontal_centered(|ui| {
                    let (volume_icon, _) = ui.allocate_exact_size(vec2(16.0, 20.0), Sense::hover());
                    studio::draw_icon(
                        ui.painter(),
                        volume_icon.center(),
                        15.0,
                        studio::Icon::Volume,
                        studio::SECONDARY,
                    );
                    ui.spacing_mut().slider_width = 56.0;
                    // bolita del volumen proporcional al transporte, no al
                    // interact_size global (36) que la inflaba
                    ui.style_mut().spacing.interact_size.y = 16.0;
                    let mut volume = model.player.volume;
                    if ui
                        .add(Slider::new(&mut volume, 0.0..=1.0).show_value(false))
                        .on_hover_text(texts::t(lang, "studio.volume"))
                        .changed()
                    {
                        actions.push(Action::SetVolume(volume));
                    }
                    let (pitch_rect, response) =
                        ui.allocate_exact_size(vec2(43.0, 38.0), Sense::click());
                    if response.hovered() {
                        ui.painter().rect_filled(pitch_rect, 5.0, studio::HOVER);
                    }
                    studio::draw_icon(
                        ui.painter(),
                        pos2(pitch_rect.center().x, pitch_rect.top() + 10.0),
                        14.0,
                        studio::Icon::Sliders,
                        studio::SECONDARY,
                    );
                    ui.painter().text(
                        pos2(pitch_rect.center().x, pitch_rect.top() + 29.0),
                        Align2::CENTER_CENTER,
                        format!("{:+.1}%", model.player.pitch_percent),
                        FontId::proportional(14.0),
                        studio::SECONDARY,
                    );
                    if response
                        .on_hover_text(texts::t(lang, "studio.pitch_hover"))
                        .clicked()
                    {
                        self.toggle_popover(Popover::Pitch);
                    }
                });
            },
        );
    }

    fn sidebar(
        &mut self,
        ui: &mut Ui,
        lang: &str,
        model: &Model,
        covers: &mut dyn crate::studio_model::CoverArt,
        actions: &mut Vec<Action>,
    ) {
        // RUEDA ARMÓNICA PRIMERA (fix ui-freeze, pedido del dueño): es el
        // primer ítem del sidebar, ANTES de BIBLIOTECA. Mismo bloque que
        // antes cerraba el panel — solo cambió de sitio; el scroll del
        // sidebar y el árbol de playlists fluyen debajo sin cambios.
        let wheel_label = texts::t(lang, "wheel.title");
        let chevron = if self.wheel_open {
            studio::Icon::ChevronDown
        } else {
            studio::Icon::ChevronRight
        };
        let response = studio::button_size(
            ui,
            &wheel_label,
            Some(chevron),
            studio::Tone::Quiet,
            vec2(ui.available_width(), 24.0),
        );
        if response.clicked() {
            self.wheel_open = !self.wheel_open;
        }
        if self.wheel_open {
            ui.add_space(6.0);
            self.harmonic_wheel(ui, lang, model, covers, actions);
        }
        ui.add_space(15.0);
        ui.separator();
        ui.add_space(10.0);
        studio::section_label(ui, &texts::t(lang, "studio.library"));
        let rows = [
            (
                Collection::All,
                texts::t(lang, "col.all"),
                studio::Icon::Library,
                Some(model.library_count),
            ),
            (
                Collection::Unanalyzed,
                texts::t(lang, "col.unanalyzed"),
                studio::Icon::Scan,
                Some(model.unanalyzed_count),
            ),
            (
                Collection::Duplicates,
                texts::t(lang, "col.duplicates"),
                studio::Icon::Copy,
                None,
            ),
        ];
        for (collection, label, icon, count) in rows {
            if studio::nav_item(ui, &label, icon, count, self.collection == collection).clicked() {
                self.change_collection(collection, actions);
            }
        }
        ui.add_space(15.0);
        ui.horizontal(|ui| {
            ui.label(studio::small(
                texts::t(lang, "side.playlists"),
                studio::SUBTLE,
            ));
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if studio::icon_button_size(
                    ui,
                    studio::Icon::Plus,
                    &texts::t(lang, "playlist.new"),
                    22.0,
                )
                .clicked()
                {
                    actions.push(Action::OpenNewPlaylistDialog { parent: None });
                }
            });
        });
        let tree = flatten_playlists(&model.playlists, &self.playlist_collapsed);
        for row in tree {
            let playlist = row.playlist;
            let collection = Collection::Playlist(playlist.id);
            let selected = self.collection == collection;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if row.depth > 1 {
                    ui.add_space(16.0 * (row.depth - 1) as f32);
                }
                if row.has_children {
                    let chevron = if row.expanded {
                        studio::Icon::ChevronDown
                    } else {
                        studio::Icon::ChevronRight
                    };
                    if studio::icon_button_size(
                        ui,
                        chevron,
                        &texts::t(lang, "playlist.toggle"),
                        18.0,
                    )
                    .clicked()
                    {
                        let collapsed = self.playlist_collapsed.entry(playlist.id).or_insert(false);
                        *collapsed = !*collapsed;
                    }
                    ui.add_space(2.0);
                } else {
                    ui.add_space(20.0);
                }
                ui.vertical(|ui| {
                    let response = studio::nav_item(
                        ui,
                        &playlist.name,
                        studio::Icon::List,
                        Some(playlist.count),
                        selected,
                    );
                    if response.clicked() {
                        self.change_collection(collection, actions);
                    }
                    response.context_menu(|ui| {
                        if ui.button(texts::t(lang, "playlist.sub")).clicked() {
                            actions.push(Action::OpenNewPlaylistDialog {
                                parent: Some(playlist.id),
                            });
                            ui.close_menu();
                        }
                        if ui.button(texts::t(lang, "menu.rename_playlist")).clicked() {
                            actions.push(Action::RenamePlaylist(playlist.id));
                            ui.close_menu();
                        }
                        if ui.button(texts::t(lang, "menu.delete_playlist")).clicked() {
                            actions.push(Action::DeletePlaylist(playlist.id));
                            ui.close_menu();
                        }
                    });
                });
            });
        }
        if model.playlists.is_empty() {
            ui.label(studio::small(
                texts::t(lang, "studio.no_playlists"),
                studio::SUBTLE,
            ));
        }
        // F-19: orden manual de la playlist manual activa
        if let Collection::Playlist(pid) = self.collection {
            if model
                .playlists
                .iter()
                .any(|p| p.id == pid && p.kind == "manual")
            {
                if studio::button(
                    ui,
                    &texts::t(lang, "playlist.order"),
                    None,
                    studio::Tone::Raised,
                    ui.available_width(),
                )
                .clicked()
                {
                    actions.push(Action::ToggleManualOrder);
                }
            }
        }

        ui.add_space(15.0);
        ui.horizontal(|ui| {
            ui.label(studio::small(
                texts::t(lang, "studio.devices"),
                studio::SUBTLE,
            ));
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if studio::icon_button_size(
                    ui,
                    studio::Icon::Refresh,
                    &texts::t(lang, "studio.refresh_devices"),
                    22.0,
                )
                .clicked()
                {
                    actions.push(Action::RefreshDevices);
                }
            });
        });
        if model.device_labels.is_empty() {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(16.0, 28.0), Sense::hover());
                studio::draw_icon(
                    ui.painter(),
                    rect.center(),
                    16.0,
                    studio::Icon::Usb,
                    studio::SUBTLE,
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(studio::small(
                        texts::t(lang, "studio.no_devices"),
                        studio::SUBTLE,
                    ));
                    ui.label(studio::small(
                        texts::t(lang, "studio.connect"),
                        studio::SUBTLE,
                    ));
                });
            });
        } else {
            for (index, label) in model.device_labels.iter().enumerate() {
                let response = studio::nav_item(ui, label, studio::Icon::Usb, None, false);
                response.context_menu(|ui| {
                    if ui.button(texts::t(lang, "usb.backup")).clicked() {
                        actions.push(Action::Device {
                            index,
                            op: crate::studio_model::DeviceOp::Backup,
                        });
                        ui.close_menu();
                    }
                    if ui.button(texts::t(lang, "usb.safe_format")).clicked() {
                        actions.push(Action::Device {
                            index,
                            op: crate::studio_model::DeviceOp::SafeFormat,
                        });
                        ui.close_menu();
                    }
                });
            }
        }
        ui.add_space(18.0);
    }

    fn harmonic_wheel(
        &mut self,
        ui: &mut Ui,
        lang: &str,
        model: &Model,
        covers: &mut dyn crate::studio_model::CoverArt,
        actions: &mut Vec<Action>,
    ) {
        // Sidebar responsivo: la rueda crece hasta su tope 202px y queda
        // CENTRADA en el ancho disponible (márgenes iguales a ambos lados
        // cuando el panel es más ancho que la rueda).
        let avail = ui.available_width();
        let size = avail.min(202.0);
        let (row, _) = ui.allocate_exact_size(vec2(avail, size), Sense::hover());
        let rect = Rect::from_center_size(row.center(), vec2(size, size));
        let response = ui.interact(rect, ui.id().with("studio_wheel"), Sense::click());
        let center = rect.center();
        let radius = size / 2.0 - 2.0;
        let reference = self
            .compatibility
            .or_else(|| model.player.track.as_ref().and_then(|track| track.key));
        let neighbours = reference.map(|key| key.compatible_keys());
        for number in 1..=12u8 {
            for mode in [KeyMode::Minor, KeyMode::Major] {
                let key = Key { number, mode };
                let (inner, outer) = match mode {
                    KeyMode::Minor => (radius * 0.52, radius * 0.755),
                    KeyMode::Major => (radius * 0.77, radius),
                };
                let angle = (number as f32 * 30.0 - 90.0).to_radians();
                let selected = reference == Some(key);
                let related = neighbours.as_ref().is_some_and(|keys| keys.contains(&key));
                let fill = if selected {
                    studio::LILAC // seleccionada INVERTIDA: lila slido
                } else if related {
                    studio::KEY_BG // compatibles: tinte lavanda evidente
                } else {
                    studio::RAISED
                };
                annular_sector(
                    ui.painter(),
                    center,
                    inner,
                    outer,
                    angle - 14.1_f32.to_radians(),
                    angle + 14.1_f32.to_radians(),
                    fill,
                );
                let label = center + vec2(angle.cos(), angle.sin()) * ((inner + outer) * 0.5);
                ui.painter().text(
                    label,
                    Align2::CENTER_CENTER,
                    key.to_string(),
                    FontId::proportional(14.0),
                    if selected {
                        studio::LIME_TEXT // tinta oscura sobre lila
                    } else if related {
                        studio::LILAC
                    } else {
                        studio::SUBTLE
                    },
                );
            }
        }
        // CENTRO DE LA RUEDA CONDICIONAL (cierre ítem 11): al FILTRAR por
        // tonalidad (clic en un gajo) el centro muestra la tonalidad
        // ELEGIDA en texto grande — estilo previo al fix ui-rb2 — y SIN
        // portada. En modo neutro con pista cargada mantiene la portada
        // circular (fix ui-rb2): portada SOLO al cargar pista, tono SOLO
        // al filtrar. Usa el pipeline de portadas EXISTENTE (CoverArt +
        // CoverCache LRU + worker; request_cover es idempotente) con el
        // MISMO fallback vinilo de la tabla. El comportamiento del resto
        // NO cambia: hover de gajos igual, compatibles resaltadas igual,
        // clic al centro = estado neutral.
        let loaded_id = model.player.track.as_ref().map(|track| track.id);
        match wheel_center(self.compatibility, loaded_id) {
            WheelCenter::Key(key) => {
                ui.painter().text(
                    center,
                    Align2::CENTER_CENTER,
                    key.to_string(),
                    egui::FontId::proportional(WHEEL_CENTER_FONT),
                    studio::LILAC,
                );
            }
            WheelCenter::Cover(id) => {
                let texture = id.and_then(|id| covers.cover_texture(id));
                if let (Some(id), None) = (id, &texture) {
                    covers.request_cover(id);
                }
                // Ø 0.82·radius < hueco central (0.52·radius de radio interior)
                let cover_r = radius * 0.41;
                match &texture {
                    Some(texture) => {
                        let uv = cover_uv(texture.size_vec2());
                        ui.painter().add(Shape::Rect(egui::epaint::RectShape {
                            rect: Rect::from_center_size(
                                center,
                                vec2(cover_r * 2.0, cover_r * 2.0),
                            ),
                            rounding: Rounding::same(cover_r), // círculo
                            fill: Color32::WHITE,
                            stroke: Stroke::NONE,
                            fill_texture_id: texture.id(),
                            uv,
                        }));
                    }
                    None => {
                        // fallback vinilo (idem celda de portada de la tabla sin
                        // arte): disco RAISED + icono Disc + centro del fondo
                        ui.painter().circle_filled(center, cover_r, studio::RAISED);
                        studio::draw_icon(
                            ui.painter(),
                            center,
                            cover_r * 0.92,
                            studio::Icon::Disc,
                            studio::SUBTLE,
                        );
                        ui.painter()
                            .circle_filled(center, cover_r * 0.10, studio::SIDE);
                    }
                }
                // borde desvanecido: anillo del color del panel con alpha 0→1
                paint_radial_fade(ui.painter(), center, cover_r * 0.80, cover_r, studio::SIDE);
            }
        }
        let hover_key = response
            .hover_pos()
            .and_then(|point| wheel_key_at(point, center, radius));
        if let Some(key) = hover_key {
            let hover = texts::t(lang, "studio.wheel_hover").replace("{k}", &key.to_string());
            response.clone().on_hover_text(hover);
            if response.clicked() {
                self.compatibility = if self.compatibility == Some(key) {
                    None
                } else {
                    Some(key)
                };
                self.checked.clear();
                self.selected = None;
                self.editor = None;
                actions.push(Action::SetCompatibility(self.compatibility));
            }
        }
        // clic al CENTRO = estado neutral: sin tono seleccionado, sin
        // resaltados de compatibles y biblioteca sin filtrar (spec del
        // dueño: la rueda debe poder quedarse "sin nada")
        if response.clicked() && self.compatibility.is_some() {
            if let Some(point) = response.hover_pos() {
                if (point - center).length() < radius * 0.52 {
                    self.compatibility = None;
                    self.checked.clear();
                    self.selected = None;
                    self.editor = None;
                    actions.push(Action::SetCompatibility(None));
                }
            }
        }
        ui.add_space(7.0);
        if let Some(keys) = neighbours {
            ui.horizontal(|ui| {
                ui.add_space(((size - 124.0) * 0.5).max(0.0));
                ui.spacing_mut().item_spacing.x = 4.0;
                for key in keys {
                    let hover =
                        texts::t(lang, "studio.wheel_hover").replace("{k}", &key.to_string());
                    if studio::button_size(
                        ui,
                        &key.to_string(),
                        None,
                        studio::Tone::Raised,
                        vec2(28.0, 24.0),
                    )
                    .on_hover_text(hover)
                    .clicked()
                    {
                        self.compatibility = Some(key);
                        self.checked.clear();
                        self.selected = None;
                        self.editor = None;
                        actions.push(Action::SetCompatibility(Some(key)));
                    }
                }
            });
        }
    }

    fn library(
        &mut self,
        ui: &mut Ui,
        lang: &str,
        model: &Model,
        actions: &mut Vec<Action>,
        covers: &mut dyn crate::studio_model::CoverArt,
    ) {
        let visible = self.visible_indices(model);
        Frame::none()
            .inner_margin(Margin::symmetric(17.0, 12.0))
            .show(ui, |ui| {
                // PITFALL 0.27: filas centradas (cross Center) en regiones de
                // alto libre reclaman TODO el alto disponible (frame del
                // widget = alto completo) — aqui SIEMPRE cross-align Min.
                ui.horizontal(|ui| {
                    // Cierre (contador pegado): el título se acota a su ancho
                    // con elipsis "…" (ancho NATURAL) y el contador vive en un
                    // chip separado — antes quedaba pegado al texto. El chip
                    // replica el lenguaje de los contadores del sidebar.
                    let title = self.collection_title(lang, model);
                    let title_width = (ui.available_width() - 335.0).max(100.0);
                    let shown = ui.fonts(|fonts| {
                        studio::ellipsize(
                            fonts,
                            &title,
                            egui::FontId::proportional(18.0),
                            title_width,
                        )
                    });
                    ui.label(RichText::new(shown).size(18.0).strong().color(studio::TEXT));
                    let count = match self.collection {
                        Collection::All => Some(model.library_count),
                        Collection::Unanalyzed => Some(model.unanalyzed_count),
                        Collection::Duplicates => None,
                        Collection::Playlist(id) => model
                            .playlists
                            .iter()
                            .find(|playlist| playlist.id == id)
                            .map(|playlist| playlist.count),
                    };
                    if let Some(count) = count {
                        ui.add_space(8.0);
                        egui::Frame::none()
                            .fill(studio::RAISED)
                            .rounding(egui::Rounding::same(10.0))
                            .inner_margin(egui::Margin::symmetric(9.0, 3.0))
                            .show(ui, |ui| {
                                ui.set_height(20.0);
                                ui.label(studio::small(count.to_string(), studio::SUBTLE));
                            });
                    }
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        if ui.available_width() > 270.0 {
                            for field in [EditField::Energy, EditField::Key, EditField::Bpm] {
                                let label = match field {
                                    EditField::Energy => texts::t(lang, "studio.energy_short"),
                                    EditField::Key => texts::t(lang, "studio.key_short"),
                                    _ => texts::t(lang, "studio.bpm_short"),
                                };
                                let width = if field == EditField::Energy {
                                    53.0
                                } else {
                                    38.0
                                };
                                if studio::button_size(
                                    ui,
                                    &label,
                                    None,
                                    studio::Tone::Raised,
                                    vec2(width, 26.0),
                                )
                                .clicked()
                                {
                                    self.open_editor(model, self.target_ids(model), field);
                                }
                            }
                        }
                        let has_target = !self.target_ids(model).is_empty();
                        ui.add_enabled_ui(has_target, |ui| {
                            let tone = if self.editor.is_some() {
                                studio::Tone::Selected
                            } else {
                                studio::Tone::Quiet
                            };
                            if studio::button(
                                ui,
                                &texts::t(lang, "studio.tag_editor"),
                                Some(studio::Icon::Tag),
                                tone,
                                121.0,
                            )
                            .clicked()
                            {
                                if self.editor.is_some() {
                                    self.editor = None;
                                } else {
                                    self.open_editor(
                                        model,
                                        self.target_ids(model),
                                        EditField::Title,
                                    );
                                }
                            }
                        });
                    });
                });
            });
        if !self.checked.is_empty() {
            Frame::none()
                .fill(studio::SELECTED)
                .inner_margin(Margin::symmetric(12.0, 5.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let n = self.checked.len();
                        ui.label(studio::small(
                            texts::t(lang, "studio.selected_n").replace("{n}", &n.to_string()),
                            studio::SECONDARY,
                        ));
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            if studio::button_size(
                                ui,
                                &texts::t(lang, "studio.export"),
                                Some(studio::Icon::Upload),
                                studio::Tone::Quiet,
                                vec2(82.0, 26.0),
                            )
                            .clicked()
                            {
                                actions.push(Action::Export(self.target_ids(model)));
                            }
                            if !model.playlists.is_empty()
                                && studio::button_size(
                                    ui,
                                    &texts::t(lang, "studio.to_playlist"),
                                    Some(studio::Icon::List),
                                    studio::Tone::Quiet,
                                    vec2(88.0, 26.0),
                                )
                                .clicked()
                            {
                                self.toggle_popover(Popover::AddToPlaylist);
                            }
                        });
                    });
                });
        }
        if visible.is_empty() {
            ui.add_space(45.0);
            ui.vertical_centered(|ui| {
                let (disc, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                studio::draw_icon(
                    ui.painter(),
                    disc.center(),
                    28.0,
                    studio::Icon::Disc,
                    studio::SUBTLE,
                );
                ui.add_space(8.0);
                let empty = match self.collection {
                    Collection::Unanalyzed => texts::t(lang, "studio.empty_analyzed"),
                    Collection::Playlist(_) => texts::t(lang, "studio.empty_playlist"),
                    _ => texts::t(lang, "studio.empty"),
                };
                ui.label(RichText::new(empty).size(18.0));
                let hint = if self.compatibility.is_some() {
                    texts::t(lang, "studio.empty_compat")
                } else {
                    texts::t(lang, "studio.empty_hint")
                };
                ui.label(studio::small(hint, studio::SECONDARY));
            });
            return;
        }

        // Receta 0.27 PROBADA (ex library.rs): TableBuilder directo con su
        // ScrollArea INTERNA virtualizada — envolverlo en un ScrollArea
        // horizontal (patron del kit 0.33) revienta con miles de filas.
        // El header (31px) ya no se descuenta: max_scroll_height es del body.
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            static LAST: AtomicU64 = AtomicU64::new(0);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            if now - LAST.load(Ordering::Relaxed) > 2000 {
                LAST.store(now, Ordering::Relaxed);
                eprintln!(
                    "[studio-lib] max_rect={:?} cursor={:?} avail_h={:.0} avail_w={:.0} visible={} clip={:?}",
                    ui.max_rect(),
                    ui.cursor(),
                    ui.available_height(),
                    ui.available_width(),
                    visible.len(),
                    ui.clip_rect()
                );
            }
        }
        let table_height = (ui.available_height() - 36.0 - 14.0).max(96.0);
        // selección por rango (shift+click): orden visible y shift leídos
        // ANTES de la tabla, igual que sort_state — las clausuras de
        // header/body toman `self` mutably y no pueden releerlo dentro.
        let visible_ids: Vec<TrackId> = visible
            .iter()
            .map(|&index| model.tracks[index].id)
            .collect();
        let shift_held = ui.input(|input| input.modifiers.shift);
        {
            let mut sort_clicked = None;
            let mut select_all_clicked = None;
            let mut clicked_track = None;
            let mut range_clicked = None;
            let mut double_clicked_track = None;
            let mut clicked_menu = None;
            let checked_before = self.checked.clone();
            let selected_before = self.selected;
            let loaded_id = model.player.track.as_ref().map(|track| track.id);
            // leídos ANTES de la tabla: las clausuras de header/body toma
            // `self` mutably (self.checked…) y no pueden releer self.sort
            let sort_state = self.sort;
            let sort_hint = texts::t(lang, "studio.sort_hint");
            // presupuesto de PEDIDOS de portada por frame (el decode vive en
            // el worker del host; esto solo limita la cola que lanza la vista)
            let mut cover_budget = 6usize;
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.spacing_mut().item_spacing.y = 0.0;
            // Aire izquierdo (~10px) antes de la columna del checkbox: el
            // header y las celdas comparten la MISMA tabla dentro del frame,
            // así que la alineación columna-a-columna queda intacta.
            // TableState PERSISTIDO: egui_extras guarda los anchos por
            // ui.id().with("__table_state") y los RECARGA aunque la tabla
            // ya no sea resizable — el estado del código PRE-fix
            // (resizable true) sombreaba para siempre los Column::exact
            // (medido: ventana maximizada seguía con los anchos viejos).
            // Id nuevo = partida limpia; resizable(false) nunca lo reescribe.
            ui.push_id(Id::new("studio_lib_tbl_v2"), |ui| {
                Frame::none()
                    .inner_margin(Margin {
                        // aire IZQUIERDO del checkbox (pedido del dueño):
                        // 16px en filas Y en el header tri-state (mismo
                        // frame → alineación columna-a-columna intacta).
                        left: 16.0,
                        right: 0.0,
                        top: 0.0,
                        bottom: 0.0,
                    })
                    .show(ui, |ui| {
                        // TABLA RESPONSIVA CON ANCHOS PROPIOS: egui_extras
                        // `.resizable(true)` PERSISTE los anchos en TableState
                        // tras el primer drag y desde ahí IGNORA los
                        // Column::initial dinámicos → la lista dejaba de
                        // ensancharse con la ventana. Aquí `.resizable(false)`
                        // (sin persistencia) + `Column::exact(width_i)` con
                        // width_i = col_widths[i] (drag manual del usuario,
                        // Some) o base_i·k (ver col_width). El título sigue
                        // siendo remainder() y absorbe el resto; las asas de
                        // drag son franjas propias de las celdas del header
                        // (resize_handle), no de egui_extras.
                        let mut col_widths = std::mem::take(&mut self.col_widths);
                        if col_widths.len() != COL_BASES.len() {
                            col_widths = vec![None; COL_BASES.len()];
                        }
                        // fix ui-rb2: available de egui_extras (menos el
                        // carril del scrollbar) y layout con SOLO título y
                        // artista flexibles — la suma de las 11 columnas es
                        // EXACTAMENTE ese ancho todos los frames.
                        let avail = table_available_width(ui);
                        let k = table_k(ui.available_width());
                        let widths: [f32; COL_COUNT] =
                            table_layout(avail, k, col_widths[COL_TITLE], col_widths[COL_ARTIST]);
                        // presupuesto flexible para los clamps dinámicos de
                        // las asas (lo que sobra tras las fijas)
                        let flex_budget = avail
                            - widths
                                .iter()
                                .enumerate()
                                .filter(|(c, _)| *c != COL_TITLE && *c != COL_ARTIST)
                                .map(|(_, &w)| w)
                                .sum::<f32>();
                        let mut table = TableBuilder::new(ui)
                            .striped(false)
                            .resizable(false)
                            .cell_layout(Layout::left_to_right(Align::Center))
                            .sense(Sense::click())
                            .max_scroll_height(table_height);
                        for &w in widths.iter() {
                            table = table.column(Column::exact(w).clip(true));
                        }
                        table
                            .header(TABLE_HEADER_HEIGHT, |mut header| {
                                // Alineación por columna (fix table-rb, estilo
                                // rekordbox): COL_ALIGNS define UNA vez cómo se
                                // alinea cada columna y header + celdas la
                                // comparten — así quedan coaxiales.
                                header.col(|ui| {
                                    center_cell(
                                        ui,
                                        ui.id().with("hdr_chk"),
                                        vec2(18.0, 18.0),
                                        |ui| {
                                            let all = visible.iter().all(|&index| {
                                                checked_before.contains(&model.tracks[index].id)
                                            });
                                            // checkbox VECTORIAL (fix ux):
                                            // checked = lavanda + tinta — mismo
                                            // widget de las filas. El clic
                                            // invierte el estado actual
                                            // (todo/parcial), igual que el
                                            // checkbox de egui que sustituye.
                                            if studio::row_checkbox(ui, all)
                                                .on_hover_text(texts::t(
                                                    lang,
                                                    "studio.select_visible",
                                                ))
                                                .clicked()
                                            {
                                                select_all_clicked = Some(!all);
                                            }
                                        },
                                    );
                                    // FIJA (fix ui-rb2): sin asa de resize
                                });
                                header.col(|ui| {
                                    // '#' AHORA sorteable: orden original por id
                                    sort_header_cell(
                                        ui,
                                        SortField::Number,
                                        "#",
                                        COL_ALIGNS[1],
                                        sort_state,
                                        &sort_hint,
                                        &mut sort_clicked,
                                    );
                                    // FIJA (fix ui-rb2): sin asa de resize
                                });
                                // portada: columna VISUAL FIJA — sin header y
                                // SIN asa
                                header.col(|_ui| {});
                                let headers = [
                                    (SortField::Title, texts::t(lang, "studio.title")),
                                    (SortField::Artist, texts::t(lang, "studio.artist")),
                                    (SortField::Key, texts::t(lang, "studio.key_short")),
                                    (SortField::Bpm, texts::t(lang, "studio.bpm_short")),
                                    (SortField::Energy, texts::t(lang, "studio.energy_short")),
                                    (SortField::Rating, texts::t(lang, "studio.rating")),
                                    (SortField::Duration, texts::t(lang, "studio.duration")),
                                ];
                                for (i, (field, label)) in headers.into_iter().enumerate() {
                                    let c = i + 3; // índice real de la columna
                                    header.col(|ui| {
                                        sort_header_cell(
                                            ui,
                                            field,
                                            &label,
                                            COL_ALIGNS[c],
                                            sort_state,
                                            &sort_hint,
                                            &mut sort_clicked,
                                        );
                                        // fix ui-rb2: SOLO título y artista
                                        // tienen asa; el clamp es DINÁMICO
                                        // (el ancho que le sobra al otro) y
                                        // el drag de uno limpia el ancho
                                        // fijado del otro — el último drag
                                        // manda y la suma nunca desborda.
                                        if c == COL_TITLE {
                                            let before = col_widths[COL_TITLE];
                                            resize_handle(
                                                ui,
                                                col_resize_id(c),
                                                widths[c],
                                                (
                                                    COL_RANGES[COL_TITLE].0,
                                                    (flex_budget - COL_RANGES[COL_ARTIST].0)
                                                        .max(COL_RANGES[COL_TITLE].0),
                                                ),
                                                &mut col_widths[COL_TITLE],
                                            );
                                            if col_widths[COL_TITLE] != before {
                                                col_widths[COL_ARTIST] = None;
                                            }
                                        } else if c == COL_ARTIST {
                                            let before = col_widths[COL_ARTIST];
                                            resize_handle(
                                                ui,
                                                col_resize_id(c),
                                                widths[c],
                                                (
                                                    COL_RANGES[COL_ARTIST].0,
                                                    (flex_budget - COL_RANGES[COL_TITLE].0)
                                                        .min(COL_RANGES[COL_ARTIST].1)
                                                        .max(COL_RANGES[COL_ARTIST].0),
                                                ),
                                                &mut col_widths[COL_ARTIST],
                                            );
                                            if col_widths[COL_ARTIST] != before {
                                                col_widths[COL_TITLE] = None;
                                            }
                                        }
                                    });
                                }
                                // última columna (menú de fila): FIJA, sin asa
                                header.col(|_ui| {});
                            })
                            .body(|body| {
                                body.rows(TABLE_ROW_HEIGHT, visible.len(), |mut row| {
                                    let index = visible[row.index()];
                                    let track = &model.tracks[index];
                                    let row_selected = selected_before == Some(track.id)
                                        || checked_before.contains(&track.id);
                                    row.set_selected(row_selected);
                                    let mut child_handled = false;
                                    row.col(|ui| {
                                        if row_selected {
                                            let cell = ui.max_rect();
                                            ui.painter().rect_filled(
                                                Rect::from_min_size(
                                                    cell.min,
                                                    vec2(2.0, cell.height()),
                                                ),
                                                0.0,
                                                studio::LIME,
                                            );
                                        }
                                        let mut checked = self.checked.contains(&track.id);
                                        let hover = texts::t(lang, "studio.select_track")
                                            .replace("{title}", &track.title);
                                        let mut changed = false;
                                        center_cell(
                                            ui,
                                            ui.id().with("row_chk"),
                                            vec2(18.0, 18.0),
                                            |ui| {
                                                // checkbox VECTORIAL (fix ux):
                                                // checked = lavanda + tinta,
                                                // visible sobre la fila
                                                // seleccionada; el toggle lo
                                                // resuelve el modelo.
                                                if studio::row_checkbox(ui, checked)
                                                    .on_hover_text(&hover)
                                                    .clicked()
                                                {
                                                    changed = true;
                                                }
                                            },
                                        );
                                        if changed {
                                            // shift+checkbox = RANGO desde el ancla
                                            // (Gmail): NO alterna — extiende la
                                            // MISMA selección hasta esta fila.
                                            if shift_held {
                                                crate::studio_model::selection::range_add(
                                                    &mut self.checked,
                                                    &mut self.anchor,
                                                    track.id,
                                                    &visible_ids,
                                                );
                                                self.editor = None;
                                            } else {
                                                crate::studio_model::selection::toggle(
                                                    &mut self.checked,
                                                    &mut self.anchor,
                                                    track.id,
                                                );
                                                self.selected = Some(track.id);
                                                self.editor = None;
                                            }
                                            child_handled = true;
                                        }
                                    });
                                    row.col(|ui| {
                                        // '#' centrado (COL_ALIGNS): número pintado
                                        // al centro exacto de la celda
                                        let cell = ui.max_rect();
                                        ui.painter().text(
                                            cell.center(),
                                            Align2::CENTER_CENTER,
                                            (index + 1).to_string(),
                                            egui::FontId::proportional(studio::FONT_SMALL),
                                            studio::SUBTLE,
                                        );
                                    });
                                    row.col(|ui| {
                                        // PORTADA: miniatura 28×28 (radio 3) del arte
                                        // EMBEBIDO. Perezosa — solo filas visibles piden,
                                        // ≤6 pedidos/frame; el decode vive en el worker
                                        // del host. Sin arte → cuadrado RAISED + vinilo
                                        // geométrico (nada de glifos).
                                        let texture = covers.cover_texture(track.id);
                                        if texture.is_none() && cover_budget > 0 {
                                            covers.request_cover(track.id);
                                            cover_budget -= 1;
                                        }
                                        let cell = ui.max_rect();
                                        let rect =
                                            Rect::from_center_size(cell.center(), vec2(28.0, 28.0));
                                        match texture {
                                            Some(texture) => {
                                                let uv = cover_uv(texture.size_vec2());
                                                ui.painter().add(Shape::Rect(
                                                    egui::epaint::RectShape {
                                                        rect,
                                                        rounding: Rounding::same(3.0),
                                                        fill: Color32::WHITE,
                                                        stroke: Stroke::NONE,
                                                        fill_texture_id: texture.id(),
                                                        uv,
                                                    },
                                                ));
                                            }
                                            None => {
                                                ui.painter().rect_filled(rect, 3.0, studio::RAISED);
                                                studio::draw_icon(
                                                    ui.painter(),
                                                    rect.center(),
                                                    14.0,
                                                    studio::Icon::Disc,
                                                    studio::SUBTLE,
                                                );
                                            }
                                        }
                                    });
                                    row.col(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 7.0;
                                            // la pista CARGADA en deck es un indicador
                                            // distinto de la selección (regla del kit)
                                            if loaded_id == Some(track.id) {
                                                let (play, _) = ui.allocate_exact_size(
                                                    vec2(12.0, 16.0),
                                                    Sense::hover(),
                                                );
                                                studio::draw_icon(
                                                    ui.painter(),
                                                    play.center(),
                                                    12.0,
                                                    studio::Icon::Play,
                                                    studio::LIME,
                                                );
                                            }
                                            // ancho NATURAL: el texto vive a la
                                            // IZQUIERDA de la celda (fix table-rb).
                                            // Un Button a ancho completo CENTRA su
                                            // texto y rompía la coaxialidad con el
                                            // header 'Título' (medido en captura).
                                            // Cierre (elipsis): el texto se acota
                                            // al ancho restante de la celda con
                                            // "…" — antes el corte era en seco.
                                            let avail = (ui.available_width() - 26.0).max(20.0);
                                            let shown = ui.fonts(|fonts| {
                                                studio::ellipsize(
                                                    fonts,
                                                    &track.title,
                                                    egui::FontId::proportional(TABLE_CELL_FONT),
                                                    avail,
                                                )
                                            });
                                            let label = egui::Button::new(
                                                RichText::new(shown)
                                                    .size(TABLE_CELL_FONT)
                                                    .color(studio::TEXT),
                                            )
                                            .fill(Color32::TRANSPARENT)
                                            .stroke(Stroke::NONE)
                                            .wrap(false);
                                            let response = ui.add(label);
                                            // CAUSA RAÍZ bug 2ª pista (egui 0.27): el
                                            // hit-test asigna el clic al ÚLTIMO widget
                                            // click-sensed bajo el puntero (hit_test.rs
                                            // `filter(sense.click).last()`), y este Button
                                            // es el más interior → `row.response()` nunca
                                            // ve clicked/double_clicked sobre el título.
                                            // El doble clic se lee AQUÍ (como el library
                                            // pre-kit), no solo en la fila.
                                            let response = response.on_hover_text(&track.title);
                                            if response.clicked() {
                                                clicked_track = Some(track.id);
                                                child_handled = true;
                                            }
                                            if response.double_clicked() {
                                                double_clicked_track = Some(track.id);
                                                child_handled = true;
                                            }
                                        });
                                    });
                                    row.col(|ui| {
                                        let artist = if track.artist.is_empty() {
                                            "—"
                                        } else {
                                            &track.artist
                                        };
                                        // ancho NATURAL — a la izquierda, idem título
                                        // (y con elipsis "…" al ancho de la celda)
                                        let avail = (ui.available_width() - 26.0).max(20.0);
                                        let shown = ui.fonts(|fonts| {
                                            studio::ellipsize(
                                                fonts,
                                                artist,
                                                egui::FontId::proportional(TABLE_CELL_FONT),
                                                avail,
                                            )
                                        });
                                        let label = egui::Button::new(
                                            RichText::new(shown)
                                                .size(TABLE_CELL_FONT)
                                                .color(studio::SECONDARY),
                                        )
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::NONE)
                                        .wrap(false);
                                        let response = ui.add(label).on_hover_text(&track.artist);
                                        if response.double_clicked() {
                                            double_clicked_track = Some(track.id);
                                        }
                                    });
                                    row.col(|ui| {
                                        // KEY centrado (COL_ALIGNS): badge en un
                                        // hijo del tamaño EXACTO del widget
                                        // (36×25) centrado en la celda
                                        center_cell(
                                            ui,
                                            ui.id().with("row_key"),
                                            vec2(36.0, 25.0),
                                            |ui| {
                                                studio::key_badge(
                                                    ui,
                                                    track.key.map(|key| key.to_string()).as_deref(),
                                                );
                                            },
                                        );
                                    });
                                    row.col(|ui| {
                                        // BPM a la DERECHA (COL_ALIGNS) en mono:
                                        // JetBrains Mono alinea los decimales
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.add_space(6.0);
                                                let bpm = track
                                                    .bpm
                                                    .map(|bpm| format!("{bpm:.2}"))
                                                    .unwrap_or_else(|| "—".into());
                                                ui.label(
                                                    RichText::new(bpm)
                                                        .size(TABLE_CELL_FONT)
                                                        .monospace()
                                                        .color(studio::SECONDARY),
                                                );
                                            },
                                        );
                                    });
                                    row.col(|ui| {
                                        // ENERGÍA centrada (COL_ALIGNS):
                                        // escala unificada de 10 swatches
                                        // (fix ui-rb2 — MISMO componente
                                        // y paleta que la métrica del
                                        // player)
                                        center_cell(
                                            ui,
                                            ui.id().with("row_energy"),
                                            vec2(60.0, 16.0),
                                            |ui| {
                                                studio::energy_cell(ui, track.energy);
                                            },
                                        );
                                    });
                                    row.col(|ui| {
                                        let hover = texts::t(lang, "studio.rating_hover")
                                            .replace("{title}", &track.title)
                                            .replace("{r}", &track.rating.to_string());
                                        // RATING centrado (COL_ALIGNS)
                                        center_cell(
                                            ui,
                                            ui.id().with("row_rating"),
                                            vec2(65.0, 26.0),
                                            |ui| {
                                                let response =
                                                    studio::rating_button(ui, track.rating, &hover);
                                                if response.clicked() {
                                                    let next = response
                                                        .interact_pointer_pos()
                                                        .and_then(|pos| {
                                                            studio::rating_star_hit(
                                                                response.rect,
                                                                pos,
                                                            )
                                                        })
                                                        .map(|star| {
                                                            crate::rating::next_rating(
                                                                track.rating,
                                                                star,
                                                            )
                                                        })
                                                        .unwrap_or((track.rating + 1) % 6);
                                                    actions.push(Action::Rate {
                                                        track_id: track.id,
                                                        rating: next,
                                                    });
                                                    child_handled = true;
                                                }
                                            },
                                        );
                                    });
                                    row.col(|ui| {
                                        // DURACIÓN a la DERECHA (COL_ALIGNS), mismo
                                        // margen derecho que BPM — coaxiales
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                ui.add_space(6.0);
                                                let dur = track
                                                    .duration_seconds
                                                    .map(studio::format_time)
                                                    .unwrap_or_else(|| "—".into());
                                                ui.label(studio::small(dur, studio::SECONDARY));
                                            },
                                        );
                                    });
                                    row.col(|ui| {
                                        // menú '···' centrado (COL_ALIGNS)
                                        center_cell(
                                            ui,
                                            ui.id().with("row_menu"),
                                            vec2(23.0, 23.0),
                                            |ui| {
                                                if studio::icon_button_size(
                                                    ui,
                                                    studio::Icon::More,
                                                    &texts::t(lang, "studio.track_actions_row"),
                                                    23.0,
                                                )
                                                .clicked()
                                                {
                                                    clicked_menu = Some(track.id);
                                                    child_handled = true;
                                                }
                                            },
                                        );
                                    });
                                    let response = row.response();
                                    // Click derecho: la selección sobre la que
                                    // abre el menú es la que se VE — clic en
                                    // una fila checked conserva N; clic fuera
                                    // colapsa el menú a esa fila (misma regla
                                    // del '···').
                                    if response.secondary_clicked() {
                                        if !self.checked.contains(&track.id) {
                                            self.checked.clear();
                                            self.selected = Some(track.id);
                                            self.editor = None;
                                        }
                                    }
                                    if response.clicked() && !child_handled {
                                        if shift_held {
                                            // shift+clic de FILA: rango desde el ancla
                                            // dentro de la MISMA selección de
                                            // checkboxes — nunca la vacía.
                                            range_clicked = Some(track.id);
                                        } else {
                                            clicked_track = Some(track.id);
                                        }
                                    }
                                    if response.double_clicked() {
                                        double_clicked_track = Some(track.id);
                                    }
                                    // Menú contextual (click derecho): MISMO
                                    // ruteo del '···' — menu_route resuelve al
                                    // enum Action compartido o abre el MISMO
                                    // editor de tags de la vista.
                                    response.context_menu(|ui| {
                                        self.row_context_menu(ui, lang, model, track.id, actions);
                                    });
                                });
                            });
                        // devolver los anchos (el drag del header en este frame
                        // escribió en el local) — mem::take evita el préstamo de
                        // `self` dentro de las clausuras de la tabla.
                        self.col_widths = col_widths;
                    });
            });
            if let Some(field) = sort_clicked {
                let descending = self
                    .sort
                    .is_some_and(|(old, descending)| old == field && !descending);
                self.sort = Some((field, descending));
            }
            if let Some(select) = select_all_clicked {
                for &index in &visible {
                    let id = model.tracks[index].id;
                    if select {
                        self.checked.insert(id);
                    } else {
                        self.checked.remove(&id);
                    }
                }
                if !select {
                    // desmarcar TODO también suelta el ancla del rango
                    self.anchor = None;
                }
                self.editor = None;
            }
            if let Some(id) = clicked_track {
                self.selected = Some(id);
                self.checked.clear();
                // clic simple: la fila queda como ancla del próximo rango
                self.anchor = Some(id);
                self.editor = None;
                actions.push(Action::SelectTrack(id));
            }
            if let Some(id) = range_clicked {
                crate::studio_model::selection::range_add(
                    &mut self.checked,
                    &mut self.anchor,
                    id,
                    &visible_ids,
                );
                self.editor = None;
            }
            if let Some(id) = double_clicked_track {
                // doble clic = seleccionar y reproducir (comportamiento real)
                self.selected = Some(id);
                self.checked.clear();
                self.editor = None;
                actions.push(Action::SelectTrack(id));
                actions.push(Action::LoadAndPlay(id));
            }
            if let Some(id) = clicked_menu {
                self.selected = Some(id);
                self.checked.clear();
                self.editor = None;
                self.popover = Some(Popover::Track(id));
            }
        }
        Frame::none()
            .inner_margin(Margin::symmetric(15.0, 8.0))
            .show(ui, |ui| {
                // analyze-live: sin "Cargada en Deck A" a la derecha — solo
                // hay un deck por ahora y el indicador vive en la propia fila
                // (▶ teal); deck_track_id y la lógica de carga NO cambian.
                let footer = texts::t(lang, "studio.view_footer")
                    .replace("{n}", &visible.len().to_string())
                    .replace("{m}", &model.library_count.to_string());
                ui.label(studio::small(footer, studio::SUBTLE));
            });
    }

    fn tag_drawer(
        &mut self,
        ctx: &Context,
        lang: &str,
        library_rect: Rect,
        model: &Model,
        actions: &mut Vec<Action>,
    ) {
        let Some(mut draft) = self.editor.take() else {
            return;
        };
        let width = DRAWER_WIDTH.min(library_rect.width()).max(240.0);
        let pos = pos2(library_rect.right() - width, library_rect.top());
        let mut close = false;
        Area::new(Id::new("smart_mix_studio_tag_drawer"))
            .order(Order::Foreground)
            .fixed_pos(pos)
            .movable(false)
            .show(ctx, |ui| {
                ui.set_width(width);
                Frame::none()
                    .fill(studio::RAISED)
                    .stroke(Stroke::new(1.0, studio::LINE))
                    .inner_margin(Margin::same(17.0))
                    .show(ui, |ui| {
                        ui.set_width((width - 34.0).max(0.0));
                        ScrollArea::vertical()
                            .id_source("studio_tags_scroll")
                            .max_height((library_rect.height() - 34.0).max(120.0))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(texts::t(lang, "studio.tag_editor"))
                                                .size(16.0)
                                                .strong(),
                                        );
                                        ui.add_sized(
                                            vec2((width - 82.0).max(100.0), 18.0),
                                            Label::new(studio::small(
                                                &draft.heading,
                                                studio::SUBTLE,
                                            ))
                                            .truncate(true),
                                        );
                                    });
                                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                        if studio::icon_button(
                                            ui,
                                            studio::Icon::Close,
                                            &texts::t(lang, "studio.close_editor"),
                                        )
                                        .clicked()
                                        {
                                            close = true;
                                        }
                                    });
                                });
                                ui.add_space(13.0);
                                let multi = draft.ids.len() > 1;
                                let title_input = edit_text(
                                    ui,
                                    lang,
                                    &texts::t(lang, "studio.title"),
                                    &mut draft.title,
                                    &mut draft.changed.title,
                                    multi,
                                );
                                if draft.focus == Some(EditField::Title) {
                                    title_input.request_focus();
                                    draft.focus = None;
                                }
                                ui.add_space(11.0);
                                let _ = edit_text(
                                    ui,
                                    lang,
                                    &texts::t(lang, "studio.artist"),
                                    &mut draft.artist,
                                    &mut draft.changed.artist,
                                    multi,
                                );
                                ui.add_space(11.0);
                                ui.columns(2, |columns| {
                                    columns[0].label(studio::small(
                                        texts::t(lang, "studio.field_key"),
                                        studio::SECONDARY,
                                    ));
                                    let old = draft.key;
                                    ComboBox::from_id_source("studio_tag_key")
                                        .width(columns[0].available_width())
                                        .selected_text(
                                            draft
                                                .key
                                                .map(|key| key.to_string())
                                                .unwrap_or_else(|| "—".into()),
                                        )
                                        .show_ui(&mut columns[0], |ui| {
                                            for number in 1..=12u8 {
                                                for mode in [KeyMode::Minor, KeyMode::Major] {
                                                    let key = Key { number, mode };
                                                    ui.selectable_value(
                                                        &mut draft.key,
                                                        Some(key),
                                                        key.to_string(),
                                                    );
                                                }
                                            }
                                        });
                                    if draft.key != old {
                                        draft.changed.key = true;
                                    }
                                    if draft.focus == Some(EditField::Key) {
                                        // el combo de 0.27 no expone focus; el
                                        // campo BPM recibe el foco en su lugar
                                        draft.focus = Some(EditField::Bpm);
                                    }
                                    columns[1].label(studio::small(
                                        texts::t(lang, "studio.bpm_short"),
                                        studio::SECONDARY,
                                    ));
                                    let bpm_width = columns[1].available_width();
                                    let bpm = columns[1].add_sized(
                                        vec2(bpm_width, 32.0),
                                        TextEdit::singleline(&mut draft.bpm)
                                            .hint_text(texts::t(lang, "studio.bpm_hint")),
                                    );
                                    if bpm.changed() {
                                        draft.changed.bpm = true;
                                    }
                                    if draft.focus == Some(EditField::Bpm) {
                                        bpm.request_focus();
                                        draft.focus = None;
                                    }
                                });
                                ui.add_space(11.0);
                                ui.columns(2, |columns| {
                                    columns[0].label(studio::small(
                                        texts::t(lang, "studio.energy_short"),
                                        studio::SECONDARY,
                                    ));
                                    // Cierre: energía EDITABLE — combo activo sobre
                                    // set_energy de sm-db. El valor entra al patch
                                    // SOLO si el usuario lo cambió (None = no tocar).
                                    let old = draft.energy;
                                    ComboBox::from_id_source("studio_tag_energy")
                                        .width(columns[0].available_width())
                                        .selected_text(
                                            draft
                                                .energy
                                                .map(|energy| format!("{energy} / 10"))
                                                .unwrap_or_else(|| "—".into()),
                                        )
                                        .show_ui(&mut columns[0], |ui| {
                                            for energy in 1..=10u8 {
                                                ui.selectable_value(
                                                    &mut draft.energy,
                                                    Some(energy),
                                                    format!("{energy} / 10"),
                                                );
                                            }
                                        });
                                    if draft.energy != old {
                                        draft.changed.energy = true;
                                    }
                                    columns[1].label(studio::small(
                                        texts::t(lang, "studio.rating"),
                                        studio::SECONDARY,
                                    ));
                                    let old = draft.rating;
                                    ComboBox::from_id_source("studio_tag_rating")
                                        .width(columns[1].available_width())
                                        .selected_text(
                                            draft
                                                .rating
                                                .map(|rating| {
                                                    if rating == 0 {
                                                        texts::t(lang, "studio.rating_none")
                                                    } else {
                                                        format!("{rating} / 5")
                                                    }
                                                })
                                                .unwrap_or_else(|| "—".into()),
                                        )
                                        .show_ui(&mut columns[1], |ui| {
                                            for rating in 0..=5u8 {
                                                let label = if rating == 0 {
                                                    texts::t(lang, "studio.rating_none")
                                                } else {
                                                    format!("{rating} / 5")
                                                };
                                                ui.selectable_value(
                                                    &mut draft.rating,
                                                    Some(rating),
                                                    label,
                                                );
                                            }
                                        });
                                    if draft.rating != old {
                                        draft.changed.rating = true;
                                    }
                                });
                                ui.add_space(15.0);
                                ui.separator();
                                ui.add_space(5.0);
                                let details = if draft.ids.len() > 1 {
                                    texts::t(lang, "studio.details_multi")
                                } else if model
                                    .player
                                    .track
                                    .as_ref()
                                    .is_some_and(|track| draft.ids.contains(&track.id))
                                {
                                    format!(
                                        "{} · {}",
                                        model.player.format,
                                        model
                                            .player
                                            .track
                                            .as_ref()
                                            .and_then(|track| track.duration_seconds)
                                            .map(studio::format_time)
                                            .unwrap_or_else(|| "—".into())
                                    )
                                } else {
                                    texts::t(lang, "studio.details_meta")
                                };
                                ui.label(studio::small(details, studio::SUBTLE));
                                if let Some(error) = &draft.error {
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(error).size(14.0).color(studio::WARNING),
                                    );
                                }
                                ui.add_space(15.0);
                                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                    if studio::button(
                                        ui,
                                        &texts::t(lang, "detail.apply"),
                                        None,
                                        studio::Tone::Primary,
                                        78.0,
                                    )
                                    .clicked()
                                    {
                                        match draft.action(lang) {
                                            Ok(action) => {
                                                actions.push(action);
                                                close = true;
                                            }
                                            Err(error) => draft.error = Some(error),
                                        }
                                    }
                                    if studio::button(
                                        ui,
                                        &texts::t(lang, "studio.cancel"),
                                        None,
                                        studio::Tone::Quiet,
                                        78.0,
                                    )
                                    .clicked()
                                    {
                                        close = true;
                                    }
                                });
                            });
                    });
            });
        if !close {
            self.editor = Some(draft);
        }
    }

    fn popover(&mut self, ctx: &Context, lang: &str, model: &Model, actions: &mut Vec<Action>) {
        let Some(popover) = self.popover else { return };
        let mut close = false;
        let title = match popover {
            Popover::Library => texts::t(lang, "studio.pop_library"),
            Popover::Analysis => texts::t(lang, "studio.pop_analysis"),
            Popover::Pitch => texts::t(lang, "studio.pop_pitch"),
            Popover::AddToPlaylist => texts::t(lang, "studio.pop_playlist"),
            Popover::Track(id) => find_track(model, id)
                .map(|track| track.title.clone())
                .unwrap_or_else(|| texts::t(lang, "studio.pop_track")),
        };
        Window::new("smart_mix_studio_popover")
            .id(Id::new("studio_popover_window"))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::RIGHT_TOP, vec2(-14.0, 55.0))
            .default_width(300.0)
            .frame(
                Frame::none()
                    .fill(studio::RAISED)
                    .stroke(Stroke::new(1.0, studio::LINE))
                    .rounding(10.0)
                    .inner_margin(Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        vec2(232.0, 29.0),
                        Label::new(RichText::new(&title).size(17.0).strong()).truncate(true),
                    );
                    if studio::icon_button(
                        ui,
                        studio::Icon::Close,
                        &texts::t(lang, "studio.close_editor"),
                    )
                    .clicked()
                    {
                        close = true;
                    }
                });
                ui.add_space(8.0);
                match popover {
                    Popover::Library => {
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "studio.reanalyze_all"),
                            studio::Icon::Refresh,
                        )
                        .clicked()
                        {
                            actions.push(Action::ReanalyzeAll);
                            close = true;
                        }
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "studio.export_library"),
                            studio::Icon::Upload,
                        )
                        .clicked()
                        {
                            // ids vacíos = exportar la biblioteca COMPLETA
                            actions.push(Action::Export(Vec::new()));
                            close = true;
                        }
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "studio.view_duplicates"),
                            studio::Icon::Copy,
                        )
                        .clicked()
                        {
                            self.change_collection(Collection::Duplicates, actions);
                            close = true;
                        }
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "menu.reset_analysis_dev"),
                            studio::Icon::Refresh,
                        )
                        .clicked()
                        {
                            actions.push(Action::ResetAnalysisFull);
                            close = true;
                        }
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "menu.prefs"),
                            studio::Icon::Sliders,
                        )
                        .clicked()
                        {
                            actions.push(Action::OpenSettings);
                            close = true;
                        }
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "menu.about"),
                            studio::Icon::More,
                        )
                        .clicked()
                        {
                            actions.push(Action::OpenAbout);
                            close = true;
                        }
                    }
                    Popover::Track(id) => {
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "studio.edit_tags"),
                            studio::Icon::Tag,
                        )
                        .clicked()
                        {
                            self.open_editor(model, vec![id], EditField::Title);
                            close = true;
                        }
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "studio.export"),
                            studio::Icon::Upload,
                        )
                        .clicked()
                        {
                            actions.push(Action::Export(vec![id]));
                            close = true;
                        }
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "studio.reanalyze"),
                            studio::Icon::Refresh,
                        )
                        .clicked()
                        {
                            actions.push(Action::Analyze(vec![id]));
                            close = true;
                        }
                        let is_loaded = model
                            .player
                            .track
                            .as_ref()
                            .is_some_and(|track| track.id == id);
                        ui.add_enabled_ui(is_loaded, |ui| {
                            if studio::menu_button(
                                ui,
                                &texts::t(lang, "studio.edit_cues"),
                                studio::Icon::Sliders,
                            )
                            .clicked()
                            {
                                actions.push(Action::EditCues);
                                close = true;
                            }
                            if studio::menu_button(
                                ui,
                                &texts::t(lang, "studio.add_cue"),
                                studio::Icon::Plus,
                            )
                            .clicked()
                            {
                                actions.push(Action::AddCue);
                                close = true;
                            }
                        });
                    }
                    Popover::Analysis => {
                        let track = model.player.track.as_ref();
                        let items = [
                            (
                                texts::t(lang, "studio.field_key"),
                                track
                                    .and_then(|track| track.key)
                                    .map(|key| key.to_string())
                                    .unwrap_or_else(|| "—".into()),
                            ),
                            (
                                texts::t(lang, "studio.confidence"),
                                model
                                    .player
                                    .key_confidence
                                    .map(|value| format!("{:.0}%", value * 100.0))
                                    .unwrap_or_else(|| "—".into()),
                            ),
                            (
                                texts::t(lang, "studio.bpm_analyzed"),
                                track
                                    .and_then(|track| track.bpm)
                                    .map(|value| format!("{value:.2}"))
                                    .unwrap_or_else(|| "—".into()),
                            ),
                            (
                                texts::t(lang, "studio.energy_short"),
                                track
                                    .and_then(|track| track.energy)
                                    .map(|value| format!("{value} / 10"))
                                    .unwrap_or_else(|| "—".into()),
                            ),
                            (
                                texts::t(lang, "studio.sonority"),
                                model
                                    .player
                                    .lufs
                                    .map(|value| format!("{value:.1} LUFS"))
                                    .unwrap_or_else(|| "—".into()),
                            ),
                            (
                                texts::t(lang, "studio.clips_detected"),
                                model.player.clips.to_string(),
                            ),
                            (texts::t(lang, "studio.format"), model.player.format.clone()),
                        ];
                        for (name, value) in items {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(name).size(14.0).color(studio::SECONDARY));
                                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                    ui.label(RichText::new(value).size(14.0));
                                });
                            });
                            ui.add_space(3.0);
                        }
                        ui.add_space(9.0);
                        ui.separator();
                        ui.add_space(7.0);
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "studio.reanalyze"),
                            studio::Icon::Refresh,
                        )
                        .clicked()
                        {
                            if let Some(track) = track {
                                actions.push(Action::Analyze(vec![track.id]));
                            }
                            close = true;
                        }
                    }
                    Popover::Pitch => {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(texts::t(lang, "studio.pitch"))
                                    .color(studio::SECONDARY),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                ui.label(format!("{:+.1}%", model.player.pitch_percent));
                            });
                        });
                        let bounds = match model.player.pitch_range {
                            PitchRange::Six => Some((-6.0, 6.0)),
                            PitchRange::Sixteen => Some((-16.0, 16.0)),
                            PitchRange::Wide => model.player.wide_pitch_bounds,
                        };
                        if let Some((min, max)) = bounds
                            .filter(|(min, max)| min.is_finite() && max.is_finite() && min < max)
                        {
                            let mut value = model.player.pitch_percent;
                            ui.spacing_mut().slider_width = ui.available_width();
                            // knob discreto también en el popover de pitch
                            ui.style_mut().spacing.interact_size.y = 16.0;
                            if ui
                                .add(
                                    Slider::new(&mut value, min..=max)
                                        .step_by(0.1)
                                        .show_value(false),
                                )
                                .changed()
                            {
                                actions.push(Action::SetPitch(value));
                            }
                        }
                        ui.add_space(7.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(texts::t(lang, "studio.range"))
                                    .color(studio::SECONDARY),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                let mut range = model.player.pitch_range;
                                let selected = match range {
                                    PitchRange::Six => "±6%",
                                    PitchRange::Sixteen => "±16%",
                                    PitchRange::Wide => "W",
                                };
                                ComboBox::from_id_source("studio_pitch_range")
                                    .selected_text(selected)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut range, PitchRange::Six, "±6%");
                                        ui.selectable_value(
                                            &mut range,
                                            PitchRange::Sixteen,
                                            "±16%",
                                        );
                                        ui.add_enabled_ui(
                                            model.player.wide_pitch_bounds.is_some(),
                                            |ui| {
                                                ui.selectable_value(
                                                    &mut range,
                                                    PitchRange::Wide,
                                                    "W",
                                                )
                                                .on_hover_text(texts::t(
                                                    lang,
                                                    "studio.range_wide_hover",
                                                ));
                                            },
                                        );
                                    });
                                if range != model.player.pitch_range {
                                    actions.push(Action::SetPitchRange(range));
                                }
                            });
                        });
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(7.0);
                        if studio::button(
                            ui,
                            &texts::t(lang, "studio.reset_pitch"),
                            None,
                            studio::Tone::Raised,
                            136.0,
                        )
                        .clicked()
                        {
                            actions.push(Action::SetPitch(0.0));
                        }
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "studio.edit_cues"),
                            studio::Icon::Sliders,
                        )
                        .clicked()
                        {
                            actions.push(Action::EditCues);
                            close = true;
                        }
                    }
                    Popover::AddToPlaylist => {
                        for playlist in &model.playlists {
                            if studio::menu_button(ui, &playlist.name, studio::Icon::List).clicked()
                            {
                                actions.push(Action::AddToPlaylist {
                                    playlist_id: playlist.id,
                                    track_ids: self.target_ids(model),
                                });
                                close = true;
                            }
                        }
                    }
                }
            });
        if close {
            self.popover = None;
        }
    }

    /// Menú contextual de fila (click derecho), adaptativo 1 vs N pistas:
    /// la VISIBILIDAD la fija `studio_model::context_menu_items` (con N>1
    /// solo acciones seguras en lote) y el RUTEO es el MISMO del menú
    /// '···' — `menu_route` resuelve al enum `Action` compartido o abre el
    /// MISMO editor de tags de la vista. Cero lógica duplicada.
    fn row_context_menu(
        &mut self,
        ui: &mut Ui,
        lang: &str,
        model: &Model,
        row_id: TrackId,
        actions: &mut Vec<Action>,
    ) {
        // La selección sobre la que abre el menú es la que se VE: la fila
        // está dentro de la selección de checkboxes → N pistas; si no, el
        // clic derecho ya colapsó la selección a esta fila (ver caller).
        let targets: Vec<TrackId> = if !self.checked.is_empty() && self.checked.contains(&row_id) {
            self.checked.iter().copied().collect()
        } else {
            vec![row_id]
        };
        if targets.len() > 1 {
            ui.label(studio::small(
                texts::t(lang, "studio.n_tracks").replace("{n}", &targets.len().to_string()),
                studio::SUBTLE,
            ));
            ui.add_space(4.0);
        }
        let track = find_track(model, row_id);
        let has_key = track.and_then(|track| track.key).is_some();
        let has_playlists = !model.playlists.is_empty();
        for item in context_menu_items(targets.len()).iter().copied() {
            match item {
                TrackMenuItem::Load => {
                    if studio::menu_button(ui, &texts::t(lang, "ctx.load"), studio::Icon::Play)
                        .clicked()
                    {
                        self.run_menu_route(ui, model, &targets, item, MenuParam::None, actions);
                    }
                }
                TrackMenuItem::Preview => {
                    if studio::menu_button(ui, &texts::t(lang, "ctx.preview"), studio::Icon::Disc)
                        .clicked()
                    {
                        self.run_menu_route(ui, model, &targets, item, MenuParam::None, actions);
                    }
                }
                TrackMenuItem::FilterCompatible => {
                    // solo si la pista tiene tono: sin key no hay rueda que activar
                    let key = track.and_then(|track| track.key);
                    ui.add_enabled_ui(has_key, |ui| {
                        if studio::menu_button(
                            ui,
                            &texts::t(lang, "ctx.filter_compatible"),
                            studio::Icon::Library,
                        )
                        .clicked()
                        {
                            self.run_menu_route(
                                ui,
                                model,
                                &targets,
                                item,
                                key.map(MenuParam::Key).unwrap_or(MenuParam::None),
                                actions,
                            );
                        }
                    });
                }
                TrackMenuItem::EditTags => {
                    if studio::menu_button(
                        ui,
                        &texts::t(lang, "studio.edit_tags"),
                        studio::Icon::Tag,
                    )
                    .clicked()
                    {
                        self.run_menu_route(ui, model, &targets, item, MenuParam::None, actions);
                    }
                }
                TrackMenuItem::Energy => {
                    // submenú 1..=10 (la escala del editor de tags)
                    ui.menu_button(texts::t(lang, "ctx.energy"), |ui| {
                        for level in 1..=10u8 {
                            if studio::menu_button(
                                ui,
                                &format!("{level} / 10"),
                                studio::Icon::Sliders,
                            )
                            .clicked()
                            {
                                self.run_menu_route(
                                    ui,
                                    model,
                                    &targets,
                                    item,
                                    MenuParam::Energy(level),
                                    actions,
                                );
                            }
                        }
                    });
                }
                TrackMenuItem::Rating => {
                    // submenú 0..=5 (0 = sin valorar) — mismas estrellas de la tabla
                    ui.menu_button(texts::t(lang, "ctx.rating"), |ui| {
                        for rating in 0..=5u8 {
                            let label = if rating == 0 {
                                texts::t(lang, "studio.rating_none")
                            } else {
                                format!("{} {rating}/5", "★".repeat(rating as usize))
                            };
                            if studio::menu_button(ui, &label, studio::Icon::Plus).clicked() {
                                self.run_menu_route(
                                    ui,
                                    model,
                                    &targets,
                                    item,
                                    MenuParam::Rating(rating),
                                    actions,
                                );
                            }
                        }
                    });
                }
                TrackMenuItem::AddToPlaylist => {
                    // submenú de playlists (igual que el popover: mismas
                    // Action::AddToPlaylist con el lote completo)
                    ui.add_enabled_ui(has_playlists, |ui| {
                        ui.menu_button(texts::t(lang, "detail.add_to_playlist"), |ui| {
                            for playlist in &model.playlists {
                                if studio::menu_button(ui, &playlist.name, studio::Icon::List)
                                    .clicked()
                                {
                                    self.run_menu_route(
                                        ui,
                                        model,
                                        &targets,
                                        item,
                                        MenuParam::Playlist(playlist.id),
                                        actions,
                                    );
                                }
                            }
                        });
                    });
                }
                TrackMenuItem::Reanalyze => {
                    if studio::menu_button(
                        ui,
                        &texts::t(lang, "studio.reanalyze"),
                        studio::Icon::Refresh,
                    )
                    .clicked()
                    {
                        self.run_menu_route(ui, model, &targets, item, MenuParam::None, actions);
                    }
                }
                TrackMenuItem::ViewDetails => {
                    if studio::menu_button(ui, &texts::t(lang, "menu.detail"), studio::Icon::More)
                        .clicked()
                    {
                        self.run_menu_route(ui, model, &targets, item, MenuParam::None, actions);
                    }
                }
                TrackMenuItem::Reveal => {
                    if studio::menu_button(ui, &texts::t(lang, "ctx.reveal"), studio::Icon::Usb)
                        .clicked()
                    {
                        self.run_menu_route(ui, model, &targets, item, MenuParam::None, actions);
                    }
                }
                TrackMenuItem::CopyTitle => {
                    if studio::menu_button(
                        ui,
                        &texts::t(lang, "ctx.copy_title"),
                        studio::Icon::Copy,
                    )
                    .clicked()
                    {
                        self.run_menu_route(ui, model, &targets, item, MenuParam::None, actions);
                    }
                }
                TrackMenuItem::CopyArtist => {
                    if studio::menu_button(
                        ui,
                        &texts::t(lang, "ctx.copy_artist"),
                        studio::Icon::Copy,
                    )
                    .clicked()
                    {
                        self.run_menu_route(ui, model, &targets, item, MenuParam::None, actions);
                    }
                }
                TrackMenuItem::CopyPath => {
                    if studio::menu_button(ui, &texts::t(lang, "ctx.copy_path"), studio::Icon::Copy)
                        .clicked()
                    {
                        self.run_menu_route(ui, model, &targets, item, MenuParam::None, actions);
                    }
                }
                TrackMenuItem::Remove => {
                    // SOLO intención: la app pide confirmación con conteo y
                    // NUNCA borra archivos (ventana existente de windows.rs).
                    if studio::menu_button(ui, &texts::t(lang, "ctx.remove"), studio::Icon::Close)
                        .clicked()
                    {
                        self.run_menu_route(ui, model, &targets, item, MenuParam::None, actions);
                    }
                }
                TrackMenuItem::ExportQueue => {
                    if studio::menu_button(
                        ui,
                        &texts::t(lang, "ctx.export_queue"),
                        studio::Icon::Upload,
                    )
                    .clicked()
                    {
                        self.run_menu_route(ui, model, &targets, item, MenuParam::None, actions);
                    }
                }
            }
        }
    }

    /// Ejecuta el ruteo COMPARTIDO de los menús de fila: la resolución la
    /// hace `studio_model::menu_route` (única fuente de verdad) y aquí
    /// solo se despacha — mismas `Action` al mismo dispatcher de la app, o
    /// el MISMO `open_editor` que usa el menú '···' de la fila.
    fn run_menu_route(
        &mut self,
        ui: &mut Ui,
        model: &Model,
        targets: &[TrackId],
        item: TrackMenuItem,
        param: MenuParam,
        actions: &mut Vec<Action>,
    ) {
        match menu_route(item, targets, param) {
            Some(MenuRoute::Actions(list)) => {
                actions.extend(list);
                ui.close_menu();
            }
            Some(MenuRoute::TagEditor(ids)) => {
                self.open_editor(model, ids, EditField::Title);
                ui.close_menu();
            }
            None => {}
        }
    }

    fn visible_indices(&self, model: &Model) -> Vec<usize> {
        // El host (app real) filtra por compatibilidad/búsqueda/estado en la
        // BD; aquí SOLO se ordena lo recibido.
        let mut indices: Vec<usize> = (0..model.tracks.len()).collect();
        if let Some((field, descending)) = self.sort {
            indices.sort_by(|&left, &right| {
                compare_tracks(&model.tracks[left], &model.tracks[right], field, descending)
            });
        }
        indices
    }

    fn collection_title(&self, lang: &str, model: &Model) -> String {
        match self.collection {
            Collection::All => texts::t(lang, "col.all"),
            Collection::Unanalyzed => texts::t(lang, "col.unanalyzed"),
            Collection::Duplicates => texts::t(lang, "col.duplicates"),
            Collection::Playlist(id) => model
                .playlists
                .iter()
                .find(|playlist| playlist.id == id)
                .map(|playlist| playlist.name.clone())
                .unwrap_or_else(|| texts::t(lang, "side.playlists")),
        }
    }

    fn target_ids(&self, model: &Model) -> Vec<TrackId> {
        if !self.checked.is_empty() {
            return self.checked.iter().copied().collect();
        }
        self.selected
            .filter(|&id| find_track(model, id).is_some())
            .into_iter()
            .collect()
    }

    fn change_collection(&mut self, collection: Collection, actions: &mut Vec<Action>) {
        self.collection = collection;
        self.checked.clear();
        self.selected = None;
        self.editor = None;
        self.popover = None;
        actions.push(Action::SetCollection(collection));
    }

    pub(crate) fn open_editor(&mut self, model: &Model, ids: Vec<TrackId>, field: EditField) {
        self.editor = TagDraft::from_model(model, &ids);
        if let Some(draft) = &mut self.editor {
            draft.focus = Some(field);
        }
        self.popover = None;
    }

    fn toggle_popover(&mut self, popover: Popover) {
        self.popover = if self.popover == Some(popover) {
            None
        } else {
            Some(popover)
        };
    }
}

fn find_track<'a>(model: &'a Model, id: TrackId) -> Option<&'a Track> {
    model
        .tracks
        .iter()
        .find(|track| track.id == id)
        .or_else(|| model.player.track.as_ref().filter(|track| track.id == id))
}

/// UV de recorte centrado (aspect-fill) para la miniatura de portada: la
/// celda es cuadrada 28×28 y el arte puede no serlo.
fn cover_uv(size: egui::Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    if size.x > size.y {
        let visible = size.y / size.x;
        Rect::from_min_max(
            pos2(0.5 - visible * 0.5, 0.0),
            pos2(0.5 + visible * 0.5, 1.0),
        )
    } else {
        let visible = size.x / size.y;
        Rect::from_min_max(
            pos2(0.0, 0.5 - visible * 0.5),
            pos2(1.0, 0.5 + visible * 0.5),
        )
    }
}

/// Anillo de FUNDIDO RADIAL (fix ui-rb2): mesh con alpha por VÉRTICE que
/// pasa de `color` totalmente transparente en `r_inner` a opaco en
/// `r_outer`. Se pinta ENCIMA del borde de la portada circular del centro
/// de la rueda para desvanecerla hacia el fondo del panel ("borde
/// levemente difuminado"). Los colores por vértice van en premult (los
/// construye from_rgba_unmultiplied) para que la interpolación del mesh
/// no deje franjas oscuras.
fn paint_radial_fade(painter: &Painter, center: Pos2, r_inner: f32, r_outer: f32, color: Color32) {
    const SEGMENTS: usize = 48;
    let transparent = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 0);
    let mut mesh = egui::epaint::Mesh::default();
    for i in 0..=SEGMENTS {
        let angle = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let dir = vec2(angle.cos(), angle.sin());
        mesh.colored_vertex(center + dir * r_inner, transparent);
        mesh.colored_vertex(center + dir * r_outer, color);
    }
    for i in 0..SEGMENTS {
        let a = (i * 2) as u32;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    painter.add(Shape::Mesh(mesh));
}

/// Triángulo del indicador de orden — painter puro (NADA de glifos unicode:
/// con Barlow cualquier ▲/▼ arriesga tofu). `up` = ascendente.
fn draw_sort_triangle(painter: &Painter, center: Pos2, up: bool, half: f32, color: Color32) {
    let vertical = half * 0.8;
    let points = if up {
        vec![
            pos2(center.x - half, center.y + vertical * 0.5),
            pos2(center.x + half, center.y + vertical * 0.5),
            pos2(center.x, center.y - vertical * 0.5),
        ]
    } else {
        vec![
            pos2(center.x - half, center.y - vertical * 0.5),
            pos2(center.x + half, center.y - vertical * 0.5),
            pos2(center.x, center.y + vertical * 0.5),
        ]
    };
    painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
}

/// Ancho de la zona del indicador de orden en el header (el grupo
/// label+indicador se mide con este valor; también entra en el chrome del
/// test de mínimos de columna).
const HEADER_INDICATOR_W: f32 = 13.0;

/// Celda de header sorteable: botón transparente (receta probada 0.27) +
/// indicador geométrico — triángulo SECONDARY del campo ACTIVO (▲ asc /
/// ▼ desc) y doble flecha tenue en hover para el resto. El GRUPO
/// label+indicador se desplaza según `align` — la MISMA alineación de la
/// columna (COL_ALIGNS), así el header queda coaxial con sus datos (fix
/// table-rb: antes TODO el header iba a la izquierda aunque el contenido
/// de BPM/duración quedara a la derecha).
fn sort_header_cell(
    ui: &mut Ui,
    field: SortField,
    label: &str,
    align: ColAlign,
    sort: Option<(SortField, bool)>,
    hint: &str,
    sort_clicked: &mut Option<SortField>,
) {
    // ancho del BOTÓN con el layout REAL (galley + padding) para
    // desplazarlo según la alineación. En Center se centra el BOTÓN solo:
    // el indicador solo se dibuja en hover/orden activo y no debe sesgar
    // el texto (medido en captura: el texto quedaba ~7px a la izquierda).
    // En Right el grupo es [indicador][label]: el LABEL queda pegado al
    // borde derecho — coaxial con los números de la columna.
    let text_w = ui
        .painter()
        .layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(studio::FONT_SMALL),
            studio::SUBTLE,
        )
        .size()
        .x;
    let button_w = text_w + ui.style().spacing.button_padding.x * 2.0;
    let avail = ui.max_rect().width();
    let (lead, indicator_first) = match align {
        ColAlign::Left => (0.0, false),
        ColAlign::Center => (((avail - button_w) * 0.5).max(0.0), false),
        ColAlign::Right => ((avail - button_w - HEADER_INDICATOR_W).max(0.0), true),
    };
    if lead > 0.0 {
        ui.add_space(lead);
    }
    ui.horizontal(|ui| {
        let indicator_before = indicator_first.then(|| {
            let (rect, _) = ui.allocate_exact_size(vec2(HEADER_INDICATOR_W, 12.0), Sense::hover());
            rect
        });
        let button = egui::Button::new(studio::small(label, studio::SUBTLE))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE);
        let response = ui.add(button).on_hover_text(hint);
        if response.clicked() {
            *sort_clicked = Some(field);
        }
        let indicator = match indicator_before {
            Some(rect) => rect,
            None => {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(HEADER_INDICATOR_W, 12.0), Sense::hover());
                rect
            }
        };
        let center = pos2(indicator.left() + 5.0, indicator.center().y);
        match sort {
            Some((active, descending)) if active == field => {
                draw_sort_triangle(ui.painter(), center, !descending, 4.0, studio::SECONDARY);
            }
            _ if response.hovered() => {
                // doble flecha tenue: "este campo también se ordena"
                let faint = studio::SUBTLE.gamma_multiply(0.7);
                draw_sort_triangle(
                    ui.painter(),
                    pos2(center.x, center.y - 3.5),
                    true,
                    3.0,
                    faint,
                );
                draw_sort_triangle(
                    ui.painter(),
                    pos2(center.x, center.y + 3.5),
                    false,
                    3.0,
                    faint,
                );
            }
            _ => {}
        }
    });
}

/// Id estable del asa de resize de la columna `c` del header.
fn col_resize_id(c: usize) -> Id {
    Id::new("studio_tbl_col_resize").with(c)
}

/// Hijo de `ui` en un rect EXACTO sin avanzar el cursor del padre: el
/// contenido debe alojar su tamaño natural dentro (una sola asignación —
/// evita la doble asignación del kit).
fn cell_at(ui: &mut Ui, id: impl std::hash::Hash, rect: Rect, f: impl FnOnce(&mut Ui)) {
    let mut child = ui.child_ui_with_id_source(rect, Layout::left_to_right(Align::Center), id);
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    f(&mut child);
}

/// Centra un widget de tamaño exacto `size` en la celda actual (checkbox,
/// key badge, energía, rating, menú): la alineación la define la columna
/// (COL_ALIGNS) y header + celdas la comparten (fix table-rb).
fn center_cell(ui: &mut Ui, id: impl std::hash::Hash, size: egui::Vec2, f: impl FnOnce(&mut Ui)) {
    let cell = ui.max_rect();
    let rect = Rect::from_center_size(cell.center(), size);
    cell_at(ui, id, rect, f);
}

/// Asa de resize MANUAL para una columna del header: franja de 8px en el
/// borde derecho de la celda con `Sense::click_and_drag`. egui_extras NO
/// participa (la tabla va `resizable(false)`): el ancho vive en
/// `StudioUi::col_widths`, así el escalado por ventana (k) sigue
/// aplicándose a las columnas que el usuario NO arrastró. El asa se
/// registra DESPUÉS del botón de sort de la misma celda → gana el
/// hit-test dentro de su franja (VERIFICADO con probe de drag sintético:
/// hover + dragged + acumulación de delta funcionan). El problema real
/// era de DESCUBRIBILIDAD: la franja era invisible → ahora lleva un grip
/// de dos rayitas verticales SIEMPRE pintado (tenue) que se ilumina en
/// hover, más el cursor ResizeHorizontal y la guía de borde.
/// fix ui-rb2: SOLO título y artista registran asa; el rango puede ser
/// DINÁMICO (el drag del artista clampa al sobrante tras fijas+título
/// mínimo, y viceversa) — la suma nunca desborda el ancho disponible.
fn resize_handle(
    ui: &mut Ui,
    id: Id,
    current: f32,
    range: (f32, f32),
    user_width: &mut Option<f32>,
) {
    let cell = ui.max_rect();
    let strip = Rect::from_min_max(
        pos2(cell.right() - 8.0, cell.top()),
        pos2(cell.right(), cell.bottom()),
    );
    let response = ui.interact(strip, id, Sense::click_and_drag());
    let active = response.hovered() || response.dragged();
    // Grip visible (2 rayitas verticales): SIEMPRE tenue, LILAC en hover.
    let grip_color = if active {
        studio::LILAC
    } else {
        studio::SUBTLE.gamma_multiply(0.6)
    };
    let grip_y0 = strip.center().y - 5.0;
    for dx in [-2.0f32, 2.0] {
        let x = strip.center().x + dx;
        ui.painter().line_segment(
            [pos2(x, grip_y0), pos2(x, grip_y0 + 10.0)],
            Stroke::new(1.5, grip_color),
        );
    }
    if active {
        ui.output_mut(|output| output.cursor_icon = CursorIcon::ResizeHorizontal);
        // guía visual del borde mientras el asa está viva
        ui.painter().vline(
            cell.right(),
            cell.y_range(),
            Stroke::new(2.0, studio::LILAC),
        );
    }
    if response.dragged() {
        // drag_delta = movimiento del puntero EN ESTE frame → acumular
        // sobre el ancho efectivo actual, con clamp al rango de la columna
        *user_width = Some((current + response.drag_delta().x).clamp(range.0, range.1));
    }
}

fn compare_tracks(left: &Track, right: &Track, field: SortField, descending: bool) -> Ordering {
    let compare = match field {
        // '#' = orden original de la biblioteca (id real de sm-db)
        SortField::Number => left.id.cmp(&right.id),
        SortField::Title => left.title.to_lowercase().cmp(&right.title.to_lowercase()),
        SortField::Artist => left.artist.to_lowercase().cmp(&right.artist.to_lowercase()),
        SortField::Rating => left.rating.cmp(&right.rating),
        SortField::Key => {
            return compare_optional(left.key, right.key, descending, |a, b| a.cmp(&b))
        }
        SortField::Bpm => {
            return compare_optional(left.bpm, right.bpm, descending, |a, b| a.total_cmp(&b))
        }
        SortField::Energy => {
            return compare_optional(left.energy, right.energy, descending, |a, b| a.cmp(&b))
        }
        SortField::Duration => {
            return compare_optional(
                left.duration_seconds,
                right.duration_seconds,
                descending,
                |a, b| a.total_cmp(&b),
            )
        }
    };
    if descending {
        compare.reverse()
    } else {
        compare
    }
}

fn compare_optional<T>(
    left: Option<T>,
    right: Option<T>,
    descending: bool,
    compare: impl FnOnce(T, T) -> Ordering,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => {
            let order = compare(left, right);
            if descending {
                order.reverse()
            } else {
                order
            }
        }
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

fn edit_text(
    ui: &mut Ui,
    lang: &str,
    label: &str,
    text: &mut String,
    changed: &mut bool,
    multiple: bool,
) -> Response {
    ui.label(studio::small(label, studio::SECONDARY));
    let hint = if multiple {
        texts::t(lang, "studio.multivalued")
    } else {
        "—".to_string()
    };
    let response = ui.add_sized(
        vec2(ui.available_width(), 32.0),
        TextEdit::singleline(text).hint_text(hint),
    );
    if response.changed() {
        *changed = true;
    }
    response
}

/// Tonalidad bajo el puntero en la rueda (geometría del kit: minor interna,
/// major externa, 12 arriba, horario, gajo de 30°).
fn wheel_key_at(point: Pos2, center: Pos2, radius: f32) -> Option<Key> {
    let offset = point - center;
    let distance = offset.length();
    if distance < radius * 0.52 || distance > radius {
        return None;
    }
    let sector =
        ((offset.y.atan2(offset.x).to_degrees() + 105.0).rem_euclid(360.0) / 30.0).floor() as u8;
    let number = if sector == 0 { 12 } else { sector };
    let mode = if distance < radius * 0.77 {
        KeyMode::Minor
    } else {
        KeyMode::Major
    };
    Key::new(number, mode)
}

/// Cuerpo tipográfico del tono elegido en el centro de la rueda (modo
/// filtro) — el tamaño del texto grande previo al fix ui-rb2.
const WHEEL_CENTER_FONT: f32 = 27.0;

/// Qué pinta el centro de la rueda (cierre ítem 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WheelCenter {
    /// Modo FILTRO: la tonalidad elegida por el usuario en texto grande.
    Key(Key),
    /// Modo neutro: portada de la pista cargada (None = fallback vinilo).
    Cover(Option<TrackId>),
}

/// Decisión PURA del centro de la rueda: la tonalidad elegida manda sobre
/// la portada — portada SOLO al cargar pista (sin filtro activo), tono
/// SOLO al filtrar. Nunca ambos a la vez.
fn wheel_center(compatibility: Option<Key>, loaded_id: Option<TrackId>) -> WheelCenter {
    match compatibility {
        Some(key) => WheelCenter::Key(key),
        None => WheelCenter::Cover(loaded_id),
    }
}

fn annular_sector(
    painter: &Painter,
    center: Pos2,
    inner: f32,
    outer: f32,
    start: f32,
    end: f32,
    color: Color32,
) {
    let steps = 10_u32;
    let mut mesh = egui::epaint::Mesh::default();
    for index in 0..=steps {
        let angle = start + (end - start) * index as f32 / steps as f32;
        let vector = vec2(angle.cos(), angle.sin());
        mesh.colored_vertex(center + vector * outer, color);
        mesh.colored_vertex(center + vector * inner, color);
    }
    for index in 0..steps {
        let v = index * 2;
        mesh.add_triangle(v, v + 1, v + 2);
        mesh.add_triangle(v + 1, v + 3, v + 2);
    }
    painter.add(Shape::mesh(mesh));
}

// ---- TagDraft del kit (IDs capturados al abrir; el clic en otra fila JAMÁS
// ---- redirige un draft ya abierto a otro archivo de audio) ---------------

#[derive(Default)]
pub(crate) struct ChangedFields {
    pub title: bool,
    pub artist: bool,
    pub key: bool,
    pub bpm: bool,
    pub energy: bool,
    pub rating: bool,
}

pub(crate) struct TagDraft {
    pub(crate) ids: Vec<TrackId>,
    pub(crate) heading: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) key: Option<Key>,
    pub(crate) bpm: String,
    pub(crate) energy: Option<u8>,
    pub(crate) rating: Option<u8>,
    pub(crate) changed: ChangedFields,
    pub(crate) error: Option<String>,
    pub(crate) focus: Option<EditField>,
}

impl TagDraft {
    pub(crate) fn from_model(model: &Model, ids: &[TrackId]) -> Option<Self> {
        let unique: BTreeSet<TrackId> = ids.iter().copied().collect();
        let tracks: Vec<&Track> = unique
            .iter()
            .filter_map(|&id| find_track(model, id))
            .collect();
        // Nunca soltar en silencio IDs desconocidos de una edición múltiple.
        if tracks.is_empty() || tracks.len() != unique.len() {
            return None;
        }
        let first = tracks[0];
        Some(Self {
            ids: unique.into_iter().collect(),
            heading: if tracks.len() == 1 {
                first.title.clone()
            } else {
                format!("{} pistas seleccionadas", tracks.len())
            },
            title: if tracks.iter().all(|track| track.title == first.title) {
                first.title.clone()
            } else {
                String::new()
            },
            artist: if tracks.iter().all(|track| track.artist == first.artist) {
                first.artist.clone()
            } else {
                String::new()
            },
            key: if tracks.iter().all(|track| track.key == first.key) {
                first.key
            } else {
                None
            },
            bpm: if tracks.iter().all(|track| track.bpm == first.bpm) {
                first.bpm.map(|bpm| format!("{bpm:.2}")).unwrap_or_default()
            } else {
                String::new()
            },
            energy: if tracks.iter().all(|track| track.energy == first.energy) {
                first.energy
            } else {
                None
            },
            rating: if tracks.iter().all(|track| track.rating == first.rating) {
                Some(first.rating)
            } else {
                None
            },
            changed: ChangedFields::default(),
            error: None,
            focus: None,
        })
    }

    pub(crate) fn action(&self, lang: &str) -> Result<Action, String> {
        let mut patch = crate::studio_model::TagPatch::default();
        if self.changed.title {
            if self.title.trim().is_empty() {
                return Err(texts::t(lang, "studio.err_title"));
            }
            patch.title = Some(self.title.trim().to_owned());
        }
        if self.changed.artist {
            patch.artist = Some(self.artist.trim().to_owned());
        }
        if self.changed.key {
            patch.key = self.key;
        }
        if self.changed.bpm {
            let value = self
                .bpm
                .trim()
                .replace(',', ".")
                .parse::<f32>()
                .map_err(|_| texts::t(lang, "studio.err_bpm"))?;
            if !value.is_finite() || value <= 0.0 {
                return Err(texts::t(lang, "studio.err_bpm_finite"));
            }
            patch.bpm = Some(value);
        }
        if self.changed.energy {
            // Cierre: energía editable — se emite SOLO el valor cambiado
            // (None en el patch = la pista no se toca).
            patch.energy = self.energy;
        }
        if self.changed.rating {
            patch.rating = self.rating;
        }
        Ok(Action::ApplyTags {
            track_ids: self.ids.clone(),
            patch,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --------------------------- analyze-live: status bar rotativo --------

    #[test]
    fn status_phase_rotates_text_and_bar_every_5s() {
        // fase TEXTO los primeros 5 s
        for t in [0.0, 2.5, 4.999] {
            assert!(!analysis_phase_shows_bar(t), "t={t} debería ser texto");
        }
        // fase BARRA de 5 s a 10 s
        for t in [5.0, 7.5, 9.999] {
            assert!(analysis_phase_shows_bar(t), "t={t} debería ser barra");
        }
        // envuelve: el reloj de egui crece sin límite, el toggle es periódico
        for t in [10.0, 12.5, 14.999, 20.0, 24.999] {
            assert!(!analysis_phase_shows_bar(t), "t={t} debería ser texto");
        }
        assert!(analysis_phase_shows_bar(15.0));
        assert!(analysis_phase_shows_bar(25.0));
        assert!(analysis_phase_shows_bar(35.0));
        // consistencia con tiempos negativos (no ocurren en egui, pero la
        // función debe ser total y sin pánico)
        assert_eq!(
            analysis_phase_shows_bar(-1.0),
            analysis_phase_shows_bar(9.0)
        );
    }

    #[test]
    fn analysis_percent_clamps_and_never_divides_by_zero() {
        assert_eq!(analysis_percent(0, 10), 0.0);
        assert_eq!(analysis_percent(5, 10), 50.0);
        assert_eq!(analysis_percent(10, 10), 100.0);
        // done>total (re-encolado a mitad de sesión) → clamp al 100 %
        assert_eq!(analysis_percent(12, 10), 100.0);
        // total 0 → 0, nunca NaN
        assert_eq!(analysis_percent(3, 0), 0.0);
        assert!(analysis_percent(1, 3).is_finite());
    }

    #[test]
    fn short_analyzing_title_is_a_truncated_file_stem() {
        assert_eq!(
            short_analyzing_title(&format!(
                "C:{}Música{}Upper Class.mp3",
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR
            )),
            "Upper Class"
        );
        assert_eq!(short_analyzing_title("sin-extensión"), "sin-extensión");
        assert_eq!(short_analyzing_title(""), "");
        let long = "A".repeat(80);
        let got = short_analyzing_title(format!("/x/{long}.flac").as_str());
        assert_eq!(got.chars().count(), ANALYZING_TITLE_MAX);
        assert!(got.ends_with('…'), "debe terminar en elipsis: {got}");
        assert_eq!(
            got.chars().filter(|c| *c == 'A').count(),
            ANALYZING_TITLE_MAX - 1
        );
        // justo en el límite: sin elipsis
        let edge = "B".repeat(ANALYZING_TITLE_MAX);
        assert_eq!(short_analyzing_title(format!("{edge}.wav").as_str()), edge);
    }

    fn sample_track(id: TrackId, title: &str, bpm: f32) -> Track {
        Track {
            id,
            title: title.into(),
            artist: "Artist".into(),
            key: "7A".parse().ok(),
            bpm: Some(bpm),
            energy: Some(5),
            rating: 0,
            duration_seconds: Some(259.0),
        }
    }

    #[test]
    fn tag_draft_keeps_original_ids_when_ui_selection_changes() {
        let model = Model {
            tracks: vec![
                sample_track(1, "First", 120.0),
                sample_track(2, "Second", 125.0),
            ],
            ..Model::default()
        };
        let mut draft = TagDraft::from_model(&model, &[1]).unwrap();
        draft.title = "Edited first".into();
        draft.changed.title = true;
        let mut ui = StudioUi::new();
        ui.selected = Some(2);
        let Action::ApplyTags { track_ids, patch } = draft.action("es").unwrap() else {
            panic!("wrong action");
        };
        assert_eq!(track_ids, [1]);
        assert_eq!(patch.title.as_deref(), Some("Edited first"));
        assert_eq!(ui.selected_track(), Some(2));
    }

    #[test]
    fn mixed_values_are_not_blank_overwrites_in_a_bulk_edit() {
        let model = Model {
            tracks: vec![
                sample_track(1, "First", 120.0),
                sample_track(2, "Second", 125.0),
            ],
            ..Model::default()
        };
        let draft = TagDraft::from_model(&model, &[1, 2]).unwrap();
        assert!(draft.bpm.is_empty());
        let Action::ApplyTags { track_ids, patch } = draft.action("es").unwrap() else {
            panic!("wrong action");
        };
        assert_eq!(track_ids, [1, 2]);
        assert!(patch.bpm.is_none());
        assert!(patch.title.is_none());
    }

    /// Cierre (energía editable): el combo emite el valor SOLO cuando el
    /// usuario lo cambió; sin cambio, el patch viaja con energy=None (la
    /// pista no se toca) — mismo contrato que key/bpm/rating.
    #[test]
    fn energy_is_emitted_only_when_changed() {
        let model = Model {
            tracks: vec![sample_track(1, "First", 120.0)],
            ..Model::default()
        };
        // sin tocar el combo → None (no tocar la pista)
        let draft = TagDraft::from_model(&model, &[1]).unwrap();
        let Action::ApplyTags { patch, .. } = draft.action("es").unwrap() else {
            panic!("wrong action");
        };
        assert_eq!(patch.energy, None);

        // cambio manual → valor nuevo en el patch
        let mut draft = TagDraft::from_model(&model, &[1]).unwrap();
        draft.energy = Some(8);
        draft.changed.energy = true;
        let Action::ApplyTags { patch, .. } = draft.action("es").unwrap() else {
            panic!("wrong action");
        };
        assert_eq!(patch.energy, Some(8));
    }

    #[test]
    fn rejects_nonfinite_bpm_before_emitting_a_write_intent() {
        let model = Model {
            tracks: vec![sample_track(1, "First", 120.0)],
            ..Model::default()
        };
        let mut draft = TagDraft::from_model(&model, &[1]).unwrap();
        draft.changed.bpm = true;
        for invalid in ["NaN", "inf", "-1", "0", ""] {
            draft.bpm = invalid.into();
            assert!(draft.action("es").is_err(), "accepted {invalid}");
        }
        draft.bpm = "120,01".into();
        assert!(draft.action("es").is_ok());
    }

    #[test]
    fn draft_rejects_unknown_ids_instead_of_dropping_them() {
        let model = Model {
            tracks: vec![sample_track(1, "First", 120.0)],
            ..Model::default()
        };
        assert!(TagDraft::from_model(&model, &[1, 99]).is_none());
    }

    // ---- geometría de la rueda (TDD: el painter jamás decide) ----

    const CENTER: Pos2 = pos2(100.0, 100.0);
    const RADIUS: f32 = 100.0;

    #[test]
    fn wheel_center_and_outer_space_are_not_keys() {
        assert_eq!(wheel_key_at(CENTER, CENTER, RADIUS), None);
        // dentro del hueco central (r < 0.52·R)
        assert_eq!(wheel_key_at(pos2(100.0, 60.0), CENTER, RADIUS), None);
        // fuera del borde exterior (distancia 110 > radio)
        assert_eq!(wheel_key_at(pos2(100.0, -10.0), CENTER, RADIUS), None);
    }

    #[test]
    fn wheel_twelve_oclock_wraps_to_key_12() {
        // justo encima del centro: sector 0 → 12, anillo interior = minor
        let up_minor = wheel_key_at(pos2(100.0, 30.0), CENTER, RADIUS).unwrap();
        assert_eq!(up_minor.to_string(), "12A");
        // arriba pero en el anillo exterior = major
        let up_major = wheel_key_at(pos2(100.0, 15.0), CENTER, RADIUS).unwrap();
        assert_eq!(up_major.to_string(), "12B");
        // media vuelta → 6
        let down = wheel_key_at(pos2(100.0, 170.0), CENTER, RADIUS).unwrap();
        assert_eq!(down.to_string(), "6A");
    }

    #[test]
    fn wheel_rings_map_minor_inside_major_outside() {
        // mismo ángulo (3 en punto), dos radios: interior minor, exterior major
        let inside = wheel_key_at(pos2(160.0, 100.0), CENTER, RADIUS).unwrap();
        let outside = wheel_key_at(pos2(190.0, 100.0), CENTER, RADIUS).unwrap();
        assert_eq!(inside.to_string(), "3A");
        assert_eq!(outside.to_string(), "3B");
    }

    /// Cierre (ítem 11): el centro de la rueda es CONDICIONAL — el tono
    /// elegido manda sobre la portada. Portada SOLO sin filtro y con pista
    /// cargada; tono SOLO al filtrar; jamás ambos.
    #[test]
    fn wheel_center_shows_key_only_when_filtering_and_cover_only_when_neutral() {
        let key_7b = Key::new(7, KeyMode::Major).expect("7B válida");
        // filtro activo + pista cargada → TONO (la portada no se pinta)
        assert_eq!(
            wheel_center(Some(key_7b), Some(42)),
            WheelCenter::Key(key_7b)
        );
        // filtro activo sin pista → TONO igualmente
        assert_eq!(wheel_center(Some(key_7b), None), WheelCenter::Key(key_7b));
        // neutro + pista cargada → PORTADA de esa pista
        assert_eq!(wheel_center(None, Some(42)), WheelCenter::Cover(Some(42)));
        // neutro sin pista → fallback vinilo (Cover sin id)
        assert_eq!(wheel_center(None, None), WheelCenter::Cover(None));
    }

    // ---- responsividad (TDD: el painter no decide escalas) ----

    #[test]
    fn sidebar_k_clamps_around_design_base_1420() {
        assert_eq!(sidebar_k(1420.0), 1.0);
        // ventana chica: el sidebar no encoge más de 0.9 (171px)
        assert_eq!(sidebar_k(1000.0), 0.9);
        // ultrawide: no crece más de 1.5 (285px)
        assert_eq!(sidebar_k(4000.0), 1.5);
        // crece linealmente dentro del rango
        assert!((sidebar_k(1704.0) - 1.2).abs() < 0.001);
    }

    #[test]
    fn sidebar_width_range_brackets_every_default_and_caps_at_360() {
        let (min, max) = sidebar_width_range();
        assert_eq!(max, 360.0);
        // el mínimo es el diseño al k más bajo: nunca más angosto que hoy
        assert!((min - studio::SIDEBAR_W * 0.9).abs() < 0.001);
        // el default_width en CUALQUIER ventana vive dentro del rango
        // (PanelState persistido también se clampa a él en la carga)
        for w in [800.0, 1000.0, 1420.0, 2560.0, 4000.0] {
            let d = studio::SIDEBAR_W * sidebar_k(w);
            assert!(
                min <= d && d <= max,
                "default {d} fuera de [{min},{max}] con ventana {w}"
            );
        }
    }

    #[test]
    fn table_k_clamps_around_design_base_1420() {
        assert_eq!(table_k(1420.0), 1.0);
        assert_eq!(table_k(900.0), 0.85);
        assert_eq!(table_k(2800.0), 1.5);
        assert!((table_k(2130.0) - 1.5).abs() < 0.001);
    }

    #[test]
    fn col_width_scales_base_by_k_and_clamps_to_the_column_range() {
        // auto: base·k mientras viva dentro del rango
        assert_eq!(col_width(118.0, (80.0, 600.0), 1.0, None), 118.0);
        assert_eq!(col_width(118.0, (80.0, 600.0), 1.5, None), 177.0);
        // ventana chica: el MÍNIMO gana (aloja el header completo — fix
        // table-rb: la duración nunca estrecha más allá de "Duración")
        assert_eq!(col_width(56.0, (84.0, 88.0), 0.85, None), 84.0);
        // el MÁXIMO acota el crecimiento (resize acotado, no indefinido)
        assert_eq!(col_width(118.0, (80.0, 120.0), 1.5, None), 120.0);
    }

    #[test]
    fn col_width_user_drag_wins_but_stays_inside_the_range() {
        // el usuario fijó 90px: manda aunque la ventana pida base·k mayor
        assert_eq!(col_width(100.0, (80.0, 600.0), 1.5, Some(90.0)), 90.0);
        // drag por encima del máximo → clamp al máximo
        assert_eq!(col_width(100.0, (24.0, 40.0), 1.5, Some(500.0)), 40.0);
        // drag por debajo del mínimo → clamp al mínimo
        assert_eq!(col_width(100.0, (24.0, 40.0), 1.5, Some(2.0)), 24.0);
    }

    /// fix ui-rb2: la suma título+artista+Σfijas es SIEMPRE exactamente el
    /// ancho disponible — la tabla nunca desborda (sin scroll horizontal,
    /// sin columnas cortadas) en ninguna ventana y con cualquier drag.
    #[test]
    fn table_layout_suma_siempre_es_el_ancho_disponible() {
        let fixed_sum: f32 = COL_RANGES
            .iter()
            .enumerate()
            .filter(|(c, _)| *c != COL_TITLE && *c != COL_ARTIST)
            .map(|(_, r)| r.0)
            .sum();
        for available in [500.0f32, 700.0, 900.0, 1379.0, 2560.0, 6000.0] {
            for (title_user, artist_user) in [
                (None, None),
                (None, Some(600.0)),
                (Some(180.0), None),
                (Some(2000.0), None),
                (Some(500.0), Some(500.0)), // ambos: manda el artista
            ] {
                let widths = table_layout(available, 1.0, title_user, artist_user);
                let sum: f32 = widths.iter().sum();
                assert!(
                    (sum - available).abs() < 0.01,
                    "available {available}: suma {sum} (t={:?} a={:?})",
                    widths[COL_TITLE],
                    widths[COL_ARTIST]
                );
                // las fijas quedan EXACTAS en su ancho
                for c in 0..COL_COUNT {
                    if c != COL_TITLE && c != COL_ARTIST {
                        assert_eq!(widths[c], COL_RANGES[c].0, "col fija {c}");
                    }
                }
                let _ = fixed_sum;
            }
        }
    }

    /// fix ui-rb2: el drag del ARTISTA clampa a (available - Σfijas -
    /// título_min): ensanchar el artista ENCOGE el título en vivo, nunca
    /// empuja a las fijas ni desborda la tabla.
    #[test]
    fn artist_drag_clamps_dinamicamente_y_el_titulo_absorbe() {
        let fixed_sum: f32 = COL_RANGES
            .iter()
            .enumerate()
            .filter(|(c, _)| *c != COL_TITLE && *c != COL_ARTIST)
            .map(|(_, r)| r.0)
            .sum();
        // ventana donde el SOBRANTE manda (no el tope 600): drag MASIVO →
        // el artista se clava en (sobrante - título_min)
        let available = 1000.0; // sobrante = 536 < 600+180 → clamp dinámico
        let widths = table_layout(available, 1.0, None, Some(9000.0));
        assert!(
            (widths[COL_ARTIST] - (available - fixed_sum - COL_RANGES[COL_TITLE].0)).abs() < 0.01,
            "artista al máximo: {:?}",
            widths[COL_ARTIST]
        );
        assert!(widths[COL_ARTIST] < COL_RANGES[COL_ARTIST].1);
        assert!(
            (widths[COL_TITLE] - COL_RANGES[COL_TITLE].0).abs() < 0.01,
            "el título cede hasta su mínimo: {:?}",
            widths[COL_TITLE]
        );
        // drag a cero → piso del artista; el título absorbe TODO el resto
        let widths = table_layout(available, 1.0, None, Some(0.0));
        assert_eq!(widths[COL_ARTIST], COL_RANGES[COL_ARTIST].0);
        assert!(
            (widths[COL_TITLE] - (available - fixed_sum - COL_RANGES[COL_ARTIST].0)).abs() < 0.01
        );
        // y viceversa: drag del TÍTULO → el artista cede (título se clava
        // en sobrante - artista_min, lejos del tope 2400)
        let widths = table_layout(available, 1.0, Some(9000.0), None);
        assert!(
            (widths[COL_TITLE] - (available - fixed_sum - COL_RANGES[COL_ARTIST].0)).abs() < 0.01,
            "título al máximo: {:?}",
            widths[COL_TITLE]
        );
        assert_eq!(widths[COL_ARTIST], COL_RANGES[COL_ARTIST].0);
        // el tope 600 del artista sigue vigente cuando el sobrante da de sobra
        let widths = table_layout(4000.0, 1.0, None, Some(9000.0));
        assert_eq!(widths[COL_ARTIST], COL_RANGES[COL_ARTIST].1);
    }

    /// fix ui-rb2: ventana raquítica (sobrante < mínimos de título y
    /// artista juntos): las FIJAS se preservan y los dos flexibles
    /// reparten el presupuesto — la suma sigue siendo el available.
    #[test]
    fn table_layout_ventana_raquitica_preserva_las_fijas() {
        let available = 500.0; // fijas ≈ 464 → sobrante ≈ 36 < 180+80
        let widths = table_layout(available, 0.85, None, None);
        let sum: f32 = widths.iter().sum();
        assert!((sum - available).abs() < 0.01);
        for c in 0..COL_COUNT {
            if c != COL_TITLE && c != COL_ARTIST {
                assert_eq!(widths[c], COL_RANGES[c].0, "la fija {c} no se toca");
            }
        }
        // reparto proporcional a los mínimos (80/260 del sobrante)
        let leftover =
            available - widths.iter().sum::<f32>() + widths[COL_TITLE] + widths[COL_ARTIST];
        let _ = leftover;
        assert!(widths[COL_ARTIST] > 0.0 && widths[COL_TITLE] > 0.0);
    }

    /// fix ui-rb2: SOLO título y artista son arrastrables (rango min <
    /// max); el resto son FIJAS (min == max en COL_RANGES) — sin asa,
    /// inmóviles ante drag, escalado y persistencia.
    #[test]
    fn only_title_and_artist_are_resizable_the_rest_is_fixed() {
        for (c, range) in COL_RANGES.iter().enumerate() {
            if c == COL_TITLE || c == COL_ARTIST {
                assert!(range.0 < range.1, "la col {c} debe ser arrastrable");
            } else {
                assert_eq!(range.0, range.1, "la col {c} debe ser FIJA");
            }
        }
        // base, drag o k: las fijas no se mueven
        const COL_COVER: usize = 2;
        assert_eq!(COL_RANGES[COL_COVER], (44.0, 44.0));
        assert_eq!(
            col_width(COL_BASES[COL_COVER], COL_RANGES[COL_COVER], 1.2, None),
            44.0
        );
        assert_eq!(
            col_width(COL_BASES[COL_COVER], COL_RANGES[COL_COVER], 1.2, Some(90.0)),
            44.0
        );
    }

    /// fix table-rb: alineación por columna como DATO — texto a la
    /// izquierda, numéricas a la derecha, simbólicas al centro (contrato
    /// rekordbox del dueño).
    #[test]
    fn column_alignments_match_the_rekordbox_contract() {
        assert_eq!(COL_ALIGNS[3], ColAlign::Left, "título");
        assert_eq!(COL_ALIGNS[4], ColAlign::Left, "artista");
        for c in [0, 1, 2, 5, 7, 8, 10] {
            assert_eq!(COL_ALIGNS[c], ColAlign::Center, "col {c}");
        }
        assert_eq!(COL_ALIGNS[6], ColAlign::Right, "bpm");
        assert_eq!(COL_ALIGNS[9], ColAlign::Right, "duración");
    }

    /// Reducción global de fuentes (fix table-rb) — contrato de la escala
    /// aprobada: si alguien re-toca un token, este test obliga a revisar
    /// también los mínimos de COL_RANGES (se miden con ESTA tipografía).
    #[test]
    fn global_font_reduction_matches_the_approved_scale() {
        assert_eq!(studio::FONT_BODY, 15.5);
        assert_eq!(studio::FONT_SMALL, 14.0);
        assert_eq!(TABLE_CELL_FONT, 14.5);
        assert_eq!(TABLE_ROW_HEIGHT, 38.0);
        assert_eq!(TABLE_HEADER_HEIGHT, 34.0);
        assert_eq!(studio::STATUS_H, 30.0);
    }

    /// Cierre (elipsis): el título de fila que no cabe en su columna se
    /// acorta con "…" medido con la MISMA fuente de la tabla — nunca excede
    /// el ancho disponible y el texto corto pasa intacto.
    #[test]
    fn ellipsize_cuts_with_ellipsis_and_never_exceeds_the_width() {
        let ctx = egui::Context::default();
        // mismo TTF Barlow SC 500 de producción (idem test de mínimos)
        let mut fonts = egui::FontDefinitions::default();
        let ttf = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/barlow-semi-condensed/barlow-semi-condensed-v16-latin-500.ttf"
        ))
        .expect("ttf barlow del repo");
        fonts
            .font_data
            .insert("barlow".into(), egui::FontData::from_owned(ttf).into());
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "barlow".into());
        ctx.set_fonts(fonts);
        let font = egui::FontId::proportional(TABLE_CELL_FONT);
        let raw = egui::RawInput::default();
        let _ = ctx.run(raw, |ctx| {
            ctx.fonts(|fonts| {
                let long = "like we were the last two people on earth (extended mix)";
                let full = fonts
                    .layout_delayed_color(long.to_owned(), font.clone(), f32::INFINITY)
                    .size()
                    .x;
                let max = full * 0.6;
                let out = studio::ellipsize(fonts, long, font.clone(), max);
                assert!(out.ends_with('…'), "debe terminar en elipsis: {out}");
                assert!(out.chars().count() < long.chars().count());
                let w = fonts
                    .layout_delayed_color(out.clone(), font.clone(), f32::INFINITY)
                    .size()
                    .x;
                assert!(w <= max + 0.5, "excede el ancho: {w} > {max}");
                // texto que ya cabe → intacto
                assert_eq!(
                    studio::ellipsize(fonts, "Short", font.clone(), 500.0),
                    "Short"
                );
            });
        });
    }

    /// fix table-rb: los MÍNIMOS de las columnas de TEXTO alojan el header
    /// COMPLETO sin abreviar, medido con el layout REAL de Barlow Semi
    /// Condensed a la fuente final (FONT_SMALL). El chrome = padding del
    /// botón (medido en vivo) + zona del indicador de sort + franja del
    /// asa de resize + 2px de aire. En las columnas de símbolo alcanza con
    /// que el control quepa antes de la franja.
    #[test]
    fn column_minimums_fit_full_headers_with_final_font() {
        let ctx = egui::Context::default();
        // Los binarios de test no heredan las fuentes del repo (viven en
        // assets/ de la RAÍZ y el exe de test corre desde target/): se
        // carga el MISMO TTF de Barlow SC 500 que usa la app en producción.
        let mut fonts = egui::FontDefinitions::default();
        let ttf = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/barlow-semi-condensed/barlow-semi-condensed-v16-latin-500.ttf"
        ))
        .expect("ttf barlow del repo");
        fonts
            .font_data
            .insert("barlow".into(), egui::FontData::from_owned(ttf).into());
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "barlow".into());
        ctx.set_fonts(fonts);
        let font = egui::FontId::proportional(studio::FONT_SMALL);
        let labels = [
            "Título",
            "Title",
            "Artista",
            "Artist",
            "Key",
            "BPM",
            "Energía",
            "Energy",
            "Rating",
            "Duración",
            "Duration",
            "#",
        ];
        let mut widths = Vec::new();
        let mut button_overhead = 0.0f32;
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                pos2(0.0, 0.0),
                egui::vec2(400.0, 100.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(raw, |ctx| {
            for t in labels {
                let w = ctx
                    .fonts(|f| f.layout_no_wrap(t.to_owned(), font.clone(), egui::Color32::WHITE))
                    .size()
                    .x;
                widths.push((t, w));
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                // overhead real del botón del header (rect menos galley)
                let r = ui.add(egui::Button::new(
                    egui::RichText::new("BPM").size(studio::FONT_SMALL),
                ));
                let bpm_w = ctx
                    .fonts(|f| {
                        f.layout_no_wrap("BPM".to_owned(), font.clone(), egui::Color32::WHITE)
                    })
                    .size()
                    .x;
                button_overhead = (r.rect.width() - bpm_w).max(0.0);
            });
        });
        let w = |t: &str| widths.iter().find(|(k, _)| *k == t).unwrap().1;
        // chrome completo de header de texto: padding del botón (2 lados)
        // + indicador + franja del asa + aire
        // overhead ya incluye el padding de AMBOS lados del botón
        let chrome = button_overhead + HEADER_INDICATOR_W + 8.0 + 2.0;
        // (columna, textos posibles por idioma, mínimo de COL_RANGES)
        let text_cols: [(usize, &[&str], f32); 7] = [
            (3, &["Título", "Title"], COL_RANGES[3].0),
            (4, &["Artista", "Artist"], COL_RANGES[4].0),
            (5, &["Key"], COL_RANGES[5].0),
            (6, &["BPM"], COL_RANGES[6].0),
            (7, &["Energía", "Energy"], COL_RANGES[7].0),
            (8, &["Rating"], COL_RANGES[8].0),
            (9, &["Duración", "Duration"], COL_RANGES[9].0),
        ];
        for (c, texts, min) in text_cols {
            for t in texts {
                let need = w(t) + chrome;
                assert!(
                    min + 0.01 >= need,
                    "col {c}: minimo {min} no aloja el header completo {t} ({need:.1}px con chrome {chrome:.1})"
                );
            }
        }
        // columnas de símbolo: el control cabe con aire antes de la franja
        assert!(COL_RANGES[1].0 >= w("#") + button_overhead + 2.0, "numeral");
        assert!(COL_RANGES[0].0 >= 18.0 + 2.0, "checkbox 18px");
        assert!(COL_RANGES[10].0 >= 23.0 + 2.0, "boton menu 23px");
        // y el header de Duración NO vuelve a abreviarse
        assert_eq!(crate::texts::t("es", "studio.duration"), "Duración");
    }

    // ---- repro doble clic (bug segunda pista): egui real + input sintético --

    struct NullWave;
    impl crate::studio_model::WaveformView for NullWave {
        fn show(
            &mut self,
            _ui: &mut egui::Ui,
            _rect: egui::Rect,
            _player: &crate::studio_model::Player,
        ) -> Option<Action> {
            None
        }
    }

    /// Host sin portadas (tests): nunca entrega texturas ni pide.
    struct NoCovers;
    impl crate::studio_model::CoverArt for NoCovers {
        fn cover_texture(&mut self, _track_id: TrackId) -> Option<egui::TextureHandle> {
            None
        }
        fn request_cover(&mut self, _track_id: TrackId) {}
    }

    fn frame(
        ctx: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
        sui: &mut StudioUi,
        model: &Model,
    ) -> Vec<Action> {
        frame_mod(ctx, time, events, egui::Modifiers::default(), sui, model)
    }

    /// `frame` con modificadores de teclado: egui 0.27 lee los modifiers
    /// del `RawInput` (no de los eventos), así que shift/ctrl van ahí.
    fn frame_mod(
        ctx: &egui::Context,
        time: f64,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
        sui: &mut StudioUi,
        model: &Model,
    ) -> Vec<Action> {
        let ri = egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                pos2(0.0, 0.0),
                egui::vec2(1400.0, 900.0),
            )),
            events,
            modifiers,
            ..Default::default()
        };
        let mut out = Vec::new();
        let mut wave = NullWave;
        let mut covers = NoCovers;
        let _ = ctx.run(ri, |ctx| {
            out = sui.show(ctx, "es", model, &mut wave, &mut covers, true);
        });
        out
    }

    fn click_at(
        ctx: &egui::Context,
        sui: &mut StudioUi,
        model: &Model,
        x: f32,
        y: f32,
        t0: f64,
    ) -> Vec<Action> {
        let pos = pos2(x, y);
        let _ = frame(ctx, t0, vec![egui::Event::PointerMoved(pos)], sui, model);
        let _ = frame(
            ctx,
            t0 + 0.01,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            sui,
            model,
        );
        frame(
            ctx,
            t0 + 0.02,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            sui,
            model,
        )
    }

    #[test]
    fn double_click_on_any_row_title_emits_load_and_play() {
        // REGRESIÓN bug 2ª pista: en egui 0.27 el hit-test entrega el clic al
        // Button de título (último widget click-sensed) y row.response() nunca
        // ve double_clicked() → LoadAndPlay jamás se emitía y la pista cargada
        // no cambiaba. Se simulan clics reales (press+release <0.3s) sobre las
        // filas 2 y 3 con un egui::Context vivo.
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut sui = StudioUi::new();
        let model = Model {
            tracks: vec![
                sample_track(11, "Alpha", 120.0),
                sample_track(22, "Beta", 121.0),
                sample_track(33, "Gamma", 122.0),
            ],
            library_count: 3,
            ..Model::default()
        };
        let _ = frame(&ctx, 0.0, vec![], &mut sui, &model);
        // Autolocalizar el centro Y de cada fila (la escala tipográfica puede
        // mover la tabla) barrendo clics simples. El reloj es MONOTÓNICO y
        // avanza >0.3s entre clics del barrido para no fabricar dobles clics.
        fn find_row_y(
            ctx: &egui::Context,
            sui: &mut StudioUi,
            model: &Model,
            want: TrackId,
            clock: &mut f64,
        ) -> f32 {
            let mut y = 380.0;
            let mut hits = Vec::new();
            while y < 620.0 {
                *clock += 0.4;
                let actions = click_at(ctx, sui, model, 500.0, y, *clock);
                if actions
                    .iter()
                    .any(|a| matches!(a, Action::SelectTrack(id) if *id == want))
                {
                    hits.push(y);
                }
                y += 6.0;
            }
            assert!(!hits.is_empty(), "no se encontró la fila de {want}");
            // el CENTRO de la zona de hit: lejos de los bordes donde el
            // interact-radius de la fila vecina interfiere con el hit-test
            hits[hits.len() / 2]
        }
        let mut clock = 100.0;
        let y2 = find_row_y(&ctx, &mut sui, &model, 22, &mut clock);
        // >0.6s desde el último clic del barrido: si no, egui cuenta el par
        // como TRIPLE clic (ventana de last_last_click = 2x0.3s)
        clock += 1.0;
        let a1 = click_at(&ctx, &mut sui, &model, 500.0, y2, clock);
        assert!(
            a1.iter().any(|a| matches!(a, Action::SelectTrack(22))),
            "clic 1 debe seleccionar: {a1:?}"
        );
        assert!(!a1.iter().any(|a| matches!(a, Action::LoadAndPlay(_))));
        let a2 = click_at(&ctx, &mut sui, &model, 500.0, y2, clock + 0.12);
        assert!(
            a2.iter().any(|a| matches!(a, Action::LoadAndPlay(22))),
            "doble clic en fila 2 debe emitir LoadAndPlay(22): {a2:?}"
        );
        // fila 3 (id 33): el mismo gesto también cambia la pista cargada
        let y3 = find_row_y(&ctx, &mut sui, &model, 33, &mut clock);
        clock += 1.0;
        let _ = click_at(&ctx, &mut sui, &model, 500.0, y3, clock);
        let a4 = click_at(&ctx, &mut sui, &model, 500.0, y3, clock + 0.12);
        assert!(
            a4.iter().any(|a| matches!(a, Action::LoadAndPlay(33))),
            "doble clic en fila 3 debe emitir LoadAndPlay(33): {a4:?}"
        );
    }

    // -- selección shift+click / ctrl+A / Escape (egui real + input sintético) --

    /// Como `click_at` pero con modificadores (shift/ctrl) y botón elegido.
    fn click_at_mod(
        ctx: &egui::Context,
        sui: &mut StudioUi,
        model: &Model,
        x: f32,
        y: f32,
        t0: f64,
        modifiers: egui::Modifiers,
        button: egui::PointerButton,
    ) -> Vec<Action> {
        let pos = pos2(x, y);
        let _ = frame_mod(
            ctx,
            t0,
            vec![egui::Event::PointerMoved(pos)],
            modifiers,
            sui,
            model,
        );
        let _ = frame_mod(
            ctx,
            t0 + 0.01,
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers,
            }],
            modifiers,
            sui,
            model,
        );
        frame_mod(
            ctx,
            t0 + 0.02,
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: false,
                modifiers,
            }],
            modifiers,
            sui,
            model,
        )
    }

    #[test]
    fn shift_click_selects_the_range_from_the_anchor() {
        // el RANGO vive en la MISMA selección de checkboxes: clic simple en
        // la fila 2 (ancla), shift+clic en la fila 4 → filas 2..4 quedan
        // checked (sin borrar lo que ya había).
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut sui = StudioUi::new();
        let model = Model {
            tracks: vec![
                sample_track(11, "Alpha", 120.0),
                sample_track(22, "Beta", 121.0),
                sample_track(33, "Gamma", 122.0),
                sample_track(44, "Delta", 123.0),
            ],
            library_count: 4,
            ..Model::default()
        };
        let _ = frame(&ctx, 0.0, vec![], &mut sui, &model);
        // localizar las filas con el barrido del test de doble clic
        // (los barridos van PRIMERO: cada clic del barrido mueve el ancla)
        let mut clock = 200.0;
        let find = |ctx: &egui::Context, sui: &mut StudioUi, want: TrackId, clock: &mut f64| {
            let mut y = 380.0;
            let mut hits = Vec::new();
            while y < 620.0 {
                *clock += 0.4;
                let actions = click_at(ctx, sui, &model, 500.0, y, *clock);
                if actions
                    .iter()
                    .any(|a| matches!(a, Action::SelectTrack(id) if *id == want))
                {
                    hits.push(y);
                }
                y += 6.0;
            }
            assert!(!hits.is_empty(), "no se encontró la fila de {want}");
            hits[hits.len() / 2]
        };
        let y2 = find(&ctx, &mut sui, 22, &mut clock);
        let y4 = find(&ctx, &mut sui, 44, &mut clock);
        clock += 1.0;
        // clic simple: fila 2 seleccionada (y anclada)
        let a1 = click_at(&ctx, &mut sui, &model, 500.0, y2, clock);
        assert!(a1.iter().any(|a| matches!(a, Action::SelectTrack(22))));
        assert!(sui.checked_tracks().is_empty(), "clic simple limpia");
        // shift+clic en la fila 4: rango 2..4 dentro de la MISMA selección
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        let a2 = click_at_mod(
            &ctx,
            &mut sui,
            &model,
            500.0,
            y4,
            clock + 1.0,
            shift,
            egui::PointerButton::Primary,
        );
        // el rango no reproduce ni selecciona-unica: NADA de LoadAndPlay y
        // la selección de checkboxes gana a la selección simple
        assert!(
            !a2.iter().any(|a| matches!(a, Action::LoadAndPlay(_))),
            "shift+clic no debe reproducir: {a2:?}"
        );
        let got: Vec<TrackId> = sui.checked_tracks().into_iter().collect();
        assert_eq!(got, vec![22, 33, 44], "rango 2..4 checked: {got:?}");
        // Escape vacía la selección (la barra de "N seleccionadas" desaparece
        // y la tabla recupera su layout — las Y vuelven a ser válidas)
        let _ = frame_mod(
            &ctx,
            clock + 3.0,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            egui::Modifiers::default(),
            &mut sui,
            &model,
        );
        assert!(sui.checked_tracks().is_empty(), "Escape vacía la selección");
        // descendente: clic simple en la fila 4 (ancla) y shift+clic en la 2
        // → el MISMO rango, sin importar la dirección
        let a4 = click_at(&ctx, &mut sui, &model, 500.0, y4, clock + 3.5);
        assert!(a4.iter().any(|a| matches!(a, Action::SelectTrack(44))));
        let a5 = click_at_mod(
            &ctx,
            &mut sui,
            &model,
            500.0,
            y2,
            clock + 4.5,
            shift,
            egui::PointerButton::Primary,
        );
        assert!(!a5.iter().any(|a| matches!(a, Action::LoadAndPlay(_))));
        let got: Vec<TrackId> = sui.checked_tracks().into_iter().collect();
        assert_eq!(got, vec![22, 33, 44], "rango descendente idéntico: {got:?}");
    }

    #[test]
    fn ctrl_a_selects_all_and_escape_empties_the_selection() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut sui = StudioUi::new();
        let model = Model {
            tracks: vec![
                sample_track(11, "Alpha", 120.0),
                sample_track(22, "Beta", 121.0),
                sample_track(33, "Gamma", 122.0),
            ],
            library_count: 3,
            ..Model::default()
        };
        let _ = frame(&ctx, 0.0, vec![], &mut sui, &model);
        let ctrl_a = vec![egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
        }];
        let _ = frame_mod(&ctx, 1.0, ctrl_a, egui::Modifiers::CTRL, &mut sui, &model);
        let got: Vec<TrackId> = sui.checked_tracks().into_iter().collect();
        assert_eq!(got, vec![11, 22, 33], "ctrl+A selecciona toda la vista");
        // Escape vacía la selección (y el ancla: un shift posterior no
        // resucita el rango viejo)
        let esc = vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }];
        let _ = frame(&ctx, 2.0, esc, &mut sui, &model);
        assert!(sui.checked_tracks().is_empty(), "Escape vacía la selección");
    }

    // -- menú contextual click derecho: apertura, visibilidad 1 vs N, ruteo --

    /// Click derecho (secundario) sobre una fila: abre el menú contextual
    /// egui EN esa posición; devuelve la posición para clicar sus ítems.
    fn right_click_row(
        ctx: &egui::Context,
        sui: &mut StudioUi,
        model: &Model,
        x: f32,
        y: f32,
        t0: f64,
    ) -> Vec<Action> {
        click_at_mod(
            ctx,
            sui,
            model,
            x,
            y,
            t0,
            egui::Modifiers::default(),
            egui::PointerButton::Secondary,
        )
    }

    #[test]
    fn right_click_single_row_opens_context_menu_that_loads_the_track() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut sui = StudioUi::new();
        let model = Model {
            tracks: vec![
                sample_track(11, "Alpha", 120.0),
                sample_track(22, "Beta", 121.0),
                sample_track(33, "Gamma", 122.0),
            ],
            library_count: 3,
            ..Model::default()
        };
        let _ = frame(&ctx, 0.0, vec![], &mut sui, &model);
        // localizar la fila 2 (barrido del patrón de doble clic)
        let mut clock = 300.0;
        let mut y = 380.0;
        let mut hits = Vec::new();
        while y < 620.0 {
            clock += 0.4;
            let actions = click_at(&ctx, &mut sui, &model, 500.0, y, clock);
            if actions
                .iter()
                .any(|a| matches!(a, Action::SelectTrack(id) if *id == 22))
            {
                hits.push(y);
            }
            y += 6.0;
        }
        assert!(!hits.is_empty(), "no se encontró la fila 2");
        let y2 = hits[hits.len() / 2];
        clock += 1.0;
        // click derecho sobre la fila 2 → el menú se abre en el puntero
        let _ = right_click_row(&ctx, &mut sui, &model, 500.0, y2, clock);
        clock += 1.0;
        // el PRIMER ítem del menú single es "Cargar pista": el menú de 15
        // ítems NO cabe debajo de la fila y egui lo VOLTEA hacia arriba
        // (constrain_to) — el barrido cubre toda la columna del popup y el
        // primer BOTÓN desde arriba es exactamente "Cargar pista" → emite
        // LoadTrack — sin reproducir. Los clics que caen fuera del menú lo
        // cierran: se reabre con otro click derecho en la misma fila.
        let mut loaded = false;
        let mut probe_y = 300.0;
        while probe_y < 880.0 && !loaded {
            let actions = click_at(&ctx, &mut sui, &model, 500.0 + 70.0, probe_y, clock);
            clock += 0.5;
            loaded = actions.iter().any(|a| matches!(a, Action::LoadTrack(22)));
            if !loaded {
                // si el clic cerró el menú sin efecto, reabrir antes de seguir
                let _ = right_click_row(&ctx, &mut sui, &model, 500.0, y2, clock);
                clock += 0.5;
            }
            probe_y += 6.0;
        }
        assert!(
            loaded,
            "el menú single debe ofrecer 'Cargar pista' → LoadTrack(22)"
        );
    }

    #[test]
    fn right_click_multi_selection_menu_hides_single_only_items() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut sui = StudioUi::new();
        // 2 pistas y las DOS checked: cualquier fila recibe el menú de N=2
        // (robusto al corrimiento vertical que introduce la barra de
        // "N seleccionadas" al aparecer).
        let model = Model {
            tracks: vec![
                sample_track(11, "Alpha", 120.0),
                sample_track(22, "Beta", 121.0),
            ],
            library_count: 2,
            ..Model::default()
        };
        let _ = frame(&ctx, 0.0, vec![], &mut sui, &model);
        // localizar la fila 1 con el barrido (la selección está vacía)
        let mut clock = 300.0;
        let mut y = 380.0;
        let mut hits = Vec::new();
        while y < 620.0 {
            clock += 0.4;
            let actions = click_at(&ctx, &mut sui, &model, 500.0, y, clock);
            if actions
                .iter()
                .any(|a| matches!(a, Action::SelectTrack(id) if *id == 11))
            {
                hits.push(y);
            }
            y += 6.0;
        }
        assert!(!hits.is_empty(), "no se encontró la fila 1");
        let y1 = hits[hits.len() / 2];
        // sembrar la selección múltiple (ambas filas) y dejar un frame para
        // que la barra de selección aparezca y corra la tabla (~42px)
        crate::studio_model::selection::toggle(&mut sui.checked, &mut sui.anchor, 11);
        crate::studio_model::selection::toggle(&mut sui.checked, &mut sui.anchor, 22);
        let _ = frame(&ctx, clock + 0.2, vec![], &mut sui, &model);
        clock += 1.0;
        // click derecho sobre una fila de la selección: conserva N=2
        let _ = right_click_row(&ctx, &mut sui, &model, 500.0, y1 + 42.0, clock);
        clock += 1.0;
        // con N>1 el primer ítem de acción es "Editar tags" (NO 'Cargar
        // pista'): el clic dentro del menú abre el EDITOR con las dos pistas
        let mut opened = false;
        let mut probe_y = y1 + 42.0 + 8.0;
        while probe_y < y1 + 42.0 + 160.0 && !opened {
            let _ = click_at(&ctx, &mut sui, &model, 500.0 + 70.0, probe_y, clock);
            clock += 0.5;
            opened = sui
                .editor
                .as_ref()
                .is_some_and(|draft| draft.ids == vec![11, 22]);
            if !opened {
                let _ = right_click_row(&ctx, &mut sui, &model, 500.0, y1 + 42.0, clock);
                clock += 0.5;
            }
            probe_y += 6.0;
        }
        assert!(
            opened,
            "el menú multi abre el editor de tags con [11, 22] (primer ítem)"
        );
    }

    #[test]
    fn right_click_outside_the_selection_collapses_to_that_row() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut sui = StudioUi::new();
        let model = Model {
            tracks: vec![
                sample_track(11, "Alpha", 120.0),
                sample_track(22, "Beta", 121.0),
                sample_track(33, "Gamma", 122.0),
                sample_track(44, "Delta", 123.0),
            ],
            library_count: 4,
            ..Model::default()
        };
        let _ = frame(&ctx, 0.0, vec![], &mut sui, &model);
        // selección en filas 2-3 y click derecho FUERA (fila 4)
        crate::studio_model::selection::toggle(&mut sui.checked, &mut sui.anchor, 22);
        crate::studio_model::selection::toggle(&mut sui.checked, &mut sui.anchor, 33);
        let mut clock = 300.0;
        let mut y = 380.0;
        let mut hits = Vec::new();
        while y < 620.0 {
            clock += 0.4;
            let actions = click_at(&ctx, &mut sui, &model, 500.0, y, clock);
            if actions
                .iter()
                .any(|a| matches!(a, Action::SelectTrack(id) if *id == 44))
            {
                hits.push(y);
            }
            y += 6.0;
        }
        assert!(!hits.is_empty(), "no se encontró la fila 4");
        let y4 = hits[hits.len() / 2];
        // el barrido limpió la selección (clics simples): resembrar y dejar
        // un frame para que la barra de selección corra la tabla (~42px)
        sui.checked.clear();
        crate::studio_model::selection::toggle(&mut sui.checked, &mut sui.anchor, 22);
        crate::studio_model::selection::toggle(&mut sui.checked, &mut sui.anchor, 33);
        let _ = frame(&ctx, clock + 0.2, vec![], &mut sui, &model);
        clock += 1.0;
        // click derecho en la fila 4 (fuera de la selección): el menú
        // colapsa a ESA fila — target = 1 pista
        let _ = right_click_row(&ctx, &mut sui, &model, 500.0, y4 + 42.0, clock);
        assert!(
            sui.checked_tracks().is_empty(),
            "click derecho fuera de la selección la colapsa"
        );
        assert_eq!(sui.selected_track(), Some(44));
        clock += 1.0;
        // primer ítem del menú single → Cargar pista (44). El menú se
        // VOLTEA hacia arriba (15 ítems): barrido de toda la columna.
        let mut loaded = false;
        let mut probe_y = 300.0;
        while probe_y < 880.0 && !loaded {
            let actions = click_at(&ctx, &mut sui, &model, 500.0 + 70.0, probe_y, clock);
            clock += 0.5;
            loaded = actions.iter().any(|a| matches!(a, Action::LoadTrack(44)));
            if !loaded {
                let _ = right_click_row(&ctx, &mut sui, &model, 500.0, y4 + 42.0, clock);
                clock += 0.5;
            }
            probe_y += 6.0;
        }
        assert!(loaded, "el menú tras colapsar ofrece LoadTrack(44)");
    }

    // ---- resize de columnas: el asa existe, es visible y responde ------

    /// REGRESIÓN resize de columnas (egui real + input sintético, patrón
    /// del test de doble clic): el asa del header debe registrarse, ser una
    /// franja de 8px, anunciar el cursor ResizeHorizontal en hover y
    /// acumular el drag sobre `col_widths`. (Probe original: la mecánica
    /// YA funcionaba — el bug reportado era de descubribilidad.)
    #[test]
    fn header_border_drag_resizes_that_column() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut sui = StudioUi::new();
        let model = Model {
            tracks: vec![
                sample_track(11, "Alpha", 120.0),
                sample_track(22, "Beta", 121.0),
            ],
            library_count: 2,
            ..Model::default()
        };
        let _ = frame(&ctx, 0.0, vec![], &mut sui, &model);
        let hr = ctx
            .read_response(col_resize_id(4))
            .expect("el asa de la columna 4 debe registrarse");
        // franja de 8px en el borde derecho de la celda del header
        assert!((hr.rect.width() - 8.0).abs() < 0.01, "{:?}", hr.rect);
        let center = hr.rect.center();
        let mut clock = 10.0;
        // hover sobre la franja → cursor de resize SIEMPRE disponible.
        // (el output del frame lo consume end_pass: leerlo DENTRO del run)
        let ri = egui::RawInput {
            time: Some(clock),
            screen_rect: Some(egui::Rect::from_min_size(
                pos2(0.0, 0.0),
                egui::vec2(1400.0, 900.0),
            )),
            events: vec![egui::Event::PointerMoved(center)],
            ..Default::default()
        };
        let mut wave = NullWave;
        let mut covers = NoCovers;
        let mut icon = egui::CursorIcon::Default;
        let _ = ctx.run(ri, |ctx| {
            let _ = sui.show(ctx, "es", &model, &mut wave, &mut covers, true);
            icon = ctx.output(|o| o.cursor_icon);
        });
        let hr = ctx.read_response(col_resize_id(4)).unwrap();
        assert!(hr.hovered(), "el asa debe ganar el hover en su franja");
        assert_eq!(icon, egui::CursorIcon::ResizeHorizontal, "cursor en hover");
        // press + drag hacia la derecha en pasos
        clock += 0.05;
        let _ = frame(
            &ctx,
            clock,
            vec![egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            &mut sui,
            &model,
        );
        // frame de armado: el PRIMER move tras el press aún no reporta
        // dragged (hit del drag se resuelve contra los rects del frame
        // anterior) — no asertar sobre él.
        clock += 0.05;
        let _ = frame(
            &ctx,
            clock,
            vec![egui::Event::PointerMoved(pos2(center.x + 4.0, center.y))],
            &mut sui,
            &model,
        );
        for dx in [8.0, 12.0] {
            clock += 0.05;
            let _ = frame(
                &ctx,
                clock,
                vec![egui::Event::PointerMoved(pos2(center.x + dx, center.y))],
                &mut sui,
                &model,
            );
            let hr = ctx.read_response(col_resize_id(4)).unwrap();
            assert!(hr.dragged(), "el asa debe ganar el drag (dx={dx})");
        }
        // release
        clock += 0.05;
        let _ = frame(
            &ctx,
            clock,
            vec![egui::Event::PointerButton {
                pos: pos2(center.x + 12.0, center.y),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            &mut sui,
            &model,
        );
        // el drag fijó el ancho de ESA columna: auto (base·k, la ventana del
        // test ~1189px → k=0.85) + delta acumulado del arrastre a la derecha
        let auto = col_width(COL_BASES[4], COL_RANGES[4], 0.85, None);
        let w = sui.col_widths[4].expect("el drag debe fijar col_widths[4]");
        assert!(
            w > auto,
            "arrastre a la derecha debe ensanchar sobre el auto {auto}: {w}"
        );
        // …y SOLO esa: las demás columnas siguen en auto (None)
        assert_eq!(sui.col_widths[5], None);
        assert_eq!(sui.col_widths[0], None);
    }

    /// fix ui-rb2: SOLO título y artista registran asa (las 9 fijas no);
    /// el clamp del drag es DINÁMICO — el límite es el sobrante del otro
    /// flexible, no el tope estático de la columna (probe estilo test
    /// existente: egui real + drag sintético sobre las asas).
    #[test]
    fn only_flex_handles_register_and_clamp_dynamically() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut sui = StudioUi::new();
        let model = Model {
            tracks: vec![
                sample_track(11, "Alpha", 120.0),
                sample_track(22, "Beta", 121.0),
            ],
            library_count: 2,
            ..Model::default()
        };
        let _ = frame(&ctx, 0.0, vec![], &mut sui, &model);
        // las 9 fijas NO registran asa; título y artista sí
        for c in [0, 1, 2, 5, 6, 7, 8, 9, 10] {
            assert!(
                ctx.read_response(col_resize_id(c)).is_none(),
                "la col fija {c} no debe tener asa"
            );
        }
        assert!(ctx.read_response(col_resize_id(COL_TITLE)).is_some());
        assert!(ctx.read_response(col_resize_id(COL_ARTIST)).is_some());
        let mut clock = 20.0;

        // drag sintético: hover → press → move (+4 armado) → move al
        // objetivo → release (patrón del probe de drag existente)
        fn drag_by(
            ctx: &egui::Context,
            sui: &mut StudioUi,
            model: &Model,
            col: usize,
            dx: f32,
            clock: &mut f64,
        ) {
            // frame de ASSENTAMIENTO sin eventos: los anchos escritos por el
            // drag anterior se aplican recién en el frame siguiente — leer
            // el rect ANTES deja una posición STALE y el press falla fuera
            // de la franja (bug real cazado por este probe)
            *clock += 0.05;
            let _ = frame(ctx, *clock, vec![], sui, model);
            let center = ctx
                .read_response(col_resize_id(col))
                .unwrap_or_else(|| panic!("el asa de la col {col} debe registrarse"))
                .rect
                .center();
            *clock += 0.05;
            let _ = frame(
                ctx,
                *clock,
                vec![egui::Event::PointerMoved(center)],
                sui,
                model,
            );
            *clock += 0.05;
            let _ = frame(
                ctx,
                *clock,
                vec![egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                }],
                sui,
                model,
            );
            *clock += 0.05;
            let _ = frame(
                ctx,
                *clock,
                vec![egui::Event::PointerMoved(pos2(center.x + 4.0, center.y))],
                sui,
                model,
            );
            *clock += 0.05;
            let _ = frame(
                ctx,
                *clock,
                vec![egui::Event::PointerMoved(pos2(center.x + dx, center.y))],
                sui,
                model,
            );
            *clock += 0.05;
            let _ = frame(
                ctx,
                *clock,
                vec![egui::Event::PointerButton {
                    pos: pos2(center.x + dx, center.y),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
                sui,
                model,
            );
        }

        // ARTISTA al máximo: el clamp DINÁMICO (sobrante - título_min) es
        // MENOR que el tope estático 600 en esta ventana de 1400px — el
        // drag masivo NO llega a 600 y el título conserva su mínimo
        drag_by(&ctx, &mut sui, &model, COL_ARTIST, 5000.0, &mut clock);
        let artist = sui.col_widths[COL_ARTIST].expect("el drag fija col_widths[4]");
        assert!(
            artist < COL_RANGES[COL_ARTIST].1,
            "el clamp dinamico ({artist}) debe quedar bajo el tope estatico {}",
            COL_RANGES[COL_ARTIST].1
        );
        assert!(artist > 400.0, " ventana holgada: {artist}");
        // cross-clear: el drag del artista borró el ancho fijado del título
        assert_eq!(sui.col_widths[COL_TITLE], None);

        // TÍTULO al máximo: idem — clava en (sobrante - artista_min),
        // MUY por debajo del tope estático 2400
        drag_by(&ctx, &mut sui, &model, COL_TITLE, 5000.0, &mut clock);
        let title = sui.col_widths[COL_TITLE].expect("el drag fija col_widths[3]");
        assert!(
            title < COL_RANGES[COL_TITLE].1 && title > 500.0,
            "clamp dinamico del titulo: {title}"
        );
        assert_eq!(sui.col_widths[COL_ARTIST], None, "cross-clear del artista");

        // y a cero: cada flexible cae a su propio mínimo
        drag_by(&ctx, &mut sui, &model, COL_ARTIST, -5000.0, &mut clock);
        let artist = sui.col_widths[COL_ARTIST].unwrap();
        assert!(
            (artist - COL_RANGES[COL_ARTIST].0).abs() < 0.01,
            "drag -5000 en artista debe clavarse en {}: {artist}",
            COL_RANGES[COL_ARTIST].0
        );
    }

    #[test]
    fn playlist_tree_flattens_nested_and_respects_collapsed() {
        use crate::studio_model::Playlist;
        let playlists = vec![
            Playlist {
                id: 1,
                name: "Root".into(),
                count: 2,
                kind: "manual".into(),
                parent: None,
            },
            Playlist {
                id: 2,
                name: "Child".into(),
                count: 1,
                kind: "manual".into(),
                parent: Some(1),
            },
            Playlist {
                id: 3,
                name: "Grandchild".into(),
                count: 0,
                kind: "manual".into(),
                parent: Some(2),
            },
            Playlist {
                id: 4,
                name: "Other".into(),
                count: 0,
                kind: "smart".into(),
                parent: None,
            },
        ];
        let flat = flatten_playlists(&playlists, &Default::default());
        let ids: Vec<(i64, usize)> = flat.iter().map(|r| (r.playlist.id, r.depth)).collect();
        assert_eq!(
            ids,
            [(1, 1), (2, 2), (3, 3), (4, 1)],
            "DFS con hijos debajo del padre"
        );
        assert!(flat[0].has_children && flat[1].has_children && !flat[2].has_children);

        // colapsada la raíz: hijos desaparecen
        let mut collapsed = std::collections::HashMap::new();
        collapsed.insert(1, true);
        let flat = flatten_playlists(&playlists, &collapsed);
        let ids: Vec<i64> = flat.iter().map(|r| r.playlist.id).collect();
        assert_eq!(ids, [1, 4]);
    }

    #[test]
    fn sorting_matches_field_and_direction_with_none_last() {
        let a = sample_track(1, "beta", 120.0);
        let mut b = sample_track(2, "Alpha", 125.0);
        b.key = None;
        b.bpm = None;
        // título insensible a mayúsculas
        assert_eq!(
            compare_tracks(&a, &b, SortField::Title, false),
            Ordering::Greater
        );
        assert_eq!(
            compare_tracks(&a, &b, SortField::Title, true),
            Ordering::Less
        );
        // Option None SIEMPRE al final, incluso en descendente (kit)
        assert_eq!(
            compare_tracks(&a, &b, SortField::Bpm, false),
            Ordering::Less
        );
        assert_eq!(compare_tracks(&a, &b, SortField::Bpm, true), Ordering::Less);
        assert_eq!(
            compare_tracks(&a, &b, SortField::Key, false),
            Ordering::Less
        );
    }

    #[test]
    fn sorting_by_number_restores_original_id_order() {
        // '#' = orden ORIGINAL de la biblioteca: tras ordenar por título, el
        // campo Number debe re-ordenar por id real (asc y desc).
        let a = sample_track(11, "alpha", 120.0);
        let b = sample_track(22, "Beta", 121.0);
        let c = sample_track(33, "gamma", 122.0);
        let shuffled = [b.clone(), c.clone(), a.clone()];
        let mut by_number = shuffled.clone();
        by_number.sort_by(|x, y| compare_tracks(x, y, SortField::Number, false));
        assert_eq!(
            by_number.iter().map(|t| t.id).collect::<Vec<_>>(),
            [11, 22, 33],
            "ascendente = orden original por id"
        );
        let mut desc = shuffled.clone();
        desc.sort_by(|x, y| compare_tracks(x, y, SortField::Number, true));
        assert_eq!(desc.iter().map(|t| t.id).collect::<Vec<_>>(), [33, 22, 11]);
    }

    #[test]
    fn sorting_by_rating_and_duration_orders_values_with_none_last() {
        let mut a = sample_track(1, "beta", 120.0);
        a.rating = 4;
        a.duration_seconds = Some(200.0);
        let mut b = sample_track(2, "Alpha", 125.0);
        b.rating = 2;
        b.duration_seconds = None;
        assert_eq!(
            compare_tracks(&a, &b, SortField::Rating, false),
            Ordering::Greater,
            "4 > 2"
        );
        assert_eq!(
            compare_tracks(&a, &b, SortField::Rating, true),
            Ordering::Less
        );
        // duración: None SIEMPRE al final (regla del kit), incluso desc
        assert_eq!(
            compare_tracks(&a, &b, SortField::Duration, false),
            Ordering::Less
        );
        assert_eq!(
            compare_tracks(&a, &b, SortField::Duration, true),
            Ordering::Less
        );
        let mut c = sample_track(3, "Gamma", 126.0);
        c.duration_seconds = Some(100.0);
        assert_eq!(
            compare_tracks(&c, &a, SortField::Duration, false),
            Ordering::Less,
            "100s < 200s"
        );
    }

    #[test]
    fn cover_uv_center_crops_non_square_art() {
        // cuadrada: UV completa
        assert_eq!(
            cover_uv(egui::vec2(64.0, 64.0)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
        );
        // horizontal 2:1 → recorte centrado del 50% en X, V completa en Y
        assert_eq!(
            cover_uv(egui::vec2(128.0, 64.0)),
            Rect::from_min_max(pos2(0.25, 0.0), pos2(0.75, 1.0))
        );
        // vertical 1:2 → recorte centrado del 50% en Y
        assert_eq!(
            cover_uv(egui::vec2(64.0, 128.0)),
            Rect::from_min_max(pos2(0.0, 0.25), pos2(1.0, 0.75))
        );
        // degenerada: UV completa (defensivo, sin panic)
        assert_eq!(
            cover_uv(egui::vec2(0.0, 0.0)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
        );
    }
}
