# Matriz de paridad funcional — Smart Mix vs Mixed In Key 11

Fuente del inventario MIK: investigación de esta sesión (docs/inventario externo
`Dev/research/mik11-inventory.md`, 50 features F-01…F-50 con fuentes
disco/UI/doc). Estados: ✅ implementada y probada · 🟡 implementada sin prueba
específica · ⏳ pendiente.

## Métricas de paridad medidas (corpus real del usuario, n=83 emparejadas)

| Métrica | Smart Mix vs MIK 11 (medido) | Referencia (test aceptación) |
|---|---|---|
| Key exacta | **95.2%** (79/83) | ≥90% ✅ |
| BPM ±0.35 | **100%** (83/83) | ≥95% ✅ |
| Energy ±1.5 | **100%** (83/83) | ≥90% ✅ |
| Energy ±0.5 | 80.7% | — |

Los 4 desacuerdos de key son 3 confusiones mayor/menor paralela (caso más
difícil del problema; MIK también baja confianza en ellos). Herramienta:
`tools/parity_check.py`.

## Análisis (Standard)

| ID | Feature MIK | Smart Mix | Estado |
|---|---|---|---|
| F-01 | Tonalidad Camelot + confianza + 2ª key | motor completo + clasificador lineal propio + confianzas | ✅ (95.2%) |
| F-02 | Key por segmento | ZKEYSEGMENT = 1 segmento/pista dominante (misma semántica); segmentos por tiempo NO ( MIK tampoco los guarda múltiples) | 🟡 |
| F-03 | BPM | pipeline onset+ACF+DP+rejilla constante | ✅ (100%) |
| F-04 | Beat grid | rejilla (period,t0,n) persistida, derivable y editable vía override | ✅ |
| F-05 | Energy global 1–10 | 14 bandas+P40/P95+ONNX global | ✅ (±1.5 = 100%) |
| F-06 | Energy por segmento | segmentos 32 beats re-anclados a 16 + ONNX segmento | ✅ (ribbon UI) |
| F-07 | Cues automáticos ≤8 | regla empírica validada (5843 cues), snap a beats, gap ≥32 beats | 🟡 [APROX] |
| F-08 | Snap Cue | añadir cue en playhead con snap a beatgrid | ✅ |
| F-09 | Edición manual de cues | añadir/eliminar/jump, persiste en BD | ✅ |
| F-10 | Volumen/clipping | RMS calibrado +1.801 dB, LUFS BS.1770, conteo clips | ✅ |
| F-11 | Caché de análisis | resultado persistido en BD por pista (re-análisis manual) | 🟡 (sin invalidación auto por mtime) |
| F-12 | Formatos de entrada | mp3/wav/aiff/flac/m4a/mp4/ogg/aac (symphonia); wma ⏳ | ✅ (corpus real: mp3/aiff/wav) |

## Colección, navegación y UI

| ID | Feature MIK | Smart Mix | Estado |
|---|---|---|---|
| F-14 | Tabla de colección | virtualizada 30k+ (bench: queries p50 4–15 ms con 30k–100k), 8 columnas | ✅ |
| F-16 | Búsqueda | FTS5 prefijo (13 coincidencias 'love' en 691 — probado) | ✅ |
| F-17 | Filtros avanzados | rango energía/BPM + género + clave compatible (SmartFilter) | ✅ |
| F-18 | Playlists (+emoji) | manuales + inteligentes (persistidas); anidadas/emoji ⏳ | ✅/⏳ |
| F-20 | Vistas fijas | Todas / Sin analizar / Duplicados con contadores | ✅ |
| F-21 | Rueda Camelot | selector "Compatible con" (24 claves); rueda visual ⏳ | 🟡 |
| F-22 | Reproductor + waveform | 2 decks, seek por waveform, cues clicables, pitch vinilo | ✅ (probado) |
| F-23 | Panel Song Info | detalle con todos los campos + override key/BPM | ✅ |
| F-24 | Tag editor masivo | vista dedicada ⏳ (edición por pista en detalle) | ⏳ |
| F-25 | Tema claro/oscuro | tema oscuro editorial propio (identidad Avant Garden) | 🟡 |
| F-26 | Rating | ⏳ | ⏳ |

## Tags y exportación

| ID | Feature MIK | Smart Mix | Estado |
|---|---|---|---|
| F-29 | Escritura de tags configurable | ID3 comment/TKEY/TBPM con formato configurable + preview; OFF por defecto (consentimiento) | ✅ (12 tests) |
| F-30 | Renombrado de archivos | ⏳ (protección de archivos: no implementado a propósito aún) | ⏳ |
| F-32 | Export cues → Serato | crates _Serato_/Subcrates formato TLV | ✅ código+tests (interop hardware ⏳) |
| F-33 | Export cues → Traktor | NML con CUE_V2 + TEMPO | ✅ código+tests |
| F-34 | Export cues → rekordbox | DJ_PLAYLISTS XML con POSITION_MARK | ✅ código+tests |
| F-35 | Engine DJ | EngineLibrary (m.db) port del tagger propio | ✅ código+tests (hardware ⏳) |
| — | CSV/M3U | CSV estilo MIK + M3U/M3U8 import/export | ✅ |

## Solo Pro

| ID | Feature MIK | Smart Mix | Estado |
|---|---|---|---|
| F-38/39 | Mashup/DJ Mix sugerencias | filtro "compatible con X" + orden por energía/BPM (núcleo de sugerencia) | 🟡 |
| F-40 | Dos decks de audición | decks A/B independientes con pitch | ✅ |
| F-41 | Stem separation | ⏳ (ONNX 176–253 MB de MIK: propietario, NO incluido; se investigará modelo propio) | ⏳ |
| F-42 | Pitch shift (key change) | pitch vinilo ±12 st (tempo+tono); time-stretch elástico ⏳ | 🟡 |
| F-43 | Looping | ⏳ | ⏳ |
| F-44 | Favoritos mashup | ⏳ | ⏳ |

## Infraestructura

| ID | Feature MIK | Smart Mix | Estado |
|---|---|---|---|
| F-50 | Idioma UI | es/en completos + infraestructura 7 idiomas (locales toolkit embebidos) | ✅ (MIK: solo inglés) |
| — | Offline | 100% offline (sin telemetría, sin nube) | ✅ |
| — | USB Safe Format / selector equipo | port pendiente desde harmonic-engine (core/usb, core/devices) | ⏳ |

## Resumen

- **P0 (paridad núcleo)**: 11/12 implementadas y probadas (F-30 renombrado
  pendiente por protección de archivos).
- **Probado sobre datos reales**: 691 pistas analizadas 0 errores; paridad
  medida contra la BD real de MIK del usuario; UI verificada por captura;
  benchmarks 30k/100k.
- **Pendientes principales**: stem separation (modelo propio), time-stretch
  elástico, tag editor masivo, rueda Camelot visual, USB tools, instalador
  firmado.
