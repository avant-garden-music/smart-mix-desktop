//! Decodificación de audio a PCM f32, nativo vía symphonia (sin ffmpeg).
//!
//! El prototipo Python decodificaba con ffmpeg → f32le 44100 Hz. Aquí:
//! symphonia → f32 interleaved a la tasa nativa, y resample lineal a
//! 44100 Hz solo cuando difiere (44.1k es la inmensa mayoría del corpus;
//! el resample lineal introduce ≈ −60 dB de distorsión ARMÓNICA, aceptable
//! para análisis espectral — no se usa para reproducción).

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const ANALYSIS_SR: u32 = 44_100;

/// PCM decodificado completo: interleaved f32 a `sample_rate`.
#[derive(Debug, Clone)]
pub struct Decoded {
    pub interleaved: Vec<f32>,
    pub sample_rate: u32,
    pub channels: usize,
    pub duration_s: f64,
}

impl Decoded {
    /// Mezcla mono (promedio de canales) — vía de tonalidad/beats/energía.
    pub fn mono(&self) -> Vec<f32> {
        mix_mono(&self.interleaved, self.channels)
    }
}

pub fn is_supported_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp3" | "wav" | "wave" | "flac" | "aiff" | "aif" | "aifc" | "m4a" | "mp4"
            | "m4b" | "ogg" | "oga" | "opus" | "aac" | "alac" | "wma" | "asf"  // F-12: via ffmpeg
    )
}

/// Decodifica un archivo completo y lo deja a 44100 Hz.
pub fn decode_file(path: &Path) -> Result<Decoded> {
    // F-12: codecs fuera de symphonia (wma/asf) van por ffmpeg externo.
    let raw = if needs_ffmpeg(path) {
        decode_via_ffmpeg(path)?
    } else {
        decode_file_native(path)?
    };
    if raw.sample_rate == ANALYSIS_SR {
        Ok(raw)
    } else {
        let interleaved = resample_linear(
            &raw.interleaved,
            raw.channels,
            raw.sample_rate,
            ANALYSIS_SR,
        );
        Ok(Decoded {
            duration_s: raw.duration_s,
            interleaved,
            sample_rate: ANALYSIS_SR,
            channels: raw.channels,
        })
    }
}

/// F-12: transcoding via ffmpeg (f32le 44100 Hz stereo por stdout).
fn decode_via_ffmpeg(path: &Path) -> Result<Decoded> {
    use std::process::{Command, Stdio};
    let out = Command::new(ffmpeg_bin())
        .args(ffmpeg_decode_args(path))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .with_context(|| {
            format!(
                "ffmpeg no disponible ({}): instala ffmpeg o setea SM_FFMPEG",
                ffmpeg_bin()
            )
        })?;
    if !out.status.success() {
        bail!("ffmpeg fallo al decodificar {} (exit {:?})", path.display(), out.status.code());
    }
    let n_samples = out.stdout.len() / 4;
    let mut interleaved = vec![0.0f32; n_samples];
    for (dst, chunk) in interleaved.iter_mut().zip(out.stdout.chunks_exact(4)) {
        *dst = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    Ok(Decoded {
        duration_s: n_samples as f64 / (2.0 * 44100.0),
        interleaved,
        sample_rate: 44100,
        channels: 2,
    })
}

fn decode_file_native(path: &Path) -> Result<Decoded> {
    let file = File::open(path).with_context(|| format!("abrir {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| anyhow::anyhow!("probe: {e}"))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .cloned()
        .with_context(|| "sin pista de audio")?;
    let track_id = track.id;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut out: Vec<f32> = Vec::new();
    let mut channels = 0usize;
    let mut sample_rate = 0u32;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(symphonia::core::errors::Error::ResetRequired) => {
                bail!("reset requerido (streaming no soportado)")
            }
            Err(e) => bail!("decode packet: {e}"),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                channels = spec.channels.count();
                sample_rate = spec.rate;
                if sample_buf.is_none() {
                    sample_buf =
                        Some(SampleBuffer::new(decoded.capacity() as u64, spec));
                }
                if let Some(buf) = sample_buf.as_mut() {
                    buf.copy_interleaved_ref(decoded);
                    out.extend_from_slice(buf.samples());
                }
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(e) => bail!("decode: {e}"),
        }
    }
    if channels == 0 || out.is_empty() {
        bail!("audio vacío");
    }
    let frames = out.len() / channels;
    Ok(Decoded {
        interleaved: out,
        sample_rate,
        channels,
        duration_s: frames as f64 / sample_rate as f64,
    })
}

/// Downmix a mono por promedio.
pub fn mix_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks(channels)
        .map(|c| c.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Resample lineal por canal (solo para análisis).
///
/// Una sola pasada sobre el interleaved desde la auditoría 2026-11-10 (la
/// versión anterior de-interleaba a un buffer planar por canal y volvía a
/// re-interlevar: 2 buffers extra del tamaño del PCM). Mismos i0/i1/f por
/// muestra de salida y misma aritmética → bit-exacto (test
/// `characterization_resample_bits`).
pub fn resample_linear(
    interleaved: &[f32],
    channels: usize,
    from: u32,
    to: u32,
) -> Vec<f32> {
    if from == to || channels == 0 {
        return interleaved.to_vec();
    }
    let frames_in = interleaved.len() / channels;
    let frames_out = (frames_in as u64 * to as u64 / from as u64) as usize;
    let ratio = from as f64 / to as f64;
    let mut inter = Vec::with_capacity(frames_out * channels);
    for i in 0..frames_out {
        let pos = i as f64 * ratio;
        let i0 = pos.floor() as usize;
        let i1 = (i0 + 1).min(frames_in - 1);
        let f = (pos - i0 as f64) as f32;
        for c in 0..channels {
            let a = interleaved[i0 * channels + c];
            let b = interleaved[i1 * channels + c];
            inter.push(a * (1.0 - f) + b * f);
        }
    }
    inter
}

// ------------------------------------------------------------- F-12 wma ----

/// Argumentos de transcodificación ffmpeg → f32le 44100 Hz estéreo por
/// stdout (pureza: testeable sin tocar el proceso real).
pub fn ffmpeg_decode_args(path: &Path) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        path.display().to_string(),
        "-map".into(),
        "a:0".into(),
        "-ac".into(),
        "2".into(),
        "-ar".into(),
        "44100".into(),
        "-f".into(),
        "f32le".into(),
        "pipe:1".into(),
    ]
}

/// ¿Este archivo requiere la vía ffmpeg (codecs fuera de symphonia)?
pub fn needs_ffmpeg(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "wma" | "asf"))
        .unwrap_or(false)
}

/// Ejecutable ffmpeg: override por env, si no `ffmpeg` del PATH.
pub fn ffmpeg_bin() -> String {
    std::env::var("SM_FFMPEG").unwrap_or_else(|_| "ffmpeg".into())
}

#[cfg(test)]
mod wma_tests {
    use super::*;

    #[test]
    fn wma_y_asf_van_por_ffmpeg() {
        assert!(needs_ffmpeg(Path::new("x/rola.WMA")));
        assert!(needs_ffmpeg(Path::new("x/rola.wma")));
        assert!(needs_ffmpeg(Path::new("x/rola.asf")));
        assert!(!needs_ffmpeg(Path::new("x/rola.mp3")));
        assert!(!needs_ffmpeg(Path::new("x/rola.flac")));
    }

    #[test]
    fn args_de_transcodificacion_son_completos() {
        let a = ffmpeg_decode_args(Path::new("C:/a/b.wma"));
        assert_eq!(a[0], "-hide_banner");
        assert!(a.contains(&"C:/a/b.wma".to_string()));
        assert!(a.contains(&"-f".to_string()));
        let f = a.iter().position(|x| x == "-f").unwrap();
        assert_eq!(a[f + 1], "f32le");
        assert_eq!(a.last().unwrap(), "pipe:1");
        // estéreo 44100 para alimentar el pipeline directo
        assert!(a.windows(2).any(|w| w[0] == "-ar" && w[1] == "44100"));
    }

    #[test]
    fn extension_soportada_incluye_wma() {
        assert!(is_supported_extension("wma"));
    }
}

#[cfg(test)]
mod characterization_tests {
    // Valores dorados bit-exactos del resample lineal y la mezcla mono
    // (auditoría 2026-11-10). El pipeline de cues usa L+R sin 1/2, que con
    // ≤2 canales es mono·2 bit-exacto (÷2 y ×2 son exactos en IEEE-754).
    use super::*;

    struct Lcg(u64);
    impl Lcg {
        fn next_f32(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as f32 / (u32::MAX >> 1) as f32) - 1.0
        }
    }

    fn fnv1a(data: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    fn hash_f32(v: &[f32]) -> u64 {
        let mut bytes = Vec::with_capacity(v.len() * 4);
        for x in v {
            bytes.extend_from_slice(&x.to_bits().to_le_bytes());
        }
        fnv1a(&bytes)
    }

    #[test]
    fn characterization_resample_bits() {
        let mut lcg = Lcg(12345);
        let src: Vec<f32> = (0..48_000 * 2).map(|_| lcg.next_f32() * 0.8).collect();
        let rs = resample_linear(&src, 2, 48_000, 44_100);
        assert_eq!(rs.len(), 88_200);
        assert_eq!(hash_f32(&rs), 0xc80849840e4cecf8);
        let src1: Vec<f32> = src.iter().step_by(2).copied().collect();
        let rs1 = resample_linear(&src1, 1, 48_000, 44_100);
        assert_eq!(rs1.len(), 44_100);
        assert_eq!(hash_f32(&rs1), 0xb8852a585f166ff1);
        // identidad: misma tasa devuelve el mismo slice
        assert_eq!(hash_f32(&resample_linear(&src1, 1, 44_100, 44_100)), hash_f32(&src1));
    }

    #[test]
    fn mono_por_dos_es_suma_bit_exacto_hasta_2_canales() {
        let mut lcg = Lcg(777);
        // estéreo
        let st: Vec<f32> = (0..10_000).map(|_| lcg.next_f32()).collect();
        let mono = mix_mono(&st, 2);
        let sum: Vec<f32> = st.chunks(2).map(|c| c.iter().sum::<f32>()).collect();
        assert!(mono.iter().zip(sum.iter()).all(|(m, s)| (m * 2.0).to_bits() == s.to_bits()));
        // mono
        let m: Vec<f32> = (0..10_000).map(|_| lcg.next_f32()).collect();
        let mono1 = mix_mono(&m, 1);
        assert!(m.iter().zip(mono1.iter()).all(|(a, b)| (b * 2.0).to_bits() == (a * 2.0).to_bits()));
    }
}

