# Auditoría de calidad y rendimiento — Smart Mix (workspace completo)

**Fecha:** 2026-11-10 · **Rama:** `studio-redesign` @ `be47bf2` (feat(analyze-live) 3/3 ya en HEAD, workspace quieto al empezar: sin builds ni instancias corriendo, árbol limpio)
**Alcance:** sm-core / sm-db / sm-app / sm-export / sm-usb (+ sm-model/sm-library/sm-audio/sm-i18n lectura)
**Método:** fase de lectura → hallazgos con evidencia → medición (release, N≥5, mediana) → mejora SOLO si equivalencia verificada (tests de caracterización bit-exacta + digests del análisis completo ANTES/DESPUÉS).

---

## 1. Resumen ejecutivo

- **5 lotes de mejoras internas equivalentes aplicados** (sin cambios de comportamiento observable): LUFS en streaming (elimina ~254 MB transitorios por pista de 3 min), planner FFT cacheado para cues, waveform sin ~15 k allocs/pista, resample en una sola pasada, reuso de beats derivados + mono_sum derivado del mono ya computado, `upsert_track` en 1 SELECT, `duplicates()` 3.0× más rápido (BD real 691 pistas: 21→7 ms), exportación Engine DJ **11.7×** más rápida (600 pistas: 3167→271 ms).
- **Equivalencia verificada con digests bit-exactos**: el `TrackAnalysis` completo serializado de 4 fixtures (2 MP3, 1 WAV 44.1k, 1 WAV 48k sintético que ejercita el resample) es **idéntico** antes y después de cada lote (FNV-1a sobre JSON del struct, `analysis_ms` excluido por ser tiempo medido). Tests de caracterización nuevos fijan los bits f64/f32 de LUFS, histograma, waveform, bandas y resample.
- **2 posibles bugs NO corregidos** (regla 3: documentar, no tocar): `band_envelope` filtra el stream interleaved aunque su doc dice "canal L"; `pyramid()` computa el buffer reducido `out` y lo descarta. Ambos cambian resultados observable si se "arreglan" → decisión del dueño.
- Suite completa **289/289** (284 en el HEAD al iniciar —los "279" del encargo eran previos a los últimos commits analyze-live— + 5 tests de caracterización nuevos; los 2 tests temporales de equivalencia se retiraron tras pasar). Build release sin errores.
- **Incidente de concurrencia:** a las 23:07 aterrizó el commit ajeno `3186948` (feat(dev): reset_analysis_full, sm-app/sm-db) MIENTRAS se auditaba; barrió por accidente 2 archivos de andamiaje de esta auditoría (examples audit_equiv/audit_golden) dentro de su commit y añadió `reset_analysis_full()` a sm-db. Los cambios de esta auditoría NO fueron commiteados (siguen como diff del árbol de trabajo, verificados contra el estado combinado: 289/289 incluye ambos); el andamiaje se eliminó del árbol (queda como borrado vs HEAD).

## 2. Fase de lectura — cobertura

- **Docs leídos:** `docs/ARCHITECTURE.md`, `README.md`, `docs/DESIGN.md` (refs), `crates/sm-usb/STRINGS.md` (existencia). **AUSENTES (registrada su ausencia):** `AGENTS.md`, `AI_CONTRIBUTING.md`/`CONTRIBUTING.md`, `CLAUDE.md`. **Sin regiones CODE-AGENT-LOCK** (grep en `crates/`).
- **Inventario funcional (import→análisis→persistencia→UI→export):**
  - *Import:* `sm-app/src/app.rs::import_folder` (hilo efímero: `sm_library::scan_root` walkdir → `read_metadata` (lofty) → `db.upsert_track` + `set_cover`), refresco UI cada 25 archivos.
  - *Análisis:* `sm-app/src/worker.rs::AnalysisPool` (N hilos `sm-analyze-*`, cola `VecDeque` bajo `Arc<Mutex<QueueState>>`, eventos por mpsc, save_analysis bajo lock global de BD) → `sm-core/pipeline.rs::analyze_track` → decode (symphonia/ffmpeg) → beats → energy (segmentación anclada a beats) → cues (ref_cues_engine envelope_exact + cue_ranker/rule) → key (frames paralelos rayon + clasificador lineal JSON) → volume (histograma calibrado, LUFS biquads, clips, waveform 2048×3, bandas 3).
  - *Persistencia:* `sm-db/src/lib.rs` (SQLite WAL, FTS5 sync manual, JSON cues/segments/beatgrid, BLOB bincode waveform/bands, migraciones user_version 1→4).
  - *UI:* `sm-app` egui — `update()` (app.rs:1190): análisis_ms ligero por frame para refrescar detalle solo si cambió; `build_studio_model()` por frame (cap 2000 filas, por diseño del kit); `ensure_rows` paginado 400+prefetch; `run_studio_view` sin acceso a BD por frame (verificado por grep); throttle 1 s del refresh durante análisis (commit analyze-live 2/3); covers LRU (app.rs tests lru_evicta_*).
  - *Export:* `sm-export` — engine DJ (SQLite v2.20.1 DDL verbatim), rekordbox XML, traktor NML, serato crates/markers2 GEOB, CSV/M3U, ID3 con preview; `sm-usb` backup (copy + re-hash del destino, zip deflate re-verificado), scanner USB por PowerShell en hilo propio (commits previos).
- **Baseline registrado:** `cargo build --release` OK (1 m 07 s, 21 warnings preexistentes en sm-app — no tocados, regla 5); `cargo test --workspace` **284/284** (todos los tests del HEAD be47bf2; el "279/279" del encargo correspondía a commits anteriores).
- **BD real (solo LECTURA para conteos):** 691 pistas, todas `analysis_status=done`.

## 3. Hallazgos priorizados

### Aplicados (equivalencia verificada; ver §5)

| # | Hallazgo | Evidencia | Severidad | Estado |
|---|---|---|---|---|
| A1 | `lufs_integrated` materializaba el PCM filtrado: 2 buffers f64 por canal del tamaño de la pista (3 min estéreo 44.1k: `chans` 2×63.5 MB + `filtered` 2×63.5 MB ≈ **254 MB** transitorios por pista, ×N hilos de análisis) | `sm-core/src/volume.rs:99-143` (pre-parche); aritmética de buffers | Alta (memoria/throughput) | **Aplicado** |
| A2 | `waveform_records`: 2 `Vec` nuevos por bloque de 1024 muestras (~15.4 k allocs por pista de 3 min) | `volume.rs:149-167` (pre) | Media | **Aplicado** |
| A3 | `envelope_exact` (cues herramienta de referencia) creaba un `FftPlanner<f32>` nuevo por pista → re-planificación FFT; beats/energy/key ya usan planner thread-local | `ref_cues_engine.rs:323` vs `beats.rs:37`, `energy.rs:58`, `key.rs:282` | Media | **Aplicado** |
| A4 | `pipeline.rs` calculaba `bg.beats().collect()` dos veces (segmentos y cues) y recomponía `mono_sum` recorriendo el PCM interleaved otra vez, aunque con ≤2 canales es `mono·2` bit-exacto (÷2 y ×2 exactos en IEEE-754; verificado por test y por digest) | `pipeline.rs:53,70-80` (pre) | Media | **Aplicado** |
| A5 | `resample_linear` de-interleaba a planar por canal y re-interleaba: 2 buffers extra del tamaño del PCM por pista 48k | `decode.rs:188-221` (pre) | Media | **Aplicado** |
| A6 | `write_engine_dj_library` insertaba sin transacción con `journal_mode=DELETE`: **autocommit con fsync por pista**, ~5.3-6.1 ms/pista medidos (en USB FAT32 sería peor) | `sm-export/src/engine.rs:165-171` (pre); benchmark §6 | Alta (export USB) | **Aplicado** |
| A7 | `duplicates()`: `title_similar` re-normalizaba AMBOS títulos por par candidato (alloc + recorrido de chars dentro del O(pares)) | `sm-db/src/lib.rs:1004-1046,1367-1377` (pre) | Media | **Aplicado** |
| A8 | `upsert_track`: 2 SELECT por pista (id, luego mtime) — en import de 30 k pistas son 30 k consultas extra | `sm-db/src/lib.rs:262-296` (pre) | Baja | **Aplicado** |

### Documentados, NO corregidos (regla 3: requieren decisión)

| # | Hallazgo | Evidencia | Severidad | Propuesta |
|---|---|---|---|---|
| B1 | **`band_envelope` filtra el stream interleaved completo (L,R,L,R…) aunque el comentario dice "sobre el canal L"** (`volume.rs:196`: `let x = interleaved[i]` en un loop `for i in 0..n`). El crossover one-pole recibe la señal alternada, no el canal 0. Alimenta `waveform_bands` persistido (BLOB) y el renderer (bajos azules/medios naranjas/altos blancos). Cambiarlo altera el render y las bandas ya persistidas → NO tocado. | Lectura del loop; cualquier pista: cambiar a `interleaved[i*ch]` cambia el BLOB | Media (doc/código divergen; posible intención original distinta) | Decisión del dueño: (a) corregir SOLO el doc, o (b) cambiar a canal 0 + re-backfill con `examples/backfill_bands.rs`. Repro: comparar `band_envelope` de un estéreo L≠R contra el mismo archivo en mono-solo-L |
| B2 | **`pyramid()` computa el buffer reducido `out` y lo descarta**: escribe la reducción piramidal en `out` pero retorna `v` truncado; nunca hace `v = out`. La reducción (salvo el truncado por nivel) no tiene efecto. Si el port Python/herramienta de referencia hace la reducción real, las envolventes ch1/ch2 (y por tanto cues) difieren de la referencia. | `ref_cues_engine.rs:76-100`: `out` sin uso tras escribirlo | Media (posible port incompleto; comportamiento actual determinista y estable) | Comparar `envelope_exact` contra la referencia Python (harmonic-engine) con `examples/env_parity.rs`; si difiere, corregir `pyramid` como lote separado con caracterización previa (cambia cues → NO es mejora equivalente) |
| B3 | **Errores silenciados en persistencia:** `waveform_bands()` mapea CUALQUIER error de query a `None` (`sm-db/src/lib.rs:628-632`, el `map_err(...).unwrap_or(None)`); `map_track_full` hace `bincode::deserialize().unwrap_or_default()` (BLOB corrupto → waveform vacía silenciosa) y `serde_json(...).ok()` para beatgrid/segments/cues; `worker.rs:103` `let _ = db.save_analysis(...)` — si falla la persistencia la UI recibe `Finished` igualmente (pista "analizada" sin datos en BD). | Lectura directa | Media (robustez) | Registrar el error (tracing) y/o emitir `Failed` cuando save_analysis falle. Cambia manejo de errores observable → decisión |
| B4 | **`import_folder` hace upsert+cover por archivo sin transacción** (app.rs:1066-1090). WAL+`synchronous=NORMAL` lo amortigua, pero en import grande cada archivo = ~5 statements autocommit. Un batching (p.ej. 200 pistas/tx con `db.begin()`, que ya existe) reduciría commits; la cadencia del progreso visible pasaría a pasos de 200 → cambio observable menor | Lectura; A8 mitiga parte (1 SELECT menos) | Baja | Batching transaccional opt-in; requiere decisión del dueño sobre la granularidad de progreso |
| B5 | **`copy_and_verify` (sm-usb) lee el archivo destino completo para verificar el SHA después de copiar** — 2 lecturas totales por archivo. Intencional (verifica lo que quedó en la USB), pero un hash streaming durante la copia evitaría la relectura | `sm-usb/src/backup.rs:225-241` | Baja | `io::copy` con wrapper que hashea al escribir + re-leer solo en `Mismatch` dudoso; medir en USB real |
| B6 | **`build_studio_model()` se reconstruye cada frame** (app.rs:1883; cap 2000 filas × 2 String clones c/u). Por diseño del kit y con throttle de refresh ya ajustado por analyze-live; costo estimado (no medido por frame) ~0.1-0.2 ms | Lectura | Baja (informativo) | Cache con invalidación por eventos si algún día limita el frame time |
| B7 | **`engine.rs::track_row`**: por pista hace búsqueda lineal de 47 columnas sobre un array de 47 (~1100 comparaciones de &str, ~20 µs) — irrelevante tras A6 | `engine.rs:241-250` | Trivial | Construir la fila directo en orden de columnas si se toca el archivo por otra razón |
| B8 | **Cola de análisis sin límite** (`worker.rs::enqueue`) — con 30 k pistas ~3 MB, sin problema real; documentado por la categoría "colas sin límite" | Lectura | Trivial | Nada (dejar como está) |

## 4. Equivalencia — qué se comparó y qué NO

**Comparado (bit-exacto):**
- Digest FNV-1a del `TrackAnalysis` COMPLETO (serde_json, todos los campos: key/confianzas/2ª key, bpm, beatgrid, energía global, segmentos, cues, volumen, LUFS, clips, waveform, bandas, duración) sobre 4 fixtures × 5 corridas, ANTES de tocar nada y DESPUÉS de cada lote: `15d21bacf78e8117` / `c2b92220720be4a7` / `954b906e8a936419` / `9c0958c843c4c31b` — **idénticos en los 5 estados**. `analysis_ms` se excluye del digest (es tiempo medido, no determinista por naturaleza).
- Tests de caracterización NUEVOS y permanentes (pasan sobre el código pre-parche, generado con un example temporal): bits de `lufs_integrated` (estéreo L=−R y mono, y rama −70), hash de bits de `waveform_records`+`display_waveform`+`band_envelope` (estéreo y mono), histograma+volumen, `resample_linear` 48k→44.1k (estéreo y mono). Ver `volume.rs` (`characterization_*`) y `decode.rs` (`characterization_tests`).
- `duplicates()`: test temporal (retirado) que replica el algoritmo de HEAD y lo compara sobre 402 pistas sintéticas con mutaciones de título (38 grupos idénticos) y sobre una COPIA de la BD real (691 pistas, 13 grupos idénticos).
- `write_engine_dj_library`: 67 tests de export pasando (incluidos los de contenido Engine DJ) + medición de contenido equivalente por tests existentes. La base de datos no es byte-comparable ni antes del cambio (uuid y playedIndicator salen de `SELECT random()`, dateCreated/dateAdded de `SystemTime::now`).
- Pruebas bit-exactas del argumento mono·2: `mono_por_dos_es_suma_bit_exacto_hasta_2_canales` (decode.rs). El camino >2 canales conserva la suma directa original.
- Argumento del prefiltro de longitud en `title_similar_norm`: `lev ≥ |len_a−len_b|` ⇒ ratio ≤ 1−diff/max; se salta el Levenshtein solo cuando el límite superior YA es < tol → decisión idéntica (comentario en el código + test de equivalencia).

**NO comparado / limitaciones:**
- No hay fixtures >2 canales en el corpus de prueba; el camino `channels>2` de `mono_sum` se conserva verbatim (sin derivación).
- No se midió el frame time de la UI (egui) ni el consumo de memoria con profiler; las cifras de memoria de A1/A2 son aritmética de los buffers eliminados (tamaños calculados), no mediciones de RSS.
- ffmpeg/wma no ejercitado (sin fixture wma); esa vía no se tocó.
- El renderer del waveform no se tocó en absoluto (regla dura).

## 5. Cambios aplicados (archivo : motivo)

| Lote | Archivo | Cambio |
|---|---|---|
| 1 | `crates/sm-core/src/ref_cues_engine.rs` | `envelope_exact`: `FftPlanner` thread-local (plan cacheado entre pistas, patrón ya usado por beats/energy/key). +7/−2 líneas |
| 2 | `crates/sm-core/src/volume.rs` | `lufs_integrated` en streaming (sin materializar PCM filtrado; mismo orden de acumulación por bloque y canal); `waveform_records` sin gathers por bloque (mismos índices/orden, sumas f32 secuenciales, len L/R separados para block impar). + tests de caracterización (3) |
| 3 | `crates/sm-core/src/pipeline.rs` | `beats_times` computado UNA vez (segmentos y cues); `mono_sum` derivado de `mono·2` con ≤2 canales (suma directa si >2). + `crates/sm-core/src/decode.rs`: `resample_linear` en una sola pasada interleaved; + tests de caracterización (2) |
| 4 | `crates/sm-db/src/lib.rs` | `upsert_track` en un solo SELECT (id+mtime); `duplicates()` con títulos normalizados precomputados y prefiltro de longitud exacto en la similitud (title_similar_norm) |
| 5 | `crates/sm-export/src/engine.rs` | INSERTs de pistas dentro de UNA transacción con commit explícito antes del `fs::copy` |

Diffs limpios por lote, verificados de forma independiente (cada lote: tests del crate + digests). Sin commits (regla del encargo). Archivos de andamiaje retirados tras la validación: `crates/sm-core/examples/audit_equiv.rs`, `crates/sm-core/examples/audit_golden.rs`, `crates/sm-db/tests/audit_dupes_equiv.rs`, `crates/sm-export/tests/audit_engine_bench.rs`.

## 6. Rendimiento antes/después (release, N=5–7, medianas; misma máquina, fixtures copiados a temp — nunca la biblioteca del dueño)

**Análisis completo por pista (`audit_equiv`, 5 corridas, mediana ms):**

| Fixture | Antes | Después | Δ |
|---|---|---|---|
| MP3 3:36 (2 Times) | 1807 | 1638 | **−9.4 %** |
| MP3 5:14 (Determined) | 2392 | 2200 | **−8.0 %** |
| WAV 44.1k ~5 min (Breathe) | 3048 | 2672 | **−12.3 %** |
| WAV 48k sintético 8 s | 74 | 63 | **−14.9 %** |

Memoria transitoria eliminada por pista de 3 min estéreo (calculada, no perfilada): LUFS 4×63.5 MB → ~KB; waveform_records ~15.4 k allocs → 0; resample (solo 48k) 2 buffers PCM → 0; mono_sum 1 buffer PCM menos (ahora del mono ya computado).

**`duplicates()` (BD real COPIA, 691 pistas, 7 corridas, mediana):** 21 ms → **7 ms (−66 %, 3.0×)**, grupos idénticos (13).

**`write_engine_dj_library` (5 runs fríos, directorios distintos, mediana ms):**

| Pistas | Antes | Después | Δ |
|---|---|---|---|
| 100 | 613 | 179 | **3.4× (−71 %)** |
| 600 | 3167 | 271 | **11.7× (−91 %)** |

(`upsert_track`: micro-mejora no medida de forma aislada — elimina 1 de ~5 sentencias por pista en import.)

## 7. Validaciones (comandos y resultados)

- `cargo test --workspace` (estado final, incluye el commit ajeno 3186948) → **289 passed, 0 failed** (284 del HEAD inicial + 5 de caracterización; los 2 tests de equivalencia temporales pasaron y se retiraron). El run intermedio con los tests temporales dio 292/0.
- `cargo build --release` → OK (sin warnings nuevos; los 21 preexistentes de sm-app intactos, regla 5).
- `cargo run -p sm-core --release --example audit_equiv -- --runs 5 <4 fixtures>` → digests idénticos al baseline dorado en cada lote y en el estado final.
- `cargo test -p sm-db --release --test audit_dupes_equiv` (temporal) → equivalencia sintética (38 grupos) y BD real (13 grupos) OK + tiempos.
- `cargo test -p sm-export --release --test audit_engine_bench` (temporal) → antes/después §6.

## 8. Propuestas pendientes (necesitan decisión del dueño)

1. **B1 band_envelope/canal L** — corregir doc (gratis) o cambiar a canal 0 + re-backfill de `waveform_bands` (cambia bandas persistidas y render → no es equivalente).
2. **B2 pyramid()** — validar contra la referencia Python (`env_parity`); si el port quedó corto, corregir como lote aparte con caracterización (cambia cues).
3. **B3 errores silenciados en sm-db/worker** — logging + evento `Failed` si `save_analysis` falla (cambia manejo observable de errores).
4. **B4 import_folder sin batching transaccional** — 200 pistas/tx cambiaría la cadencia de progreso visible.
5. **B5 copy_and_verify streaming-hash** — eliminar la relectura del destino midiendo en USB real.

## 9. Riesgos y límites

- La mejora de LUFS depende del argumento de bit-exactitud del orden de acumulación (verificado por digest y tests); si un futuro cambio de compiler/optimización reasocia sumas (no debería: Rust/LLVM respeta el orden de `+=` en f64), los tests de caracterización lo detectarán al instante.
- La transacción de Engine DJ altera el estado intermedio ante un fallo a mitad de export (quedaría la base con schema sin filas en vez de parcialmente poblada); el destino se rechaza si `m.db` existe, así que el flujo de reintento es el mismo. Per-row failure es virtualmente imposible (`INSERT OR IGNORE`, sin FK enforcement).
- No se afirmó "sin regresiones" más allá de la evidencia: digests bit-exactos en 4 fixtures + 284 tests + equivalencia duplicates en 2 corpus. Formatos wma/ffmpeg y pistas >2 canales siguen sin cobertura de fixture (código intocado o camino verbatim).

## Decisiones del dueño (2026-11-17)

- **B1 (band_envelope interleaved): DECIDIDO — MANTENER** el filtrado del stream
  interleaved completo. Sin cambio de código ni re-backfill; la documentacion
  ya fue corregida (T2, commit 307bc80). El render actual es el aprobado.
- **B2 (pyramid/cues): AUTORIZADO** — validar contra la referencia Python
  (harmonic-engine, env_parity) y, con evidencia clara de port incompleto,
  corregir + caracterizacion + re-analisis completo de la biblioteca.

## T7 — Resolución B2: pyramid() era un port incompleto; corregido y verificado (2026-11-17)

**FASE A (lectura).** `pyramid()` de `ref_cues_engine.rs` computaba el buffer
reducido `out` y lo DESCARTABA (`v.truncate(o)` conserva el frente SIN reducir).
La referencia Python (`_pyramid`, ref_waveform_env.py — validada EXACTA contra
el blob ZWAVEFORM de herramienta de referencia) hace `v = out[:o]`. Comparación sobre inputs
sintéticos idénticos (example temporal `t7_env_parity` + dumps Python, ambos
alimentados con el MISMO mono):

- Rust PRE-fix vs Python **sin** reducción (réplica exacta del bug): corr
  1.000000, maxd ≤ 2.4e-5, mean ≤ 8e-7 (ruido f32 de rustfft vs scipy.fft) →
  **el Rust actual equivalía a Python SIN pyramid**.
- Rust PRE-fix vs Python CON reducción: corr 0.98–0.99 (ch1 real 0.992/0.980,
  ch2 0.987/0.998), maxd hasta 4.6 (valores O(1)) → divergencia ESTRUCTURAL.
- A nivel de magnitudes (entrada directa de pyramid): divergen desde la banda
  80 (head) en 127/207 bandas (impulso/rampa/ruido/musical); largos idénticos
  (766→423→271→215→207) — el bug cambia VALORES, no la cuenta de bandas.

**FASE B (fix, decisión del dueño aplicada).**
- Test de caracterización ANTES del cambio: `pyramid_matches_python_reference`
  (3 inputs deterministas reproducibles sin RNG compartida: impulso, rampa,
  xorshift32) contra el output Python bit-exacto. RED con el bug (primera
  divergencia banda 90: 1e-4 crudo vs 1e-2 = media geométrica ausente).
- Fix: `out.truncate(o); v = out;` (2 líneas). GREEN bit-exacto vs Python.
- Post-fix a nivel envolvente: Rust vs referencia corr 1.000000, maxd ≤ 8e-5
  en sintéticas y en pistas reales del corpus (ambos canales) — el port ya es
  fiel a la referencia.
- `cargo test --workspace`: **301 passed, 0 failed** (291 del inicio de rama +
  tests de T4/T5 + 1 nuevo).
- Digests FNV-1a del TrackAnalysis completo (analysis_ms excluido) en 4
  fixtures (2 MP3 corpus, 1 WAV 44.1k, 1 WAV 48k sintético), antes/después:
  cambia SOLO `cues` (f1: 3/8 cues movidas; f3: 2/8; f2/f4 idénticos) — key,
  bpm, beatgrid, energía, waveform, LUFS intactos, como espera el diseño
  (la envolvente herramienta de referencia solo alimenta los scores de cues).
- Paridad vs ground truth herramienta de referencia (measure_parity.py, corpus 92 pistas/712
  cues, mismo harness antes/después): **±1 beat 555→566 (77.9%→79.5%)**,
  **±20 ms 86→91 (12.1%→12.8%)**, **count_match 74→77/92**; huérfanas nuestras
  24.6%→23.1%. Fase de cues cubiertas ~+0.17s sin cambio (el sesgo de
  anclaje del selector sigue siendo la palanca pendiente, ver
  docs/CUE-PARITY-DIAGNOSIS.md).
- Re-análisis de la biblioteca del dueño: backup byte a byte verificado
  (`sm_shots/backups/library_pre_t7_*.db`), reset sqlite (patrón
  `reset_analysis_full`, 691 filas → pending, user_version intacto), lote vía
  hook QA `SM_PLAY=1 SM_ANALYZE=691`: **`lote terminado: 691/691 ok,
  0 fallidas`** (log `sm_shots/t7/reanalysis.log`). BD verificada post-lote:
  691 done, 691 con cues/key, 5.528 cues, min 0.00027 s, **0 cues negativos**.
