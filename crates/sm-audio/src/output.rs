//! Salida de audio cpal + bomba de comandos + cargador de decks.

use crate::deck::{decode_full, DeckCmd, DeckHandle};
use crate::mixer::DeckShared;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::unbounded;
use parking_lot::RwLock;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Player: N decks sobre un único stream cpal f32.
pub struct Player {
    pub decks: Vec<DeckHandle>,
    pub sample_rate: u32,
    pub device_name: String,
    // mantiene vivo el stream y la bomba de comandos
    _stream: cpal::Stream,
    _pump: std::thread::JoinHandle<()>,
}

impl Player {
    pub fn new(num_decks: usize) -> anyhow::Result<Player> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("sin dispositivo de salida de audio"))?;
        let device_name = device.name()?;
        // buscar config f32
        let supported = device.default_output_config()?;
        let mut cfg = cpal::StreamConfig {
            channels: 2,
            sample_rate: supported.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };
        if supported.channels() >= 2 {
            cfg.channels = 2;
        }
        let sample_rate = cfg.sample_rate.0;
        // un canal de comandos + un slot de estado por deck
        let mut handles = Vec::with_capacity(num_decks);
        let mut rxs = Vec::with_capacity(num_decks);
        let mut shared_list = Vec::with_capacity(num_decks);
        for _ in 0..num_decks {
            let (tx, rx) = unbounded::<DeckCmd>();
            let shared = Arc::new(RwLock::new(Arc::new(DeckShared::empty())));
            handles.push(DeckHandle { cmd: tx, shared: shared.clone() });
            rxs.push(rx);
            shared_list.push(shared);
        }
        // rampa ≈ 5 ms
        let ramp = 1.0f32 / (0.005 * sample_rate as f32);
        let mix_state = shared_list.clone();
        let stream = device.build_output_stream(
            &cfg,
            move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                for s in data.iter_mut() {
                    *s = 0.0;
                }
                for deck in &mix_state {
                    let cur = deck.read().clone();
                    cur.mix_into(data, sample_rate, ramp);
                }
            },
            |err| tracing::error!("audio stream: {err}"),
            None,
        )?;
        stream.play()?;
        // bomba de comandos: aplica transporte; Load decodifica y hace swap
        let pump = std::thread::Builder::new()
            .name("sm-audio-pump".into())
            .spawn(move || {
                loop {
                    // atender todos los canales sin bloquear indefinidamente
                    let mut work = false;
                    for (i, rx) in rxs.iter().enumerate() {
                        while let Ok(cmd) = rx.try_recv() {
                            work = true;
                            apply_cmd(&shared_list[i], cmd);
                        }
                    }
                    if !work {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
            })?;
        Ok(Player {
            decks: handles,
            sample_rate,
            device_name,
            _stream: stream,
            _pump: pump,
        })
    }
}

fn apply_cmd(shared: &Arc<RwLock<Arc<DeckShared>>>, cmd: DeckCmd) {
    match cmd {
        DeckCmd::Play => {
            let cur = shared.read().clone();
            if cur.is_loaded() {
                cur.playing.store(true, Ordering::Relaxed);
            }
        }
        DeckCmd::Pause => {
            let cur = shared.read().clone();
            cur.playing.store(false, Ordering::Relaxed);
            cur.gain.store(0f32.to_bits(), Ordering::Relaxed);
        }
        DeckCmd::Stop => {
            let cur = shared.read().clone();
            cur.playing.store(false, Ordering::Relaxed);
            cur.gain.store(0f32.to_bits(), Ordering::Relaxed);
            cur.cursor_frames.store(0f64.to_bits(), Ordering::Relaxed);
        }
        DeckCmd::Seek(t) => {
            let cur = shared.read().clone();
            let frames = (t.max(0.0) * cur.sample_rate as f64).min(cur.total_frames());
            cur.cursor_frames.store(frames.to_bits(), Ordering::Relaxed);
        }
        DeckCmd::SetVolume(v) => {
            let cur = shared.read().clone();
            *cur.volume.write() = v.clamp(0.0, 2.0);
        }
        DeckCmd::SetPitch(p) => {
            let cur = shared.read().clone();
            *cur.pitch_semis.write() = p.clamp(-12.0, 12.0);
        }
        DeckCmd::Clear => {
            *shared.write() = Arc::new(DeckShared::empty());
        }
        DeckCmd::Load(path) => {
            tracing::info!("deck load: {}", path.display());
            match decode_full(&path) {
                Ok((samples, rate, channels)) => {
                    let old = shared.read().clone();
                    let duration_s =
                        (samples.len() / channels.max(1)) as f64 / rate as f64;
                    let title = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string();
                    let was_playing = old.playing.load(Ordering::Relaxed);
                    let new = DeckShared {
                        samples: Arc::new(samples),
                        channels,
                        sample_rate: rate,
                        duration_s,
                        path: path.to_string_lossy().to_string(),
                        title,
                        playing: AtomicBoolLike(was_playing),
                        cursor_frames: std::sync::atomic::AtomicU64::new(0),
                        volume: old.volume.clone(),
                        pitch_semis: old.pitch_semis.clone(),
                        gain: std::sync::atomic::AtomicU32::new(0),
                    };
                    *shared.write() = Arc::new(new);
                }
                Err(e) => tracing::error!("deck load error {}: {e}", path.display()),
            }
        }
    }
}

/// Alias para construir AtomicBool por valor en la struct literal.
#[allow(non_snake_case)]
fn AtomicBoolLike(v: bool) -> std::sync::atomic::AtomicBool {
    std::sync::atomic::AtomicBool::new(v)
}

/// Dos decks (A/B) — configuración de la vista DJ de Smart Mix.
pub fn deck_pair() -> anyhow::Result<Player> {
    Player::new(2)
}
