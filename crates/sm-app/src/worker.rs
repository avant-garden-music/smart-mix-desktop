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
    pub failed_session: u64,
    pub analyzed_total_ms: u64,
}

pub struct AnalysisPool {
    pub state: Arc<Mutex<QueueState>>,
    pub events: std::sync::mpsc::Receiver<AnalysisEvent>,
    workers: Vec<JoinHandle<()>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
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
                                if let Ok(mut db) = db.lock() {
                                    let _ = db.save_analysis(id, &analysis);
                                }
                                let mut s = state.lock().unwrap();
                                s.done_session += 1;
                                s.analyzed_total_ms += ms;
                                s.running -= 1;
                                let _ = tx.send(AnalysisEvent::Finished(id, ms));
                            }
                            Err(e) => {
                                if let Ok(mut db) = db.lock() {
                                    db.set_analysis_status(id, AnalysisStatus::Failed).ok();
                                }
                                let mut s = state.lock().unwrap();
                                s.failed_session += 1;
                                s.running -= 1;
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
