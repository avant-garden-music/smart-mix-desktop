//! sm-audio — motor de reproducción para Smart Mix.
//!
//! Dos decks independientes sobre un único stream cpal (WASAPI/CoreAudio/ALSA).
//! Cada deck decodifica el archivo completo en memoria en un hilo aparte
//! (preview de DJ: 1–2 pistas cargadas), y el callback de audio mezcla con
//! interpolación lineal, volumen y pitch tipo vinilo (rate = 2^(semis/12)).
//! El análisis y la UI nunca bloquean el hilo de audio: los comandos entran
//! por un canal lock-free (crossbeam try_recv) y el estado legible se expone
//! en atómicos.

pub mod deck;
pub mod mixer;
pub mod output;

pub use deck::{DeckCmd, DeckHandle, LoadedInfo};
pub use mixer::DeckShared;
pub use output::{deck_pair, Player};

#[cfg(test)]
mod tests {
    use super::mixer::DeckShared;
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::Arc;

    fn deck(samples: Vec<f32>, playing: bool, cursor: f64, volume: f32, pitch: f32) -> DeckShared {
        let frames = samples.len() / 2;
        DeckShared {
            samples: samples.into(),
            channels: 2,
            sample_rate: 44100,
            duration_s: frames as f64 / 44100.0,
            path: String::new(),
            title: "test".into(),
            playing: AtomicBool::new(playing),
            cursor_frames: AtomicU64::new(cursor.to_bits()),
            volume: Arc::new(parking_lot::RwLock::new(volume)),
            pitch_semis: Arc::new(parking_lot::RwLock::new(pitch)),
            gain: AtomicU32::new(if playing { (1.0f32).to_bits() } else { 0 }),
        }
    }

    fn stereo_tone(seconds: f32, freq: f32) -> Vec<f32> {
        let n = (seconds * 44100.0) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * freq * i as f32 / 44100.0).sin() * 0.5;
                [v, v]
            })
            .collect()
    }

    #[test]
    fn play_advances_position() {
        let shared = deck(stereo_tone(2.0, 440.0), true, 0.0, 1.0, 0.0);
        let mut buf = vec![0.0f32; 2 * 4410]; // 0.1 s
        let wrote = shared.mix_into(&mut buf, 44100, 0.0008);
        assert!(wrote);
        let pos = shared.position_s();
        assert!((pos - 0.1).abs() < 0.01, "pos = {pos}");
    }

    #[test]
    fn volume_zero_silences() {
        let shared = deck(stereo_tone(1.0, 440.0), true, 1000.0, 0.0, 0.0);
        let mut buf = vec![0.0f32; 2 * 100];
        let wrote = shared.mix_into(&mut buf, 44100, 0.0008);
        assert!(wrote);
        assert!(buf.iter().all(|v| v.abs() < 1e-6));
    }

    #[test]
    fn pitch_doubles_rate() {
        let shared = deck(stereo_tone(4.0, 440.0), true, 0.0, 1.0, 12.0);
        let mut buf = vec![0.0f32; 2 * 4410];
        let _ = shared.mix_into(&mut buf, 44100, 0.0008);
        let pos = shared.position_s();
        // +12 semitonas = ×2 → tras 0.1 s de salida, posición ≈ 0.2 s
        assert!((pos - 0.2).abs() < 0.02, "pos = {pos}");
    }

    #[test]
    fn stops_at_end() {
        let shared = deck(stereo_tone(0.05, 440.0), true, 2000.0, 1.0, 0.0);
        let mut buf = vec![0.0f32; 2 * 22050];
        let _ = shared.mix_into(&mut buf, 44100, 0.0008);
        assert!(!shared.playing.load(Ordering::Relaxed));
    }
}

