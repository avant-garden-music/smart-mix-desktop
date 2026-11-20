# Matriz de paridad funcional — Smart Mix vs herramienta comercial de referencia 11

Fuente del inventario herramienta de referencia: investigación de esta sesión (docs/inventario externo
`Dev/research/ref-inventory.md`, 50 features F-01…F-50 con fuentes
disco/UI/doc). Estados: ✅ implementada y probada · 🟡 implementada sin prueba
específica · ⏳ pendiente.

## Métricas de paridad medidas

| Métrica | Corpus | Smart Mix vs herramienta de referencia (medido) | Referencia |
|---|---|---|---|
| Key exacta | 588 filenames herramienta de referencia (53% major) | **75.8% CV5** (v5; v4: 36.2%) | ≥90% ⏳ |
| Key letra A/B | 588 filenames herramienta de referencia | **82.3% CV5 / 84.5% BD** (v4: 46.8%, mayores 0%) | — |
| BPM ±0.35 | n=83 herramienta de referenciaStore | **100%** (83/83) | ≥95% ✅ |
| Energy ±1.5 | n=83 herramienta de referenciaStore | **100%** (83/83) | ≥90% ✅ |
| Cues ±1 beat | GT 712 cues / 92 pistas | **77.9%** (was 41.2%; holdout 78.5%, tune 77.5%) · ±20 ms 12.1% · count 74/92 · cue[0] ≥ 0 en 92/92 | — |

> **Nota honesta (QA-F-015)**: la métrica previa "Key 95.2% (n=83)" era un
> artefacto del sesgo del corpus (96% menores): un clasificador que NUNCA
> detecta mayores acierta ~96% ahí. La referencia discriminante ahora es el
> corpus de 588 filenames renombrados por herramienta de referencia (313 mayores). v5 corrige el
> colapso (mayores 0%→~83%); los ~18% restantes de error de letra es la
> ambigüedad real mayor/menor en chroma de música de baile.

## Análisis (Standard)

| ID | Feature herramienta de referencia | Smart Mix | Estado |
|---|---|---|---|
| F-01 | Tonalidad Camelot + confianza + 2ª key | motor completo + clasificador lineal propio + confianzas | ✅ (95.2%) |
| F-02 | Key por segmento | ZKEYSEGMENT = 1 segmento/pista dominante (misma semántica); segmentos por tiempo NO ( herramienta de referencia tampoco los guarda múltiples) | 🟡 |
| F-03 | BPM | pipeline onset+ACF+DP+rejilla constante | ✅ (100%) |
| F-04 | Beat grid | rejilla (period,t0,n) persistida, derivable y editable vía override | ✅ |
| F-05 | Energy global 1–10 | 14 bandas+P40/P95+ONNX global | ✅ (±1.5 = 100%) |
| F-06 | Energy por segmento | segmentos 32 beats re-anclados a 16 + ONNX segmento | ✅ (ribbon UI) |
| F-07 | Cues automáticos ≤8 | regla empírica validada (5843 cues), snap a beats, gap ≥32 beats | 🟡 [APROX] |
| F-08 | Snap Cue | añadir cue en playhead con snap a beatgrid | ✅ |
| F-09 | Edición manual de cues | añadir/eliminar/jump, persiste en BD | ✅ |
| F-10 | Volumen/clipping | RMS calibrado +1.801 dB, LUFS BS.1770, conteo clips | ✅ |
| F-11 | Caché de análisis | resultado persistido en BD por pista (re-análisis manual) | 🟡 (sin invalidación auto por mtime) |
| F-12 | Formatos de entrada | mp3/wav/aiff/flac/m4a/mp4/ogg/aac (symphonia) + **wma/asf via ffmpeg externo** (e2e verificado) | ✅ (corpus real: mp3/aiff/wav) |

## Colección, navegación y UI

| ID | Feature herramienta de referencia | Smart Mix | Estado |
|---|---|---|---|
| F-14 | Tabla de colección | virtualizada 30k+ (bench: queries p50 4–15 ms con 30k–100k), 8 columnas | ✅ |
| F-16 | Búsqueda | FTS5 prefijo (13 coincidencias 'love' en 691 — probado) | ✅ |
| F-17 | Filtros avanzados | rango energía/BPM + género + clave compatible (SmartFilter) | ✅ |
| F-18 | Playlists (+emoji) | manuales + inteligentes; **anidadas 3 niveles + emoji** (F-18), **orden manual** (F-19), árbol colapsable, menú contextual | ✅/⏳ |
| F-20 | Vistas fijas | Todas / Sin analizar / Duplicados con contadores | ✅ |
| F-21 | Rueda Camelot | selector "Compatible con" (24 claves); rueda visual ✅ (wheel.rs, botón CAMELOT en topbar) | 🟡 |
| F-22 | Reproductor + waveform | 2 decks, seek por waveform, cues clicables, pitch vinilo | ✅ (probado) |
| F-23 | Panel Song Info | detalle con todos los campos + override key/BPM | ✅ |
| F-24 | Tag editor masivo | ✅ vista dedicada estilo herramienta de referencia: selección múltiple (checkbox columna + header tri-state + ctrl/shift) + Tag Editor (Title/Artist/Album/Genre/Year, vacío = sin cambio, preview 20, aplicar diff vía upsert_track, escritura opcional de tags de archivo mp3/aiff/wav/flac OFF por defecto; hook QA SM_VIEW=tags) | ✅ código+tests (validación fina de usuario ⏳) |
| F-25 | Tema claro/oscuro | tema oscuro editorial propio (identidad Avant Garden) | 🟡 |
| F-26 | Rating | ✅ 1-5 estrellas (columna clic, deck y detail; anillos=vacías); orden por rating; persistencia v3 | ✅ código+tests (clics: usuario ⏳) |

## Tags y exportación

| ID | Feature herramienta de referencia | Smart Mix | Estado |
|---|---|---|---|
| F-29 | Escritura de tags configurable | ID3 comment/TKEY/TBPM con formato configurable + preview; OFF por defecto (consentimiento) | ✅ (12 tests) |
| F-30 | Renombrado de archivos | ✅ API segura + UI (plan/preview/apply, patrón herramienta de referencia/Camelot/custom, nunca sobrescribe, BD sincronizada); paridad 17/17 vs rename.py | ✅ (falta validación de usuario) |
| F-31 | Limpieza de tags | ✅ detección de comments basura con PREVIEW por pista; opt-in iTunSMPB/TCMP; gated tras consentimiento de archivos; comment inocente sobrevive por contrato | ✅ código+tests |
| F-32 | Export cues → Serato | crates TLV ✅ **+ hot cues GEOB "Serato Markers2" dentro del audio (mp3/aiff/wav)** — paridad byte-exacta vs referencia Python (payload y archivo completo), checkbox en export (OFF por defecto) | ✅ código+tests (interop Serato DJ real ⏳) |
| F-33 | Export cues → Traktor | NML con CUE_V2 + TEMPO | ✅ código+tests |
| F-34 | Export cues → rekordbox | DJ_PLAYLISTS XML con POSITION_MARK | ✅ código+tests |
| F-35 | Engine DJ | EngineLibrary (m.db) port del tagger propio | ✅ código+tests (hardware ⏳) |
| — | CSV/M3U | CSV estilo herramienta de referencia + M3U/M3U8 import/export | ✅ |

## Solo Pro

| ID | Feature herramienta de referencia | Smart Mix | Estado |
|---|---|---|---|
| F-38/39 | Mashup/DJ Mix sugerencias | filtro "compatible con X" + orden por energía/BPM (núcleo de sugerencia) | 🟡 |
| F-40 | Dos decks de audición | decks A/B independientes con pitch | ✅ |
| F-41 | Stem separation | ⏳ (ONNX 176–253 MB de herramienta de referencia: propietario, NO incluido; se investigará modelo propio) | ⏳ |
| F-42 | Pitch shift (key change) | pitch vinilo ±12 st (tempo+tono); time-stretch elástico ⏳ | 🟡 |
| F-43 | Looping | ⏳ | ⏳ |
| F-44 | Favoritos mashup | ⏳ | ⏳ |

## Infraestructura

| ID | Feature herramienta de referencia | Smart Mix | Estado |
|---|---|---|---|
| F-50 | Idioma UI | es/en completos + infraestructura 7 idiomas (locales toolkit embebidos) | ✅ (herramienta de referencia: solo inglés) |
| — | Offline | 100% offline (sin telemetría, sin nube) | ✅ |
| — | Export directo a USB | dropdown Destino con unidades removibles (label/espacio/FAT32), exporta a <unidad>:/SmartMix/ + avisos FAT32 nombres >31 | ✅ código+tests |
| — | USB Safe Format / backup | **sm-usb (crate nuevo)**: scan SHA-256 → backup carpeta/zip verificado → restore → warnings FAT32 → plan de formato con doble confirmación (la librería NUNCA ejecuta el format — test guardián). UI: botón "Copia de seguridad verificada" por unidad (hilo de fondo). Safe Format completo ⏳ | 🟡 (48 tests; hardware ⏳) |

## Resumen

- **P0 (paridad núcleo)**: 11/12 implementadas y probadas (F-30 renombrado
  pendiente por protección de archivos).
- **Probado sobre datos reales**: 691 pistas analizadas 0 errores; paridad
  medida contra la BD real de herramienta de referencia del usuario; UI verificada por captura;
  benchmarks 30k/100k.
- **Pendientes principales**: stem separation (modelo propio), time-stretch
  elástico, tag editor masivo, rueda Camelot visual, USB tools, instalador
  firmado.
