//! Pool de análisis: N hilos consumen la cola, analizan con sm-core y
//! persisten con sm-db. La UI recibe eventos por canal y repinta.

use sm_core::models::Models;
use sm_core::pipeline::{analyze_track, AnalyzeOptions};
use sm_db::LibraryDb;
use sm_model::AnalysisStatus;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

#[derive(Debug, Clone)]
#[allow(dead_code)] // variantes de diagnostico conservadas
pub enum AnalysisEvent {
    Started(i64),
    Finished(i64, u64 /*ms*/),
    Failed(i64, String),
    QueueEmpty,
}

pub struct QueueState {
    pub queue: VecDeque<(i64, String)>,
    pub running: usize,
    pub done_session: u64,
    /// Total encolado en la sesión (para progreso n/n del status bar §4).
    pub session_total: u64,
    pub failed_session: u64,
    pub analyzed_total_ms: u64,
    /// Pistas EN análisis ahora mismo (id, path) — alimenta el "Analizando:
    /// <pista>" rotativo del status bar. Los workers la mantienen bajo lock.
    pub running_items: Vec<(i64, String)>,
}

pub struct AnalysisPool {
    pub state: Arc<Mutex<QueueState>>,
    pub events: std::sync::mpsc::Receiver<AnalysisEvent>,
    workers: Vec<JoinHandle<()>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    #[allow(dead_code)]
    pub processed: Arc<AtomicU64>,
}

impl AnalysisPool {
    pub fn new(
        db: Arc<Mutex<LibraryDb>>,
        models: Arc<Models>,
        workers: usize,
        repaint: Arc<dyn Fn() + Send + Sync>,
    ) -> AnalysisPool {
        let state = Arc::new(Mutex::new(QueueState {
            queue: VecDeque::new(),
            running: 0,
            done_session: 0,
            failed_session: 0,
            analyzed_total_ms: 0,
            session_total: 0,
            running_items: Vec::new(),
        }));
        let (tx, rx) = std::sync::mpsc::channel::<AnalysisEvent>();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let processed = Arc::new(AtomicU64::new(0));
        let mut handles = Vec::new();
        for wid in 0..workers.max(1) {
            let db = db.clone();
            let models = models.clone();
            let state = state.clone();
            let tx = tx.clone();
            let stop = stop.clone();
            let repaint = repaint.clone();
            let processed = processed.clone();
            handles.push(
                std::thread::Builder::new()
                    .name(format!("sm-analyze-{wid}"))
                    .spawn(move || loop {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let item = {
                            let mut s = state.lock().unwrap();
                            s.queue.pop_front()
                        };
                        let Some((id, path)) = item else {
                            std::thread::sleep(std::time::Duration::from_millis(120));
                            continue;
                        };
                        {
                            let mut s = state.lock().unwrap();
                            s.running += 1;
                            // pista EN análisis para el status bar (analyze-live)
                            s.running_items.push((id, path.clone()));
                        }
                        let _ = tx.send(AnalysisEvent::Started(id));
                        let res = analyze_track(
                            std::path::Path::new(&path),
                            &AnalyzeOptions::default(),
                            &models,
                        );
                        match res {
                            Ok(analysis) => {
                                let ms = analysis.analysis_ms.unwrap_or(0);
                                // Audit B3: persistencia fallida ⇒ pista NO
                                // analizada. Antes `let _ =` se tragaba el
                                // fallo y se emitía `Finished` igual — la UI
                                // mostraba "analizada" sin datos en BD. Ahora:
                                // log + status Failed + evento Failed.
                                let persist: Result<(), String> = match db.lock() {
                                    Ok(db) => db.save_analysis(id, &analysis).map_err(|e| {
                                        eprintln!("[worker] save_analysis fallo id={id}: {e}");
                                        db.set_analysis_status(id, AnalysisStatus::Failed).ok();
                                        e.to_string()
                                    }),
                                    Err(e) => {
                                        eprintln!("[worker] db lock fallo id={id}: {e}");
                                        Err(format!("db lock: {e}"))
                                    }
                                };
                                let mut s = state.lock().unwrap();
                                s.running -= 1;
                                s.running_items.retain(|&(rid, _)| rid != id);
                                match persist {
                                    Ok(()) => {
                                        s.done_session += 1;
                                        s.analyzed_total_ms += ms;
                                        drop(s);
                                        let _ = tx.send(AnalysisEvent::Finished(id, ms));
                                    }
                                    Err(e) => {
                                        s.failed_session += 1;
                                        drop(s);
                                        let _ = tx.send(AnalysisEvent::Failed(id, e));
                                    }
                                }
                            }
                            Err(e) => {
                                if let Ok(mut db) = db.lock() {
                                    db.set_analysis_status(id, AnalysisStatus::Failed).ok();
                                }
                                let mut s = state.lock().unwrap();
                                s.failed_session += 1;
                                s.running -= 1;
                                s.running_items.retain(|&(rid, _)| rid != id);
                                let _ = tx.send(AnalysisEvent::Failed(id, e.to_string()));
                            }
                        }
                        processed.fetch_add(1, Ordering::Relaxed);
                        repaint();
                    })
                    .expect("spawn analyzer"),
            );
        }
        AnalysisPool {
            state,
            events: rx,
            workers: handles,
            stop,
            processed,
        }
    }

    pub fn enqueue(&self, items: Vec<(i64, String)>) {
        {
            let mut s = self.state.lock().unwrap();
            s.session_total += items.len() as u64;
        }
        let n = items.len();
        let mut s = self.state.lock().unwrap();
        for it in items {
            s.queue.push_back(it);
        }
        drop(s);
        tracing::info!("encolados {n} tracks para análisis");
    }

    pub fn queue_len(&self) -> usize {
        self.state.lock().unwrap().queue.len()
    }

    pub fn is_busy(&self) -> bool {
        let s = self.state.lock().unwrap();
        !s.queue.is_empty() || s.running > 0
    }
}

impl Drop for AnalysisPool {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for h in self.workers.drain(..) {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Audit B3: `save_analysis` fallido ⇒ el worker debe emitir `Failed`
    /// (antes emitía `Finished` y la pista quedaba "analizada" sin datos en
    /// BD). Se fuerza el fallo analizando un WAV PCM16 real de 1 s con un id
    /// que NO existe en la BD en memoria: `analyze_track` da Ok y
    /// `save_analysis` hace `bail!` en `ensure_track`.
    #[test]
    fn save_analysis_fallo_emite_failed_y_sube_contador() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("sm-worker-b3-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("probe.wav");
        {
            use std::io::Write;
            const SR: u32 = 8_000;
            const N: usize = SR as usize; // 1 s
            let mut pcm: Vec<u8> = Vec::with_capacity(N * 2);
            for i in 0..N {
                let t = i as f32 / SR as f32;
                let s = (t * 2.0 * std::f32::consts::PI * 440.0).sin() * 0.5;
                let v = (s * 32767.0) as i16;
                pcm.extend_from_slice(&v.to_le_bytes());
            }
            let data_len = pcm.len() as u32;
            let mut f = std::fs::File::create(&wav).unwrap();
            f.write_all(b"RIFF").unwrap();
            f.write_all(&(36 + data_len).to_le_bytes()).unwrap();
            f.write_all(b"WAVEfmt ").unwrap();
            f.write_all(&16u32.to_le_bytes()).unwrap(); // tamaño chunk fmt
            f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
            f.write_all(&1u16.to_le_bytes()).unwrap(); // mono
            f.write_all(&SR.to_le_bytes()).unwrap();
            f.write_all(&(SR * 2).to_le_bytes()).unwrap(); // byte rate
            f.write_all(&2u16.to_le_bytes()).unwrap(); // block align
            f.write_all(&16u16.to_le_bytes()).unwrap(); // bits
            f.write_all(b"data").unwrap();
            f.write_all(&data_len.to_le_bytes()).unwrap();
            f.write_all(&pcm).unwrap();
        }

        let db = Arc::new(Mutex::new(LibraryDb::open_in_memory().unwrap()));
        let pool = AnalysisPool::new(db.clone(), Arc::new(Models::none()), 1, Arc::new(|| {}));
        // id 424_242 NO existe ⇒ análisis Ok, persistencia Err
        pool.enqueue(vec![(424_242, wav.to_string_lossy().into_owned())]);

        let mut failure: Option<String> = None;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "timeout esperando evento del worker"
            );
            match pool
                .events
                .recv_timeout(std::time::Duration::from_millis(1_000))
            {
                Ok(AnalysisEvent::Started(_)) => continue,
                Ok(AnalysisEvent::Failed(id, e)) => {
                    assert_eq!(id, 424_242);
                    failure = Some(e);
                    break;
                }
                Ok(ev) => panic!("B3 regresó: {ev:?} pese a save_analysis fallido"),
                Err(_) => continue, // sin evento todavía
            }
        }
        let e = failure.expect("Failed emitido");
        assert!(
            e.contains("no existe"),
            "el fallo debe venir de save_analysis (ensure_track), no del decode: {e}"
        );
        // contadores que lee el status bar: el Failed suma a failed_session
        {
            let s = pool.state.lock().unwrap();
            assert_eq!(s.failed_session, 1, "Failed suma al contador visible");
            assert_eq!(s.done_session, 0, "no cuenta como terminada");
            assert!(s.running_items.is_empty(), "la pista sale de 'analizando'");
        }
        drop(pool);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
