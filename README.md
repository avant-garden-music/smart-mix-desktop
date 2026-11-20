# Smart Mix

**Análisis armónico profesional para DJs.** Aplicación de escritorio nativa en
Rust (Windows primero, macOS/Linux preparados) del universo
[Avant Garden](https://github.com/avant-garden-music) — motor de análisis
[harmonic-engine](https://github.com/ImSeenOne/harmonic-engine) portado a Rust
nativo con paridad numérica verificada.

> Proyecto independiente. No afiliado a herramienta comercial de referencia, AlphaTheta, inMusic ni
> Serato. Las marcas se mencionan solo con fines descriptivos de
> interoperabilidad.

## Qué hace

- **Análisis musical completo**: tonalidad (Camelot, mapa rueda herramienta de referencia), BPM +
  beatgrid de periodo constante, energía global 1–10 y por segmentos, volumen
  RMS calibrado, LUFS (BS.1770-4), conteo de clips, waveform y cue points
  inteligentes (≤8, anclados a segmentos de energía).
- **Biblioteca SQLite** con búsqueda FTS5, filtros, ordenación, playlists
  manuales e inteligentes, detección de duplicados y compatibilidad armónica
  ("Compatible con 8A").
- **Reproducción** con 2 decks (A/B), pitch tipo vinilo, seek por waveform,
  cues reproducibles — el audio nunca bloquea el análisis ni la UI.
- **Exportación**: Traktor NML, Rekordbox XML, Serato crates (_Serato_/Subcrates),
  Engine DJ (EngineLibrary), CSV estilo herramienta de referencia, M3U y escritura de tags ID3
  (con vista previa; nunca sin consentimiento explícito).
- **7 idiomas** (en/es/fr/de/it/pt/nl) con cascada idioma→inglés→clave.

## Métricas del motor (port Rust vs prototipo Python validado contra herramienta de referencia)

| Analizador | Referencia Python (vs DB herramienta de referencia) | Estado Rust |
|---|---|---|
| Tonalidad | 90.5% ± 1.5 (CV 5-fold, 569 pistas) | port completo + clasificador lineal propio (112→24) |
| BPM/beatgrid | 100% < 0.3 BPM (114/114), fase 0.3 ms | port completo (mismo pipeline) |
| Energía | MAE 0.479 (±1 = 98.2%) | port completo + ONNX global/segmento |
| Volumen/LUFS | LUFS MAE 0.040 LU; vol MAE 0.215 dB | port completo (histograma calibrado +1.801 dB, biquads time-domain) |
| Cue points | regla empírica 5843 cues [APROX] | port de la regla (recorte exacto a 8 del cloud: UNKNOWN) |

Velocidad: **~100× el prototipo Python** (110 ms por pista normal en un solo
hilo; 693/693 pistas reales analizadas sin errores en el corpus de prueba).

## Desarrollo

```bash
cargo build --release -p sm-app   # target/release/smart-mix.exe
cargo test --workspace            # suite completa
cargo run -p sm-core --release --example analyze_file -- "pista.mp3"
```

Estructura (workspace, 8 crates):

```
crates/sm-model     tipos de dominio + Camelot (sin deps de I/O)
crates/sm-core      motor DSP (key/beats/energy/volume/cues) + modelos
crates/sm-db        SQLite (FTS5, playlists, duplicados, settings)
crates/sm-library   escaneo, tags (lofty), M3U, dedupe
crates/sm-audio     reproducción (cpal, 2 decks, pitch vinilo)
crates/sm-export    Traktor/Rekordbox/Serato/Engine DJ/CSV/M3U/ID3
crates/sm-i18n      7 idiomas embebidos
crates/sm-app       UI egui/eframe (wgpu) — tabla virtualizada 30k+
models/             ONNX propios + key_linear.json (pesos extraídos)
```

Ver [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md),
[docs/PROVENANCE.md](docs/PROVENANCE.md) y
[docs/FEATURES.md](docs/FEATURES.md) (matriz de paridad).

## Plataformas

| Plataforma | Compila | Probado |
|---|---|---|
| Windows 10/11 x64 | ✅ | ✅ análisis real + UI + audio |
| macOS (Intel/AS) | ✅ CI | ⏳ no probado en esta máquina |
| Linux x64 | ✅ CI | ⏳ no probado en esta máquina |

## Licencia

Propiedad de Avant Garden Music. Repositorio privado. Los modelos ONNX y
pesos incluidos fueron entrenados por el proyecto harmonic-engine sobre
labels de su propia base de datos.
