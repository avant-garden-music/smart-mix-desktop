//! Smart Mix — aplicación principal (egui).
//!
//! Estructura de pantalla:
//!   top: menú + búsqueda + filtro Camelot
//!   left: colecciones y playlists
//!   right: detalle de pista (análisis, waveform, cues)
//!   center: tabla virtualizada (30k+ filas fluidas)
//!   bottom: decks A/B + barra de estado

use crate::texts;
use crate::theme::{energy_color, key_color, Theme};
use crate::widgets::WaveView;
use crate::worker::{AnalysisEvent, AnalysisPool};
use egui::{Color32, Sense};
use sm_audio::{deck_pair, Player};
use sm_core::models::Models;
use sm_db::{LibraryDb, LibraryQuery, SortField};
use sm_library::{read_metadata, scan_root};
use sm_model::{AnalysisStatus, CuePoint, Preferences, TrackRow};
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

const ROW_H: f32 = 22.0;

pub struct SmartMixApp {
    db: Arc<Mutex<LibraryDb>>,
    player: Option<Player>,
    models: Arc<Models>,
    pool: AnalysisPool,
    pub prefs: Preferences,
    // vista
    search: String,
    sort: (SortField, bool),
    total: i64,
    rows: BTreeMap<usize, TrackRow>,
    fetched_range: (usize, usize),
    selected_id: Option<i64>,
    detail: Option<sm_db::TrackFull>,
    detail_version: u64,
    cover_tex: Option<egui::TextureHandle>,
    compat_key: Option<String>,
    collection: Collection,
    playlists: Vec<sm_model::Playlist>,
    active_playlist: Option<i64>,
    show_prefs: bool,
    show_export: bool,
    show_new_playlist: bool,
    show_about: bool,
    auto_queue_after_scan: bool,
    new_playlist_name: String,
    new_playlist_smart: bool,
    export_log: String,
    status_msg: String,
    scan_busy: Arc<Mutex<bool>>,
    scan_found: Arc<std::sync::atomic::AtomicU64>,
    ctx_repaint: Arc<dyn Fn() + Send + Sync>,
    deck_b_visible: bool,
    active_deck: u8,
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
        let models_dir = sm_core::pipeline::default_models_dir();
        let models = Arc::new(Models::load(&models_dir));
        let player = deck_pair().ok();
        if player.is_none() {
            tracing::error!("no se pudo abrir el dispositivo de audio");
        }
        let mut prefs: Preferences = db.get_preferences().unwrap_or_default();
        let db = Arc::new(Mutex::new(db));
        let ctx = cc.egui_ctx.clone();
        let repaint_ctx = ctx.clone();
        let repaint: Arc<dyn Fn() + Send + Sync> =
            Arc::new(move || repaint_ctx.request_repaint());
        let pool = AnalysisPool::new(
            db.clone(),
            models.clone(),
            prefs.analysis_workers.max(1).min(16),
            repaint.clone(),
        );
        let mut app = SmartMixApp {
            db,
            player,
            models,
            pool,
            prefs,
            search: String::new(),
            sort: (SortField::AddedAt, true),
            total: 0,
            rows: BTreeMap::new(),
            fetched_range: (0, 0),
            selected_id: None,
            detail: None,
            detail_version: 0,
            cover_tex: None,
            compat_key: None,
            collection: Collection::All,
            playlists: Vec::new(),
            active_playlist: None,
            show_prefs: false,
            show_export: false,
            show_new_playlist: false,
            show_about: false,
            auto_queue_after_scan: false,
            new_playlist_name: String::new(),
            new_playlist_smart: false,
            export_log: String::new(),
            status_msg: String::new(),
            scan_busy: Arc::new(Mutex::new(false)),
            scan_found: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            ctx_repaint: repaint,
            deck_b_visible: true,
            active_deck: 0,
        };
        app.refresh();
        Ok(app)
    }

    fn query(&self) -> LibraryQuery {
        let mut smart = sm_model::SmartFilter {
            text: None,
            key_compatible_with: self.compat_key.clone(),
            ..Default::default()
        };
        if self.collection == Collection::Unanalyzed {
            smart.energy_min = None;
        }
        let mut q = LibraryQuery {
            text: if self.search.is_empty() { None } else { Some(self.search.clone()) },
            sort: self.sort.0,
            desc: self.sort.1,
            limit: 400,
            offset: 0,
            playlist_id: self.active_playlist,
            smart: Some(smart),
            status_filter: if self.collection == Collection::Unanalyzed {
                Some(AnalysisStatus::Pending)
            } else {
                None
            },
        };
        if self.collection == Collection::Duplicates {
            q.text = q.text.take(); // duplicados se gestionan aparte
        }
        q
    }

    fn refresh(&mut self) {
        self.rows.clear();
        self.fetched_range = (0, 0);
        if let Ok(db) = self.db.lock() {
            let mut q = self.query();
            if self.collection == Collection::Duplicates {
                // lista plana de duplicados
                match db.duplicates() {
                    Ok(groups) => {
                        let mut rows = Vec::new();
                        for g in &groups {
                            for (id, path, dur) in &g.tracks {
                                rows.push(TrackRow {
                                    id: *id,
                                    title: std::path::Path::new(path)
                                        .file_stem()
                                        .map(|s| s.to_string_lossy().to_string())
                                        .unwrap_or_default(),
                                    artist: g.reason.clone(),
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
                    Err(e) => tracing::error!("duplicates: {e}"),
                }
            }
            self.total = db.count(&q).unwrap_or(0);
            self.playlists = db.playlists().unwrap_or_default();
        }
    }

    fn ensure_rows(&mut self, range: (usize, usize)) {
        if range.0 >= range.1 {
            return;
        }
        let (lo, hi) = self.fetched_range;
        if range.0 >= lo && range.1 <= hi {
            return;
        }
        let fetch_lo = range.0.saturating_sub(100);
        let fetch_hi = (range.1 + 400) ;
        if let Ok(db) = self.db.lock() {
            let mut q = self.query();
            q.offset = fetch_lo as u64;
            q.limit = fetch_hi - fetch_lo;
            match db.query(&q) {
                Ok(rows) => {
                    self.rows.clear();
                    for (i, r) in rows.into_iter().enumerate() {
                        self.rows.insert(fetch_lo + i, r);
                    }
                    self.fetched_range = (fetch_lo, fetch_hi);
                }
                Err(e) => tracing::error!("query: {e}"),
            }
        }
    }

    fn select(&mut self, id: i64, ctx: &egui::Context) {
        self.selected_id = Some(id);
        if let Ok(db) = self.db.lock() {
            self.detail = db.track_full(id).unwrap_or(None);
            self.detail_version += 1;
            if let Some(full) = &self.detail {
                if let Some(cover) = db.cover(id).unwrap_or(None) {
                    if let Some(img) = image::load_from_memory(&cover).ok() {
                        let rgba = img.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        let tex = ctx.load_texture(
                            "cover",
                            egui::ColorImage::from_rgba_unmultiplied(
                                [w as usize, h as usize],
                                &rgba,
                            ),
                            egui::TextureOptions::LINEAR,
                        );
                        self.cover_tex = Some(tex);
                        return;
                    }
                }
            }
        }
        self.cover_tex = None;
    }

    fn import_folder(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            self.status_msg = format!("Escaneando {}…", folder.display());
            let db = self.db.clone();
            let busy = self.scan_busy.clone();
            let found = self.scan_found.clone();
            let repaint = self.ctx_repaint.clone();
            {
                let mut b = busy.lock().unwrap();
                if *b {
                    return; // ya hay un escaneo en curso
                }
                *b = true;
            }
            if self.prefs.analyze_on_import {
                self.auto_queue_after_scan = true;
            }
            std::thread::spawn(move || {
                let files = scan_root(&folder);
                found.store(files.len() as u64, Ordering::Relaxed);
                let mut new_ids = Vec::new();
                for (i, f) in files.iter().enumerate() {
                    if i % 25 == 0 {
                        repaint();
                    }
                    match read_metadata(f) {
                        Ok(scanned) => {
                            if let Ok(mut db) = db.lock() {
                                match db.upsert_track(&scanned.meta) {
                                    Ok(id) => {
                                        if let Some(cover) = scanned.cover {
                                            let _ = db.set_cover(id, Some(cover));
                                        }
                                        new_ids.push((id, scanned.meta.path.clone()));
                                    }
                                    Err(e) => tracing::warn!("upsert {}: {e}", f.display()),
                                }
                            }
                        }
                        Err(e) => tracing::warn!("meta {}: {e}", f.display()),
                    }
                }
                let _ = new_ids;
                *busy.lock().unwrap() = false;
                repaint();
            });
        }
    }

    fn analyze_pending(&mut self) {
        if let Ok(db) = self.db.lock() {
            let pending = db
                .track_ids_by_status(AnalysisStatus::Pending, 1_000_000)
                .unwrap_or_default();
            self.pool.enqueue(pending);
            let n = self.pool.queue_len();
            self.status_msg = format!("Analizando {n} pistas…");
        }
    }

    fn reanalyze_selected(&mut self) {
        if let Some(id) = self.selected_id {
            if let Some(full) = &self.detail {
                let path = full.meta.path.clone();
                if let Ok(mut db) = self.db.lock() {
                    let _ = db.set_analysis_status(id, AnalysisStatus::Pending);
                }
                self.pool.enqueue(vec![(id, path)]);
            }
        }
    }

    fn play_selected(&mut self, deck: usize) {
        if let (Some(id), Some(player)) = (self.selected_id, &self.player) {
            if let Some(full) = &self.detail {
                let p = full.meta.path.clone();
                let d = &player.decks[deck];
                d.load(&p);
                d.play();
            }
            let _ = id;
        }
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.pool.events.try_recv() {
            match ev {
                AnalysisEvent::Finished(id, ms) => {
                    self.status_msg = format!("Pista analizada en {ms} ms");
                    if Some(id) == self.selected_id {
                        // recargar detalle
                        // (select necesita ctx — se hará en update)
                    }
                }
                AnalysisEvent::Failed(id, e) => {
                    self.status_msg = format!("Error analizando #{id}: {e}");
                }
                _ => {}
            }
        }
    }
}

// (helpers de import movidos inline)

impl eframe::App for SmartMixApp {
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        if let Ok(db) = self.db.lock() {
            if let Ok(js) = serde_json::to_string(&self.prefs) {
                let _ = db.set_setting("preferences", &js);
            }
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_events();
        if self.pool.is_busy() || *self.scan_busy.lock().unwrap() {
            ctx.request_repaint();
        }
        // tras un escaneo con análisis automático
        if self.auto_queue_after_scan && !*self.scan_busy.lock().unwrap() {
            self.auto_queue_after_scan = false;
            self.refresh();
            self.analyze_pending();
        }
        self.top_bar(ctx);
        self.side_panel(ctx);
        self.player_bar(ctx);
        self.status_bar(ctx);
        self.central_table(ctx);
        if self.selected_id.is_some() {
            self.detail_panel(ctx);
        }
        self.windows(ctx);
    }
}

impl SmartMixApp {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                crate::theme::wordmark(ui);
                ui.separator();
                // menú
                egui::menu::bar(ui, |ui| {
                    ui.menu_button(texts::t(&self.prefs.language, "menu.file"), |ui| {
                        if ui.button(texts::t(&self.prefs.language, "menu.import_folder")).clicked()
                        {
                            self.import_folder();
                            ui.close_menu();
                        }
                        if ui.button(texts::t(&self.prefs.language, "menu.analyze_pending")).clicked()
                        {
                            self.analyze_pending();
                            ui.close_menu();
                        }
                        if ui.button(texts::t(&self.prefs.language, "menu.reanalyze")).clicked() {
                            self.reanalyze_selected();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(texts::t(&self.prefs.language, "menu.export")).clicked() {
                            self.show_export = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(texts::t(&self.prefs.language, "menu.prefs")).clicked() {
                            self.show_prefs = true;
                            ui.close_menu();
                        }
                    });
                    ui.menu_button(texts::t(&self.prefs.language, "menu.view"), |ui| {
                        ui.checkbox(&mut self.deck_b_visible, "Deck B");
                        if ui.button(texts::t(&self.prefs.language, "menu.about")).clicked() {
                            self.show_about = true;
                            ui.close_menu();
                        }
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // filtro camelot
                    let combo = egui::ComboBox::from_id_source("compat")
                        .selected_text(
                            self.compat_key
                                .clone()
                                .unwrap_or_else(|| "—".into()),
                        )
                        .width(60.0);
                    combo.show_ui(ui, |ui| {
                        if ui
                            .selectable_label(self.compat_key.is_none(), "—")
                            .clicked()
                        {
                            self.compat_key = None;
                            self.refresh();
                        }
                        for n in 1..=12 {
                            for l in ["A", "B"] {
                                let k = format!("{n}{l}");
                                if ui.selectable_label(
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
                        .desired_width(220.0);
                    if ui.add(search).changed() {
                        self.refresh();
                    }
                });
            });
        });
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("side")
            .resizable(false)
            .default_width(198.0)
            .show(ctx, |ui| {
                let t = Theme::default();
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(texts::t(&self.prefs.language, "side.collections"))
                        .color(t.text_dim)
                        .small(),
                );
                let items = [
                    (Collection::All, texts::t(&self.prefs.language, "col.all")),
                    (Collection::Unanalyzed, texts::t(&self.prefs.language, "col.unanalyzed")),
                    (Collection::Duplicates, texts::t(&self.prefs.language, "col.duplicates")),
                ];
                for (col, label) in items {
                    let sel = self.collection == col && self.active_playlist.is_none();
                    if ui
                        .selectable_label(sel, format!("  {label}"))
                        .clicked()
                    {
                        self.collection = col;
                        self.active_playlist = None;
                        self.refresh();
                    }
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(texts::t(&self.prefs.language, "side.playlists"))
                            .color(t.text_dim)
                            .small(),
                    );
                    if ui.small_button("+").clicked() {
                        self.new_playlist_name.clear();
                        self.new_playlist_smart = false;
                        self.show_new_playlist = true;
                    }
                });
                for pl in self.playlists.clone() {
                    let sel = self.active_playlist == Some(pl.id);
                    let label = if pl.kind == "smart" {
                        format!("  ✦ {}", pl.name)
                    } else {
                        format!("  {}", pl.name)
                    };
                    if ui.selectable_label(sel, label).clicked() {
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
            });
    }

    fn central_table(&mut self, ctx: &egui::Context) {
        // recargar detalle si cambió (análisis terminado de la selección)
        if let Some(id) = self.selected_id {
            if let Ok(db) = self.db.lock() {
                if let Some(full) = db.track_full(id).unwrap_or(None) {
                    if full.analysis.analysis_ms != self.detail.as_ref().map(|d| d.analysis.analysis_ms).flatten() {
                        let v = self.detail_version;
                        self.detail = Some(full);
                        self.detail_version = v + 1;
                    }
                }
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            let t = Theme::default();
            // cabecera
            let hdr = [
                ("#", 26.0, None),
                ("title", 340.0, Some(SortField::Title)),
                ("artist", 150.0, Some(SortField::Artist)),
                ("album", 130.0, Some(SortField::Album)),
                ("key", 44.0, Some(SortField::Key)),
                ("bpm", 56.0, Some(SortField::Bpm)),
                ("energy", 58.0, Some(SortField::Energy)),
                ("time", 50.0, Some(SortField::Duration)),
            ];
            let mut x = ui.available_width();
            ui.horizontal(|ui| {
                ui.style_mut().wrap = Some(false);
                for (name, w, sort) in hdr {
                    let label = texts::t(&self.prefs.language, &format!("col.{name}"));
                    let btn = egui::Button::new(
                        egui::RichText::new(label).color(t.text_dim).small(),
                    )
                    .small();
                    let resp = ui.add_sized([w, 16.0], btn);
                    if resp.clicked() {
                        if let Some(f) = sort {
                            if self.sort.0 == f {
                                self.sort.1 = !self.sort.1;
                            } else {
                                self.sort = (f, false);
                            }
                            self.refresh();
                        }
                    }
                }
            });
            ui.add_space(2.0);
            let total = self.total.max(0) as usize;
            let mut need_refresh = false;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, ROW_H, total, |ui, range| {
                    self.ensure_rows((range.start, range.end));
                    let _ = &mut x;
                    let painter = ui.painter().clone();
                    for i in range {
                        let Some(row) = self.rows.get(&i) else {
                            ui.add_space(ROW_H);
                            continue;
                        };
                        let selected = self.selected_id == Some(row.id);
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H - 2.0), Sense::click());
                        if i % 2 == 1 {
                            painter.rect_filled(rect, 0.0, t.surface);
                        }
                        if selected {
                            painter.rect_filled(
                                rect,
                                0.0,
                                Color32::from_rgba_premultiplied(180, 244, 97, 26),
                            );
                        }
                        let cy = rect.center().y;
                        let mut cx = rect.left();
                        let put = |x: f32, txt: String, color: Color32, mono: bool| {
                            painter.text(
                                egui::pos2(x, cy),
                                egui::Align2::LEFT_CENTER,
                                txt,
                                if mono {
                                    egui::FontId::monospace(12.0)
                                } else {
                                    egui::FontId::proportional(12.0)
                                },
                                color,
                            );
                        };
                        put(cx + 6.0, format!("{}", i + 1), t.text_dim, true);
                        cx += 26.0;
                        // title
                        let title = if row.title.is_empty() {
                            std::path::Path::new(&row.id.to_string()).file_stem().map(|_| "").unwrap_or("")
                        } else {
                            row.title.as_str()
                        };
                        painter.text(
                            egui::pos2(cx + 6.0, cy),
                            egui::Align2::LEFT_CENTER,
                            truncate(title, 46),
                            egui::FontId::proportional(12.5),
                            if selected { t.text } else { t.text },
                        );
                        cx += 340.0;
                        painter.text(
                            egui::pos2(cx + 6.0, cy),
                            egui::Align2::LEFT_CENTER,
                            truncate(&row.artist, 20),
                            egui::FontId::proportional(12.0),
                            t.text_dim,
                        );
                        cx += 150.0;
                        painter.text(
                            egui::pos2(cx + 6.0, cy),
                            egui::Align2::LEFT_CENTER,
                            truncate(&row.album, 18),
                            egui::FontId::proportional(12.0),
                            t.text_dim,
                        );
                        cx += 130.0;
                        if let Some(k) = &row.key_camelot {
                            let (bg, fg) = (key_color(k), Color32::BLACK);
                            let r = egui::Rect::from_center_size(
                                egui::pos2(cx + 18.0, cy),
                                egui::vec2(34.0, 15.0),
                            );
                            painter.rect_filled(r, 3.0, bg);
                            painter.text(
                                r.center(),
                                egui::Align2::CENTER_CENTER,
                                k.clone(),
                                egui::FontId::monospace(11.0),
                                fg,
                            );
                        }
                        cx += 44.0;
                        if let Some(b) = row.bpm {
                            put(cx + 6.0, format!("{b:.2}"), t.text, true);
                        }
                        cx += 56.0;
                        if let Some(e) = row.energy {
                            let r = egui::Rect::from_center_size(
                                egui::pos2(cx + 22.0, cy),
                                egui::vec2(38.0, 8.0),
                            );
                            painter.rect_filled(r, 4.0, t.surface2);
                            let frac = ((e - 1.0) / 9.0).clamp(0.0, 1.0);
                            let mut fill = r;
                            fill.set_width(38.0 * frac as f32);
                            painter.rect_filled(fill, 4.0, energy_color(e));
                            put(cx + 46.0, format!("{}", e.round() as i64), t.text_dim, true);
                        }
                        cx += 58.0;
                        if row.duration_s > 0.0 {
                            put(cx + 6.0, fmt_time(row.duration_s), t.text_dim, true);
                        }
                        if resp.clicked() {
                            self.selected_id = Some(row.id);
                            need_refresh = true;
                        }
                        if resp.double_clicked() {
                            self.selected_id = Some(row.id);
                            let id = row.id;
                            let path = self
                                .rows
                                .get(&i)
                                .map(|r| {
                                    // path completo desde db
                                    r.id
                                })
                                .unwrap_or(id);
                            let _ = path;
                            // cargar en deck activo
                            if let Some(player) = &self.player {
                                if let Ok(db) = self.db.lock() {
                                    if let Some(full) = db.track_full(id).unwrap_or(None) {
                                        let p = full.meta.path.clone();
                                        let deck = &player.decks[self.active_deck as usize];
                                        deck.load(&p);
                                        deck.play();
                                    }
                                }
                            }
                        }
                    }
                });
            if need_refresh {
                let id = self.selected_id.unwrap();
                let ctx2 = ui.ctx().clone();
                self.select(id, &ctx2);
            }
        });
    }

    fn detail_panel(&mut self, ctx: &egui::Context) {
        let mut close = false;
        egui::SidePanel::right("detail")
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                let t = Theme::default();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // clon local: el panel necesita &mut self (select/status)
                    let Some(full) = self.detail.clone() else { return };
                    let full = &full;
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&full.meta.title)
                                .font(egui::FontId::new(
                                    17.0,
                                    egui::FontFamily::Name("Heading".into()),
                                ))
                                .color(t.text),
                        );
                        if ui.small_button("×").clicked() {
                            close = true;
                        }
                    });
                    ui.label(
                        egui::RichText::new(format!("{} — {}", full.meta.artist, full.meta.album))
                            .color(t.text_dim),
                    );
                    ui.add_space(6.0);
                    if let Some(tex) = &self.cover_tex {
                        ui.add(egui::Image::new(egui::ImageSource::Texture(egui::load::SizedTexture::new(tex.id(), egui::vec2(160.0, 160.0)))));
                    }
                    ui.add_space(6.0);
                    // métricas
                    ui.horizontal(|ui| {
                        if let Some(k) = &full.analysis.key_camelot {
                            let r = ui.allocate_exact_size(egui::vec2(52.0, 22.0), Sense::hover()).1.rect;
                            ui.painter().rect_filled(r, 4.0, key_color(k));
                            ui.painter().text(
                                r.center(),
                                egui::Align2::CENTER_CENTER,
                                k,
                                egui::FontId::monospace(14.0),
                                Color32::BLACK,
                            );
                            ui.label(
                                egui::RichText::new(
                                    full.analysis.key_name.clone().unwrap_or_default(),
                                )
                                .color(t.text_dim),
                            );
                        }
                        if let Some(b) = full.analysis.tempo {
                            ui.label(egui::RichText::new(format!("{b:.2} BPM")).monospace());
                        }
                        if let Some(e) = full.analysis.energy {
                            ui.label(
                                egui::RichText::new(format!("Energy {}", e.round()))
                                    .color(energy_color(e)),
                            );
                        }
                    });
                    ui.horizontal(|ui| {
                        if let Some(v) = full.analysis.volume_db {
                            ui.label(
                                egui::RichText::new(format!("vol {v:+.1} dB"))
                                    .color(t.text_dim)
                                    .monospace(),
                            );
                        }
                        if let Some(l) = full.analysis.lufs {
                            ui.label(
                                egui::RichText::new(format!("{l:+.1} LUFS"))
                                    .color(t.text_dim)
                                    .monospace(),
                            );
                        }
                        if let Some(c) = full.analysis.clipped_peak_count {
                            if c > 0 {
                                ui.label(
                                    egui::RichText::new(format!("⚠ {c} clips"))
                                        .color(t.danger),
                                );
                            }
                        }
                        if let Some(conf) = full.analysis.key_confidence {
                            ui.label(
                                egui::RichText::new(format!("{:.0}% key", conf * 100.0))
                                    .color(t.text_dim),
                            );
                        }
                    });
                    ui.separator();
                    // transport
                    ui.horizontal(|ui| {
                        if ui.button("▶ A").clicked() {
                            self.play_selected(0);
                        }
                        if ui.button("▶ B").clicked() {
                            self.play_selected(1);
                        }
                        if ui.button("↻").clicked() {
                            self.reanalyze_selected();
                        }
                    });
                    ui.separator();
                    // waveform
                    let wave = full.analysis.waveform.clone();
                    let dur = full.analysis.duration_s.max(0.001);
                    let cues: Vec<f64> = full.analysis.cues.iter().map(|c| c.time_s).collect();
                    let segs = full.analysis.segments.clone();
                    let deck = self.player.as_ref().map(|p| &p.decks[self.active_deck as usize]);
                    let pos = deck.filter(|d| d.is_loaded()).map(|d| d.position_s());
                    let track_loaded = deck
                        .map(|d| d.path())
                        .map(|p| p == full.meta.path)
                        .unwrap_or(false);
                    let pos = if track_loaded { pos } else { None };
                    let version = self.detail_version;
                    let _ = version;
                    let mut seek_tx: Option<f64> = None;
                    ui.add(WaveView {
                        wave: &wave,
                        duration_s: dur,
                        position_s: pos,
                        cues: &cues,
                        segments: &segs,
                        height: 84.0,
                        on_seek: &mut |t| seek_tx = Some(t),
                    });
                    if let (Some(t), Some(player)) = (seek_tx, &self.player) {
                        player.decks[self.active_deck as usize].seek(t);
                    }
                    // cues
                    ui.label(
                        egui::RichText::new(texts::t(&self.prefs.language, "detail.cues"))
                            .color(t.text_dim)
                            .small(),
                    );
                    let mut del: Option<usize> = None;
                    let mut jump: Option<f64> = None;
                    for (i, cue) in full.analysis.cues.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.monospace(format!("{}  {:>7.2}", i + 1, cue.time_s));
                            ui.label(
                                egui::RichText::new(
                                    cue.energy_level.map(|l| format!("E{l}")).unwrap_or_default(),
                                )
                                .color(t.text_dim),
                            );
                            if ui.small_button("▶").clicked() {
                                jump = Some(cue.time_s);
                            }
                            if ui.small_button("−").clicked() {
                                del = Some(i);
                            }
                        });
                    }
                    if let Some(t) = jump {
                        if let Some(player) = &self.player {
                            let d = &player.decks[self.active_deck as usize];
                            if !d.is_loaded() || d.path() != full.meta.path {
                                d.load(&full.meta.path);
                            }
                            d.seek(t);
                            d.play();
                        }
                    }
                    if let Some(i) = del {
                        let mut cues = full.analysis.cues.clone();
                        cues.remove(i);
                        if let Ok(mut db) = self.db.lock() {
                            db.replace_cues(full.id, &cues).ok();
                        }
                        let id = full.id;
                        let ctx2 = ui.ctx().clone();
                        self.select(id, &ctx2);
                    }
                    if ui.button(texts::t(&self.prefs.language, "detail.add_cue")).clicked() {
                        let mut cues = full.analysis.cues.clone();
                        let t = self
                            .player
                            .as_ref()
                            .map(|p| p.decks[self.active_deck as usize].position_s())
                            .unwrap_or(0.0);
                        // snap al beat
                        let mut snapped = t;
                        if let Some(bg) = &full.analysis.beatgrid {
                            snapped = bg.beat_time(bg.beat_index_near(t));
                        }
                        cues.push(CuePoint {
                            time_s: snapped,
                            energy_level: None,
                            origin: "user".into(),
                            label: None,
                        });
                        cues.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
                        if let Ok(mut db) = self.db.lock() {
                            db.replace_cues(full.id, &cues).ok();
                        }
                        let id = full.id;
                        let ctx2 = ui.ctx().clone();
                        self.select(id, &ctx2);
                    }
                    ui.separator();
                    // playlist add
                    ui.horizontal(|ui| {
                        let label = texts::t(&self.prefs.language, "detail.add_to_playlist");
                        egui::ComboBox::from_id_source("addpl")
                            .selected_text(label)
                            .show_ui(ui, |ui| {
                                for pl in self.playlists.clone() {
                                    if ui.selectable_label(false, &pl.name).clicked() {
                                        if let Ok(mut db) = self.db.lock() {
                                            db.add_track_to_playlist(pl.id, full.id).ok();
                                        }
                                        self.status_msg =
                                            format!("Añadida a {}", pl.name);
                                    }
                                }
                            });
                        // compatibles
                        if let Some(k) = &full.analysis.key_camelot {
                            if ui
                                .button(texts::t(&self.prefs.language, "detail.show_compatible"))
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
            });
        if close {
            self.selected_id = None;
        }
    }

    fn player_bar(&mut self, ctx: &egui::Context) {
        let t = Theme::default();
        egui::TopBottomPanel::bottom("player")
            .frame(egui::Frame::none().fill(t.surface).inner_margin(egui::Margin::symmetric(10.0, 6.0)))
            .show(ctx, |ui| {
                let decks: &[bool] = &[true, self.deck_b_visible];
                ui.horizontal(|ui| {
                    for (i, visible) in decks.iter().enumerate() {
                        if !visible {
                            continue;
                        }
                        self.deck_ui(ui, i);
                        ui.separator();
                    }
                });
            });
    }

    fn deck_ui(&mut self, ui: &mut egui::Ui, idx: usize) {
        let t = Theme::default();
        let (name, color) = if idx == 0 {
            ("A", t.deck_a)
        } else {
            ("B", t.deck_b)
        };
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(name)
                        .color(color)
                        .monospace()
                        .strong(),
                );
                if self.active_deck != idx as u8 {
                    if ui.small_button("⇆").clicked() {
                        self.active_deck = idx as u8;
                    }
                }
                let title = self
                    .player
                    .as_ref()
                    .map(|p| p.decks[idx].title())
                    .unwrap_or_default();
                ui.label(truncate(&title, 34)).on_hover_text(title.clone());
                let pos = self
                    .player
                    .as_ref()
                    .map(|p| p.decks[idx].position_s())
                    .unwrap_or(0.0);
                let dur = self
                    .player
                    .as_ref()
                    .map(|p| p.decks[idx].duration_s())
                    .unwrap_or(0.0);
                let playing = self
                    .player
                    .as_ref()
                    .map(|p| p.decks[idx].is_playing())
                    .unwrap_or(false);
                if ui
                    .button(if playing { "⏸" } else { "▶" })
                    .clicked()
                {
                    if let Some(p) = &self.player {
                        if playing {
                            p.decks[idx].pause();
                        } else {
                            p.decks[idx].play();
                        }
                    }
                }
                if ui.button("■").clicked() {
                    if let Some(p) = &self.player {
                        p.decks[idx].stop();
                    }
                }
                let mut frac = if dur > 0.0 { (pos / dur) as f32 } else { 0.0 };
                let slider = egui::Slider::new(&mut frac, 0.0..=1.0).show_value(false);
                let resp = ui.add(slider);
                if resp.changed() && dur > 0.0 {
                    if let Some(p) = &self.player {
                        p.decks[idx].seek(frac as f64 * dur);
                    }
                }
                ui.monospace(format!("{} / {}", fmt_time(pos), fmt_time(dur)));
                let mut vol = self
                    .player
                    .as_ref()
                    .map(|p| p.decks[idx].volume())
                    .unwrap_or(0.85);
                if ui
                    .add(egui::Slider::new(&mut vol, 0.0..=1.5).text("vol").show_value(false))
                    .changed()
                {
                    if let Some(p) = &self.player {
                        *p.decks[idx].current().volume.write() = vol;
                    }
                }
                let mut pitch = self
                    .player
                    .as_ref()
                    .map(|p| p.decks[idx].pitch())
                    .unwrap_or(0.0);
                if ui
                    .add(
                        egui::Slider::new(&mut pitch, -12.0..=12.0)
                            .text("pitch")
                            .show_value(false),
                    )
                    .changed()
                {
                    if let Some(p) = &self.player {
                        *p.decks[idx].current().pitch_semis.write() = pitch;
                    }
                }
            });
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        let t = Theme::default();
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::none().fill(t.bg).inner_margin(egui::Margin::symmetric(10.0, 3.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let qs = self.pool.state.lock().unwrap();
                    let speed = if qs.done_session > 0 {
                        qs.analyzed_total_ms / qs.done_session
                    } else {
                        0
                    };
                    ui.label(
                        egui::RichText::new(format!(
                            "{} pistas · {} analizadas · cola {} · {:.1}s/pista",
                            self.total,
                            qs.done_session,
                            qs.queue.len(),
                            speed as f64 / 1000.0
                        ))
                        .small()
                        .color(t.text_dim),
                    );
                    let _ = self.total;
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
                                if ui.selectable_label(
                                    self.prefs.language == l,
                                    l.to_uppercase(),
                                )
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
                            if let Ok(js) = serde_json::to_string(&self.prefs) {
                                let _ = db.set_setting("preferences", &js);
                            }
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
                    ui.checkbox(&mut self.new_playlist_smart, "Playlist inteligente (vista actual como filtro)");
                    if ui.button("Crear").clicked() {
                        if !self.new_playlist_name.is_empty() {
                            let filter = if self.new_playlist_smart {
                                Some(sm_model::SmartFilter {
                                    text: if self.search.is_empty() { None } else { Some(self.search.clone()) },
                                    key_compatible_with: self.compat_key.clone(),
                                    ..Default::default()
                                })
                            } else {
                                None
                            };
                            let kind = if self.new_playlist_smart { "smart" } else { "manual" };
                            if let Ok(mut db) = self.db.lock() {
                                if let Ok(pl) =
                                    db.create_playlist(&self.new_playlist_name, kind, filter.as_ref())
                                {
                                    // playlist manual: añadir la selección actual
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
                        .font(egui::FontId::new(
                            24.0,
                            egui::FontFamily::Name("Heading".into()),
                        ))
                        .color(t.accent),
                );
                ui.label("Análisis armónico profesional para DJs.");
                ui.label("Motor: Harmonic Engine (port Rust nativo).");
                ui.label("© 2026 Avant Garden Music. Identidad visual propia.");
                ui.label(format!(
                    "Modelos ONNX: {}",
                    if self.models.available() { "activos" } else { "fallback Krumhansl" }
                ));
                if let Some(p) = &self.player {
                    ui.label(format!("Audio: {}", p.device_name));
                }
            });
            self.show_about = open;
        }
    }

    fn do_export(&mut self, dir: &std::path::Path) -> String {
        use sm_export::ExportEntry;
        let mut log = String::new();
        let Ok(db) = self.db.lock() else { return "sin db".into() };
        let mut q = self.query();
        q.limit = self.total as usize;
        q.offset = 0;
        let Ok(rows) = db.query(&q) else { return "error query".into() };
        // datos owned (los ExportEntry toman referencias)
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
            cues: Vec<sm_model::CuePoint>,
            beatgrid: Option<sm_model::BeatGrid>,
        }
        let mut own: Vec<Own> = Vec::new();
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
            let crate_list = vec![(
                "Smart Mix".to_string(),
                entries.iter().map(|e| e.path.to_string()).collect::<Vec<_>>(),
            )];
            match sm_export::write_serato_crates(&crate_list, dir) {
                Ok(_) => log.push_str("✓ Serato crate\n"),
                Err(e) => log.push_str(&format!("✗ Serato: {e}\n")),
            }
        }
        match sm_export::write_m3u(
            &entries.iter().map(|e| e.path.to_string()).collect::<Vec<_>>(),
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
            if let Ok(js) = serde_json::to_string(&self.prefs) {
                let _ = db.set_setting("preferences", &js);
            }
        }
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let t: String = s.chars().take(max_chars).collect();
        format!("{t}…")
    }
}

fn fmt_time(s: f64) -> String {
    let m = (s / 60.0).floor() as i64;
    let sec = (s - m as f64 * 60.0) as i64;
    format!("{m}:{sec:02}")
}
