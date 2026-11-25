//! Smart Mix — aplicación principal (egui).
//!
//! Layout Studio (kit GPT portado en studio_view.rs):
//!   appbar — marca, importar/analizar/herramientas, búsqueda, compat
//!   player — now playing (métricas, waveform EXISTENTE, transporte, cues)
//!   left   — biblioteca · playlists (árbol F-18) · dispositivos USB · rueda
//!   center — tabla del kit (checkbox + selección + edición de tags overlay)
//!   bottom — estado (mensaje, progreso de análisis, cola)
//!   vistas alternativas — Settings / Tag Editor / Track Detail ocupan el
//!   central cuando están activas; ventanas contextuales intactas.

use crate::studio_model;
use crate::texts;
use crate::theme::{energy_color, font_heading, Theme};
use crate::usb::{scan_drives, UsbDrive};
use crate::widgets::WaveView;
use crate::worker::{AnalysisEvent, AnalysisPool};
use egui::Sense;
use sm_audio::{deck_pair, Player};
use sm_core::models::Models;
use sm_db::{LibraryDb, LibraryQuery, SortField};
use sm_library::{read_metadata, scan_root, ScannedFile};
use sm_model::{AnalysisStatus, CuePoint, Preferences, TrackRow};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

mod detail;
mod windows;
pub mod telemetry {
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
    pub static SQL_US: AtomicU64 = AtomicU64::new(0);
    pub static SQL_N: AtomicU32 = AtomicU32::new(0);
    /// QA-F-008: upload de textura (hilo UI) y decode (worker) separados.
    pub static UPLOAD_US: AtomicU64 = AtomicU64::new(0);
    pub static UPLOAD_N: AtomicU32 = AtomicU32::new(0);
    pub static DECODE_US: AtomicU64 = AtomicU64::new(0);
    pub static DECODE_N: AtomicU32 = AtomicU32::new(0);
    pub fn reset() {
        SQL_US.store(0, Ordering::Relaxed);
        SQL_N.store(0, Ordering::Relaxed);
        UPLOAD_US.store(0, Ordering::Relaxed);
        UPLOAD_N.store(0, Ordering::Relaxed);
        DECODE_US.store(0, Ordering::Relaxed);
        DECODE_N.store(0, Ordering::Relaxed);
    }
    #[allow(dead_code)]
    pub fn add_sql(us: u64) {
        SQL_US.fetch_add(us, Ordering::Relaxed);
        SQL_N.fetch_add(1, Ordering::Relaxed);
    }
    pub fn add_upload(us: u64) {
        UPLOAD_US.fetch_add(us, Ordering::Relaxed);
        UPLOAD_N.fetch_add(1, Ordering::Relaxed);
    }
    pub fn add_decode(us: u64) {
        DECODE_US.fetch_add(us, Ordering::Relaxed);
        DECODE_N.fetch_add(1, Ordering::Relaxed);
    }
}

/// QA-F-008: pipeline de miniaturas — decode FUERA del hilo de UI.
///
/// El hilo UI solo sube texturas (presupuesto ~2 ms/frame); un único
/// worker con conexión propia a la BD decodifica y reescala; la caché es
/// LRU por capacidad (sin clear() global) y preserva el caché negativo.
#[allow(dead_code)] // pipeline dormido: la tabla del kit no pide miniaturas
pub mod cover_pipeline {
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Arc, Condvar, Mutex};

    /// Miniatura decodificada lista para subir como textura.
    pub struct ThumbRGBA {
        pub w: u32,
        pub h: u32,
        pub rgba: Vec<u8>,
    }

    /// Estados por pista (ausencia del mapa = Missing).
    pub enum CoverState {
        /// solicitada al worker (o en vuelo)
        Queued,
        /// textura lista en GPU
        Ready(egui::TextureHandle),
        /// sin arte — caché negativo preservado
        NoArt,
        /// decode fallido — caché negativo de sesión (no reintenta)
        Failed,
    }

    /// Caché LRU por capacidad: sustituye al clear() global (>400) que
    /// provocaba re-decodificaciones masivas al volver a zonas vistas.
    pub struct CoverCache {
        map: HashMap<i64, (CoverState, u64)>,
        order: u64,
        cap: usize,
    }
    impl Default for CoverCache {
        fn default() -> Self {
            CoverCache {
                map: HashMap::new(),
                order: 0,
                cap: 800,
            }
        }
    }
    impl CoverCache {
        /// Estado por id tocando LRU; None = Missing.
        pub fn get_mut(&mut self, rid: i64) -> Option<&mut CoverState> {
            if let Some((st, last)) = self.map.get_mut(&rid) {
                self.order += 1;
                *last = self.order;
                Some(st)
            } else {
                None
            }
        }
        pub fn insert(&mut self, rid: i64, st: CoverState) {
            self.order += 1;
            self.map.insert(rid, (st, self.order));
            // evicción LRU (nunca evicta solicitudes en vuelo)
            while self.map.len() > self.cap {
                let oldest = self
                    .map
                    .iter()
                    .filter(|(_, (st, _))| !matches!(st, CoverState::Queued))
                    .min_by_key(|(_, (_, last))| *last)
                    .map(|(k, _)| *k);
                match oldest {
                    Some(k) => {
                        self.map.remove(&k);
                    }
                    None => break,
                }
            }
        }
        /// true si queda alguna solicitud en vuelo (mantiene el repaint).
        pub fn has_queued(&self) -> bool {
            self.map
                .values()
                .any(|(st, _)| matches!(st, CoverState::Queued))
        }
        /// Las solicitudes Queued que dejaron de estar visibles vuelven a
        /// Missing: el rebuild de la cola descarta lo obsoleto tras fling.
        pub fn demote_queued(&mut self, keep: &HashSet<i64>) {
            let stale: Vec<i64> = self
                .map
                .iter()
                .filter(|(k, (st, _))| matches!(st, CoverState::Queued) && !keep.contains(*k))
                .map(|(k, _)| *k)
                .collect();
            for k in stale {
                self.map.remove(&k);
            }
        }
        pub fn len(&self) -> usize {
            self.map.len()
        }
    }

    /// Resultado del worker: Ok(Some)=decodificada, Ok(None)=sin arte, Err=falló.
    pub struct CoverResult {
        pub rid: i64,
        pub thumb: Result<Option<ThumbRGBA>, ()>,
    }

    /// Handle del worker: al dropearlo, cancel + notify + join (cierre limpio).
    pub struct CoverWorker {
        pub queue: Arc<(Mutex<VecDeque<i64>>, Condvar)>,
        cancel: Arc<AtomicBool>,
        join: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for CoverWorker {
        fn drop(&mut self) {
            self.cancel.store(true, Ordering::Relaxed);
            self.queue.1.notify_all();
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    /// Arranca UN worker con su PROPIA conexión a la BD (WAL: apertura
    /// idempotente; no comparte el Mutex del hilo UI).
    pub fn spawn(db_path: std::path::PathBuf) -> (CoverWorker, Receiver<CoverResult>) {
        let queue = Arc::new((Mutex::new(VecDeque::<i64>::new()), Condvar::new()));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx): (Sender<CoverResult>, Receiver<CoverResult>) = std::sync::mpsc::channel();
        let q = queue.clone();
        let cx = cancel.clone();
        let join = std::thread::Builder::new()
            .name("cover-decode".to_string())
            .spawn(move || {
                let db = match sm_db::LibraryDb::open(&db_path) {
                    Ok(d) => d,
                    Err(_) => return,
                };
                loop {
                    let rid = {
                        let (dq, cv) = &*q;
                        let mut dq = dq.lock().unwrap();
                        loop {
                            if let Some(r) = dq.pop_front() {
                                break r;
                            }
                            if cx.load(Ordering::Relaxed) {
                                return;
                            }
                            dq = cv.wait(dq).unwrap();
                        }
                    };
                    if cx.load(Ordering::Relaxed) {
                        return;
                    }
                    let t0 = std::time::Instant::now();
                    let thumb = match db.cover(rid) {
                        Ok(Some(bytes)) => match image::load_from_memory(&bytes) {
                            Ok(img) => {
                                let img = img.thumbnail(64, 64);
                                let rgba = img.to_rgba8();
                                let (w, h) = rgba.dimensions();
                                Ok(Some(ThumbRGBA {
                                    w,
                                    h,
                                    rgba: rgba.into_raw(),
                                }))
                            }
                            Err(_) => Err(()),
                        },
                        Ok(None) => Ok(None),
                        Err(_) => Err(()),
                    };
                    super::telemetry::add_decode(t0.elapsed().as_micros() as u64);
                    if tx.send(CoverResult { rid, thumb }).is_err() {
                        return; // la app se cerró
                    }
                }
            })
            .expect("spawn cover worker");
        (
            CoverWorker {
                queue,
                cancel,
                join: Some(join),
            },
            rx,
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn lru_evicta_el_mas_viejo_no_encolado() {
            let mut c = CoverCache {
                map: HashMap::new(),
                order: 0,
                cap: 3,
            };
            c.insert(1, CoverState::NoArt);
            c.insert(2, CoverState::NoArt);
            c.insert(3, CoverState::NoArt);
            assert_eq!(c.len(), 3);
            // tocar el 1 lo rejuvenece: evicta el 2
            c.get_mut(1);
            c.insert(4, CoverState::NoArt);
            assert_eq!(c.len(), 3);
            assert!(c.get_mut(2).is_none());
            assert!(c.get_mut(1).is_some());
        }

        #[test]
        fn lru_no_evicta_encoladas() {
            let mut c = CoverCache {
                map: HashMap::new(),
                order: 0,
                cap: 2,
            };
            c.insert(1, CoverState::Queued);
            c.insert(2, CoverState::NoArt);
            c.insert(3, CoverState::NoArt);
            assert!(c.get_mut(1).is_some(), "una Queued no debe evictarse");
        }

        #[test]
        fn demote_deja_missing_las_no_visibles() {
            let mut c = CoverCache::default();
            let mut keep = HashSet::new();
            keep.insert(7);
            c.insert(7, CoverState::Queued);
            c.insert(9, CoverState::Queued);
            c.demote_queued(&keep);
            assert!(matches!(c.get_mut(7), Some(CoverState::Queued)));
            assert!(c.get_mut(9).is_none());
        }
    }
}

/// Estado del backup USB que corre en un hilo de fondo (poll en update()).
#[derive(Default)]
pub(crate) struct UsbBackupState {
    lines: Vec<String>,
    done: bool,
}

/// Estado de una operación Safe Format en hilo de fondo (dry-run o real).
pub(crate) struct UsbOpState {
    pub lines: Vec<String>,
    pub done: bool,
    /// Resultado del dry-run (para extraer plan/token).
    pub dry_result: Option<Box<sm_usb::WorkflowResult>>,
}

impl Default for UsbOpState {
    fn default() -> Self {
        Self {
            lines: Vec::new(),
            done: false,
            dry_result: None,
        }
    }
}

/// Fases del flujo Safe Format en la UI.
#[derive(PartialEq, Clone, Copy)]
pub(crate) enum SfPhase {
    Idle,
    DryRunning,
    Ready,
    Running,
    Finished,
}

pub struct SmartMixApp {
    /// Textura del lockup horizontal oficial (branding Penpot).
    pub(crate) brand_lockup: Option<egui::TextureHandle>,
    pub(crate) db: Arc<Mutex<LibraryDb>>,
    pub(crate) player: Option<Player>,
    pub(crate) models: Arc<Models>,
    pub(crate) pool: AnalysisPool,
    pub prefs: Preferences,
    pub(crate) egui_ctx: egui::Context,
    // biblioteca
    pub(crate) search: String,
    pub(crate) sort: (SortField, bool),
    pub(crate) total: i64,
    /// Stats de biblioteca para el sidebar (§6: contadores BIBLIOTECA).
    pub(crate) sidebar_stats: Option<sm_db::LibraryStats>,
    /// Track Detail abierto para esta pista (None = tabla). §10.
    pub(crate) detail_view: Option<i64>,
    /// El hook SM_PLAY ya corrió (una sola vez).
    pub(crate) play_hook_done: bool,
    /// QA (cierre): SM_TAG_DRAWER=1 ya abrió el drawer del studio.
    pub(crate) tag_drawer_hook_done: bool,
    /// hook QA (fix usb-scan): SM_USB_SCAN=1 dispara el scan de unidades al
    /// arrancar (una sola vez) — evidencia [usb-scan] en el log sin input GUI.
    pub(crate) usb_scan_hook_done: bool,
    /// Rueda armónica del sidebar expandida (F-21; ex "Camelot").
    pub(crate) show_wheel: bool,
    /// Vista Settings/Export/Integrations (§11).
    pub(crate) settings_view: bool,
    pub(crate) settings_tab: u8,
    pub(crate) wheel_state: crate::wheel::WheelState,
    /// Vista Studio del kit GPT (estado UI entre frames).
    pub(crate) studio_ui: crate::studio_view::StudioUi,
    /// Chips de filtro del toolbar (§6): BPM y ENERGÍA reales.
    pub(crate) filter_bpm: Option<(f64, f64)>,
    pub(crate) filter_energy: Option<(f64, f64)>,
    /// Nº de pistas por playlist (contadores del sidebar §6).
    pub(crate) playlist_counts: std::collections::HashMap<i64, i64>,
    pub(crate) rows: BTreeMap<usize, TrackRow>,
    pub(crate) fetched_range: (usize, usize),
    pub(crate) selected_id: Option<i64>,
    /// F-24: selección múltiple de la tabla — ctrl+clic añade/quita,
    /// shift+clic extiende el rango desde el ancla, ✓TODO lo filtrado.
    pub(crate) selected_set: std::collections::HashSet<i64>,
    /// Confirmacion pendiente de "Quitar de la biblioteca" (ids).
    pub(crate) confirm_remove: Option<Vec<i64>>,
    pub(crate) detail: Option<sm_db::TrackFull>,
    pub(crate) cover_tex: Option<egui::TextureHandle>,
    pub(crate) cover_tex_id: Option<i64>,
    pub(crate) compat_key: Option<String>,
    pub(crate) collection: Collection,
    pub(crate) playlists: Vec<sm_model::Playlist>,
    pub(crate) active_playlist: Option<i64>,
    // now playing
    pub(crate) active_deck: usize,
    pub(crate) deck_track_id: [Option<i64>; 2],
    /// Caché PERSISTENTE de miniaturas (antes se recreaba cada frame:
    /// decodificaba ~30 imágenes y subía texturas GPU por frame → lento).
    pub(crate) covers: cover_pipeline::CoverCache,
    /// QA-F-008: worker de decode de carátulas (hilo aparte, BD propia).
    pub(crate) cover_worker: Option<cover_pipeline::CoverWorker>,
    pub(crate) cover_rx: Option<std::sync::mpsc::Receiver<cover_pipeline::CoverResult>>,
    /// FPS suavizado para el status bar.
    pub(crate) fps: f32,
    pub(crate) wave_zoom: (f64, f64),
    /// Clave (pista, duración×20) para la que vale `wave_zoom`: el zoom se
    /// resetea a ventana completa al cambiar de pista O cuando la duración
    /// cambia (p. ej. el deck completa la carga y la duración real crece).
    pub(crate) wave_zoom_key: (Option<i64>, i64),
    // usb
    pub(crate) usb_drives: Vec<UsbDrive>,
    pub(crate) usb_expanded: HashMap<String, bool>,
    pub(crate) usb_crate_view: Option<(String, Vec<String>)>,
    /// Scan de unidades en hilo de fondo (fix usb-scan): None = inactivo;
    /// Some(state) mientras escanea — el hilo deja Some(Vec<UsbDrive>) al
    /// terminar y update() lo drena. El spinner del dropdown de export
    /// vive de esto (scan_drives() tarda ~1.5 s: JAMÁS en el hilo UI).
    pub(crate) device_scan: Option<Arc<Mutex<Option<Vec<UsbDrive>>>>>,
    // ventanas
    pub(crate) show_prefs: bool,
    pub(crate) show_export: bool,
    /// F-25 export USB: índice de destino (0 = carpeta local, i+1 = usb_drives[i]).
    pub(crate) export_dest: usize,
    /// F-30: ventana de renombrado de archivos (preview + apply).
    pub(crate) show_rename_files: bool,
    /// Modo EDIT de cues (patrón rekordbox): los × de borrado solo aparecen
    /// cuando este toggle está activo.
    pub(crate) cues_editing: bool,
    /// Rango de tempo del fader estilo CDJ: 6/10/16/50 (WIDE) por ciento.
    pub(crate) pitch_range_pct: f32,
    pub(crate) rename_custom: String,
    pub(crate) rename_pattern_idx: usize,
    pub(crate) rename_plan: Vec<sm_export::RenamePlan>,
    pub(crate) rename_report: String,
    /// F-24 fase 2: vista Tag Editor (edición masiva estilo herramienta de referencia).
    pub(crate) tags_view: bool,
    /// Copia (id, TrackMeta) de las pistas bajo edición — se carga SOLO al
    /// entrar/refrescar la vista (los BLOBs de track_full NO se leen por
    /// frame; lección track_full).
    pub(crate) tag_tracks: Vec<(i64, sm_model::TrackMeta)>,
    pub(crate) tag_fields: crate::tag_editor::TagEditFields,
    pub(crate) tag_report: String,
    /// F-24 fase 3: escribir también el tag de los ARCHIVOS (OFF por
    /// defecto — modifica archivos, consentimiento explícito).
    pub(crate) tag_write_files: bool,
    /// F-31: incluir iTunSMPB/TCMP en la limpieza (OFF — peligroso para
    /// gapless; opt-in explícito del usuario).
    pub(crate) tag_junk_gapless: bool,
    /// F-31: líneas de preview "pista · campos" del último escaneo.
    pub(crate) tag_junk_preview: Vec<String>,
    /// F-31: true tras un escaneo (habilita LIMPIAR).
    pub(crate) tag_junk_scanned: bool,
    /// F-31: reporte del último escaneo/limpieza.
    pub(crate) tag_junk_report: String,
    /// Backup USB en curso (hilo de fondo) — None = inactivo.
    pub(crate) usb_backup: Option<Arc<Mutex<UsbBackupState>>>,
    pub(crate) usb_backup_log: String,
    /// Safe Format USB: flujo backup→formato→restore con doble confirmación.
    pub(crate) show_safe_format: bool,
    pub(crate) sf_drive: Option<(std::path::PathBuf, String, f64)>,
    pub(crate) sf_label: String,
    pub(crate) sf_dest: Option<std::path::PathBuf>,
    pub(crate) sf_phase: SfPhase,
    pub(crate) sf_token: String,
    pub(crate) sf_cmd: String,
    pub(crate) sf_admin: bool,
    pub(crate) sf_notes: Vec<String>,
    pub(crate) sf_tok_a: String,
    pub(crate) sf_tok_b: String,
    pub(crate) sf_op: Option<Arc<Mutex<UsbOpState>>>,
    pub(crate) sf_log: String,
    pub(crate) show_new_playlist: bool,
    /// Renombrar playlist (id en curso).
    pub(crate) show_rename_playlist: bool,
    pub(crate) rename_playlist_id: Option<i64>,
    /// F-18: carpetas colapsadas del árbol de playlists (id -> true).
    pub(crate) playlist_collapsed: HashMap<i64, bool>,
    /// F-18: padre destino al crear una sub-playlist (None = raíz).
    pub(crate) new_playlist_parent: Option<i64>,
    /// F-19: modo orden manual activo para la playlist abierta.
    pub(crate) manual_order: bool,
    /// F-19: filas del panel de orden manual: (track_id, título, artista).
    pub(crate) manual_order_rows: Vec<(i64, String, String)>,
    pub(crate) show_about: bool,
    pub(crate) show_corrections: bool,
    pub(crate) auto_queue_after_scan: bool,
    pub(crate) new_playlist_name: String,
    pub(crate) new_playlist_smart: bool,
    pub(crate) export_log: String,
    pub(crate) status_msg: String,
    /// analyze-live: una pista terminó y la tabla/deck aún no reflejan su
    /// análisis — el refresh throttled de update() lo consume.
    pub(crate) analysis_dirty: bool,
    /// analyze-live: instante del último refresh incremental (throttle ~1 s).
    pub(crate) last_analysis_refresh: std::time::Instant,
    /// analyze-live: la cola estaba ocupada en el frame anterior — detecta
    /// la transición fin-de-lote para el último refresh + status final.
    pub(crate) analysis_was_busy: bool,
    pub(crate) scan_busy: Arc<Mutex<bool>>,
    pub(crate) ctx_repaint: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Collection {
    All,
    Unanalyzed,
    Duplicates,
}

// ------------------------------------------------- analyze-live (puro) ----

/// Throttle del refresh incremental: un reload del Model como mucho cada
/// 1 s mientras la cola de análisis trabaja.
pub(crate) const ANALYSIS_REFRESH_EVERY: std::time::Duration =
    std::time::Duration::from_millis(1000);

/// ¿Toca refrescar? Solo si hay pistas terminadas sin mostrar Y ya pasó el
/// throttle desde el último refresh — nunca por pista.
fn analysis_refresh_due(dirty: bool, since_last: std::time::Duration) -> bool {
    dirty && since_last >= ANALYSIS_REFRESH_EVERY
}

/// Adaptador CoverArt: enchufa la tabla del kit al cover_pipeline existente
/// (QA-F-008) — LRU por capacidad + worker con decode fuera del hilo de UI.
/// `request_cover` es idempotente (el estado en caché deduplica).
struct StudioCovers<'a> {
    cache: &'a mut cover_pipeline::CoverCache,
    worker: Option<&'a cover_pipeline::CoverWorker>,
}
impl studio_model::CoverArt for StudioCovers<'_> {
    fn cover_texture(&mut self, track_id: i64) -> Option<egui::TextureHandle> {
        match self.cache.get_mut(track_id) {
            Some(cover_pipeline::CoverState::Ready(tex)) => Some(tex.clone()),
            _ => None,
        }
    }
    fn request_cover(&mut self, track_id: i64) {
        if self.cache.get_mut(track_id).is_some() {
            return; // en vuelo, lista o caché negativa: no re-pedir
        }
        if let Some(worker) = self.worker {
            let (queue, wakeup) = &*worker.queue;
            let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
            if !queue.contains(&track_id) {
                queue.push_back(track_id);
            }
            wakeup.notify_one();
        }
        self.cache
            .insert(track_id, cover_pipeline::CoverState::Queued);
    }
}

impl SmartMixApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<SmartMixApp> {
        crate::theme::install(&cc.egui_ctx);
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("smart-mix");
        std::fs::create_dir_all(&data_dir)?;
        let db = LibraryDb::open(&data_dir.join("library.db"))?;
        let (cover_worker, cover_rx) = cover_pipeline::spawn(data_dir.join("library.db"));
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
        // F-25: tema persistido — instala la paleta ANTES del primer frame
        // (el install() de arriba corrió con el default oscuro; es idempotente)
        crate::theme::set_light_mode(prefs.theme_light);
        crate::theme::install(&cc.egui_ctx);
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
            brand_lockup: None,
            db,
            player: Some(player),
            models,
            pool,
            prefs,
            egui_ctx: cc.egui_ctx.clone(),
            search: String::new(),
            sort: (SortField::AddedAt, true),
            total: 0,
            sidebar_stats: None,
            detail_view: None,
            play_hook_done: false,
            tag_drawer_hook_done: false,
            usb_scan_hook_done: false,
            show_wheel: true, // rueda del sidebar expandida por defecto
            settings_view: false,
            settings_tab: 0,
            wheel_state: crate::wheel::WheelState::default(),
            studio_ui: crate::studio_view::StudioUi::new(),
            filter_bpm: None,
            filter_energy: None,
            playlist_counts: std::collections::HashMap::new(),
            rows: BTreeMap::new(),
            fetched_range: (0, 0),
            selected_id: None,
            selected_set: std::collections::HashSet::new(),
            confirm_remove: None,
            detail: None,
            cover_tex: None,
            cover_tex_id: None,
            compat_key: None,
            collection: Collection::All,
            playlists: Vec::new(),
            active_playlist: None,
            active_deck: 0,
            deck_track_id: [None, None],
            covers: cover_pipeline::CoverCache::default(),
            cover_worker: Some(cover_worker),
            cover_rx: Some(cover_rx),
            fps: 0.0,
            wave_zoom: (0.0, 0.0),
            wave_zoom_key: (None, 0),
            usb_drives: Vec::new(),
            usb_expanded: HashMap::new(),
            usb_crate_view: None,
            device_scan: None,
            show_prefs: false,
            show_export: false,
            export_dest: 0,
            show_rename_files: false,
            cues_editing: false,
            pitch_range_pct: 10.0,
            rename_custom: "{key}_{mode}_-_{bpm}_-_{title}_-_{artist}".into(),
            rename_pattern_idx: 0,
            rename_plan: Vec::new(),
            rename_report: String::new(),
            tags_view: false,
            tag_tracks: Vec::new(),
            tag_fields: Default::default(),
            tag_report: String::new(),
            tag_write_files: false,
            tag_junk_gapless: false,
            tag_junk_preview: Vec::new(),
            tag_junk_scanned: false,
            tag_junk_report: String::new(),
            usb_backup: None,
            usb_backup_log: String::new(),
            show_safe_format: false,
            sf_drive: None,
            sf_label: "SMARTMIX".into(),
            sf_dest: None,
            sf_phase: SfPhase::Idle,
            sf_token: String::new(),
            sf_cmd: String::new(),
            sf_admin: false,
            sf_notes: Vec::new(),
            sf_tok_a: String::new(),
            sf_tok_b: String::new(),
            sf_op: None,
            sf_log: String::new(),
            show_new_playlist: false,
            show_rename_playlist: false,
            rename_playlist_id: None,
            playlist_collapsed: HashMap::new(),
            new_playlist_parent: None,
            manual_order: false,
            manual_order_rows: Vec::new(),
            show_about: false,
            show_corrections: false,
            auto_queue_after_scan: false,
            new_playlist_name: String::new(),
            new_playlist_smart: false,
            export_log: String::new(),
            status_msg: String::new(),
            analysis_dirty: false,
            last_analysis_refresh: std::time::Instant::now(),
            analysis_was_busy: false,
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
                bpm_min: self.filter_bpm.map(|f| f.0),
                bpm_max: self.filter_bpm.map(|f| f.1),
                energy_min: self.filter_energy.map(|f| f.0),
                energy_max: self.filter_energy.map(|f| f.1),
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
        if let Ok(db) = self.db.lock() {
            self.sidebar_stats = db.stats().ok();
            self.playlist_counts = self
                .playlists
                .iter()
                .map(|pl| {
                    (
                        pl.id,
                        db.playlist_track_ids(pl.id)
                            .map(|v| v.len() as i64)
                            .unwrap_or(0),
                    )
                })
                .collect();
        }
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
                                artist: format!("{}", g.reason),
                                bpm: None,
                                key_camelot: None,
                                energy: None,
                                duration_s: *dur,
                                album: String::new(),
                                genre: String::new(),
                                rating: 0,
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

    /// F-19: recarga las filas del panel de orden manual (id, título,
    /// artista) en el orden por `position` de la playlist activa.
    fn reload_manual_order(&mut self) {
        self.manual_order_rows.clear();
        if let Some(pid) = self.active_playlist {
            if let Ok(db) = self.db.lock() {
                let q = LibraryQuery {
                    playlist_id: Some(pid),
                    limit: 0,
                    ..Default::default()
                };
                if let Ok(page) = db.query(&q) {
                    self.manual_order_rows = page
                        .into_iter()
                        .map(|r| (r.id, r.title, r.artist))
                        .collect();
                }
            }
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

    /// F-24 fase 2: abrir la vista Tag Editor con la selección actual.
    fn open_tag_editor(&mut self) {
        self.settings_view = false;
        self.detail_view = None;
        self.tags_view = true;
        if self.selected_set.is_empty() {
            if let Some(id) = self.selected_id {
                self.selected_set.insert(id);
            }
        }
        self.tag_load_tracks();
    }

    /// F-24 fase 2: carga los TrackMeta de las pistas seleccionadas. Se
    /// llama SOLO al entrar/refrescar la vista o tras aplicar — nunca por
    /// frame.
    fn tag_load_tracks(&mut self) {
        let mut ids: Vec<i64> = if self.selected_set.is_empty() {
            self.selected_id.into_iter().collect()
        } else {
            self.selected_set.iter().copied().collect()
        };
        ids.sort_unstable();
        ids.dedup();
        let mut tracks = Vec::with_capacity(ids.len());
        if let Ok(db) = self.db.lock() {
            for id in ids {
                if let Ok(Some(full)) = db.track_full(id) {
                    tracks.push((id, full.meta));
                }
            }
        }
        eprintln!("[f24] tag editor: {} pista(s) cargada(s)", tracks.len());
        self.tag_tracks = tracks;
        self.tag_report.clear();
        // F-31: el preview de basura queda obsoleto al recargar
        self.tag_junk_scanned = false;
        self.tag_junk_preview.clear();
    }

    /// F-31: escanea los archivos de la selección y construye el preview
    /// de qué campos se quitarían por pista (lectura, sin escribir).
    fn tag_junk_scan(&mut self) {
        let policy = crate::tag_editor::JunkPolicy {
            clean_comments: true,
            clean_itunes_gapless: self.tag_junk_gapless,
        };
        let mut lines = Vec::new();
        let mut unreadable = 0usize;
        let mut skipped = 0usize;
        for (_id, meta) in &self.tag_tracks {
            if !crate::tag_editor::is_taggable(&meta.path) {
                skipped += 1;
                continue;
            }
            match sm_export::id3::read_junk_state(&meta.path) {
                Ok(fs) => {
                    let st = crate::tag_editor::JunkTagState {
                        comment: fs.comment,
                        itun_smpb: fs.itun_smpb,
                        tcmp: fs.tcmp,
                    };
                    let line = crate::tag_editor::junk_preview_line(&st, &policy);
                    if !line.is_empty() {
                        let name = if meta.title.is_empty() {
                            std::path::Path::new(&meta.path)
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default()
                        } else {
                            meta.title.clone()
                        };
                        lines.push(format!("{name} · {line}"));
                    }
                }
                Err(_) => unreadable += 1,
            }
        }
        let junk_n = lines.len();
        eprintln!(
            "[f31] escaneo: {junk_n} con basura de {} pista(s), {unreadable} ilegible(s), {skipped} omitida(s)",
            self.tag_tracks.len()
        );
        self.tag_junk_preview = lines;
        self.tag_junk_scanned = true;
        self.tag_junk_report = format!(
            "{junk_n} pista(s) con tags basura{}",
            if unreadable > 0 {
                format!(" · {unreadable} ilegible(s)")
            } else {
                String::new()
            }
        );
    }

    /// F-31: aplica la limpieza a los archivos de la selección. Requiere
    /// el consentimiento de escritura (`tag_write_files`): la BD no guarda
    /// comments, así que sin escritura de archivos no hay nada que limpiar.
    fn tag_junk_clean(&mut self) {
        if !self.tag_write_files {
            self.tag_junk_report =
                "La limpieza requiere 'escribir también tags de los archivos': la BD no guarda comments (F-31 limpia el archivo).".into();
            return;
        }
        let gapless = self.tag_junk_gapless;
        let (mut cleaned, mut noop, mut errs, mut skipped) = (0usize, 0usize, 0usize, 0usize);
        for (_id, meta) in &self.tag_tracks {
            if !crate::tag_editor::is_taggable(&meta.path) {
                skipped += 1;
                continue;
            }
            match sm_export::id3::remove_junk(&meta.path, true, gapless) {
                Ok(true) => cleaned += 1,
                Ok(false) => noop += 1,
                Err(e) => {
                    errs += 1;
                    eprintln!("[f31] error en {}: {e:#}", meta.path);
                }
            }
        }
        eprintln!(
            "[f31] limpiar: {cleaned} limpiada(s), {noop} sin basura, {errs} error(es), {skipped} omitida(s)"
        );
        self.tag_junk_report = format!(
            "✓ {cleaned} pista(s) limpiada(s) · {noop} sin basura · {errs} error(es) · {skipped} omitida(s)"
        );
        // invalidar el preview: el estado ya cambió
        self.tag_junk_scanned = false;
        self.tag_junk_preview.clear();
    }

    /// F-24 fase 2: aplica el diff a las N pistas — lee cada track_full,
    /// aplica SOLO los campos no vacíos y guarda con upsert_track (el
    /// análisis guardado no se toca: upsert refresca metadatos solamente).
    /// Fase 3: si tag_write_files, escribe además el tag del ARCHIVO
    /// (sm_export::write_meta_tags, mp3/aiff/wav/flac); los errores por
    /// archivo se CUENTAN, no abortan el lote.
    fn tag_apply(&mut self) {
        let fields = self.tag_fields.clone();
        let write_files = self.tag_write_files;
        let ids: Vec<i64> = self.tag_tracks.iter().map(|(id, _)| *id).collect();
        let mut updated = 0usize;
        let mut unchanged = 0usize;
        let mut missing = 0usize;
        let mut file_ok = 0usize;
        let mut file_err = 0usize;
        let mut file_skip = 0usize;
        fn ne(v: &str) -> Option<&str> {
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        }
        if let Ok(db) = self.db.lock() {
            for id in &ids {
                let Ok(Some(mut full)) = db.track_full(*id) else {
                    missing += 1;
                    continue;
                };
                if crate::tag_editor::apply_fields(&mut full.meta, &fields) {
                    if db.upsert_track(&full.meta).is_ok() {
                        updated += 1;
                        if write_files {
                            if !crate::tag_editor::is_taggable(&full.meta.path) {
                                file_skip += 1;
                            } else {
                                match sm_export::write_meta_tags(
                                    &full.meta.path,
                                    ne(&fields.title),
                                    ne(&fields.artist),
                                    ne(&fields.album),
                                    ne(&fields.genre),
                                    fields.year_value(),
                                ) {
                                    Ok(()) => file_ok += 1,
                                    Err(e) => {
                                        file_err += 1;
                                        eprintln!(
                                            "[f24] error de tags en {}: {e:#}",
                                            full.meta.path
                                        );
                                    }
                                }
                            }
                        }
                    }
                } else {
                    unchanged += 1;
                }
            }
        }
        eprintln!(
            "[f24] aplicar: {updated} actualizada(s), {unchanged} sin cambios, {missing} ausente(s); archivos: {file_ok} ok, {file_err} error, {file_skip} omitido(s)"
        );
        let files_part = if write_files {
            format!(
                " · archivos: {file_ok} etiquetado(s), {file_err} error(es), {file_skip} omitido(s)"
            )
        } else {
            String::new()
        };
        self.tag_report = format!(
            "✓ {updated} pista(s) actualizada(s) · {unchanged} sin cambios{}{}",
            if missing > 0 {
                format!(" · {missing} no encontrada(s)")
            } else {
                String::new()
            },
            files_part
        );
        self.tag_fields = Default::default();
        self.refresh();
        self.tag_load_tracks();
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
            // B4 (auditoría): la ESCRITURA a BD va en lotes de
            // IMPORT_TX_CHUNK pistas por transacción; la lectura de
            // metadatos y el repintado siguen siendo por archivo, con la
            // cadencia de siempre (una por cada IMPORT_REPAINT_EVERY).
            let mut chunk: Vec<ScannedFile> = Vec::with_capacity(IMPORT_TX_CHUNK);
            for (i, f) in files.iter().enumerate() {
                if i % IMPORT_REPAINT_EVERY == 0 {
                    repaint();
                }
                if let Ok(scanned) = read_metadata(f) {
                    chunk.push(scanned);
                    if import_chunk_full(chunk.len()) {
                        if let Ok(db) = db.lock() {
                            n += import_flush_chunk(&db, &chunk);
                        }
                        chunk.clear();
                    }
                }
            }
            if !chunk.is_empty() {
                if let Ok(db) = db.lock() {
                    n += import_flush_chunk(&db, &chunk);
                }
            }
            tracing::info!("import: {n}/{} pistas", files.len());
            *busy.lock().unwrap() = false;
            repaint();
        });
    }

    /// ANALIZAR consciente de selección: con pistas seleccionadas se
    /// re-analizan ELLAS (fix del bug "Analizando 0 pistas": la selección
    /// se ignoraba); sin selección, las pendientes como siempre.
    #[allow(dead_code)] // el appbar del kit emite Analyze(ids) explicitos
    fn analyze_selection_or_pending(&mut self) {
        let ids: Vec<i64> = if !self.selected_set.is_empty() {
            self.selected_set.iter().copied().collect()
        } else {
            self.selected_id.into_iter().collect()
        };
        if !ids.is_empty() {
            if let Ok(mut db) = self.db.lock() {
                let mut pairs = Vec::new();
                for id in &ids {
                    if let Ok(Some(f)) = db.track_full(*id) {
                        let _ = db.set_analysis_status(*id, AnalysisStatus::Pending);
                        pairs.push((*id, f.meta.path.clone()));
                    }
                }
                let n = pairs.len();
                self.pool.enqueue(pairs);
                self.status_msg = format!("Analizando {n} pistas…");
            }
            return;
        }
        self.analyze_pending();
    }

    /// REANALIZAR BIBLIOTECA: (1) borra los tags generados por Smart Mix
    /// (tono/bpm/energía) de TODAS las pistas —refrescando la lista para
    /// que el borrado se VEA— y (2) re-analiza la biblioteca completa.
    fn reanalyze_library(&mut self) {
        if let Ok(db) = self.db.lock() {
            let n = db.clear_sm_tags_all().unwrap_or(0);
            eprintln!("[feature] reanalizar biblioteca: tags de {n} pistas borrados");
            self.status_msg = format!("Tags de {n} pistas borrados; reanalizando…");
        }
        self.refresh();
        self.analyze_pending();
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
            match ev {
                AnalysisEvent::Started(_) => {}
                // analyze-live: cada pista terminada marca la UI como sucia
                // (el refresh throttled de update() la recarga a lo sumo cada
                // 1 s) y recarga YA el detalle si está en pantalla, para que
                // la waveform/métricas del deck aparezcan sin esperar al lote.
                AnalysisEvent::Finished(id, ms) => {
                    self.analysis_dirty = true;
                    if self.detail.as_ref().map(|d| d.id) == Some(id) {
                        self.load_detail(id);
                    }
                    let (done, total) = {
                        let s = self.pool.state.lock().unwrap();
                        (s.done_session, s.session_total)
                    };
                    eprintln!("[analyze-live] #{id} ok {ms} ms — sesión {done}/{total}");
                }
                AnalysisEvent::Failed(id, e) => {
                    self.status_msg = format!("Error analizando #{id}: {e}");
                    self.analysis_dirty = true;
                    eprintln!("[analyze-live] #{id} FALLÓ: {e}");
                }
                AnalysisEvent::QueueEmpty => {}
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
        // Safe Format: drenar líneas del hilo de fondo y extraer el plan del dry-run
        if let Some(state) = &self.sf_op {
            let (new_lines, done, dry) = {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                (std::mem::take(&mut s.lines), s.done, s.dry_result.take())
            };
            for l in new_lines {
                self.sf_log.push_str(&l);
                self.sf_log.push('\n');
            }
            let dry = dry;
            let had_dry = dry.is_some();
            if let Some(res) = dry {
                if let Some(plan) = &res.format_plan {
                    self.sf_token = plan.confirm_token.clone();
                    self.sf_cmd = plan.command.clone();
                    self.sf_admin = plan.needs_admin;
                    self.sf_notes = plan.notes.clone();
                }
                self.sf_log.push_str(&format!(
                    "unidad: {} archivos · {:.2} GB · {} avisos FAT32\n",
                    res.scan.files.len(),
                    res.scan.total_bytes as f64 / 1e9,
                    res.fat32_warnings.len()
                ));
                self.sf_phase = if res.format_plan.is_some() {
                    SfPhase::Ready
                } else {
                    SfPhase::Idle
                };
            }
            if done && !had_dry {
                // ejecución real terminada (éxito o error ya en el log)
                self.sf_op = None;
                if matches!(self.sf_phase, SfPhase::Running) {
                    self.sf_phase = SfPhase::Finished;
                }
            } else if !done {
                ctx.request_repaint();
            }
        }
        // backup USB en hilo de fondo: drenar líneas nuevas y mantener repaint
        if let Some(state) = &self.usb_backup {
            let (new_lines, done) = {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                (std::mem::take(&mut s.lines), s.done)
            };
            for l in new_lines {
                self.usb_backup_log.push_str(&l);
                self.usb_backup_log.push('\n');
            }
            if done && self.usb_backup_log.lines().count() > 0 {
                // conserva la ventana con el resultado; el hilo ya terminó
                self.usb_backup = None;
            } else if !done {
                ctx.request_repaint();
            }
        }
        if !self.usb_backup_log.is_empty() {
            let mut show = true;
            let log = self.usb_backup_log.clone();
            egui::Window::new("Backup USB")
                .open(&mut show)
                .default_width(560.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            ui.monospace(&log);
                        });
                    if ui.small_button("Cerrar").clicked() {
                        self.usb_backup_log.clear();
                    }
                });
            if !show {
                self.usb_backup_log.clear();
            }
        }
        // log del frame ANTERIOR si hubo trabajo (diagnóstico de scroll)
        {
            use std::sync::atomic::Ordering;
            let sn = telemetry::SQL_N.load(Ordering::Relaxed);
            let un = telemetry::UPLOAD_N.load(Ordering::Relaxed);
            let dn = telemetry::DECODE_N.load(Ordering::Relaxed);
            if sn > 0 || un > 0 || dn > 0 {
                eprintln!(
                    "[perf] fps={:.0} sql={}×{:.1}ms cover-up={}×{:.1}ms cover-decode={}×{:.1}ms",
                    self.fps,
                    sn,
                    telemetry::SQL_US.load(Ordering::Relaxed) as f64 / 1000.0,
                    un,
                    telemetry::UPLOAD_US.load(Ordering::Relaxed) as f64 / 1000.0,
                    dn,
                    telemetry::DECODE_US.load(Ordering::Relaxed) as f64 / 1000.0,
                );
            }
        }
        telemetry::reset();
        // QA-F-008: drenar resultados del worker — el hilo UI SOLO sube
        // texturas a GPU con presupuesto temporal (~2 ms por frame)
        {
            let mut deadline_hit = false;
            if let Some(rx) = &self.cover_rx {
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2);
                while let Ok(res) = rx.try_recv() {
                    let t0 = std::time::Instant::now();
                    match res.thumb {
                        Ok(Some(th)) => {
                            let tex = ctx.load_texture(
                                format!("thumb-{}", res.rid),
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [th.w as usize, th.h as usize],
                                    &th.rgba,
                                ),
                                egui::TextureOptions::LINEAR,
                            );
                            self.covers
                                .insert(res.rid, cover_pipeline::CoverState::Ready(tex));
                        }
                        Ok(None) => {
                            self.covers
                                .insert(res.rid, cover_pipeline::CoverState::NoArt);
                        }
                        Err(()) => {
                            self.covers
                                .insert(res.rid, cover_pipeline::CoverState::Failed);
                        }
                    }
                    telemetry::add_upload(t0.elapsed().as_micros() as u64);
                    if std::time::Instant::now() >= deadline {
                        deadline_hit = true;
                        break;
                    }
                }
            }
            if deadline_hit {
                ctx.request_repaint();
            }
        }
        // FPS suavizado (para el status bar)
        {
            static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
            let mut last = LAST.lock().unwrap();
            match *last {
                Some(t) => {
                    let dt = t.elapsed().as_secs_f32();
                    if dt > 0.0 {
                        self.fps = self.fps * 0.9 + (1.0 / dt) * 0.1;
                    }
                    *last = Some(std::time::Instant::now());
                }
                None => *last = Some(std::time::Instant::now()),
            }
        }
        self.drain_events();
        // analyze-live: refresh incremental con throttle. Cada pista
        // terminada marca la UI sucia; mientras la cola trabaja se recarga
        // el Model a lo sumo cada ~1 s (no por pista: con 691 pendientes y
        // decenas de análisis por segundo, un refresh por pista mataría el
        // frame time). Sin esto la tabla/deck no ven key/bpm/energía ni
        // waveform hasta el fin del lote completo.
        let analysis_busy = self.pool.is_busy();
        if analysis_busy
            && analysis_refresh_due(self.analysis_dirty, self.last_analysis_refresh.elapsed())
        {
            self.last_analysis_refresh = std::time::Instant::now();
            self.analysis_dirty = false;
            self.refresh();
            let (done, total, queued, running) = {
                let s = self.pool.state.lock().unwrap();
                (
                    s.done_session,
                    s.session_total,
                    s.queue.len(),
                    s.running as u64,
                )
            };
            self.status_msg = format!("Analizando {} pistas…", queued + running as usize);
            eprintln!(
                "[analyze-live] refresh incremental: {done}/{total} analizadas, cola {queued}"
            );
        }
        // fin del lote (transición ocupado→reposo): último refresh para que
        // la tanda final también se vea sin esperar input del usuario, y
        // status de cierre — antes el "Analizando N" quedaba congelado.
        if !analysis_busy && self.analysis_was_busy {
            self.analysis_dirty = false;
            self.refresh();
            let (done, failed, total) = {
                let s = self.pool.state.lock().unwrap();
                (s.done_session, s.failed_session, s.session_total)
            };
            self.status_msg = format!("Análisis completado: {done} de {total} pistas");
            eprintln!("[analyze-live] lote terminado: {done}/{total} ok, {failed} fallidas");
        }
        self.analysis_was_busy = analysis_busy;
        // fix usb-scan: drenar el resultado del scan de unidades en background
        // (mientras Some, el dropdown de export muestra spinner).
        if let Some(state) = self.device_scan.take() {
            let done = state.lock().unwrap().take();
            if let Some(drives) = done {
                if self.export_dest > drives.len() {
                    self.export_dest = 0;
                }
                self.usb_drives = drives;
                self.status_msg =
                    format!("{} dispositivo(s) USB detectados", self.usb_drives.len());
            } else {
                self.device_scan = Some(state); // aún escaneando
            }
        }
        // Repaint continuo mientras reproduce o hay trabajo de fondo:
        // sin esto el playhead solo avanza cuando mueves el ratón (bug v1).
        let playing = self
            .player()
            .map(|p| p.decks.iter().any(|d| d.is_playing()))
            .unwrap_or(false);
        if playing
            || self.pool.is_busy()
            || *self.scan_busy.lock().unwrap()
            || self.device_scan.is_some()
        {
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
        // hook QA (fix usb-scan): SM_USB_SCAN=1 dispara el scan de unidades al
        // arrancar — evidencia [usb-scan] en el log sin input GUI.
        if std::env::var("SM_USB_SCAN").as_deref() == Ok("1") && !self.usb_scan_hook_done {
            self.usb_scan_hook_done = true;
            self.spawn_device_scan();
        }
        // hook QA (analyze-live): SM_ANALYZE=<n> encola las primeras n
        // pendientes al arrancar — evidencia [analyze-live] en el log
        // (eventos de progreso + refresh throttled + pistas pasando a
        // analyzed) sin input GUI. Smoke del worker en la app real.
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static ANALYZE_HOOK: AtomicBool = AtomicBool::new(false);
            if !ANALYZE_HOOK.swap(true, Ordering::Relaxed) {
                if let Ok(v) = std::env::var("SM_ANALYZE") {
                    if let Ok(n) = v.trim().parse::<usize>() {
                        let mut items = self
                            .db
                            .lock()
                            .ok()
                            .and_then(|db| {
                                db.track_ids_by_status(AnalysisStatus::Pending, n.max(1))
                                    .ok()
                            })
                            .unwrap_or_default();
                        // sin pendientes (p. ej. el lote del dueño ya terminó):
                        // re-analizar las n primeras — MISMA vía que el botón
                        // Reanalizar (status→Pending + encolar); la BD no se
                        // restaura ni se altera más allá de dejar correr el
                        // análisis sobre esas n pistas.
                        let reanalyzed = items.is_empty().then(|| -> Option<Vec<(i64, String)>> {
                            let db = self.db.lock().ok()?;
                            let all = db
                                .track_ids_by_status(AnalysisStatus::Done, n.max(1))
                                .unwrap_or_default();
                            for (id, _) in &all {
                                let _ = db.set_analysis_status(*id, AnalysisStatus::Pending);
                            }
                            Some(all)
                        });
                        if let Some(Some(all)) = reanalyzed {
                            items = all;
                        }
                        let n = items.len();
                        self.pool.enqueue(items);
                        self.status_msg = format!("Analizando {n} pistas…");
                        eprintln!("[analyze-live] SM_ANALYZE: encoladas {n} pendientes (smoke QA)");
                    }
                }
            }
        }
        // hook de verificación (dev/QA): SM_DETAIL=1 abre Track Detail de la
        // primera pista al arrancar — permite capturar la vista sin input GUI.
        // SM_PLAY=1 además CARGA la pista en el deck A en pausa con seek al
        // 60% — habilita medir playhead magenta y tiempo magenta sin audio.
        if std::env::var("SM_PLAY").as_deref() == Ok("1") && !self.play_hook_done {
            self.play_hook_done = true;
            if let Some(rid) = self.rows.values().next().map(|r| r.id) {
                self.select(rid);
                if let (Some(full), Some(player)) = (&self.detail, &self.player) {
                    let path = full.meta.path.clone();
                    let d = &player.decks[0];
                    d.load(&path);
                    let dur = full.analysis.duration_s.max(0.001);
                    d.seek(dur * 0.6);
                    self.deck_track_id[0] = Some(rid);
                    self.active_deck = 0;
                    eprintln!(
                        "[feature] SM_PLAY: pista {rid} cargada en A (pausa, seek {:.1}s)",
                        dur * 0.6
                    );
                }
            }
        }
        // hook de verificación (cierre): SM_TAG_DRAWER=1 abre el drawer del
        // editor de tags del studio sobre la primera pista — captura del
        // combo de energía editable sin input de usuario.
        if std::env::var("SM_TAG_DRAWER").as_deref() == Ok("1") && !self.tag_drawer_hook_done {
            if let Some(rid) = self.rows.values().next().map(|r| r.id) {
                self.tag_drawer_hook_done = true;
                self.select(rid);
                let model = self.build_studio_model();
                self.studio_ui
                    .open_editor(&model, vec![rid], studio_model::EditField::Energy);
                eprintln!("[qa] SM_TAG_DRAWER: drawer de tags abierto (id={rid})");
            }
        }
        // hook de verificación (cierre ítem 11): SM_COMPAT=<k> (p. ej. 7B)
        // activa el filtro de tonalidad al arrancar — captura del centro de
        // la rueda en modo tono elegido sin input GUI.
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static COMPAT_HOOK: AtomicBool = AtomicBool::new(false);
            if !COMPAT_HOOK.swap(true, Ordering::Relaxed) {
                if let Ok(v) = std::env::var("SM_COMPAT") {
                    if let Ok(key) = v.trim().parse::<studio_model::Key>() {
                        self.compat_key = Some(key.to_string());
                        self.refresh();
                        eprintln!("[qa] SM_COMPAT: filtro de tonalidad {key} activo");
                    }
                }
            }
        }
        // hook de verificación (cierre ítem 10): SM_SELECT=<id> selecciona
        // una pista concreta al arrancar — captura de la portada en la
        // cabecera del player con una pista que SÍ tiene arte.
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static SELECT_HOOK: AtomicBool = AtomicBool::new(false);
            // espera a que las filas estén cargadas antes de consumir el hook
            if !SELECT_HOOK.load(Ordering::Relaxed) && !self.rows.is_empty() {
                if let Ok(v) = std::env::var("SM_SELECT") {
                    if let Ok(id) = v.trim().parse::<i64>() {
                        if self.rows.values().any(|r| r.id == id) {
                            self.select(id);
                            SELECT_HOOK.store(true, Ordering::Relaxed);
                            eprintln!("[qa] SM_SELECT: pista {id} seleccionada");
                        }
                    }
                }
            }
        }
        // hook de verificación: SM_WHEEL=1 abre la rueda Camelot al arrancar
        {
            use std::sync::atomic::{AtomicBool, Ordering};
            static WHEEL_HOOK: AtomicBool = AtomicBool::new(false);
            if !WHEEL_HOOK.swap(true, Ordering::Relaxed)
                && std::env::var("SM_WHEEL").as_deref() == Ok("1")
            {
                self.show_wheel = true;
            }
            static SETTINGS_HOOK: AtomicBool = AtomicBool::new(false);
            if !SETTINGS_HOOK.swap(true, Ordering::Relaxed)
                && std::env::var("SM_SETTINGS").as_deref() == Ok("1")
            {
                self.settings_view = true;
            }
            // hook de verificación (F-18/F-19): SM_PL=<id> abre esa playlist
            // al arrancar; SM_PL_ORDER=1 además activa el modo orden manual
            // — permite capturar el árbol y el panel sin input GUI.
            static PL_HOOK: AtomicBool = AtomicBool::new(false);
            if !PL_HOOK.swap(true, Ordering::Relaxed) {
                if let Ok(v) = std::env::var("SM_PL") {
                    if let Ok(pid) = v.trim().parse::<i64>() {
                        if self.playlists.is_empty() {
                            self.refresh();
                        }
                        if self.playlists.iter().any(|p| p.id == pid) {
                            self.active_playlist = Some(pid);
                            self.collection = Collection::All;
                            if std::env::var("SM_PL_ORDER").as_deref() == Ok("1") {
                                self.manual_order = true;
                            }
                            self.refresh();
                            if self.manual_order {
                                self.reload_manual_order();
                            }
                            eprintln!(
                                "[feature] SM_PL: playlist {pid} abierta (orden manual: {})",
                                self.manual_order
                            );
                        } else {
                            eprintln!("[feature] SM_PL: playlist {pid} no existe");
                        }
                    }
                }
            }
            static TAGS_HOOK: AtomicBool = AtomicBool::new(false);
            if !TAGS_HOOK.swap(true, Ordering::Relaxed)
                && std::env::var("SM_VIEW").as_deref() == Ok("tags")
            {
                // QA: sembrar selección múltiple si no la hay, para capturar
                // la vista con varias pistas sin input de usuario
                if self.selected_set.is_empty() {
                    let ids: Vec<i64> = self.rows.values().take(8).map(|r| r.id).collect();
                    crate::tag_editor::extend_selection(&mut self.selected_set, &ids);
                }
                // y siembra un campo para que la captura muestre el
                // preview antes→después de las pistas (QA solamente)
                self.tag_fields.genre = "Smart Mix QA".into();
                self.open_tag_editor();
                eprintln!("[f24] SM_VIEW=tags: vista Tag Editor abierta");
            }
        }
        if self.detail_view.is_none()
            && std::env::var("SM_DETAIL").as_deref() == Ok("1")
            && !self.rows.is_empty()
        {
            if let Some(rid) = self.rows.values().next().map(|r| r.id) {
                self.select(rid);
                self.detail_view = Some(rid);
                eprintln!("[feature] SM_DETAIL: abriendo Track Detail id={rid}");
            }
        }
        // refrescar detalle SOLO si el análisis tocó la selección:
        // consulta ligera (analysis_ms, sin BLOBs/JSONs) por frame y
        // track_full completa únicamente cuando de verdad cambió.
        // (Antes: track_full con waveform 24KB + beatgrid JSON ~30KB CADA
        // frame — 3-15ms/frame, causa principal del scroll no fluido.)
        if let Some(id) = self.selected_id {
            let ams = self
                .db
                .lock()
                .ok()
                .and_then(|db| db.analysis_ms(id).unwrap_or(None));
            if let Some(ams) = ams {
                if Some(ams) != self.detail.as_ref().and_then(|d| d.analysis.analysis_ms) {
                    self.load_detail(id);
                }
            }
        }
        // Vista Studio del kit GPT: paneles + intenciones reales. Cuando una
        // vista alternativa (Settings/Tags/Detail) está activa ocupa el
        // CentralPanel y la biblioteca del kit no se dibuja.
        let alt_view = self.settings_view || self.tags_view || self.detail_view.is_some();
        let actions = self.run_studio_view(ctx, !alt_view);
        if alt_view {
            if self.settings_view {
                self.settings_view_panel(ctx);
            } else if self.tags_view {
                self.tag_editor_view(ctx);
            } else if self.detail_view.is_some() {
                self.track_detail(ctx);
            }
        }
        self.windows(ctx);
        self.dispatch_studio_actions(ctx, actions);
    }
}

impl SmartMixApp {
    // -------------------------------------------------- vista Studio (kit) --

    /// Colección de la app → colección del modelo de la vista.
    fn studio_collection(&self) -> studio_model::Collection {
        if let Some(pid) = self.active_playlist {
            return studio_model::Collection::Playlist(pid);
        }
        match self.collection {
            Collection::All => studio_model::Collection::All,
            Collection::Unanalyzed => studio_model::Collection::Unanalyzed,
            Collection::Duplicates => studio_model::Collection::Duplicates,
        }
    }

    /// Construye el `Model` de la vista desde el estado REAL: filas visibles
    /// con IDs reales, player desde los decks, playlists, dispositivos y
    /// contadores. Sin fabricar filas con `library_count`.
    fn build_studio_model(&mut self) -> studio_model::Model {
        // filas de la vista actual (cap 2000: la tabla del kit no virtualiza)
        if self.collection != Collection::Duplicates {
            let want = (self.total.max(0) as usize).min(2000);
            if want > 0 {
                self.ensure_rows((0, want));
            }
        }
        // now-playing: el detalle al día para métricas/waveform/cues
        let now_id = self.now_playing_id();
        if let Some(id) = now_id {
            if self.detail.as_ref().map(|d| d.id) != Some(id) {
                self.load_detail(id);
            }
        }
        let tracks: Vec<studio_model::Track> = self
            .rows
            .values()
            .map(|r| studio_model::Track {
                id: r.id,
                title: r.title.clone(),
                artist: r.artist.clone(),
                key: r.key_camelot.as_deref().and_then(|k| k.parse().ok()),
                // BPM: precisión interna intacta — f64→f32 solo al presentar
                bpm: r.bpm.map(|b| b as f32),
                energy: r.energy.map(|e| e.round().clamp(1.0, 10.0) as u8),
                rating: r.rating,
                duration_seconds: Some(r.duration_s as f32),
            })
            .collect();
        let playlists: Vec<studio_model::Playlist> = self
            .playlists
            .iter()
            .map(|pl| studio_model::Playlist {
                id: pl.id,
                name: pl.name.clone(),
                count: self.playlist_counts.get(&pl.id).copied().unwrap_or(0) as usize,
                kind: pl.kind.clone(),
                parent: pl.parent_id,
            })
            .collect();

        // player desde el deck activo
        let deck = self
            .player
            .as_ref()
            .map(|p| p.decks[self.active_deck].clone());
        // TODOS los datos del detalle se leen primero a locales (id, Strings,
        // Options) para que el auto-heal pueda tomar &mut self.detail después.
        let full_id = self
            .detail
            .as_ref()
            .filter(|d| Some(d.id) == now_id)
            .map(|f| f.id);
        let analysis_duration = self
            .detail
            .as_ref()
            .filter(|d| Some(d.id) == now_id)
            .map(|f| f.analysis.duration_s)
            .unwrap_or(0.0);
        let (deck_loaded, position, playing, duration) = match (&deck, analysis_duration) {
            (Some(d), db_duration) if db_duration > 0.0 => {
                let loaded = self
                    .detail
                    .as_ref()
                    .filter(|x| Some(x.id) == now_id)
                    .map(|f| d.path() == f.meta.path)
                    .unwrap_or(false);
                let duration = if loaded { d.duration_s() } else { db_duration };
                (
                    loaded,
                    if loaded { Some(d.position_s()) } else { None },
                    loaded && d.is_playing(),
                    duration,
                )
            }
            _ => (false, None, false, 0.0),
        };
        let pitch_percent = deck
            .as_ref()
            .map(|d| ((2.0f32.powf(d.pitch() / 12.0)) - 1.0) * 100.0)
            .unwrap_or(0.0);
        let volume = deck.as_ref().map(|d| d.volume()).unwrap_or(0.8);
        // auto-heal: duración real del deck vs análisis (>0.5 s) → corregir BD
        if deck_loaded {
            if let Some(id_fix) = now_id {
                if (duration - analysis_duration).abs() > 0.5 {
                    if let Ok(db) = self.db.lock() {
                        if db.set_duration(id_fix, duration).is_ok() {
                            eprintln!(
                                "[heal] duración corregida id={id_fix}: {analysis_duration:.2} → {duration:.2}"
                            );
                        }
                    }
                    if let Some(f) = self.detail.as_mut() {
                        if Some(f.id) == now_id {
                            f.analysis.duration_s = duration;
                        }
                    }
                }
            }
        }
        // player track desde el detalle (ya saneado)
        let player_track = self
            .detail
            .as_ref()
            .filter(|f| Some(f.id) == full_id)
            .map(|f| studio_model::Track {
                id: f.id,
                title: {
                    let t = &f.meta.title;
                    if t.is_empty() {
                        "(sin título)".into()
                    } else {
                        t.clone()
                    }
                },
                artist: f.meta.artist.clone(),
                key: f
                    .analysis
                    .key_camelot
                    .as_deref()
                    .and_then(|k| k.parse().ok()),
                bpm: f.analysis.tempo.map(|b| b as f32),
                energy: f.analysis.energy.map(|e| e.round().clamp(1.0, 10.0) as u8),
                rating: f.rating,
                duration_seconds: Some(duration as f32),
            });
        let cues: Vec<studio_model::Cue> = self
            .detail
            .as_ref()
            .filter(|f| Some(f.id) == full_id)
            .map(|f| {
                f.analysis
                    .cues
                    .iter()
                    .enumerate()
                    .map(|(i, c)| studio_model::Cue {
                        label: crate::theme::hotcue_letter(i),
                        seconds: c.time_s as f32,
                        color: crate::theme::hotcue_color(i),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let format = self
            .detail
            .as_ref()
            .filter(|f| Some(f.id) == full_id)
            .map(|f| {
                std::path::Path::new(&f.meta.path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_uppercase())
                    .unwrap_or_else(|| "—".into())
            })
            .unwrap_or_default();
        let key_confidence = self
            .detail
            .as_ref()
            .filter(|f| Some(f.id) == full_id)
            .and_then(|f| f.analysis.key_confidence)
            .map(|c| c as f32);
        let lufs = self
            .detail
            .as_ref()
            .filter(|f| Some(f.id) == full_id)
            .and_then(|f| f.analysis.lufs)
            .map(|l| l as f32);
        let clips = self
            .detail
            .as_ref()
            .filter(|f| Some(f.id) == full_id)
            .and_then(|f| f.analysis.clipped_peak_count)
            .unwrap_or(0)
            .max(0) as usize;

        let (library_count, unanalyzed_count) = self
            .sidebar_stats
            .map(|s| (s.total.max(0) as usize, s.pending.max(0) as usize))
            .unwrap_or((self.total.max(0) as usize, 0));
        let (queue_length, analysis, current_analyzing) = {
            let qs = self.pool.state.lock().unwrap();
            let busy = !qs.queue.is_empty() || qs.running > 0;
            let progress = busy.then(|| studio_model::AnalysisProgress {
                done: qs.done_session,
                total: qs
                    .session_total
                    .max(qs.done_session + qs.queue.len() as u64 + qs.running as u64),
            });
            // pista EN análisis (la última arrancada) para el status bar
            // rotativo — file_stem corto, listo para pintar
            let current = qs
                .running_items
                .last()
                .map(|(_, path)| crate::studio_view::short_analyzing_title(path));
            (qs.queue.len() + qs.running, progress, current)
        };
        let device_labels: Vec<String> = self
            .usb_drives
            .iter()
            .map(|d| {
                let mut s = format!("{}:", d.letter_or_name);
                if !d.label.is_empty() {
                    s.push_str(&format!(" · {}", d.label));
                }
                s.push_str(&format!(" · {:.1} GB", d.free_gb));
                s
            })
            .collect();

        studio_model::Model {
            workspace_name: "Smart Mix".into(),
            library_count,
            unanalyzed_count,
            tracks,
            playlists,
            player: studio_model::Player {
                track: player_track,
                deck: if self.active_deck == 0 {
                    studio_model::Deck::A
                } else {
                    studio_model::Deck::B
                },
                position_seconds: position.unwrap_or(0.0) as f32,
                playing,
                pitch_percent,
                pitch_range: match self.pitch_range_pct {
                    p if p <= 6.0 => studio_model::PitchRange::Six,
                    p if p <= 16.0 => studio_model::PitchRange::Sixteen,
                    _ => studio_model::PitchRange::Wide,
                },
                wide_pitch_bounds: Some((-50.0, 50.0)),
                volume,
                key_confidence,
                lufs,
                clips,
                format,
                cues,
            },
            queue_length,
            analysis,
            current_analyzing,
            device_labels,
            status: self.status_msg.clone(),
        }
    }

    /// Un frame de la vista Studio: sincroniza estado app→vista, construye
    /// el Model, adapta el waveform EXISTENTE y devuelve las intenciones.
    fn run_studio_view(
        &mut self,
        ctx: &egui::Context,
        draw_library: bool,
    ) -> Vec<studio_model::Action> {
        let compat = self.compat_key.as_deref().and_then(|k| k.parse().ok());
        self.studio_ui
            .sync_from_app(self.studio_collection(), compat);
        let model = self.build_studio_model();

        // zoom ligado a (pista, duración×20): ventana completa al cambiar de
        // pista O cuando la duración crece (deck completa la carga async)
        let now_id = self.now_playing_id();
        let duration = model
            .player
            .track
            .as_ref()
            .and_then(|t| t.duration_seconds)
            .unwrap_or(0.0)
            .max(0.001) as f64;
        let zkey = (now_id, (duration * 20.0).round() as i64);
        if self.wave_zoom_key != zkey {
            self.wave_zoom = (0.0, duration.max(0.001));
            self.wave_zoom_key = zkey;
        }

        // adaptador del renderer EXISTENTE (widgets::WaveView) — el waveform
        // SIEMPRE sobre su lienzo propio #0B0B0D (theme::WAVE_BG)
        let position_s = if model.player.playing || model.player.position_seconds > 0.0 {
            Some(model.player.position_seconds as f64)
        } else {
            None
        };
        let SmartMixApp {
            studio_ui,
            detail,
            wave_zoom,
            covers,
            cover_worker,
            ..
        } = self;
        let full = detail.as_ref().filter(|d| Some(d.id) == now_id);
        let wave: &[f32] = full.map(|f| f.analysis.waveform.as_slice()).unwrap_or(&[]);
        let bands: Option<&[f32]> = full.and_then(|f| f.analysis.waveform_bands.as_deref());
        let cues: Vec<f64> = full
            .map(|f| f.analysis.cues.iter().map(|c| c.time_s).collect())
            .unwrap_or_default();
        let segments: &[sm_model::EnergySegment] =
            full.map(|f| f.analysis.segments.as_slice()).unwrap_or(&[]);
        let mut wave_view = DeckWaveView {
            wave,
            bands,
            duration_s: duration.max(0.001),
            position_s,
            cues: &cues,
            segments,
            zoom: wave_zoom,
        };
        let lang = self.prefs.language.clone();
        // portadas de la tabla: caché LRU del cover_pipeline + worker de
        // decode (hilo aparte); el hilo UI solo sube texturas en update()
        let mut studio_covers = StudioCovers {
            cache: covers,
            worker: cover_worker.as_ref(),
        };
        let actions = studio_ui.show(
            ctx,
            &lang,
            &model,
            &mut wave_view,
            &mut studio_covers,
            draw_library,
        );
        if covers.has_queued() {
            ctx.request_repaint(); // despierta cuando llegue la miniatura
        }
        actions
    }

    /// Despacha las intenciones de la vista a los métodos EXISTENTES.
    fn dispatch_studio_actions(&mut self, ctx: &egui::Context, actions: Vec<studio_model::Action>) {
        use studio_model::{Action as A, Collection as SC, PitchRange};
        for action in actions {
            match action {
                A::Import => self.import_folder(),
                A::Analyze(ids) => self.analyze_ids(ids),
                A::ReanalyzeAll => self.reanalyze_library(),
                A::Export(ids) => {
                    if ids.is_empty() {
                        self.status_msg = "Exportando biblioteca…".into();
                    }
                    eprintln!("[studio] exportar: {} pista(s)", ids.len());
                    self.show_export = true;
                }
                A::SelectTrack(id) => self.select(id),
                A::LoadAndPlay(id) => {
                    self.select(id);
                    self.deck_play(self.active_deck);
                }
                A::SetCollection(c) => {
                    match c {
                        SC::All => {
                            self.collection = Collection::All;
                            self.active_playlist = None;
                        }
                        SC::Unanalyzed => {
                            self.collection = Collection::Unanalyzed;
                            self.active_playlist = None;
                        }
                        SC::Duplicates => {
                            self.collection = Collection::Duplicates;
                            self.active_playlist = None;
                        }
                        SC::Playlist(pid) => {
                            self.active_playlist = Some(pid);
                            self.collection = Collection::All;
                            self.manual_order = false;
                        }
                    }
                    self.refresh();
                }
                A::SetCompatibility(key) => {
                    self.compat_key = key.map(|k| k.to_string());
                    self.refresh();
                }
                A::SetSearch(text) => {
                    self.search = text;
                    self.refresh();
                }
                A::TogglePlayback => self.studio_toggle_playback(),
                A::Seek(t) => self.studio_seek(t as f64),
                A::SetDeck(deck) => {
                    self.active_deck = if deck == studio_model::Deck::A { 0 } else { 1 };
                }
                A::SetVolume(v) => {
                    if let Some(p) = &self.player {
                        *p.decks[self.active_deck].current().volume.write() = v;
                    }
                }
                A::SetPitch(pct) => {
                    let snapped = if pct.abs() < 0.15 { 0.0 } else { pct };
                    let semis = 12.0 * (1.0 + snapped / 100.0).log2();
                    if let Some(p) = &self.player {
                        *p.decks[self.active_deck].current().pitch_semis.write() = semis;
                    }
                }
                A::SetPitchRange(range) => {
                    self.pitch_range_pct = match range {
                        PitchRange::Six => 6.0,
                        PitchRange::Sixteen => 16.0,
                        PitchRange::Wide => 50.0,
                    };
                }
                A::Cue(letter) => {
                    if let Some(f) = &self.detail {
                        // cue de DJ real (CDJ): saltar a la posición y SONAR
                        // desde ahí — si ya sonaba sigue, si estaba en pausa
                        // arranca. NUNCA pausa (el toggle antiguo pausaba).
                        if let Some(t) = cue_time_by_letter(&f.analysis.cues, letter) {
                            self.studio_seek(t);
                            self.studio_play();
                        }
                    }
                }
                A::AddCue => self.add_cue_at_playhead(),
                A::EditCues => {
                    self.detail_view = self.now_playing_id().or(self.selected_id);
                }
                A::CancelAnalysis => {
                    let mut qs = self.pool.state.lock().unwrap();
                    let n = qs.queue.len();
                    qs.queue.clear();
                    drop(qs);
                    eprintln!("[feature] CANCELAR análisis: {n} pendientes descartados");
                    self.status_msg = format!("Análisis cancelado ({n} pendientes)");
                }
                A::Rate { track_id, rating } => {
                    if let Ok(db) = self.db.lock() {
                        if db.set_rating(track_id, rating).is_ok() {
                            eprintln!("[f26] studio rating id={track_id} -> {rating}");
                        }
                    }
                    if let Some(f) = self.detail.as_mut() {
                        if f.id == track_id {
                            f.rating = rating;
                        }
                    }
                    if let Some(row) = self.rows.values_mut().find(|r| r.id == track_id) {
                        row.rating = rating;
                    }
                }
                A::ApplyTags { track_ids, patch } => {
                    self.apply_tag_patch(track_ids, patch);
                }
                A::CreatePlaylist(name) => {
                    if let Ok(db) = self.db.lock() {
                        if db.create_playlist(&name, "manual", None).is_ok() {
                            self.status_msg = format!("Playlist \"{name}\" creada");
                        }
                    }
                    self.refresh();
                }
                A::AddToPlaylist {
                    playlist_id,
                    track_ids,
                } => {
                    let n = track_ids.len();
                    if let Ok(db) = self.db.lock() {
                        for id in &track_ids {
                            let _ = db.add_track_to_playlist(playlist_id, *id);
                        }
                    }
                    self.status_msg = format!("{n} pista(s) agregada(s) a la playlist");
                    self.refresh();
                }
                A::ResetAnalysisFull => {
                    // dev: usuario nuevo — wipe TOTAL (cues/waveform/lufs/cover
                    // incluidos) + re-análisis inmediato, UN click
                    if let Ok(db) = self.db.lock() {
                        let n = db.reset_analysis_full().unwrap_or(0);
                        eprintln!("[feature] reinicio total: {n} pistas a pending");
                        self.status_msg = format!("Análisis de {n} pistas vaciado; reanalizando…");
                    }
                    self.refresh();
                    self.analyze_pending();
                }
                A::RefreshDevices => {
                    // fix usb-scan: el PowerShell tarda ~1.5 s — hilo de fondo,
                    // JAMÁS en el hilo UI (congelaba la ventana). El resultado
                    // llega por update() al drenar device_scan.
                    self.spawn_device_scan();
                    self.status_msg = "Buscando dispositivos…".into();
                }
                A::OpenNewPlaylistDialog { parent } => {
                    self.new_playlist_parent = parent;
                    self.new_playlist_name.clear();
                    self.new_playlist_smart = false;
                    self.show_new_playlist = true;
                }
                A::RenamePlaylist(id) => {
                    if let Some(pl) = self.playlists.iter().find(|p| p.id == id) {
                        self.new_playlist_name = pl.name.clone();
                    }
                    self.rename_playlist_id = Some(id);
                    self.show_rename_playlist = true;
                }
                A::DeletePlaylist(id) => {
                    if let Ok(db) = self.db.lock() {
                        let _ = db.delete_playlist(id);
                    }
                    if self.active_playlist == Some(id) {
                        self.active_playlist = None;
                        self.manual_order = false;
                    }
                    self.refresh();
                }
                A::ToggleManualOrder => {
                    self.manual_order = !self.manual_order;
                    if self.manual_order {
                        self.reload_manual_order();
                    }
                }
                A::Device { index, op } => self.studio_device_op(index, op),
                A::OpenSettings => {
                    self.settings_view = true;
                    self.settings_tab = 0;
                }
                A::OpenAbout => self.show_about = true,
                // ---- menú contextual de fila (click derecho) ----
                A::LoadTrack(id) => {
                    // Cargar pista: al reproductor SIN reproducir (la fila ya
                    // queda marcada ▶; el audio arranca con Play/Preview).
                    self.select(id);
                    self.deck_load(self.active_deck);
                }
                A::ViewDetails(id) => {
                    self.select(id);
                    self.detail_view = Some(id);
                }
                A::RevealInExplorer(id) => {
                    match self.track_field_value(id, studio_model::TrackField::Path) {
                        Some(path) => reveal_in_explorer(&path),
                        None => self.status_msg = "Sin ruta en la biblioteca para revelar.".into(),
                    }
                }
                A::CopyTrackField { id, field } => match self.track_field_value(id, field) {
                    Some(value) => {
                        ctx.output_mut(|output| output.copied_text = value);
                        self.status_msg = "Copiado al portapapeles.".into();
                    }
                    None => self.status_msg = "Nada que copiar.".into(),
                },
                A::RemoveFromLibrary(ids) => {
                    // SOLO la intención: la ventana de confirmación existente
                    // (windows.rs) muestra el conteo y NUNCA borra archivos.
                    self.confirm_remove = Some(ids);
                }
            }
        }
        // selección de la vista → estado de la app (tag editor, hooks)
        let checked = self.studio_ui.checked_tracks();
        self.selected_set = checked.into_iter().collect();
    }

    /// Operaciones USB reales sobre un dispositivo del sidebar (menú cont.).
    fn studio_device_op(&mut self, index: usize, op: studio_model::DeviceOp) {
        use studio_model::DeviceOp;
        let Some(drive) = self.usb_drives.get(index).cloned() else {
            return;
        };
        match op {
            DeviceOp::Backup => {
                if let Some(dest) = rfd::FileDialog::new()
                    .set_title("Destino del backup USB")
                    .pick_folder()
                {
                    self.usb_backup_log.clear();
                    self.start_usb_backup(drive.root.clone(), dest);
                }
            }
            DeviceOp::SafeFormat => {
                let dev = drive
                    .root
                    .to_string_lossy()
                    .trim_end_matches(['/', '\\'])
                    .to_string();
                self.sf_drive = Some((drive.root.clone(), dev, drive.size_gb));
                self.sf_dest = None;
                self.sf_phase = SfPhase::Idle;
                self.sf_token.clear();
                self.sf_cmd.clear();
                self.sf_tok_a.clear();
                self.sf_tok_b.clear();
                self.sf_log.clear();
                self.sf_op = None;
                self.show_safe_format = true;
            }
        }
    }

    /// ANALIZAR ids concretos (vacío = pendientes, igual que el topbar).
    fn analyze_ids(&mut self, ids: Vec<i64>) {
        if ids.is_empty() {
            self.analyze_pending();
            return;
        }
        if let Ok(mut db) = self.db.lock() {
            let mut pairs = Vec::new();
            for id in &ids {
                if let Ok(Some(f)) = db.track_full(*id) {
                    let _ = db.set_analysis_status(*id, AnalysisStatus::Pending);
                    pairs.push((*id, f.meta.path.clone()));
                }
            }
            let n = pairs.len();
            self.pool.enqueue(pairs);
            self.status_msg = format!("Analizando {n} pistas…");
        }
    }

    /// Aplica un TagPatch del drawer con los setters REALES: título/artista
    /// vía track_full+upsert (conserva el análisis, F-11), key/bpm vía
    /// override_key_bpm, rating vía set_rating, energía vía set_energy
    /// (cierre). Solo campos modificados.
    fn apply_tag_patch(&mut self, ids: Vec<i64>, patch: studio_model::TagPatch) {
        use studio_model::TagPatch;
        let TagPatch {
            title,
            artist,
            key,
            bpm,
            energy,
            rating,
        } = patch;
        let mut changes = 0usize;
        if let Ok(mut db) = self.db.lock() {
            for id in &ids {
                if key.is_some() || bpm.is_some() {
                    let k = key.map(|k| k.to_string());
                    let b = bpm.map(|b| b as f64);
                    if db.override_key_bpm(*id, k.as_deref(), b).is_ok() {
                        changes += 1;
                    }
                }
                if let Some(energy) = energy {
                    if db.set_energy(*id, Some(energy)).is_ok() {
                        changes += 1;
                    }
                }
                if let Some(rating) = rating {
                    if db.set_rating(*id, rating).is_ok() {
                        changes += 1;
                    }
                }
                if title.is_some() || artist.is_some() {
                    if let Ok(Some(mut full)) = db.track_full(*id) {
                        if let Some(t) = &title {
                            full.meta.title = t.clone();
                        }
                        if let Some(a) = &artist {
                            full.meta.artist = a.clone();
                        }
                        // mtime igual → el análisis se conserva (F-11)
                        if db.upsert_track(&full.meta).is_ok() {
                            changes += 1;
                        }
                    }
                }
            }
        }
        eprintln!(
            "[studio] tags: {changes} cambio(s) aplicados a {} pista(s)",
            ids.len()
        );
        self.status_msg = format!("{changes} cambio(s) en {} pista(s)", ids.len());
        self.refresh();
        if let Some(id) = self.selected_id {
            if ids.contains(&id) {
                self.load_detail(id);
            }
        }
    }

    /// Cue en el playhead con snap a beat (puerto del now-playing viejo).
    fn add_cue_at_playhead(&mut self) {
        let Some(full) = self.detail.clone() else {
            return;
        };
        let tpos = self
            .player()
            .map(|p| p.decks[self.active_deck].position_s())
            .unwrap_or(0.0);
        let mut cues = full.analysis.cues.clone();
        let mut snapped = tpos;
        if let Some(bg) = &full.analysis.beatgrid {
            snapped = bg.beat_time(bg.beat_index_near(tpos));
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
        eprintln!("[studio] cue añadida en {snapped:.2}s (id={})", full.id);
        self.load_detail(full.id);
    }

    /// Seek del transporte/waveform: carga la pista en el deck si hace falta.
    fn studio_seek(&mut self, t: f64) {
        let Some(player) = &self.player else { return };
        let deck = player.decks[self.active_deck].clone();
        let path = self.detail.as_ref().map(|f| (f.id, f.meta.path.clone()));
        if let Some((id, path)) = path {
            if !deck.is_loaded() || deck.path() != path {
                deck.load(&path);
                self.deck_track_id[self.active_deck] = Some(id);
            }
        }
        deck.seek(t);
        // doble garantía: frame tras el apply del pump (~5 ms)
        self.egui_ctx
            .request_repaint_after(std::time::Duration::from_millis(16));
    }

    /// Play/pausa del transporte: carga la pista seleccionada si el deck
    /// está vacío (mismo comportamiento que el now-playing viejo).
    fn studio_toggle_playback(&mut self) {
        let Some(player) = &self.player else { return };
        let deck = player.decks[self.active_deck].clone();
        if deck.is_playing() {
            deck.pause();
            return;
        }
        if !deck.is_loaded() {
            let Some(full) = self.detail.as_ref() else {
                return;
            };
            let path = full.meta.path.clone();
            deck.load(&path);
            self.deck_track_id[self.active_deck] = Some(full.id);
        }
        deck.play();
    }

    /// Play GARANTIZADO (cue): a diferencia de `studio_toggle_playback`,
    /// NUNCA pausa — si ya sonaba sigue sonando, si estaba en pausa arranca
    /// (comportamiento CDJ: pulsar un cue siempre reproduce desde él).
    /// `DeckCmd::Play` es idempotente (playing=true, no reinicia el cursor).
    fn studio_play(&mut self) {
        let Some(player) = &self.player else { return };
        let deck = player.decks[self.active_deck].clone();
        if !deck.is_loaded() {
            let Some(full) = self.detail.as_ref() else {
                return;
            };
            let path = full.meta.path.clone();
            deck.load(&path);
            self.deck_track_id[self.active_deck] = Some(full.id);
        }
        deck.play();
    }

    pub(crate) fn deck_play(&mut self, deck: usize) {
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

    /// Menú contextual — Cargar pista: al reproductor SIN reproducir.
    /// Espejo exacto de `deck_play` sin `.play()`: mismo deck activo y
    /// mismo `deck_track_id`; el audio arranca con Play/Preview.
    pub(crate) fn deck_load(&mut self, deck: usize) {
        eprintln!("[load] deck={deck} sel={:?}", self.selected_id);
        let Some(id) = self.selected_id else { return };
        let Some(full) = &self.detail else { return };
        let Some(player) = &self.player else { return };
        let path = full.meta.path.clone();
        player.decks[deck].load(&path);
        self.deck_track_id[deck] = Some(id);
        self.active_deck = deck;
    }

    /// Menú contextual — Copiar/Revelar: resuelve título/artista/ruta de la
    /// pista DESDE LA BD (la tabla del studio no carga la ruta por frame).
    fn track_field_value(&self, id: i64, field: studio_model::TrackField) -> Option<String> {
        let full = self
            .db
            .lock()
            .ok()
            .and_then(|db| db.track_full(id).unwrap_or(None))?;
        Some(match field {
            studio_model::TrackField::Title => full.meta.title,
            studio_model::TrackField::Artist => full.meta.artist,
            studio_model::TrackField::Path => full.meta.path,
        })
    }

    pub(crate) fn now_playing_id(&self) -> Option<i64> {
        self.deck_track_id[self.active_deck].or(self.selected_id)
    }

    /// Safe Format paso 1: dry-run (scan + validaciones + plan; NADA se escribe).
    fn sf_start_dry(&mut self) {
        let Some((root, dev, size_gb)) = self.sf_drive.clone() else {
            return;
        };
        let Some(dest) = self.sf_dest.clone() else {
            return;
        };
        let label = self.sf_label.clone();
        let state = Arc::new(Mutex::new(UsbOpState::default()));
        self.sf_op = Some(state.clone());
        self.sf_phase = SfPhase::DryRunning;
        self.sf_log.clear();
        self.sf_token.clear();
        self.sf_cmd.clear();
        std::thread::spawn(move || {
            let st = state.clone();
            let mut prog = move |msg: &str, pct: f64| {
                if let Ok(mut s) = st.lock() {
                    s.lines.push(format!("[{:3.0}%] {msg}", pct * 100.0));
                }
            };
            let opts = sm_usb::WorkflowOptions {
                mode: None,
                dry_run: true,
                os_name: Some("windows"),
                format_device: Some(&dev),
                label: &label,
                size_gb: Some(size_gb),
                confirmations: Default::default(),
                format_executor: None,
                restore_after_format: false,
                restore_root: None,
            };
            match sm_usb::run_workflow(&root, &dest, opts, Some(&mut prog)) {
                Ok(res) => {
                    if let Ok(mut s) = state.lock() {
                        s.lines.push(format!(
                            "DRY-RUN OK: {} archivos, {} avisos FAT32",
                            res.scan.files.len(),
                            res.fat32_warnings.len()
                        ));
                        s.dry_result = Some(Box::new(res));
                        s.done = true;
                    }
                }
                Err(e) => {
                    if let Ok(mut s) = state.lock() {
                        s.lines.push(format!("✗ dry-run: {e}"));
                        s.done = true;
                    }
                }
            }
        });
    }

    /// fix usb-scan: lanza el scan de unidades USB en un hilo de fondo (el
    /// PowerShell de enumeración tarda ~1.5 s medido — en el hilo UI
    /// congelaba la ventana). Idempotente: ignora llamadas si ya corre.
    /// El hilo deja el resultado en el Mutex y pide repaint; update() drena.
    pub(crate) fn spawn_device_scan(&mut self) {
        if self.device_scan.is_some() {
            return;
        }
        let state: Arc<Mutex<Option<Vec<UsbDrive>>>> = Arc::new(Mutex::new(None));
        self.device_scan = Some(state.clone());
        let repaint = self.ctx_repaint.clone();
        let res = std::thread::Builder::new()
            .name("usb-scan".to_string())
            .spawn(move || {
                let t0 = std::time::Instant::now();
                let drives = scan_drives();
                eprintln!(
                    "[usb-scan] hilo terminó en {:?} ({} unidad/es)",
                    t0.elapsed(),
                    drives.len()
                );
                if let Ok(mut s) = state.lock() {
                    *s = Some(drives);
                }
                repaint(); // despierta la UI para drenar el resultado
            });
        if res.is_err() {
            // sin hilo no hay resultado que drenar: volver a idle
            self.device_scan = None;
        }
    }

    /// Safe Format paso 2: ejecución real (backup verificado → formato → restore).
    /// Solo se llama con doble confirmación validada en la UI; la librería
    /// re-verifica el backup ANTES de invocar el executor.
    fn sf_start_run(&mut self) {
        let Some((root, dev, size_gb)) = self.sf_drive.clone() else {
            return;
        };
        let Some(dest) = self.sf_dest.clone() else {
            return;
        };
        let label = self.sf_label.clone();
        let toks = vec![self.sf_tok_a.clone(), self.sf_tok_b.clone()];
        let state = Arc::new(Mutex::new(UsbOpState::default()));
        self.sf_op = Some(state.clone());
        self.sf_phase = SfPhase::Running;
        self.sf_log.clear();
        std::thread::spawn(move || {
            let st = state.clone();
            let mut prog = move |msg: &str, pct: f64| {
                if let Ok(mut s) = st.lock() {
                    s.lines.push(format!("[{:3.0}%] {msg}", pct * 100.0));
                }
            };
            // Executor: EJECUTA el comando de formato preparado. Vive en la
            // app (la capa sm-usb jamás ejecuta nada por diseño).
            let st2 = state.clone();
            let mut executor = move |plan: &sm_usb::FormatPlan| -> bool {
                if plan.shell == "bash" {
                    if let Ok(mut s) = st2.lock() {
                        s.lines.push("✗ shell bash no soportado en Windows".into());
                    }
                    return false;
                }
                if let Ok(mut s) = st2.lock() {
                    s.lines.push(format!("⚡ EJECUTANDO: {}", plan.command));
                }
                match std::process::Command::new("cmd")
                    .args(["/C", &plan.command])
                    .output()
                {
                    Ok(out) => {
                        let ok = out.status.success();
                        if !ok {
                            let err = String::from_utf8_lossy(&out.stderr);
                            if let Ok(mut s) = st2.lock() {
                                s.lines.push(format!("✗ formato falló: {err}"));
                            }
                        }
                        ok
                    }
                    Err(e) => {
                        if let Ok(mut s) = st2.lock() {
                            s.lines.push(format!("✗ spawn: {e}"));
                        }
                        false
                    }
                }
            };
            let opts = sm_usb::WorkflowOptions {
                mode: None,
                dry_run: false,
                os_name: Some("windows"),
                format_device: Some(&dev),
                label: &label,
                size_gb: Some(size_gb),
                confirmations: sm_usb::Confirmations::Tokens(toks),
                format_executor: Some(&mut executor),
                restore_after_format: true,
                restore_root: None,
            };
            match sm_usb::run_workflow(&root, &dest, opts, Some(&mut prog)) {
                Ok(res) => {
                    let restore_ok = res.restore.as_ref().map(|r| r.ok()).unwrap_or(false);
                    if let Ok(mut s) = state.lock() {
                        s.lines.push(format!(
                            "FIN: backup {}verificado · restore {}",
                            if res.backup_verified { "" } else { "NO " },
                            if restore_ok {
                                "OK"
                            } else {
                                "sin datos/pendiente"
                            },
                        ));
                        s.done = true;
                    }
                }
                Err(e) => {
                    if let Ok(mut s) = state.lock() {
                        s.lines.push(format!("✗ workflow: {e} (la unidad NO se formateó si el error es previo al paso de formato)"));
                        s.done = true;
                    }
                }
            }
        });
    }

    fn do_export(&mut self, dir: &std::path::Path, usb: Option<&UsbDrive>) -> String {
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
        if let Some(d) = usb {
            // F-25: nombres > 31 caracteres en FAT32 — equipos DJ leyendo la
            // unidad pueden truncarlos u omitirlos (aviso, no bloqueo)
            let names: Vec<String> = own.iter().map(|f| f.path.clone()).collect();
            for w in sm_usb::usb_export_warnings(d.is_fat32(), &names) {
                log.push_str(
                    &texts::t(&self.prefs.language, "export.warn.fat32")
                        .replace("{path}", &w.relpath),
                );
                log.push('\n');
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
        if self.prefs.export_serato_cues {
            // F-32: hot cues como GEOB "Serato Markers2" DENTRO de los
            // archivos (el canal que lee Serato DJ) — paridad byte-exacta
            // con core/tagger/serato.py; conserva el resto de frames.
            let mut okn = 0usize;
            let mut errn = 0usize;
            let mut skipn = 0usize;
            for f in &own {
                if f.cues.is_empty() {
                    skipn += 1;
                    continue;
                }
                match sm_export::write_serato_markers(
                    &f.path,
                    &sm_export::cues_from_points(&f.cues),
                    false,
                ) {
                    Ok(_) => okn += 1,
                    Err(_) => errn += 1,
                }
            }
            log.push_str(&format!(
                "✓ Serato cues GEOB: {okn} archivos, {skipn} sin cues, {errn} errores\n"
            ));
        }
        if self.prefs.export_engine {
            match sm_export::write_engine_dj_library(&entries, dir) {
                Ok(_) => log.push_str(
                    "✓ Engine DJ (EngineLibrary)
",
                ),
                Err(e) => log.push_str(&format!(
                    "✗ Engine DJ: {e}
"
                )),
            }
        }
        if self.prefs.export_vdj {
            // F-36: VirtualDJ lee M3U (recargar tags en la app) — ruta canónica
            let vdj_dir = dir.join("VirtualDJ").join("Playlists");
            let _ = std::fs::create_dir_all(&vdj_dir);
            match sm_export::write_m3u(
                &entries
                    .iter()
                    .map(|e| e.path.to_string())
                    .collect::<Vec<_>>(),
                &vdj_dir.join("smart-mix.m3u"),
            ) {
                Ok(_) => log.push_str(
                    "✓ VirtualDJ playlist (recargar tags en VirtualDJ)
",
                ),
                Err(e) => log.push_str(&format!(
                    "✗ VirtualDJ: {e}
"
                )),
            }
        }
        if self.prefs.write_tags_id3 {
            let mut ok = 0usize;
            let mut err = 0usize;
            let mut first_err = String::new();
            for f in &own {
                if let (Some(k), Some(b)) = (&f.key_camelot, f.bpm) {
                    match sm_export::write_id3_tags(
                        &f.path,
                        k,
                        b,
                        f.energy,
                        &self.prefs.id3_comment_format,
                    ) {
                        Ok(_) => ok += 1,
                        Err(e) => {
                            err += 1;
                            if first_err.is_empty() {
                                first_err = format!("{e}");
                            }
                        }
                    }
                }
            }
            log.push_str(&format!(
                "✓ Tags ID3 escritos: {ok} pistas
"
            ));
            if err > 0 {
                log.push_str(&format!(
                    "✗ Tags: {err} errores (primero: {first_err})
"
                ));
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
        // QA-F-008: cierre limpio del worker (cancel + notify + join)
        self.cover_worker = None;
        if let Ok(db) = self.db.lock() {
            let _ = db.set_preferences(&self.prefs);
        }
    }
}

/// Título de tarjeta Track Detail — Label 11 uppercase (§4).
fn card_title(txt: &str) -> egui::RichText {
    egui::RichText::new(txt)
        .font(crate::theme::font_heading(15.5))
        .color(crate::theme::Theme::default().text_dim)
}

/// Adaptador `WaveformView` del kit → renderer EXISTENTE (widgets::WaveView).
///
/// Regla del kit: la vista reserva el rect y llama a este trait; el renderer
/// (bandas azul/naranja/blanco, zoom, cues, playhead magenta) queda INTACTO y
/// siempre pinta sobre su lienzo propio #0B0B0D (`theme::WAVE_BG`).
struct DeckWaveView<'a> {
    wave: &'a [f32],
    bands: Option<&'a [f32]>,
    duration_s: f64,
    position_s: Option<f64>,
    cues: &'a [f64],
    segments: &'a [sm_model::EnergySegment],
    zoom: &'a mut (f64, f64),
}

impl studio_model::WaveformView for DeckWaveView<'_> {
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        _player: &studio_model::Player,
    ) -> Option<studio_model::Action> {
        let Self {
            wave,
            bands,
            duration_s,
            position_s,
            cues,
            segments,
            zoom,
        } = self;
        let mut seek: Option<f64> = None;
        // El rect ya está reservado por la vista: el hijo pinta dentro sin
        // avanzar el cursor del padre (lección del fader/waveform a 0px).
        let mut child = ui.child_ui_with_id_source(
            rect,
            egui::Layout::top_down(egui::Align::Min),
            "studio_deck_waveview",
        );
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        egui::Frame::none()
            .fill(crate::theme::WAVE_BG)
            .inner_margin(egui::Margin::same(0.0))
            .show(&mut child, |ui| {
                ui.set_min_size(rect.size());
                ui.add(&mut WaveView {
                    zoom: &mut **zoom,
                    wave,
                    bands: *bands,
                    duration_s: *duration_s,
                    position_s: *position_s,
                    cues,
                    segments,
                    height: 100.0, // spec §3: onda ~100px de alto
                    on_seek: &mut |t| seek = Some(t),
                });
            });
        seek.map(|t| studio_model::Action::Seek(t as f32))
    }
}

fn fmt_time(s: f64) -> String {
    let m = (s / 60.0).floor() as i64;
    let sec = (s - m as f64 * 60.0) as i64;
    format!("{m}:{sec:02}")
}

/// Resuelve el tiempo (s) del cue con esa letra de pad A..H dentro de la
/// lista de cues de la pista cargada. Regla CDJ del kit/theme: el orden de
/// la lista ES el orden de los pads (`theme::hotcue_letter(i)` = índice).
pub(crate) fn cue_time_by_letter(cues: &[sm_model::CuePoint], letter: char) -> Option<f64> {
    let index = (letter.to_ascii_uppercase() as u8).wrapping_sub(b'A');
    (index < 8)
        .then(|| cues.get(index as usize))
        .flatten()
        .map(|cue| cue.time_s)
}

/// Menú contextual — Revelar en Explorer: abre la ventana del Explorer con
/// el archivo SELECCIONADO (`/select,`). Proceso despegado, sin bloquear
/// la UI; en otras plataformas queda como no-op registrado.
fn reveal_in_explorer(path: &str) {
    #[cfg(target_os = "windows")]
    let spawned = std::process::Command::new("explorer")
        .arg(format!("/select,{path}"))
        .spawn();
    #[cfg(not(target_os = "windows"))]
    let spawned = {
        let _ = path;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no windows",
        ))
    };
    match spawned {
        Ok(_) => eprintln!("[ctx] revelar en Explorer: {path}"),
        Err(err) => eprintln!("[ctx] no se pudo abrir el Explorer ({err}): {path}"),
    }
}

/// Etiqueta del dropdown de destino USB (F-25): letra, etiqueta del
/// volumen, espacio libre y marca FAT32 cuando aplica.
fn usb_dest_label(d: &crate::usb::UsbDrive) -> String {
    let mut s = format!("{}:", d.letter_or_name);
    if !d.label.is_empty() {
        s.push_str(&format!(" · {}", d.label));
    }
    s.push_str(&format!(" · {:.1} GB libres", d.free_gb));
    if d.is_fat32() {
        s.push_str(" · FAT32");
    }
    s
}

// ---------------------------------------------------------------------------
// Import: escritura por lotes (B4, auditoría)
// ---------------------------------------------------------------------------
// `import_folder` hacía un upsert por archivo, cada uno con SU transacción
// (y su lock de BD). El batching agrupa la escritura en transacciones de
// ~200 pistas manteniendo intacto lo observable: lectura de metadatos y
// repintado por archivo con la cadencia histórica, y si una pista falla el
// resto del lote sigue (una tx fallida NO tira el import).

/// Pistas por transacción de escritura durante el import (B4: "~200").
const IMPORT_TX_CHUNK: usize = 200;
/// Cadencia de repintado del escaneo: una señal cada N archivos leídos,
/// exactamente como antes del batching (era `i % 25`).
const IMPORT_REPAINT_EVERY: usize = 25;

/// ¿El buffer de escaneo pide flush a BD? Al llenarse el lote.
#[inline]
fn import_chunk_full(buffered: usize) -> bool {
    buffered >= IMPORT_TX_CHUNK
}

/// Escribe un lote completo: fast path en UNA transacción; si la tx muere
/// (begin/commit fallidos), cada pista se escribe sola en autocommit —el
/// comportamiento histórico exacto—. Devuelve cuántas quedaron persistidas.
fn import_flush_chunk(db: &LibraryDb, chunk: &[ScannedFile]) -> usize {
    match import_try_batch(db, chunk) {
        Some(ok) => ok,
        None => import_upsert_each(db, chunk),
    }
}

/// Intenta persistir todo el lote en una sola transacción (los métodos de
/// `&self` de `LibraryDb` ejecutan dentro de ella hasta el `commit()`).
/// `Some(ok)` si el commit llegó a buen fin; `None` si la tx murió y el
/// llamador debe reintentar pista a pista.
fn import_try_batch(db: &LibraryDb, chunk: &[ScannedFile]) -> Option<usize> {
    let tx = db.begin().ok()?;
    let mut ok = 0usize;
    // Las pistas que fallen DENTRO de la tx se reintentan solas después del
    // commit: mismo destino que tendrían con tx por pista. (Un error de
    // statement —p. ej. constraint— no aborta la tx de SQLite.)
    let mut retry: Vec<ScannedFile> = Vec::new();
    for f in chunk {
        match db.upsert_track(&f.meta) {
            Ok(id) => {
                if let Some(cover) = &f.cover {
                    let _ = db.set_cover(id, Some(cover.clone()));
                }
                ok += 1;
            }
            Err(e) => {
                tracing::warn!("upsert {}: {e}", f.meta.path);
                retry.push(f.clone());
            }
        }
    }
    match tx.commit() {
        Ok(()) => {
            ok += import_upsert_each(db, &retry);
            Some(ok)
        }
        Err(e) => {
            tracing::warn!("commit del lote ({}) falló: {e}", chunk.len());
            None
        }
    }
}

/// Upsert pista a pista, cada una en su autocommit: réplica exacta del
/// comportamiento pre-batching (un archivo malo solo se salta a sí mismo).
fn import_upsert_each(db: &LibraryDb, chunk: &[ScannedFile]) -> usize {
    let mut ok = 0usize;
    for f in chunk {
        match db.upsert_track(&f.meta) {
            Ok(id) => {
                if let Some(cover) = &f.cover {
                    let _ = db.set_cover(id, Some(cover.clone()));
                }
                ok += 1;
            }
            Err(e) => tracing::warn!("upsert {}: {e}", f.meta.path),
        }
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_refresh_due_throttles_to_once_per_second() {
        let sec = std::time::Duration::from_secs(1);
        // sin pistas terminadas no refresca jamás
        assert!(!analysis_refresh_due(false, sec));
        assert!(!analysis_refresh_due(
            false,
            std::time::Duration::from_secs(30)
        ));
        // con trabajo sucio: antes del throttle no, a partir de 1 s sí
        assert!(!analysis_refresh_due(
            true,
            std::time::Duration::from_millis(999)
        ));
        assert!(analysis_refresh_due(true, sec));
        assert!(analysis_refresh_due(
            true,
            std::time::Duration::from_secs(5)
        ));
        // el throttle es exactamente ANALYSIS_REFRESH_EVERY
        assert_eq!(
            ANALYSIS_REFRESH_EVERY,
            std::time::Duration::from_millis(1000)
        );
    }

    #[test]
    fn etiqueta_destino_usb() {
        let d = crate::usb::UsbDrive {
            letter_or_name: "E".into(),
            root: std::path::PathBuf::from("E:\\"),
            label: "SANDISK".into(),
            size_gb: 64.0,
            free_gb: 29.71,
            serato_crates: vec![],
            has_pioneer: false,
            has_engine: false,
            filesystem: "FAT32".into(),
        };
        assert_eq!(usb_dest_label(&d), "E: · SANDISK · 29.7 GB libres · FAT32");
        let mut d2 = d.clone();
        d2.label = String::new();
        d2.filesystem = "exFAT".into();
        assert_eq!(usb_dest_label(&d2), "E: · 29.7 GB libres");
    }

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

    #[test]
    fn cue_resuelve_letra_por_orden_de_pad() {
        let cue = |t: f64| sm_model::CuePoint {
            time_s: t,
            energy_level: None,
            origin: "auto".into(),
            label: None,
        };
        let cues = vec![cue(0.0), cue(12.5), cue(61.25)];
        // orden de la lista = orden de pads A..H (regla theme::hotcue_letter)
        assert_eq!(cue_time_by_letter(&cues, 'A'), Some(0.0));
        assert_eq!(cue_time_by_letter(&cues, 'B'), Some(12.5));
        assert_eq!(cue_time_by_letter(&cues, 'C'), Some(61.25));
        // pads sin cue asignado → None (la UI no emite seek)
        assert_eq!(cue_time_by_letter(&cues, 'D'), None);
        // minúsculas se normalizan (mismo pad)
        assert_eq!(cue_time_by_letter(&cues, 'b'), Some(12.5));
        // fuera del alfabeto de pads → None (nunca pánicas ni inventa)
        for bad in ['I', 'Z', '1', '?'] {
            assert_eq!(cue_time_by_letter(&cues, bad), None, "letra {bad}");
        }
        assert_eq!(cue_time_by_letter(&[], 'A'), None);
    }

    // -- Import por lotes (B4, auditoría) ------------------------------------

    /// Pista de prueba: path único por índice; `duration_s` inyectable para
    /// provocar el fallo por-pista (NaN → NULL → viola REAL NOT NULL).
    fn scanned(i: usize, duration_s: f64) -> ScannedFile {
        ScannedFile {
            meta: sm_model::TrackMeta {
                path: format!("C:/mix/t{i:03}.mp3"),
                title: format!("Track {i}"),
                artist: "Test".into(),
                album: "Batch".into(),
                genre: String::new(),
                year: None,
                track_no: Some(i as i32),
                duration_s,
                bitrate_kbps: Some(320),
                sample_rate: Some(44_100),
                channels: Some(2),
                format: "mp3".into(),
                file_size: 1_000,
                file_modified_unix: 1_700_000_000,
            },
            cover: None,
        }
    }

    fn pending_count(db: &LibraryDb) -> usize {
        db.track_ids_by_status(AnalysisStatus::Pending, 1_000_000)
            .unwrap()
            .len()
    }

    #[test]
    fn import_chunking_parte_en_lotes_de_200() {
        // contrato del batching (B4): lotes de ~200 por tx y la cadencia de
        // repintado histórica intacta
        assert_eq!(IMPORT_TX_CHUNK, 200);
        assert_eq!(IMPORT_REPAINT_EVERY, 25);
        assert!(!import_chunk_full(0));
        assert!(!import_chunk_full(199));
        assert!(import_chunk_full(200));
        assert!(import_chunk_full(1_000));
    }

    #[test]
    fn import_flush_chunk_persiste_lote_y_cola() {
        let db = LibraryDb::open_in_memory().unwrap();
        // réplica del lazo del hilo de import: flush al llenar el lote y un
        // flush final para la cola que no lo llena (250 = 200 + 50)
        let mut n = 0usize;
        let mut buf: Vec<ScannedFile> = Vec::with_capacity(IMPORT_TX_CHUNK);
        for i in 0..250 {
            let mut f = scanned(i, 180.0);
            if i == 42 {
                f.meta.title = "Uniqu3 title".into();
            }
            buf.push(f);
            if import_chunk_full(buf.len()) {
                n += import_flush_chunk(&db, &buf);
                buf.clear();
            }
        }
        if !buf.is_empty() {
            n += import_flush_chunk(&db, &buf);
        }
        assert_eq!(n, 250, "lote completo + cola persistidos");
        assert_eq!(pending_count(&db), 250);
        // el FTS5 quedó sync tras ejecutar los upserts dentro de las tx
        let hit = db
            .query(&LibraryQuery {
                text: Some("Uniqu3".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(hit.len(), 1, "búsqueda FTS consistente tras el batching");
    }

    #[test]
    fn import_flush_chunk_pista_mala_no_tira_el_lote() {
        let db = LibraryDb::open_in_memory().unwrap();
        // duration_s NaN: SQLite lo convierte a NULL y viola REAL NOT NULL —
        // error de statement que NO aborta la tx del lote
        let chunk = vec![scanned(0, 181.0), scanned(1, f64::NAN), scanned(2, 202.0)];
        assert_eq!(import_flush_chunk(&db, &chunk), 2, "solo cae la inválida");
        assert_eq!(pending_count(&db), 2);
        // y la conexión queda sana para el lote siguiente
        assert_eq!(import_flush_chunk(&db, &[scanned(3, 190.0)]), 1);
        assert_eq!(pending_count(&db), 3);
    }

    #[test]
    fn import_upsert_each_salta_solo_la_pista_mala() {
        // el camino pista-a-pista (fallback si la tx del lote muere) es el
        // comportamiento histórico exacto
        let db = LibraryDb::open_in_memory().unwrap();
        let n = import_upsert_each(&db, &[scanned(0, 1.0), scanned(1, f64::NAN)]);
        assert_eq!(n, 1);
        assert_eq!(pending_count(&db), 1);
    }

    #[test]
    fn import_flush_chunk_duplicado_mismo_path_no_duplica_filas() {
        let db = LibraryDb::open_in_memory().unwrap();
        let mut a = scanned(7, 181.0);
        a.meta.title = "v1".into();
        let mut b = scanned(7, 183.0); // mismo path → rama UPDATE en la misma tx
        b.meta.title = "v2".into();
        let n = import_flush_chunk(&db, &[a, b]);
        assert_eq!(n, 2, "ambas exitosas, igual que con tx por pista");
        assert_eq!(pending_count(&db), 1, "upsert por path UNIQUE: una fila");
    }

    /// Menú contextual — Quitar de la biblioteca: `delete_track` toca SOLO
    /// la base de datos. El archivo en disco queda INTACTO (contrato del
    /// dueño: "SOLO BD, nunca borra archivos").
    #[test]
    fn remove_from_library_never_touches_the_files() {
        let db = LibraryDb::open_in_memory().unwrap();
        // pista cuyo archivo EXISTE de verdad en disco
        let dir = std::env::temp_dir().join("sm_ctx_remove_guard");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("keepme.mp3");
        std::fs::write(&path, b"fake audio bytes").unwrap();
        let mut f = scanned(0, 181.0);
        f.meta.path = path.display().to_string();
        assert_eq!(import_flush_chunk(&db, &[f]), 1);
        let id = db.query(&LibraryQuery::default()).unwrap()[0].id;
        assert!(db.delete_track(id).is_ok(), "la BD acepta el borrado");
        // la fila desapareció de la biblioteca…
        assert!(
            db.track_full(id).unwrap().is_none(),
            "la pista ya no está en la BD"
        );
        // …pero el ARCHIVO sigue en disco, byte a byte
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"fake audio bytes",
            "el archivo de audio NO se toca jamás"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
