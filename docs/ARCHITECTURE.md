# Arquitectura

## Principios

1. **Nativo total**: Rust + egui/eframe (wgpu) + SQLite + symphonia + cpal +
   ONNX Runtime (ort). Sin runtimes externos ni llamadas a la nube: todas las
   operaciones funcionan offline.
2. **El hilo de audio es sagrado**: el callback de cpal solo lee atómicos y
   mezcla buffers en memoria; decodificar, analizar y renderizar la
   biblioteca ocurren en otros hilos.
3. **UI fluida con 30k+ pistas**: tabla virtualizada (solo se pintan las
   filas visibles), consultas SQLite paginadas (400 en 400), FTS5 indexado,
   waveform/caratulas como BLOBs leídos bajo demanda.
4. **El código de producción separado de la investigación**: los prototipos
   Python y datasets viven en harmonic-engine (repo independiente); aquí solo
   entra código de producto y modelos entrenados propios.

## Crates y dependencias

```
sm-model  ← serde
   ↑
sm-core (rustfft, symphonia, rayon, ort*)
   ↑                          ↑
sm-db (rusqlite bundled)   sm-audio (cpal, symphonia)
   ↑                          ↑
sm-library (lofty, walkdir)   |
   ↑                          ↑
sm-export (lofty)  sm-i18n    |
   ↘              ↘          ↘
              sm-app (eframe/wgpu, rfd, image)
```

- `sm-model`: tipos de dominio (TrackMeta, TrackAnalysis, BeatGrid, CuePoint,
  Playlist, SmartFilter, Preferences) + rueda Camelot con el mapa de MIK
  (mayor↔menor paralela). Sin I/O.
- `sm-core`: el motor. Un puerto por analizador (`key.rs`, `beats.rs`,
  `energy.rs`, `volume.rs`, `cues.rs`) + `pipeline.rs` (orquestador
  `analyze_track`) + `models.rs` (clasificadores). FFT vía rustfft con
  planner thread-local; frames paralelos con rayon. Decode symphonia a
  44100 Hz (resample lineal solo si la fuente difiere).
- `sm-db`: SQLite (rusqlite bundled). Esquema `tracks` + FTS5
  (`tracks_fts`) + `segments`/`cues` (JSON) + `playlists`/`playlist_tracks` +
  `settings`. Waveform como bincode BLOB.
- `sm-audio`: `Player` con N decks sobre un stream cpal f32; cada deck es un
  `DeckShared` (swap atómico al cargar) + bomba de comandos en hilo propio.
- `sm-export`: exportadores (ver lib.rs de cada uno para el formato exacto).
- `sm-app`: egui. Paneles: top (búsqueda/menú), left (colecciones/playlists),
  center (tabla virtualizada pintada a mano para rendimiento), right
  (detalle: waveform + cues + ribbon de energía), bottom (decks A/B + estado).
  Pool de análisis con N hilos configurables; la UI repinta por eventos.

## Hilos

| Hilo | Trabajo |
|---|---|
| main | egui update (60 fps) |
| sm-audio-pump | comandos de transporte + decodificación al cargar deck |
| callback cpal | mezcla de decks (solo atómicos) |
| sm-analyze-* (N) | cola de análisis → sm-core → save_analysis |
| escaneo (efímero) | scan_root + read_metadata + upsert |

## Decisiones clave documentadas

- **Resample lineal para análisis** (−60 dB armónico, suficiente para el
  pipeline espectral); la reproducción usa la tasa nativa del archivo.
- **Clasificador de tonalidad en Rust puro** (key_linear.json): los pesos del
  ONNX v4 se extrajeron del protobuf porque ort 2.0-rc rechaza su nodo
  ZipMap con doubles. Idéntico MatMul+Softmax, sin runtime externo.
- **Camelot**: `num = (8 + 7·(pc + 3·minor)) mod 12` (0→12) — el mapa de la
  rueda de MIK, NO el Camelot estándar (cada mayor ↔ su menor paralela).
- **Beatgrid persistido como (period, t0, n)**: rejilla de periodo constante
  (la estructura exacta que persiste MIK; los tiempos completos se derivan).
