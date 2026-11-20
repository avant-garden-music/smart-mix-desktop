//! Deck: decodificación en hilo propio + comandos por canal.

use crate::mixer::DeckShared;
use crossbeam_channel::Sender;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub enum DeckCmd {
    Load(PathBuf),
    Play,
    Pause,
    Stop,
    /// Segundos absolutos dentro de la pista.
    Seek(f64),
    SetVolume(f32),
    /// Semitonos (vinyl-style: cambia tempo y tono).
    SetPitch(f32),
    Clear,
}

pub struct LoadedInfo {
    pub title: String,
    pub path: PathBuf,
    pub duration_s: f64,
}

/// Handle hacia un deck: enviar comandos + leer estado del deck actual.
/// `shared` apunta siempre al DeckShared ACTIVO (se sustituye al cargar).
#[derive(Clone)]
pub struct DeckHandle {
    pub cmd: Sender<DeckCmd>,
    pub shared: Arc<RwLock<Arc<DeckShared>>>,
}

impl DeckHandle {
    pub fn current(&self) -> Arc<DeckShared> {
        self.shared.read().clone()
    }
    pub fn send(&self, cmd: DeckCmd) {
        let _ = self.cmd.send(cmd);
    }
    pub fn play(&self) {
        self.send(DeckCmd::Play);
    }
    pub fn pause(&self) {
        self.send(DeckCmd::Pause);
    }
    pub fn stop(&self) {
        self.send(DeckCmd::Stop);
    }
    pub fn seek(&self, s: f64) {
        self.send(DeckCmd::Seek(s));
    }
    pub fn load(&self, p: impl Into<PathBuf>) {
        self.send(DeckCmd::Load(p.into()));
    }
    pub fn position_s(&self) -> f64 {
        self.current().position_s()
    }
    pub fn is_playing(&self) -> bool {
        self.current().playing.load(Ordering::Relaxed)
    }
    pub fn duration_s(&self) -> f64 {
        self.current().duration_s
    }
    pub fn path(&self) -> String {
        self.current().path.clone()
    }
    pub fn title(&self) -> String {
        self.current().title.clone()
    }
    pub fn is_loaded(&self) -> bool {
        self.current().is_loaded()
    }
    pub fn volume(&self) -> f32 {
        *self.current().volume.read()
    }
    pub fn pitch(&self) -> f32 {
        *self.current().pitch_semis.read()
    }
}

/// Decodifica un archivo completo a f32 interleaved (symphonia, sin resample).
pub fn decode_full(path: &std::path::Path) -> anyhow::Result<(Vec<f32>, u32, usize)> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;
    let file = std::fs::File::open(path)?;
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
        .ok_or_else(|| anyhow::anyhow!("sin pista de audio"))?;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut out = Vec::new();
    let (mut channels, mut rate) = (2usize, 44_100u32);
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => anyhow::bail!("packet: {e}"),
        };
        if packet.track_id() != track.id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                channels = spec.channels.count();
                rate = spec.rate;
                if sample_buf.is_none() {
                    sample_buf = Some(SampleBuffer::new(decoded.capacity() as u64, spec));
                }
                if let Some(buf) = sample_buf.as_mut() {
                    buf.copy_interleaved_ref(decoded);
                    out.extend_from_slice(buf.samples());
                }
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }
    if out.is_empty() {
        anyhow::bail!("audio vacío");
    }
    Ok((out, rate, channels))
}
