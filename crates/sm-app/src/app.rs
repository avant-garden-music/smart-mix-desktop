//! Smart Mix — aplicación principal (egui).
//!
//! Layout v2:
//!   top    — barra slim (wordmark, menús, búsqueda, compat) + NOW PLAYING
//!            (carátula, métricas, transporte, waveform con zoom, cues)
//!   left   — colecciones | playlists | USB (expandible, SOLO LECTURA)
//!   center — tabla virtualizada egui_extras (columnas sincronizadas,
//!            redimensionables, encabezado fijo, truncado con clip)
//!   bottom — estado y progreso del análisis (nada más)
//!   ventanas contextuales — correcciones, crate USB, prefs, export…

use crate::texts;
use crate::theme::{energy_color, font_heading, key_color, Theme};
use crate::usb::{scan_drives, UsbDrive};
use crate::widgets::WaveView;
use crate::worker::{AnalysisEvent, AnalysisPool};
use egui::{Color32, Sense};
use sm_audio::{deck_pair, Player};
use sm_core::models::Models;
use sm_db::{LibraryDb, LibraryQuery, SortField};
use sm_library::{read_metadata, scan_root};
use sm_model::{AnalysisStatus, CuePoint, Preferences, TrackRow};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

const ROW_H: f32 = 20.0;

/// Definición ÚNICA de columnas (encabezado y filas comparten estructura).
const COLS: [(&str, Option<SortField>); 8] = [
    ("#", None),
    ("col.title", Some(SortField::Title)),
    ("col.artist", Some(SortField::Artist)),
    ("col.album", Some(SortField::Album)),
    ("col.key", Some(SortField::Key)),
    ("col.bpm", Some(SortField::Bpm)),
    ("col.energy", Some(SortField::Energy)),
    ("col.time", Some(SortField::Duration)),
];

/// Caché de filas por página con acceso por índice (virtualización).
pub struct TableCache {
    db: Arc<Mutex<LibraryDb>>,
    query: LibraryQuery,
    rows: BTreeMap<usize, TrackRow>,
    fetched: (usize, usize),
}

impl TableCache {
    fn new(db: Arc<Mutex<LibraryDb>>, query: LibraryQuery) -> Self {
        TableCache {
            db,
            query,
            rows: BTreeMap::new(),
            fetched: (0, 0),
        }
    }
    /// Fila i, trayendo página de 500 si está fuera de caché.
    fn get(&mut self, i: usize) -> Option<TrackRow> {
        if i < self.fetched.0 || i >= self.fetched.1 {
            let lo = i.saturating_sub(100);
            let mut q = self.query.clone();
            q.offset = lo as u64;
            q.limit = 500;
            if let Ok(db) = self.db.lock() {
                if let Ok(page) = db.query(&q) {
                    let n = page.len();
                    self.rows.clear();
                    for (j, r) in page.into_iter().enumerate() {
                        self.rows.insert(lo + j, r);
                    }
                    if n > 0 {
                        self.fetched = (lo, lo + n);
                    }
                }
            }
        }
        self.rows.get(&i).cloned()
    }
    fn path_of(&self, id: i64) -> Option<String> {
        self.db
            .lock()
            .ok()
            .and_then(|d| d.track_full(id).unwrap_or(None))
            .map(|f| f.meta.path)
    }
}

pub struct SmartMixApp {
    db: Arc<Mutex<LibraryDb>>,
    player: Option<Player>,
    models: Arc<Models>,
    pool: AnalysisPool,
    pub prefs: Preferences,
    egui_ctx: egui::Context,
    // biblioteca
    search: String,
    sort: (SortField, bool),
    total: i64,
    rows: BTreeMap<usize, TrackRow>,
    fetched_range: (usize, usize),
    hover_row: Option<usize>,
    selected_id: Option<i64>,
    detail: Option<sm_db::TrackFull>,
    cover_tex: Option<egui::TextureHandle>,
    cover_tex_id: Option<i64>,
    compat_key: Option<String>,
    collection: Collection,
    playlists: Vec<sm_model::Playlist>,
    active_playlist: Option<i64>,
    // now playing
    active_deck: usize,
    deck_track_id: [Option<i64>; 2],
    wave_zoom: (f64, f64),
    /// Clave (pista, duración×20) para la que vale `wave_zoom`: el zoom se
    /// resetea a ventana completa al cambiar de pista O cuando la duración
    /// cambia (p. ej. el deck completa la carga y la duración real crece).
    wave_zoom_key: (Option<i64>, i64),
    // usb
    usb_drives: Vec<UsbDrive>,
    usb_expanded: HashMap<String, bool>,
    usb_crate_view: Option<(String, Vec<String>)>,
    // ventanas
    show_prefs: bool,
    show_export: bool,
    show_new_playlist: bool,
    show_about: bool,
    show_corrections: bool,
    auto_queue_after_scan: bool,
    new_playlist_name: String,
    new_playlist_smart: bool,
    export_log: String,
    status_msg: String,
    scan_busy: Arc<Mutex<bool>>,
    ctx_repaint: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Debug, Clone, PartialEq)]
enum Collection {
    All,
    Unanalyzed,
    Duplicates,
}

impl SmartMixApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<SmartMixApp> {
        crate::theme::install(&cc.egui_ctx);
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("smart-mix");
        std::fs::create_dir_all(&data_dir)?;
        let db = LibraryDb::open(&data_dir.join("library.db"))?;
        let models = Arc::new(Models::load(&sm_core::pipeline::default_models_dir()));
        let Some(player) = deck_pair().ok() else {
            tracing::error!("no se pudo abrir el dispositivo de audio");
            return Err(anyhow::anyhow!("sin dispositivo de audio de salida"));
        };
        // motor → egui: repaint al aplicar comandos (seek/play): el playhead
        // refleja el estado real de inmediato, no en el próximo input
        {
            let ctx = cc.egui_ctx.clone();
            let cb: Arc<dyn Fn() + Send + Sync> = Arc::new(move || ctx.request_repaint());
            player.set_repaint(cb);
        }
        let prefs = db.get_preferences().unwrap_or_default();
        let db = Arc::new(Mutex::new(db));
        let repaint_ctx = cc.egui_ctx.clone();
        let repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(move || repaint_ctx.request_repaint());
        let pool = AnalysisPool::new(
            db.clone(),
            models.clone(),
            prefs.analysis_workers.max(1).min(16),
            repaint.clone(),
        );
        let mut app = SmartMixApp {
            db,
            player: Some(player),
            models,
            pool,
            prefs,
            egui_ctx: cc.egui_ctx.clone(),
            search: String::new(),
            sort: (SortField::AddedAt, true),
            total: 0,
            rows: BTreeMap::new(),
            fetched_range: (0, 0),
            hover_row: None,
            selected_id: None,
            detail: None,
            cover_tex: None,
            cover_tex_id: None,
            compat_key: None,
            collection: Collection::All,
            playlists: Vec::new(),
            active_playlist: None,
            active_deck: 0,
            deck_track_id: [None, None],
            wave_zoom: (0.0, 0.0),
            wave_zoom_key: (None, 0),
            usb_drives: Vec::new(),
            usb_expanded: HashMap::new(),
            usb_crate_view: None,
            show_prefs: false,
            show_export: false,
            show_new_playlist: false,
            show_about: false,
            show_corrections: false,
            auto_queue_after_scan: false,
            new_playlist_name: String::new(),
            new_playlist_smart: false,
            export_log: String::new(),
            status_msg: String::new(),
            scan_busy: Arc::new(Mutex::new(false)),
            ctx_repaint: repaint,
        };
        app.refresh();
        // primera pista preseleccionada: el panel Now Playing es visible
        // desde el primer frame
        if app.total > 0 {
            app.ensure_rows((0, 1));
            if let Some(r) = app.rows.get(&0).cloned() {
                app.select(r.id);
            }
        }
        Ok(app)
    }

    fn player(&self) -> Option<&Player> {
        self.player.as_ref()
    }

    // ------------------------------------------------------------- datos ----

    fn query(&self) -> LibraryQuery {
        LibraryQuery {
            text: if self.search.is_empty() {
                None
            } else {
                Some(self.search.clone())
            },
            sort: self.sort.0,
            desc: self.sort.1,
            limit: 400,
            offset: 0,
            playlist_id: self.active_playlist,
            smart: Some(sm_model::SmartFilter {
                text: None,
                key_compatible_with: self.compat_key.clone(),
                ..Default::default()
            }),
            status_filter: if self.collection == Collection::Unanalyzed {
                Some(AnalysisStatus::Pending)
            } else {
                None
            },
        }
    }

    fn refresh(&mut self) {
        self.rows.clear();
        self.fetched_range = (0, 0);
        if let Ok(db) = self.db.lock() {
            if self.collection == Collection::Duplicates {
                if let Ok(groups) = db.duplicates() {
                    let mut rows = Vec::new();
                    for g in &groups {
                        for (id, path, dur) in &g.tracks {
                            rows.push(TrackRow {
                                id: *id,
                                title: std::path::Path::new(path)
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_default(),
                                artist: format!("⟲ {}", g.reason),
                                bpm: None,
                                key_camelot: None,
                                energy: None,
                                duration_s: *dur,
                                album: String::new(),
                                genre: String::new(),
                            });
                        }
                    }
                    self.total = rows.len() as i64;
                    for (i, r) in rows.into_iter().enumerate() {
                        self.rows.insert(i, r);
                    }
                    self.fetched_range = (0, self.total as usize);
                    self.playlists = db.playlists().unwrap_or_default();
                    return;
                }
            }
            self.total = db.count(&self.query()).unwrap_or(0);
            self.playlists = db.playlists().unwrap_or_default();
        }
    }

    fn ensure_rows(&mut self, range: (usize, usize)) {
        if range.0 >= range.1
            || (range.0 >= self.fetched_range.0 && range.1 <= self.fetched_range.1)
        {
            return;
        }
        let lo = range.0.saturating_sub(100);
        let hi = range.1 + 400;
        if let Ok(db) = self.db.lock() {
            let mut q = self.query();
            q.offset = lo as u64;
            q.limit = hi - lo;
            if let Ok(page) = db.query(&q) {
                self.rows.clear();
                for (i, r) in page.into_iter().enumerate() {
                    self.rows.insert(lo + i, r);
                }
                self.fetched_range = (lo, hi);
            }
        }
    }

    fn select(&mut self, id: i64) {
        self.selected_id = Some(id);
        self.load_detail(id);
    }

    fn load_detail(&mut self, id: i64) {
        if let Ok(db) = self.db.lock() {
            self.detail = db.track_full(id).unwrap_or(None);
            if let Some(cover) = db.cover(id).unwrap_or(None) {
                if let Some(img) = image::load_from_memory(&cover).ok() {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    self.cover_tex = Some(self.egui_ctx.load_texture(
                        format!("cover-{id}"),
                        egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                        egui::TextureOptions::LINEAR,
                    ));
                    self.cover_tex_id = Some(id);
                    return;
                }
            }
        }
        self.cover_tex = None;
        self.cover_tex_id = None;
    }

    fn deck_play(&mut self, deck: usize) {
        eprintln!("[play] deck={deck} sel={:?}", self.selected_id);
        let Some(id) = self.selected_id else { return };
        let Some(full) = &self.detail else { return };
        let Some(player) = &self.player else { return };
        let path = full.meta.path.clone();
        player.decks[deck].load(&path);
        player.decks[deck].play();
        self.deck_track_id[deck] = Some(id);
        self.active_deck = deck;
    }

    fn now_playing_id(&self) -> Option<i64> {
        self.deck_track_id[self.active_deck].or(self.selected_id)
    }

    fn import_folder(&mut self) {
        let Some(folder) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        self.status_msg = format!("Escaneando {}…", folder.display());
        let db = self.db.clone();
        let busy = self.scan_busy.clone();
        let repaint = self.ctx_repaint.clone();
        {
            let mut b = busy.lock().unwrap();
            if *b {
                return;
            }
            *b = true;
        }
        if self.prefs.analyze_on_import {
            self.auto_queue_after_scan = true;
        }
        std::thread::spawn(move || {
            let files = scan_root(&folder);
            let mut n = 0usize;
            for (i, f) in files.iter().enumerate() {
                if i % 25 == 0 {
                    repaint();
                }
                if let Ok(scanned) = read_metadata(f) {
                    if let Ok(mut db) = db.lock() {
                        match db.upsert_track(&scanned.meta) {
                            Ok(id) => {
                                if let Some(cover) = scanned.cover {
                                    let _ = db.set_cover(id, Some(cover));
                                }
                                n += 1;
                            }
                            Err(e) => tracing::warn!("upsert {}: {e}", f.display()),
                        }
                    }
                }
            }
            tracing::info!("import: {n}/{} pistas", files.len());
            *busy.lock().unwrap() = false;
            repaint();
        });
    }

    fn analyze_pending(&mut self) {
        if let Ok(db) = self.db.lock() {
            let pending = db
                .track_ids_by_status(AnalysisStatus::Pending, 1_000_000)
                .unwrap_or_default();
            self.pool.enqueue(pending);
            self.status_msg = format!("Analizando {} pistas…", self.pool.queue_len());
        }
    }

    fn reanalyze_track(&mut self, id: i64) {
        let Some(full) = &self.detail else { return };
        let path = full.meta.path.clone();
        if let Ok(mut db) = self.db.lock() {
            let _ = db.set_analysis_status(id, AnalysisStatus::Pending);
        }
        self.pool.enqueue(vec![(id, path)]);
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.pool.events.try_recv() {
            if let AnalysisEvent::Failed(id, e) = ev {
                self.status_msg = format!("Error analizando #{id}: {e}");
            }
        }
    }
}

impl eframe::App for SmartMixApp {
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        if let Ok(db) = self.db.lock() {
            let _ = db.set_preferences(&self.prefs);
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        // Repaint continuo mientras reproduce o hay trabajo de fondo:
        // sin esto el playhead solo avanza cuando mueves el ratón (bug v1).
        let playing = self
            .player()
            .map(|p| p.decks.iter().any(|d| d.is_playing()))
            .unwrap_or(false);
        if playing || self.pool.is_busy() || *self.scan_busy.lock().unwrap() {
            ctx.request_repaint();
        } else {
            // reposo: repintado diferido liviano. Sin esto la superficie puede
            // quedar sin pintar al lanzar (pantalla negra) hasta que llegue un
            // evento del usuario.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if self.auto_queue_after_scan && !*self.scan_busy.lock().unwrap() {
            self.auto_queue_after_scan = false;
            self.refresh();
            self.analyze_pending();
        }
        // refrescar detalle si el análisis tocó la selección
        if let Some(id) = self.selected_id {
            if let Ok(db) = self.db.lock() {
                if let Some(f) = db.track_full(id).unwrap_or(None) {
                    if f.analysis.analysis_ms
                        != self.detail.as_ref().and_then(|d| d.analysis.analysis_ms)
                    {
                        self.detail = Some(f);
                    }
                }
            }
        }
        self.top_bar(ctx);
        self.now_playing(ctx);
        self.decks_strip(ctx);
        self.side_panel(ctx);
        self.status_bar(ctx);
        self.central_table(ctx);
        self.windows(ctx);
    }
}

impl SmartMixApp {
    // ------------------------------------------------------------ panels ----

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(10.0, 5.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    crate::theme::wordmark(ui);
                    ui.separator();
                    egui::menu::bar(ui, |ui| {
                        ui.menu_button(texts::t(&self.prefs.language, "menu.file"), |ui| {
                            if ui
                                .button(texts::t(&self.prefs.language, "menu.import_folder"))
                                .clicked()
                            {
                                self.import_folder();
                                ui.close_menu();
                            }
                            if ui
                                .button(texts::t(&self.prefs.language, "menu.analyze_pending"))
                                .clicked()
                            {
                                self.analyze_pending();
                                ui.close_menu();
                            }
                            if ui
                                .button(texts::t(&self.prefs.language, "menu.reanalyze"))
                                .clicked()
                            {
                                if let Some(id) = self.selected_id {
                                    self.reanalyze_track(id);
                                }
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui
                                .button(texts::t(&self.prefs.language, "menu.export"))
                                .clicked()
                            {
                                self.show_export = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui
                                .button(texts::t(&self.prefs.language, "menu.prefs"))
                                .clicked()
                            {
                                self.show_prefs = true;
                                ui.close_menu();
                            }
                        });
                        ui.menu_button(texts::t(&self.prefs.language, "menu.view"), |ui| {
                            if ui
                                .button(texts::t(&self.prefs.language, "menu.about"))
                                .clicked()
                            {
                                self.show_about = true;
                                ui.close_menu();
                            }
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        egui::ComboBox::from_id_source("compat")
                            .selected_text(self.compat_key.clone().unwrap_or_else(|| "—".into()))
                            .width(64.0)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(self.compat_key.is_none(), "—")
                                    .clicked()
                                {
                                    self.compat_key = None;
                                    self.refresh();
                                }
                                for n in 1..=12u8 {
                                    for l in ["A", "B"] {
                                        let k = format!("{n}{l}");
                                        if ui
                                            .selectable_label(
                                                self.compat_key.as_deref() == Some(k.as_str()),
                                                &k,
                                            )
                                            .clicked()
                                        {
                                            self.compat_key = Some(k);
                                            self.refresh();
                                        }
                                    }
                                }
                            });
                        ui.label(texts::t(&self.prefs.language, "toolbar.compat"));
                        let search = egui::TextEdit::singleline(&mut self.search)
                            .hint_text(texts::t(&self.prefs.language, "toolbar.search"))
                            .desired_width(210.0);
                        if ui.add(search).changed() {
                            self.refresh();
                        }
                    });
                });
            });
    }

    fn now_playing(&mut self, ctx: &egui::Context) {
        let t = Theme::default();
        egui::TopBottomPanel::top("now")
            .frame(
                egui::Frame::none()
                    .fill(t.surface)
                    .inner_margin(egui::Margin::symmetric(10.0, 7.0)),
            )
            .show(ctx, |ui| {
                let Some(id) = self.now_playing_id() else {
                    ui.set_min_height(34.0);
                    ui.vertical_centered(|ui| {
                        ui.add_space(9.0);
                        ui.label(
                            egui::RichText::new(
                                "Selecciona una pista · doble clic la reproduce en el deck activo",
                            )
                            .color(t.text_dim)
                            .small(),
                        );
                    });
                    return;
                };
                if self.detail.as_ref().map(|d| d.id) != Some(id) {
                    self.load_detail(id);
                }
                if self.cover_tex_id != Some(id) {
                    self.load_detail(id);
                }
                let Some(full) = self.detail.clone() else {
                    return;
                };
                let deck_handles = self.player.as_ref().map(|p| p.decks.clone());
                let deck = deck_handles.as_ref().map(|d| &d[self.active_deck]);
                let track_loaded = deck.map(|d| d.path() == full.meta.path).unwrap_or(false);
                let pos = if track_loaded {
                    deck.map(|d| d.position_s())
                } else {
                    None
                };
                let duration = if track_loaded {
                    deck.map(|d| d.duration_s())
                        .unwrap_or(full.analysis.duration_s)
                } else {
                    full.analysis.duration_s
                };
                let playing = track_loaded && deck.map(|d| d.is_playing()).unwrap_or(false);

                // DIAGNÓSTICO waveform: estado coherente una vez por segundo
                {
                    use std::sync::atomic::{AtomicU64, Ordering};
                    static LAST: AtomicU64 = AtomicU64::new(0);
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;
                    if now - LAST.load(Ordering::Relaxed) > 1000 && playing {
                        LAST.store(now, Ordering::Relaxed);
                        eprintln!(
                            "[wave] title={:?} pos={:.2} dur={:.2} db_dur={:.2} deck_dur={:.2} zoom=({:.2},{:.2})",
                            full.meta.title,
                            pos.unwrap_or(0.0),
                            duration,
                            full.analysis.duration_s,
                            deck.map(|d| d.duration_s()).unwrap_or(0.0),
                            self.wave_zoom.0,
                            self.wave_zoom.1,
                        );
                    }
                }

                ui.horizontal(|ui| {
                    // carátula
                    let cs = 56.0;
                    if let Some(tex) = &self.cover_tex {
                        ui.add(egui::Image::new(egui::ImageSource::Texture(
                            egui::load::SizedTexture::new(tex.id(), egui::vec2(cs, cs)),
                        )));
                    } else {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(cs, cs), Sense::hover());
                        ui.painter().rect_filled(rect, 4.0, t.surface2);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "♪",
                            font_heading(22.0),
                            t.text_dim,
                        );
                    }
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(if full.meta.title.is_empty() {
                                    "(sin título)"
                                } else {
                                    &full.meta.title
                                })
                                .font(crate::theme::font_editorial(20.0))
                                .color(t.text),
                            );
                            if let Some(p) = &deck_handles {
                                for (i, name) in ["A", "B"].iter().enumerate() {
                                    let active = self.active_deck == i;
                                    let loaded = p[i].is_loaded();
                                    let col = if i == 0 { t.deck_a } else { t.deck_b };
                                    let btn = egui::Button::new(
                                        egui::RichText::new(*name)
                                            .color(if active || loaded { col } else { t.text_dim })
                                            .strong(),
                                    )
                                    .small()
                                    .fill(if active {
                                        Color32::from_rgba_premultiplied(
                                            col.r(),
                                            col.g(),
                                            col.b(),
                                            36,
                                        )
                                    } else {
                                        t.surface2
                                    });
                                    if ui.add(btn).clicked() {
                                        self.active_deck = i;
                                    }
                                }
                            }
                        });
                        ui.label(
                            egui::RichText::new(if full.meta.artist.is_empty() {
                                if full.meta.album.is_empty() {
                                    String::new()
                                } else {
                                    full.meta.album.clone()
                                }
                            } else if full.meta.album.is_empty() {
                                full.meta.artist.clone()
                            } else {
                                format!("{} — {}", full.meta.artist, full.meta.album)
                            })
                            .color(t.text_dim),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // transporte
                        if let Some(p) = &deck_handles {
                            let d = &p[self.active_deck];
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(if playing { "⏸" } else { "▶" })
                                            .strong(),
                                    )
                                    .min_size(egui::vec2(34.0, 26.0)),
                                )
                                .clicked()
                            {
                                if playing {
                                    d.pause();
                                } else if d.is_loaded() {
                                    d.play();
                                } else {
                                    let path = full.meta.path.clone();
                                    d.load(&path);
                                    d.play();
                                    self.deck_track_id[self.active_deck] = Some(id);
                                }
                            }
                            if ui
                                .add(egui::Button::new("■").min_size(egui::vec2(28.0, 26.0)))
                                .clicked()
                            {
                                d.stop();
                            }
                            ui.separator();
                            let mut vol = d.volume();
                            if ui
                                .add(
                                    egui::Slider::new(&mut vol, 0.0..=1.5)
                                        .text("vol")
                                        .show_value(false),
                                )
                                .changed()
                            {
                                *d.current().volume.write() = vol;
                            }
                            let mut pitch = d.pitch();
                            if ui
                                .add(
                                    egui::Slider::new(&mut pitch, -12.0..=12.0)
                                        .text("pitch")
                                        .show_value(false),
                                )
                                .changed()
                            {
                                *d.current().pitch_semis.write() = pitch;
                            }
                        }
                        ui.separator();
                        if ui
                            .button("⋯")
                            .on_hover_text("Correcciones y acciones")
                            .clicked()
                        {
                            self.show_corrections = true;
                        }
                        // métricas (de derecha a izquierda: dur, LUFS… al final key)
                        ui.horizontal(|ui| {
                            // posición / duración del deck activo (verificable)
                            if track_loaded {
                                if let Some(p) = pos {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} / {}",
                                            fmt_time(p),
                                            fmt_time(duration)
                                        ))
                                        .monospace()
                                        .color(t.text),
                                    );
                                } else {
                                    ui.label(
                                        egui::RichText::new(fmt_time(duration))
                                            .monospace()
                                            .color(t.text_dim),
                                    );
                                }
                            } else {
                                ui.label(
                                    egui::RichText::new(fmt_time(duration))
                                        .monospace()
                                        .color(t.text_dim),
                                );
                            }
                            if let Some(v) = full.analysis.volume_db {
                                ui.label(
                                    egui::RichText::new(format!("{v:+.1} dB"))
                                        .monospace()
                                        .color(t.text_dim),
                                );
                            }
                            if let Some(l) = full.analysis.lufs {
                                ui.label(
                                    egui::RichText::new(format!("{l:+.1} LU"))
                                        .monospace()
                                        .color(t.text_dim),
                                );
                            }
                            if let Some(c) = full.analysis.clipped_peak_count {
                                if c > 0 {
                                    ui.label(egui::RichText::new(format!("⚠{c}")).color(t.danger));
                                }
                            }
                            if let Some(e) = full.analysis.energy {
                                let r = ui
                                    .allocate_exact_size(egui::vec2(42.0, 7.0), Sense::hover())
                                    .1
                                    .rect;
                                ui.painter().rect_filled(r, 3.0, t.surface2);
                                let frac = ((e - 1.0) / 9.0).clamp(0.0, 1.0);
                                let mut fill = r;
                                fill.set_width(42.0 * frac as f32);
                                ui.painter().rect_filled(fill, 3.0, energy_color(e));
                            }
                            if let Some(b) = full.analysis.tempo {
                                ui.label(
                                    egui::RichText::new(format!("{b:.2} BPM"))
                                        .monospace()
                                        .strong(),
                                );
                            }
                            if let Some(k) = &full.analysis.key_camelot {
                                let r = ui
                                    .allocate_exact_size(egui::vec2(40.0, 18.0), Sense::hover())
                                    .1
                                    .rect;
                                ui.painter().rect_filled(r, 4.0, key_color(k));
                                ui.painter().text(
                                    r.center(),
                                    egui::Align2::CENTER_CENTER,
                                    k,
                                    egui::FontId::monospace(12.0),
                                    Color32::BLACK,
                                );
                            }
                        });
                    });
                });
                ui.add_space(4.0);
                // waveform a ancho completo
                let wave = full.analysis.waveform.clone();
                let cues: Vec<f64> = full.analysis.cues.iter().map(|c| c.time_s).collect();
                let segs = full.analysis.segments.clone();
                let mut seek_tx: Option<f64> = None;
                // zoom ligado a (pista, duración): la ventana completa se
                // re-establece al cambiar de pista O al cambiar la duración
                // (deck completa la carga → duración real sustituye a la de BD)
                let zkey = (self.selected_id, (duration * 20.0).round() as i64);
                if self.wave_zoom_key != zkey {
                    self.wave_zoom = (0.0, duration.max(0.001));
                    self.wave_zoom_key = zkey;
                }
                // auto-heal: si el deck decodificó una duración real distinta
                // a la del análisis (>0.5 s), corregir la BD para que tabla,
                // lectura pos/total y waveform usen siempre la duración real
                if track_loaded && (duration - full.analysis.duration_s).abs() > 0.5 {
                    if let Some(id_fix) = self.selected_id {
                        let d_fix = duration;
                        if let Ok(db) = self.db.lock() {
                            if db.set_duration(id_fix, d_fix).is_ok() {
                                eprintln!(
                                    "[heal] duración corregida id={id_fix}: {:.2} → {d_fix:.2}",
                                    full.analysis.duration_s
                                );
                            }
                        }
                        if let Some(f) = self.detail.as_mut() {
                            f.analysis.duration_s = duration;
                        }
                    }
                }
                let zoom = &mut self.wave_zoom;
                ui.add(&mut WaveView {
                    zoom,
                    wave: &wave,
                    duration_s: duration.max(0.001),
                    position_s: pos,
                    cues: &cues,
                    segments: &segs,
                    height: 62.0,
                    on_seek: &mut |t| seek_tx = Some(t),
                });
                if let Some(t) = seek_tx {
                    if let Some(p) = deck_handles.as_ref() {
                        let d: &sm_audio::DeckHandle = &p[self.active_deck];
                        if !d.is_loaded() || d.path() != full.meta.path {
                            d.load(&full.meta.path);
                            self.deck_track_id[self.active_deck] = Some(id);
                        }
                        d.seek(t);
                    }
                    // doble garantía: frame tras el apply del pump (~5 ms)
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
                // cues compactas
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("CUES")
                            .font(font_heading(8.5))
                            .color(t.text_dim),
                    );
                    let mut jump: Option<f64> = None;
                    let mut del: Option<usize> = None;
                    for (i, cue) in full.analysis.cues.iter().enumerate() {
                        let btn = egui::Button::new(
                            egui::RichText::new(format!("{} {:>6.1}", i + 1, cue.time_s))
                                .monospace()
                                .color(t.text),
                        )
                        .small()
                        .fill(t.surface2);
                        if ui.add(btn).on_hover_text("saltar").clicked() {
                            jump = Some(cue.time_s);
                        }
                        if ui.small_button("×").clicked() {
                            del = Some(i);
                        }
                    }
                    if ui
                        .small_button("+")
                        .on_hover_text("cue en playhead (snap a beat)")
                        .clicked()
                    {
                        let mut cues2 = full.analysis.cues.clone();
                        let tpos = self
                            .player()
                            .map(|p| p.decks[self.active_deck].position_s())
                            .unwrap_or(0.0);
                        let mut snapped = tpos;
                        if let Some(bg) = &full.analysis.beatgrid {
                            snapped = bg.beat_time(bg.beat_index_near(tpos));
                        }
                        cues2.push(CuePoint {
                            time_s: snapped,
                            energy_level: None,
                            origin: "user".into(),
                            label: None,
                        });
                        cues2.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
                        if let Ok(mut db) = self.db.lock() {
                            db.replace_cues(full.id, &cues2).ok();
                        }
                        self.load_detail(full.id);
                    }
                    if let Some(tp) = jump {
                        if let Some(p) = &deck_handles {
                            let d = &p[self.active_deck];
                            if !d.is_loaded() || d.path() != full.meta.path {
                                d.load(&full.meta.path);
                                self.deck_track_id[self.active_deck] = Some(id);
                            }
                            d.seek(tp);
                            d.play();
                            ctx.request_repaint_after(std::time::Duration::from_millis(16));
                        }
                    }
                    if let Some(i) = del {
                        let mut cues2 = full.analysis.cues.clone();
                        cues2.remove(i);
                        if let Ok(mut db) = self.db.lock() {
                            db.replace_cues(full.id, &cues2).ok();
                        }
                        self.load_detail(full.id);
                    }
                });
            });
    }

    /// Tira de decks Pro: A y B siempre visibles con transporte propio,
    /// SYNC de BPM (via pitch), pitch con lectura % y vol por deck.
    fn decks_strip(&mut self, ctx: &egui::Context) {
        use sm_audio::DeckHandle;
        let t = Theme::default();
        let decks: Vec<DeckHandle> = self
            .player()
            .map(|p| p.decks.to_vec())
            .unwrap_or_default();
        if decks.is_empty() {
            return;
        }
        let db = self.db.clone();
        // estado de decks 1/s (verificación sin visión)
        {
            use std::sync::atomic::{AtomicU64, Ordering};
            static LAST: AtomicU64 = AtomicU64::new(0);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            let any_playing = decks.iter().any(|d| d.is_playing());
            if any_playing && now - LAST.load(Ordering::Relaxed) > 1000 {
                LAST.store(now, Ordering::Relaxed);
                for (i, d) in decks.iter().enumerate() {
                    eprintln!(
                        "[decks] {} title={:?} pos={:.2}/{:.2} playing={} pitch={:+.2}st",
                        if i == 0 { 'A' } else { 'B' },
                        d.title(),
                        d.position_s(),
                        d.duration_s(),
                        d.is_playing(),
                        d.pitch()
                    );
                }
            }
        }
        egui::TopBottomPanel::top("decks")
            .frame(
                egui::Frame::none()
                    .fill(t.bg)
                    .inner_margin(egui::Margin::symmetric(10.0, 4.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for i in 0..2usize {
                        let d = &decks[i];
                        let col = if i == 0 { t.deck_a } else { t.deck_b };
                        let active = self.active_deck == i;
                        let loaded = d.is_loaded();
                        let playing = loaded && d.is_playing();
                        egui::Frame::none()
                            .fill(if active {
                                Color32::from_rgba_premultiplied(col.r(), col.g(), col.b(), 24)
                            } else {
                                t.surface
                            })
                            .stroke(egui::Stroke::new(1.0, if active { col } else { t.border }))
                            .rounding(4.0)
                            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                            .show(ui, |ui| {
                                ui.set_width(430.0);
                                ui.horizontal_centered(|ui| {
                                    // pill nombre: activa el deck
                                    let pill = egui::Button::new(
                                        egui::RichText::new(if i == 0 { "A" } else { "B" })
                                            .strong()
                                            .color(col),
                                    )
                                    .small()
                                    .fill(if active {
                                        Color32::from_rgba_premultiplied(
                                            col.r(), col.g(), col.b(), 40,
                                        )
                                    } else {
                                        t.surface2
                                    });
                                    if ui.add(pill).clicked() {
                                        self.active_deck = i;
                                    }
                                    // título truncado en caja de ancho fijo
                                    let title = d.title();
                                    let tl = ui.add_sized(
                                        [140.0, 15.0],
                                        egui::Label::new(if loaded { &title } else { "— vacío —" })
                                            .truncate(true)
                                            .selectable(false),
                                    );
                                    tl.on_hover_text(if loaded { &title } else { "deck vacío" });
                                    // pos / dur (JetBrains Mono)
                                    if loaded {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{} / {}",
                                                fmt_time(d.position_s()),
                                                fmt_time(d.duration_s())
                                            ))
                                            .monospace()
                                            .small()
                                            .color(t.text_dim),
                                        );
                                    }
                                    // transporte
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new(if playing { "⏸" } else { "▶" })
                                                    .strong(),
                                            )
                                            .small(),
                                        )
                                        .clicked()
                                    {
                                        eprintln!("[deck{}] play/pause (loaded={loaded}, playing={playing})", i);
                                        if playing {
                                            d.pause();
                                        } else if loaded {
                                            d.play();
                                        }
                                    }
                                    if ui.add(egui::Button::new("■").small()).clicked() {
                                        d.stop();
                                    }
                                    // cargar pista seleccionada (sin reproducir)
                                    let eject = ui.add(egui::Button::new("⏏").small());
                                    if eject.clicked() {
                                        eprintln!("[deck{}] eject pulsado", i);
                                        if let Some(full) = &self.detail {
                                            let path = full.meta.path.clone();
                                            d.load(&path);
                                            self.deck_track_id[i] = self.selected_id;
                                            self.status_msg = format!(
                                                "Deck {}: {} cargado",
                                                if i == 0 { "A" } else { "B" },
                                                full.meta.title
                                            );
                                        }
                                    }
                                    // SYNC: igualar BPM al otro deck via pitch
                                    let other = 1 - i;
                                    let other_loaded = decks[other].is_loaded();
                                    let sync_txt = egui::RichText::new("SYNC")
                                        .small()
                                        .color(if other_loaded { col } else { t.text_dim });
                                    if ui
                                        .add_enabled(other_loaded, egui::Button::new(sync_txt).small())
                                        .clicked()
                                    {
                                        eprintln!("[deck{}] SYNC pulsado", i);
                                        let bpm_of = |id: Option<i64>| -> Option<f64> {
                                            id.and_then(|id| {
                                                db.lock().ok().and_then(|db| {
                                                    db.track_full(id).ok().flatten()
                                                })
                                            })
                                            .and_then(|f| f.analysis.tempo)
                                        };
                                        let bo = bpm_of(self.deck_track_id[other]);
                                        let bm = bpm_of(self.deck_track_id[i]);
                                        if let (Some(bo), Some(bm)) = (bo, bm) {
                                            if bm > 1.0 {
                                                let semis = (12.0 * (bo / bm).log2()).clamp(-12.0, 12.0);
                                                *d.current().pitch_semis.write() = semis as f32;
                                                self.status_msg = if semis.abs() >= 12.0 {
                                                    format!("SYNC {}→{:.2} BPM limitado a {:+.2} st", bm, bo, semis)
                                                } else {
                                                    format!("SYNC {}→{:.2} BPM ({:+.2} st)", bm, bo, semis)
                                                };
                                            }
                                        }
                                    }
                                    // pitch: slider + lectura % + doble clic reset
                                    let mut pitch = d.pitch();
                                    let resp = ui.add(
                                        egui::Slider::new(&mut pitch, -12.0..=12.0)
                                            .show_value(false)
                                            .clamp_to_range(false),
                                    );
                                    if resp.changed() {
                                        *d.current().pitch_semis.write() = pitch;
                                    }
                                    if resp.double_clicked() {
                                        *d.current().pitch_semis.write() = 0.0;
                                    }
                                    let pct = (2f32.powf(pitch / 12.0) - 1.0) * 100.0;
                                    ui.label(
                                        egui::RichText::new(format!("{pct:+.1}%"))
                                            .monospace()
                                            .small()
                                            .color(if pitch.abs() > 0.01 { t.accent } else { t.text_dim }),
                                    );
                                    // volumen por deck
                                    let mut vol = d.volume();
                                    if ui
                                        .add(
                                            egui::Slider::new(&mut vol, 0.0..=1.5)
                                                .show_value(false),
                                        )
                                        .changed()
                                    {
                                        *d.current().volume.write() = vol;
                                    }
                                });
                            });
                        if i == 0 {
                            ui.separator();
                        }
                    }
                });
            });
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        let t = Theme::default();
        egui::SidePanel::left("side")
            .resizable(false)
            .default_width(214.0)
            .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(10.0, 8.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("COLECCIONES")
                            .font(font_heading(9.0))
                            .color(t.text_dim),
                    );
                    let items = [
                        (Collection::All, "col.all", "◈"),
                        (Collection::Unanalyzed, "col.unanalyzed", "◌"),
                        (Collection::Duplicates, "col.duplicates", "⟲"),
                    ];
                    for (col, key, icon) in items {
                        let sel = self.collection == col && self.active_playlist.is_none();
                        let label = texts::t(&self.prefs.language, key);
                        let base = egui::RichText::new(format!("{icon}  {label}")).color(if sel {
                            t.accent
                        } else {
                            t.text_dim
                        });
                        let txt = if sel { base.strong() } else { base };
                        if ui.selectable_label(sel, txt).clicked() {
                            self.collection = col;
                            self.active_playlist = None;
                            self.refresh();
                        }
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("PLAYLISTS")
                                .font(font_heading(9.0))
                                .color(t.text_dim),
                        );
                        if ui.small_button("+").clicked() {
                            self.new_playlist_name.clear();
                            self.new_playlist_smart = false;
                            self.show_new_playlist = true;
                        }
                    });
                    for pl in self.playlists.clone() {
                        let sel = self.active_playlist == Some(pl.id);
                        let icon = if pl.kind == "smart" { "✦" } else { "≡" };
                        let txt = egui::RichText::new(format!("{icon}  {}", pl.name))
                            .color(if sel { t.accent } else { t.text });
                        if ui.selectable_label(sel, txt).clicked() {
                            self.active_playlist = Some(pl.id);
                            self.collection = Collection::All;
                            self.refresh();
                        }
                    }
                    if self.active_playlist.is_some()
                        && ui
                            .button(texts::t(&self.prefs.language, "playlist.clear"))
                            .clicked()
                    {
                        self.active_playlist = None;
                        self.refresh();
                    }
                    // ---- USB (solo lectura) ----
                    ui.add_space(10.0);
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("USB")
                                .font(font_heading(9.0))
                                .color(t.text_dim),
                        );
                        if ui
                            .small_button("↻")
                            .on_hover_text("detectar dispositivos")
                            .clicked()
                        {
                            self.usb_drives = scan_drives();
                            self.status_msg =
                                format!("{} dispositivo(s) USB detectados", self.usb_drives.len());
                        }
                    });
                    for drive in self.usb_drives.clone() {
                        let key = drive.letter_or_name.clone();
                        let open = *self.usb_expanded.get(&key).unwrap_or(&false);
                        let arrow = if open { "▾" } else { "▸" };
                        let header = egui::RichText::new(format!(
                            "{arrow} {}: {} ({:.0} GB)",
                            key,
                            if drive.label.is_empty() {
                                "USB"
                            } else {
                                &drive.label
                            },
                            drive.size_gb
                        ))
                        .color(t.text_dim);
                        if ui.selectable_label(open, header).clicked() {
                            self.usb_expanded.insert(key.clone(), !open);
                        }
                        if open {
                            ui.indent(format!("usb-{key}"), |ui| {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.1} GB libres · solo lectura",
                                        drive.free_gb
                                    ))
                                    .small()
                                    .color(t.text_dim),
                                );
                                if drive.has_pioneer {
                                    ui.label(
                                        egui::RichText::new("◈ export rekordbox (PIONEER/)")
                                            .small()
                                            .color(t.text_dim),
                                    );
                                }
                                if drive.has_engine {
                                    ui.label(
                                        egui::RichText::new("◈ Engine DJ (EngineLibrary/)")
                                            .small()
                                            .color(t.text_dim),
                                    );
                                }
                                if !drive.serato_crates.is_empty() {
                                    ui.label(
                                        egui::RichText::new("Serato Subcrates")
                                            .small()
                                            .color(t.text_dim),
                                    );
                                    for (name, entries) in &drive.serato_crates {
                                        if ui
                                            .selectable_label(
                                                false,
                                                egui::RichText::new(format!(
                                                    "  ≡ {name} ({})",
                                                    entries.len()
                                                ))
                                                .small()
                                                .color(t.text_dim),
                                            )
                                            .on_hover_text("ver pistas")
                                            .clicked()
                                        {
                                            self.usb_crate_view =
                                                Some((name.clone(), entries.clone()));
                                        }
                                    }
                                } else if !drive.has_pioneer && !drive.has_engine {
                                    ui.label(
                                        egui::RichText::new("sin playlists detectadas")
                                            .small()
                                            .color(t.text_dim),
                                    );
                                }
                            });
                        }
                    }
                    if self.usb_drives.is_empty() {
                        ui.label(
                            egui::RichText::new("sin dispositivos — pulsa ↻")
                                .small()
                                .color(t.text_dim),
                        );
                    }
                });
            });
    }

    fn central_table(&mut self, ctx: &egui::Context) {
        use egui_extras::{Column, TableBuilder};
        egui::CentralPanel::default()
            .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(10.0, 4.0)))
            .show(ctx, |ui| {
                let t = Theme::default();
                // visibilidad aproximada para el fetch inicial (evita salto)
                self.ensure_rows((0, 60.min(self.total.max(0) as usize)));
                let cache = Rc::new(RefCell::new(TableCache::new(self.db.clone(), self.query())));
                let table_full_rect = ui.max_rect();
                let hover_prev = Rc::new(self.hover_row);
                let new_hover: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
                let row_y0: Rc<RefCell<Option<f32>>> = Rc::new(RefCell::new(None));
                // siembra la caché con lo ya traído
                {
                    let mut c = cache.borrow_mut();
                    c.rows = self.rows.clone();
                    c.fetched = self.fetched_range;
                }
                let clicked: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));
                let dbl: Rc<RefCell<Option<i64>>> = Rc::new(RefCell::new(None));
                let sort_req: Rc<RefCell<Option<SortField>>> = Rc::new(RefCell::new(None));
                let sel_id = self.selected_id;
                let cur_sort = self.sort;
                let total = self.total.max(0) as usize;
                let lang = self.prefs.language.clone();

                TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::exact(30.0))
                    .column(Column::remainder().at_least(90.0).clip(true))
                    .column(Column::initial(160.0).at_least(70.0).clip(true))
                    .column(Column::initial(140.0).at_least(70.0).clip(true))
                    .column(Column::exact(52.0))
                    .column(Column::exact(64.0))
                    .column(Column::exact(58.0))
                    .column(Column::exact(54.0))
                    .header(22.0, |mut header| {
                        for (key, sort) in COLS {
                            header.col(|ui| {
                                let name = if key == "#" {
                                    "#".to_string()
                                } else {
                                    texts::t(&lang, key)
                                };
                                let active = sort.as_ref() == Some(&cur_sort.0);
                                let txt = egui::RichText::new(if active && cur_sort.1 {
                                    format!("{name} ↓")
                                } else if active {
                                    format!("{name} ↑")
                                } else {
                                    name
                                })
                                .font(font_heading(9.0))
                                .color(if active {
                                    t.accent
                                } else {
                                    t.text_dim
                                });
                                if ui.selectable_label(false, txt).clicked() {
                                    *sort_req.borrow_mut() = sort;
                                }
                            });
                        }
                    })
                    .body(|mut body| {
                        body.rows(ROW_H, total, |mut row| {
                            let i = row.index();
let tr = cache.borrow_mut().get(i);
                            let Some(tr) = tr else {
                                for _ in 0..COLS.len() {
                                    row.col(|ui| {
                                        ui.add_space(ROW_H - 4.0);
                                    });
                                }
                                return;
                            };
                            let selected = Some(tr.id) == sel_id;
                            let row_hov = *hover_prev == Some(i);
                            let cell_bg = |ui: &mut egui::Ui| {
                                if row_hov {
                                    ui.painter().rect_filled(
                                        ui.max_rect(),
                                        0.0,
                                        t.row_hover,
                                    );
                                } else if selected {
                                    ui.painter().rect_filled(
                                        ui.max_rect(),
                                        0.0,
                                        t.row_selected,
                                    );
                                }
                            };
                            let mut any_click = false;
                            let mut dbl_click = false;
                            // #
                            row.col(|ui| {
                                *row_y0.borrow_mut() = Some(ui.max_rect().top());
                                cell_bg(ui);
                                let resp = ui
                                    .add(
                                        egui::Label::new(
                                            egui::RichText::new(format!("{}", i + 1))
                                                .monospace()
                                                .color(t.text_dim),
                                        )
                                        .selectable(false),
                                    )
                                    .interact(egui::Sense::click());
                                any_click |= resp.clicked();
                                if resp.hovered() {
                                    *new_hover.borrow_mut() = Some(i);
                                }
                            });
                            // título
                            row.col(|ui| {
                                cell_bg(ui);
                                let base = egui::RichText::new(tr.title.clone())
                                    .color(if selected { t.accent } else { t.text });
                                let base = if selected { base.strong() } else { base };
                                let btn = egui::Button::new(base)
                                    .fill(Color32::TRANSPARENT)
                                    .small()
                                    .wrap(false);
                                let resp = ui.add_sized(
                                    [ui.available_width(), ROW_H - 2.0],
                                    btn,
                                );
                                any_click |= resp.clicked();
                                dbl_click |= resp.double_clicked();
                                if resp.hovered() {
                                    *new_hover.borrow_mut() = Some(i);
                                }
                            });
                            // artista / álbum
                            for txt in [&tr.artist, &tr.album] {
                                let rt = egui::RichText::new(txt.clone()).color(t.text_dim);
                                let mut hit = false;
                                let mut dbl2 = false;
                                row.col(|ui| {
                                    cell_bg(ui);
                                    let btn = egui::Button::new(rt.clone())
                                        .fill(Color32::TRANSPARENT)
                                        .small()
                                        .wrap(false);
                                    let resp = ui.add_sized(
                                        [ui.available_width(), ROW_H - 2.0],
                                        btn,
                                    );
                                    hit = resp.clicked();
                                    dbl2 |= resp.double_clicked();
                                    if resp.hovered() {
                                        *new_hover.borrow_mut() = Some(i);
                                    }
                                });
                                any_click |= hit;
                                dbl_click |= dbl2;
                            }
                            // tonalidad (chip)
                            let key_clicked = Rc::new(RefCell::new(false));
                            row.col(|ui| {
                                cell_bg(ui);
                                if tr.key_camelot.is_some() {
                                    let r = ui.available_rect_before_wrap();
                                    let rr = egui::Rect::from_center_size(
                                        r.center(),
                                        egui::vec2(r.width().min(38.0), 15.0),
                                    );
                                    let resp = ui.allocate_rect(rr, Sense::click());
                                    ui.painter().rect_filled(
                                        rr,
                                        4.0,
                                        key_color(tr.key_camelot.as_deref().unwrap_or("")),
                                    );
                                    ui.painter().text(
                                        rr.center(),
                                        egui::Align2::CENTER_CENTER,
                                        tr.key_camelot.clone().unwrap_or_default(),
                                        egui::FontId::monospace(11.0),
                                        Color32::BLACK,
                                    );
                                    *key_clicked.borrow_mut() = resp.clicked();
                                    if resp.hovered() {
                                        *new_hover.borrow_mut() = Some(i);
                                    }
                                }
                            });
                            any_click |= *key_clicked.borrow();
                            // bpm (derecha)
                            let bpm_clicked = Rc::new(RefCell::new(false));
                            row.col(|ui| {
                                cell_bg(ui);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if let Some(b) = tr.bpm {
                                            let resp = ui.label(
                                                egui::RichText::new(format!("{b:.2}"))
                                                    .monospace()
                                                    .color(t.text),
                                            );
                                            *bpm_clicked.borrow_mut() = resp.clicked();
                                            if resp.hovered() {
                                                *new_hover.borrow_mut() = Some(i);
                                            }
                                        }
                                    },
                                );
                            });
                            any_click |= *bpm_clicked.borrow();
                            // energía (barra)
                            let en_clicked = Rc::new(RefCell::new(false));
                            row.col(|ui| {
                                cell_bg(ui);
                                if let Some(e) = tr.energy {
                                    let r = ui.available_rect_before_wrap();
                                    let bar = egui::Rect::from_center_size(
                                        r.center(),
                                        egui::vec2(r.width().min(40.0), 7.0),
                                    );
                                    let resp = ui.allocate_rect(bar, Sense::click());
                                    ui.painter().rect_filled(bar, 3.0, t.surface2);
                                    let frac = ((e - 1.0) / 9.0).clamp(0.0, 1.0);
                                    let mut fill = bar;
                                    fill.set_width(bar.width() * frac as f32);
                                    ui.painter().rect_filled(fill, 3.0, energy_color(e));
                                    *en_clicked.borrow_mut() = resp.clicked();
                                    if resp.hovered() {
                                        *new_hover.borrow_mut() = Some(i);
                                    }
                                }
                            });
                            any_click |= *en_clicked.borrow();
                            // duración (derecha)
                            row.col(|ui| {
                                cell_bg(ui);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if tr.duration_s > 0.0 {
                                            let resp = ui.label(
                                                egui::RichText::new(fmt_time(tr.duration_s))
                                                    .monospace()
                                                    .color(t.text_dim),
                                            );
                                            if resp.hovered() {
                                                *new_hover.borrow_mut() = Some(i);
                                            }
                                        }
                                    },
                                );
                                // FILA COMPLETA: ui.interact sobre el rect de toda
                                // la fila (ancho del panel) — clic y doble clic en
                                // cualquier parte de la fila.
                                if let Some(y0) = *row_y0.borrow() {
                                    let row_rect = egui::Rect::from_min_max(
                                        egui::pos2(table_full_rect.left(), y0),
                                        egui::pos2(table_full_rect.right(), y0 + ROW_H),
                                    );
                                    let rid = egui::Id::new("tbl-row").with(i);
                                    let rr = ui.interact(
                                        row_rect,
                                        rid,
                                        egui::Sense::click(),
                                    );
                                    if rr.hovered() {
                                        *new_hover.borrow_mut() = Some(i);
                                    }
                                    if rr.clicked() {
                                        any_click = true;
                                    }
                                    if rr.double_clicked() {
                                        dbl_click = true;
                                    }
                                }
                            });
                            if any_click {
                                *clicked.borrow_mut() = Some(tr.id);
                            }
                            if dbl_click {
                                *dbl.borrow_mut() = Some(tr.id);
                            }
                        });
                    });
                // aplicar interacciones tras construir la tabla
                let mut refresh = false;
                if let Some(f) = sort_req.borrow_mut().take() {
                    if self.sort.0 == f {
                        self.sort.1 = !self.sort.1;
                    } else {
                        self.sort = (f, false);
                    }
                    refresh = true;
                }
                if let Some(id) = *clicked.borrow() {
                    self.select(id);
                }
                if let Some(id) = *dbl.borrow() {
                    self.select(id);
                    self.deck_play(self.active_deck);
                }
                // devolver caché para el siguiente frame
                self.hover_row = *new_hover.borrow();
                {
                    let c = cache.borrow();
                    self.rows = c.rows.clone();
                    self.fetched_range = c.fetched;
                }
                if refresh {
                    self.refresh();
                }
            });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        let t = Theme::default();
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::none()
                    .fill(t.bg)
                    .inner_margin(egui::Margin::symmetric(10.0, 3.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let qs = self.pool.state.lock().unwrap();
                    let speed = if qs.done_session > 0 {
                        qs.analyzed_total_ms / qs.done_session
                    } else {
                        0
                    };
                    let sc = *self.scan_busy.lock().unwrap();
                    ui.label(
                        egui::RichText::new(format!(
                            "{} pistas · {} · {} · {:.1}s/pista{}",
                            self.total,
                            if qs.queue.is_empty() && qs.running == 0 {
                                "cola vacía".to_string()
                            } else {
                                format!("cola {} · {} en curso", qs.queue.len(), qs.running)
                            },
                            format!(
                                "session: {} ok · {} err",
                                qs.done_session, qs.failed_session
                            ),
                            speed as f64 / 1000.0,
                            if sc { " · escaneando…" } else { "" },
                        ))
                        .small()
                        .color(t.text_dim),
                    );
                    if !self.status_msg.is_empty() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(&self.status_msg)
                                    .small()
                                    .color(t.text_dim),
                            );
                        });
                    }
                });
            });
    }

    fn windows(&mut self, ctx: &egui::Context) {
        let t = Theme::default();
        if self.show_prefs {
            let mut open = true;
            egui::Window::new(texts::t(&self.prefs.language, "prefs.title"))
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::ComboBox::from_id_source("lang")
                        .selected_text(self.prefs.language.clone())
                        .show_ui(ui, |ui| {
                            for l in sm_i18n::I18n::available() {
                                if ui
                                    .selectable_label(self.prefs.language == l, l.to_uppercase())
                                    .clicked()
                                {
                                    self.prefs.language = l.to_string();
                                }
                            }
                        });
                    ui.add_space(4.0);
                    ui.add(
                        egui::Slider::new(&mut self.prefs.analysis_workers, 1..=16)
                            .text("hilos de análisis"),
                    );
                    ui.checkbox(&mut self.prefs.analyze_on_import, "Analizar al importar");
                    ui.checkbox(
                        &mut self.prefs.write_tags_id3,
                        "Permitir escritura de tags (requiere confirmación)",
                    );
                    ui.text_edit_singleline(&mut self.prefs.id3_comment_format);
                    if ui.button("Guardar").clicked() {
                        if let Ok(db) = self.db.lock() {
                            let _ = db.set_preferences(&self.prefs);
                        }
                        self.show_prefs = false;
                    }
                });
            self.show_prefs = open;
        }
        if self.show_new_playlist {
            let mut open = true;
            egui::Window::new(texts::t(&self.prefs.language, "playlist.new"))
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.text_edit_singleline(&mut self.new_playlist_name);
                    ui.checkbox(
                        &mut self.new_playlist_smart,
                        "Playlist inteligente (vista actual como filtro)",
                    );
                    if ui.button("Crear").clicked() && !self.new_playlist_name.is_empty() {
                        let filter = self.new_playlist_smart.then(|| sm_model::SmartFilter {
                            text: if self.search.is_empty() {
                                None
                            } else {
                                Some(self.search.clone())
                            },
                            key_compatible_with: self.compat_key.clone(),
                            ..Default::default()
                        });
                        let kind = if self.new_playlist_smart {
                            "smart"
                        } else {
                            "manual"
                        };
                        if let Ok(mut db) = self.db.lock() {
                            if let Ok(pl) =
                                db.create_playlist(&self.new_playlist_name, kind, filter.as_ref())
                            {
                                if kind == "manual" {
                                    if let Some(sid) = self.selected_id {
                                        db.add_track_to_playlist(pl.id, sid).ok();
                                    }
                                }
                            }
                        }
                        self.refresh();
                        self.show_new_playlist = false;
                    }
                });
            self.show_new_playlist = open;
        }
        if self.show_export {
            let mut open = true;
            let mut log = self.export_log.clone();
            egui::Window::new(texts::t(&self.prefs.language, "export.title"))
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(texts::t(&self.prefs.language, "export.hint"));
                    ui.add_space(4.0);
                    ui.checkbox(&mut self.prefs.export_traktor, "Traktor NML");
                    ui.checkbox(&mut self.prefs.export_serato, "Serato crates");
                    ui.checkbox(&mut self.prefs.export_rekordbox, "Rekordbox XML");
                    ui.add_space(6.0);
                    if ui.button("Exportar vista actual…").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            log = self.do_export(&dir);
                        }
                    }
                    if !log.is_empty() {
                        ui.add_space(6.0);
                        ui.monospace(&log);
                    }
                });
            self.export_log = log;
            self.show_export = open;
        }
        if self.show_about {
            let mut open = true;
            egui::Window::new("Smart Mix").show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("SMART MIX")
                        .font(crate::theme::font_display(24.0))
                        .color(t.accent),
                );
                ui.label("Análisis armónico profesional para DJs.");
                ui.label("Motor: Harmonic Engine (port Rust nativo).");
                ui.label("© 2026 Avant Garden Music. Identidad visual propia.");
                ui.label(format!(
                    "Modelos ONNX: {}",
                    if self.models.available() {
                        "activos"
                    } else {
                        "fallback"
                    }
                ));
                if let Some(p) = &self.player {
                    ui.label(format!("Audio: {} @ {} Hz", p.device_name, p.sample_rate));
                }
            });
            self.show_about = open;
        }
        // correcciones (antes panel derecho)
        if self.show_corrections {
            let mut open = true;
            egui::Window::new("Correcciones")
                .open(&mut open)
                .show(ctx, |ui| {
                    let Some(full) = self.detail.clone() else {
                        ui.label("Selecciona una pista primero.");
                        return;
                    };
                    let track_id = full.id;
                    ui.label(egui::RichText::new(&full.meta.title).font(font_heading(13.0)));
                    ui.separator();
                    // override key/bpm
                    ui.label(
                        egui::RichText::new("Tonalidad / BPM manuales")
                            .font(font_heading(9.0))
                            .color(t.text_dim),
                    );
                    let cur_key = full.analysis.key_camelot.clone();
                    let cur_bpm = full.analysis.tempo;
                    let mut apply_key: Option<Option<String>> = None;
                    let mut apply_bpm: Option<f64> = None;
                    let mut do_apply = false;
                    ui.horizontal(|ui| {
                        let sel = cur_key.clone().unwrap_or_default();
                        egui::ComboBox::from_id_source("ovr_key")
                            .selected_text(if sel.is_empty() { "—".into() } else { sel })
                            .width(58.0)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(cur_key.is_none(), "—").clicked() {
                                    apply_key = Some(None);
                                }
                                for n in 1..=12u8 {
                                    for l in ["A", "B"] {
                                        let k = format!("{n}{l}");
                                        if ui
                                            .selectable_label(
                                                cur_key.as_deref() == Some(k.as_str()),
                                                &k,
                                            )
                                            .clicked()
                                        {
                                            apply_key = Some(Some(k));
                                        }
                                    }
                                }
                            });
                        let mut b = cur_bpm.unwrap_or(128.0);
                        if ui
                            .add(egui::DragValue::new(&mut b).speed(0.1).fixed_decimals(2))
                            .changed()
                        {
                            apply_bpm = Some(b);
                        }
                        if ui.button("Aplicar").clicked() {
                            do_apply = true;
                        }
                    });
                    if do_apply && (apply_key.is_some() || apply_bpm.is_some()) {
                        if let Ok(mut db) = self.db.lock() {
                            let _ = db.override_key_bpm(
                                track_id,
                                apply_key.clone().flatten().as_deref(),
                                apply_bpm.or(cur_bpm),
                            );
                        }
                        self.load_detail(track_id);
                        self.refresh();
                    }
                    // beatgrid
                    if full.analysis.beatgrid.is_some() {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new("Beatgrid")
                                .font(font_heading(9.0))
                                .color(t.text_dim),
                        );
                        let mut act: Option<&str> = None;
                        ui.horizontal(|ui| {
                            if ui.small_button("◀ 10 ms").clicked() {
                                act = Some("left");
                            }
                            if ui.small_button("10 ms ▶").clicked() {
                                act = Some("right");
                            }
                            if ui.small_button("⚓ playhead").clicked() {
                                act = Some("anchor");
                            }
                        });
                        if let Some(a) = act {
                            let mut an = full.analysis.clone();
                            if let Some(bg) = an.beatgrid.as_mut() {
                                match a {
                                    "left" => bg.t0_s -= 0.010,
                                    "right" => bg.t0_s += 0.010,
                                    "anchor" => {
                                        let ph = self
                                            .player()
                                            .map(|p| p.decks[self.active_deck].position_s())
                                            .unwrap_or(0.0);
                                        let near = bg.beat_time(bg.beat_index_near(ph));
                                        bg.t0_s += ph - near;
                                    }
                                    _ => {}
                                }
                            }
                            if let Ok(mut db) = self.db.lock() {
                                let _ = db.save_analysis(track_id, &an);
                            }
                            self.load_detail(track_id);
                            self.refresh();
                        }
                    }
                    // acciones
                    ui.add_space(6.0);
                    ui.separator();
                    if ui.button("↻ Re-analizar").clicked() {
                        self.reanalyze_track(track_id);
                    }
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_source("addpl")
                            .selected_text("Añadir a playlist…")
                            .show_ui(ui, |ui| {
                                for pl in self.playlists.clone() {
                                    if ui.selectable_label(false, &pl.name).clicked() {
                                        if let Ok(mut db) = self.db.lock() {
                                            db.add_track_to_playlist(pl.id, track_id).ok();
                                        }
                                        self.status_msg = format!("Añadida a {}", pl.name);
                                    }
                                }
                            });
                        if let Some(k) = &full.analysis.key_camelot {
                            if ui
                                .button("Ver compatibles")
                                .on_hover_text(format!("compatibles con {k}"))
                                .clicked()
                            {
                                self.compat_key = Some(k.clone());
                                self.active_playlist = None;
                                self.collection = Collection::All;
                                self.refresh();
                            }
                        }
                    });
                });
            self.show_corrections = open;
        }
        // visor de crate USB (solo lectura)
        if let Some((name, entries)) = self.usb_crate_view.clone() {
            let mut open = true;
            egui::Window::new(format!("USB · crate «{name}»"))
                .open(&mut open)
                .default_width(480.0)
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} pistas · dispositivo SOLO LECTURA",
                            entries.len()
                        ))
                        .small()
                        .color(t.text_dim),
                    );
                    egui::ScrollArea::vertical().show_rows(ui, 18.0, entries.len(), |ui, range| {
                        for i in range {
                            ui.monospace(egui::RichText::new(&entries[i]).small());
                        }
                    });
                });
            if !open {
                self.usb_crate_view = None;
            }
        }
    }

    fn do_export(&mut self, dir: &std::path::Path) -> String {
        use sm_export::ExportEntry;
        let mut log = String::new();
        let Ok(db) = self.db.lock() else {
            return "sin db".into();
        };
        let mut q = self.query();
        q.limit = self.total as usize;
        q.offset = 0;
        let Ok(rows) = db.query(&q) else {
            return "error query".into();
        };
        struct Own {
            path: String,
            title: String,
            artist: String,
            album: String,
            genre: String,
            bpm: Option<f64>,
            key_camelot: Option<String>,
            key_name: Option<String>,
            energy: Option<f64>,
            duration_s: f64,
            cues: Vec<CuePoint>,
            beatgrid: Option<sm_model::BeatGrid>,
        }
        let mut own = Vec::new();
        for r in &rows {
            if let Some(f) = db.track_full(r.id).unwrap_or(None) {
                own.push(Own {
                    path: f.meta.path.clone(),
                    title: f.meta.title.clone(),
                    artist: f.meta.artist.clone(),
                    album: f.meta.album.clone(),
                    genre: f.meta.genre.clone(),
                    bpm: f.analysis.tempo,
                    key_camelot: f.analysis.key_camelot.clone(),
                    key_name: f.analysis.key_name.clone(),
                    energy: f.analysis.energy,
                    duration_s: f.analysis.duration_s,
                    cues: f.analysis.cues.clone(),
                    beatgrid: f.analysis.beatgrid.clone(),
                });
            }
        }
        let entries: Vec<ExportEntry> = own
            .iter()
            .map(|f| ExportEntry {
                path: &f.path,
                title: &f.title,
                artist: &f.artist,
                album: &f.album,
                genre: &f.genre,
                bpm: f.bpm,
                key_camelot: f.key_camelot.as_deref(),
                key_name: f.key_name.as_deref(),
                energy: f.energy,
                duration_s: f.duration_s,
                cues: &f.cues,
                beatgrid: f.beatgrid.as_ref(),
            })
            .collect();
        if self.prefs.export_traktor {
            match sm_export::write_traktor_nml(&entries, &dir.join("smart-mix.nml")) {
                Ok(_) => log.push_str("✓ Traktor NML\n"),
                Err(e) => log.push_str(&format!("✗ Traktor: {e}\n")),
            }
        }
        if self.prefs.export_rekordbox {
            match sm_export::write_rekordbox_xml(&entries, &dir.join("rekordbox.xml")) {
                Ok(_) => log.push_str("✓ Rekordbox XML\n"),
                Err(e) => log.push_str(&format!("✗ Rekordbox: {e}\n")),
            }
        }
        if self.prefs.export_serato {
            let cl = vec![(
                "Smart Mix".to_string(),
                entries
                    .iter()
                    .map(|e| e.path.to_string())
                    .collect::<Vec<_>>(),
            )];
            match sm_export::write_serato_crates(&cl, dir) {
                Ok(_) => log.push_str("✓ Serato crate\n"),
                Err(e) => log.push_str(&format!("✗ Serato: {e}\n")),
            }
        }
        match sm_export::write_m3u(
            &entries
                .iter()
                .map(|e| e.path.to_string())
                .collect::<Vec<_>>(),
            &dir.join("smart-mix.m3u"),
        ) {
            Ok(_) => log.push_str("✓ M3U\n"),
            Err(e) => log.push_str(&format!("✗ M3U: {e}\n")),
        }
        match sm_export::write_csv(&entries, &dir.join("smart-mix.csv")) {
            Ok(_) => log.push_str("✓ CSV\n"),
            Err(e) => log.push_str(&format!("✗ CSV: {e}\n")),
        }
        log.push_str(&format!("→ {} pistas en {}", entries.len(), dir.display()));
        log
    }
}

impl Drop for SmartMixApp {
    fn drop(&mut self) {
        if let Ok(db) = self.db.lock() {
            let _ = db.set_preferences(&self.prefs);
        }
    }
}

fn fmt_time(s: f64) -> String {
    let m = (s / 60.0).floor() as i64;
    let sec = (s - m as f64 * 60.0) as i64;
    format!("{m}:{sec:02}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn sync_bpm_semitonos() {
        // misma fórmula del botón SYNC: 12·log2(bpm_objetivo/bpm_actual)
        let semis = |bo: f64, bm: f64| (12.0 * (bo / bm).log2()).clamp(-12.0, 12.0);
        assert!(semis(120.0, 120.0).abs() < 1e-9);
        // una octava = +12 st (limitado)
        assert_eq!(semis(240.0, 120.0), 12.0);
        // +5.946% ≈ +1 semitono
        assert!((semis(127.14, 120.0) - 1.0).abs() < 0.01);
        // doble bajada
        assert!((semis(60.0, 120.0) + 12.0).abs() < 1e-9);
    }
}
