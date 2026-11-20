//! sm-core — motor de análisis musical de Smart Mix.
//!
//! Port nativo a Rust del prototipo Python `harmonic-engine` (referencia de
//! comportamiento validada contra la DB de herramienta de referencia; ver docs/PROVENANCE.md).
//!
//! Pipeline (`pipeline::analyze_track`):
//! decode → beats (BPM/beatgrid) → energía (segmentos/cues) → tonalidad →
//! volumen (histograma/LUFS/waveform) → clasificadores ONNX si existen.

pub mod beats;
pub mod cues;
pub mod decode;
pub mod energy;
pub mod key;
pub mod cue_ranker;
pub mod cue_engine;
pub mod models;
pub mod pipeline;
pub mod volume;
