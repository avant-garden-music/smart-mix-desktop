//! Lógica pura del deck: estado compartido atómico + mezcla interpolada.
//! Sin dependencias de cpal → testeable sin dispositivo de audio.

use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

/// Estado del deck leído por el callback de audio y por la UI.
/// Inmutable tras la carga (samples); para cambiar de pista se sustituye el
/// DeckShared entero (Arc swap). volume/pitch son Arc para sobrevivir swaps.
pub struct DeckShared {
    pub samples: Arc<Vec<f32>>,
    pub channels: usize,
    pub sample_rate: u32,
    pub duration_s: f64,
    pub path: String,
    pub title: String,
    pub playing: AtomicBool,
    /// Cursor en frames fraccionales del archivo (bits de f64).
    pub cursor_frames: AtomicU64,
    pub volume: Arc<RwLock<f32>>,
    /// Pitch en semitonos (positivo acelera y sube el tono — vinyl style).
    pub pitch_semis: Arc<RwLock<f32>>,
    /// Ganancia con rampa anti-click (bits de f32; 0 = silencio).
    pub gain: AtomicU32,
}

impl DeckShared {
    pub fn empty() -> DeckShared {
        DeckShared {
            samples: Arc::new(Vec::new()),
            channels: 2,
            sample_rate: 44_100,
            duration_s: 0.0,
            path: String::new(),
            title: String::new(),
            playing: AtomicBool::new(false),
            cursor_frames: AtomicU64::new(0),
            volume: Arc::new(RwLock::new(0.85)),
            pitch_semis: Arc::new(RwLock::new(0.0)),
            gain: AtomicU32::new(0),
        }
    }

    pub fn position_s(&self) -> f64 {
        let f = f64::from_bits(self.cursor_frames.load(Ordering::Relaxed));
        f.min(self.total_frames()) / self.sample_rate as f64
    }

    pub fn total_frames(&self) -> f64 {
        (self.samples.len() / self.channels.max(1)) as f64
    }

    pub fn is_loaded(&self) -> bool {
        !self.samples.is_empty()
    }

    /// Mezcla este deck en `buf` (interleaved stereo f32 de salida).
    /// Devuelve true si contribuyó audio. `ramp_step` = Δgain/muestra.
    pub fn mix_into(&self, buf: &mut [f32], out_rate: u32, ramp_step: f32) -> bool {
        if !self.playing.load(Ordering::Relaxed) || self.samples.is_empty() {
            self.gain.store(0f32.to_bits(), Ordering::Relaxed);
            return false;
        }
        let vol = *self.volume.read();
        let pitch = 2f32.powf(self.pitch_semis.read().clone() / 12.0);
        // avance por frame de SALIDA (en frames del archivo)
        let step = self.sample_rate as f64 * pitch as f64 / out_rate as f64;
        let total = self.total_frames();
        let mut cursor = f64::from_bits(self.cursor_frames.load(Ordering::Relaxed));
        let mut g = f32::from_bits(self.gain.load(Ordering::Relaxed));
        let mut contributed = false;
        let ch = self.channels.max(1);
        for frame in buf.chunks_mut(2) {
            g += (1.0 - g).clamp(-ramp_step, ramp_step);
            if cursor < total {
                let i0 = cursor.floor() as usize;
                let frac = (cursor - i0 as f64) as f32;
                let base = i0 * ch;
                let i1 = ((i0 + 1) * ch).min(self.samples.len());
                for (oc, out) in frame.iter_mut().enumerate() {
                    let sc = if oc < ch { oc } else { ch - 1 };
                    let s0 = self.samples[base + sc];
                    let s1 = self.samples[i1 - ch + sc];
                    let v = s0 * (1.0 - frac) + s1 * frac;
                    *out += v * vol * g;
                }
                contributed = true;
            }
            cursor += step;
            if cursor >= total {
                // fin de pista: stop
                self.playing.store(false, Ordering::Relaxed);
                self.gain.store(0f32.to_bits(), Ordering::Relaxed);
                self.cursor_frames.store(0f64.to_bits(), Ordering::Relaxed);
                return contributed;
            }
        }
        self.cursor_frames.store(cursor.to_bits(), Ordering::Relaxed);
        self.gain.store(g.to_bits(), Ordering::Relaxed);
        contributed
    }
}

/// Helper para construir f64 atómico en bits.
pub fn f64_bits(v: f64) -> u64 {
    v.to_bits()
}
