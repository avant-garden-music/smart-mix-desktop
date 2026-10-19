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
            | "m4b" | "ogg" | "oga" | "opus" | "aac" | "alac"
    )
}

/// Decodifica un archivo completo y lo deja a 44100 Hz.
pub fn decode_file(path: &Path) -> Result<Decoded> {
    let raw = decode_file_native(path)?;
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
    let mut out = Vec::with_capacity(frames_out * channels);
    for ch in 0..channels {
        let ch_data: Vec<f32> =
            interleaved[ch..].iter().step_by(channels).copied().collect();
        for i in 0..frames_out {
            let pos = i as f64 * ratio;
            let i0 = pos.floor() as usize;
            let i1 = (i0 + 1).min(frames_in - 1);
            let f = (pos - i0 as f64) as f32;
            let v = ch_data[i0] * (1.0 - f) + ch_data[i1] * f;
            out.push(v);
        }
    }
    // re-interleave
    let mut inter = Vec::with_capacity(frames_out * channels);
    for i in 0..frames_out {
        for c in 0..channels {
            inter.push(out[c * frames_out + i]);
        }
    }
    inter
}
