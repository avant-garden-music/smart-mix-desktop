# Inventario funcional verificado — Mixed In Key 11 (Standard / Pro)

**Proyecto:** Smart Mix (app de escritorio Rust, paridad funcional frente a Mixed In Key 11 Standard/Pro)
**Fecha de elaboración:** 2026-10-19 · **Entorno:** Windows 11 · **Versión instalada:** Mixed In Key **11.1.0.846** ( exe `MixedInKey.exe`, ventana "Mixed In Key 11" )

## Método y restricciones respetadas

1. **Inspección READ-ONLY del disco** en: `C:\Users\Crist\AppData\Local\Programs\Mixed In Key`, `%LOCALAPPDATA%\Mixed In Key`, `%LOCALAPPDATA%\Mixed_In_Key_LLC`, `C:\Users\Crist\Documents` (colecciones XML/NML) y carpetas DJ de `Music\`. No se modificó, movió ni borró nada de esas rutas. La BD `MIKStore.db` se abrió en modo `readonly` (URI `mode=ro`).
2. **Sin descompilación** de binarios y **sin copiar assets/código propietario**: solo nombres de archivos, esquemas de datos y comportamientos. Muestras de datos **anonimizadas** (máx. 20 líneas por formato, sin datos personales del usuario).
3. **Investigación web** de la documentación pública (citas `[n]`, ver sección *Sources*).
4. **Observación UI**: se lanzó la app real, se esperó ~15 s, se capturó **solo la ventana de MIK** (API `PrintWindow`) → `C:\Users\Crist\Dev\research\mik11-main-window.png`. No se cambió ningún ajuste, no se analizó ningún archivo nuevo, no se pulsó ningún botón de exportación/escritura; la ventana se cerró de forma normal (`CloseMainWindow`, cerró sin forzar). Dos capturas intermedias de escritorio completo se **descartaron y eliminaron** por contener contenido privado ajeno a la tarea.

> **Hallazgo crítico para el equipo:** en un `.aiff` de la biblioteca analizada se encontró el tag `COMM = "SM: analyzed by Smart Mix"` y `TKEY = "A"` (valor inválido). **Smart Mix ya ha escrito tags de prueba en esta biblioteca**; cualquier test de comparación MIK vs Smart Mix debe usar archivos limpios o restaurar tags antes de medir (ver A.5 y §5).

---

## 1. Resumen ejecutivo

- MIK 11 es una app **.NET 4.x / WPF** (MaterialDesign + MahApps) con motor de análisis nativo (DirectShow filters `EnergyAnalysis.ax`, `MagellanAnalysisFilter.dll`, `Danceability.AudioEngine.dll`), ORM **NHibernate + Dapper sobre SQLite** (`MIKStore.db`), tagging con **TagLibSharp**, decodificación **FFmpeg**, pitch-shift **Zplane élastique Pro 3** y **stems vía ONNX Runtime + DirectML** (modelos `sep01_medium.bin` 176 MB / `sep01_high.bin` 253 MB) — todo observado en disco.
- **Standard ($58, compra única)** = análisis (Key/BPM/Energy), ≤8 cue points automáticos, edición de tags, playlists. **Pro ($99) = Standard +** mashups/decks/mixouts/stems/pitch-shift/loops [1][4].
- Versiones de algoritmo observadas en `user.config`: **Key 94, Energy 13, CuePoints 14, BeatGrid 12**.
- La colección del usuario tiene **125 pistas analizadas** (`.aiff` 112, `.mp3` 11, `.wav` 2), Energy 4–8, Tempo 112–136, keys Camelot 1A–12A/B — coincide con "All music (125)" visto en UI.
- Persistencia: colección en **SQLite** (`MIKStore.db`, SchemaVersion 11009), energía/keys por segmento como **JSON** en `SerializedSongStructure.Data`, caché de análisis por archivo en `AudioAnalysisCache3\*.wfd` (125 archivos ≈ 125 pistas).

---

## 2. Instalación observada (frente 1 — solo lectura)

### 2.1 Rutas

| Ruta | Contenido |
|---|---|
| `C:\Users\Crist\AppData\Local\Programs\Mixed In Key\Mixed in Key 11\` | Binarios: `MixedInKey.exe` (16.8 MB), DLLs de análisis/UI, modelos de stems, samples de piano |
| `C:\Users\Crist\AppData\Local\Mixed In Key\Mixed In Key\11.0\` | Datos por versión: `MIKStore.db` (2.1 MB), `Config.cache`, `midi.log`, `StemSeparationEngineReport.json`, `AudioAnalysisCache3\` (`.wfd`), `QueryLogs\`, `OfflineEvents\Analytics\` (`.ofl1`), `Temp\WPF\` |
| `C:\Users\Crist\AppData\Local\Mixed_In_Key_LLC\MixedInKey.exe_Url_<hash>\11.1.0.846\` | `user.config` (ajustes de usuario .NET, patrón ClickOnce) |
| `C:\Users\Crist\Documents\Native Instruments\Traktor 4.4.1\` | `collection.nml` (13 entradas, 194 `CUE_V2`) — destino de integración Traktor |
| `C:\Users\Crist\Documents\rekordbox\`, `Music\PioneerDJ\`, `Music\rekordbox\`, `AppData\Roaming\Pioneer\` | rekordbox 7.2.x presente; no se encontró XML de exportación de colección en las rutas exploradas |
| `C:\Users\Crist\Music\all\` | Biblioteca del usuario (125 archivos); varios con **renombrado estilo MIK** `Key_-_Tempo_-_Titulo_-_Artista.aiff` |

### 2.2 Binarios relevantes y qué implican

| Archivo (directorio de instalación) | Rol inferido (solo por nombre/dependencias) | Relevancia para Smart Mix |
|---|---|---|
| `Danceability.AudioEngine.dll` (3.8 MB) | Motor de análisis (key/energy/BPM) | Núcleo a replicar en Rust |
| `EnergyAnalysis.ax`, `MagellanAnalysisFilter.dll`, `MP3Source.dll`, `ClipRepair.ax` (+ `.sxs.manifest`) | Filtros DirectShow de análisis por etapas | Pipeline por etapas; no aplicable tal cual en Rust |
| `avcodec-57/avformat-57/avutil-55.dll`, `FFmpegLock.dll` | Decodificación de audio (FFmpeg 3.x/4.x) | Paridad: decodificar mp3/wav/aiff/flac/m4a/mp4/wma |
| `TagLibSharp.dll` | Lectura/escritura de tags ID3/AIFF | Ídem con `lofty`/`id3` en Rust |
| `ElastiqueProV3.dll` | Pitch-shift/time-stretch (Zplane) | Feature Pro (cambiar key) |
| `onnxruntime.dll` + `DirectML.dll` + `sep01_medium.bin`/`sep01_high.bin` | Stems (inferencia ONNX por GPU DirectML, 2 calidades) | Feature Pro; requisitos GPU ≥6 GB [4] |
| `NAudio.dll` | Reproductor interno (decks/waveform) | Paridad de audición |
| `System.Data.SQLite.dll` + `x64\SQLite.Interop.dll`, `NHibernate/FluentNHibernate/Dapper` | Persistencia SQLite + ORM | Smart Mix puede usar `rusqlite` con esquema similar |
| `Emoji.Wpf.dll` | Emojis en playlists [1][6] | Feature de playlists |
| `MaterialDesignThemes.Wpf`, `MahApps.Metro.IconPacks`, `ControlzEx` | UI WPF (tema claro/oscuro) | Referencia visual |
| `Sentry.dll`, `Sentry.Protocol.dll` | Crash reporting | Telemetría (opcional en Smart Mix) |
| `System.Data.HashFunction.xxHash.dll` | Hashing (probablemente del caché `.wfd`/FilePathHash) | Ver A.4 |
| `Autofac*`, `Polly`, `Newtonsoft.Json`, `PListNet`, `Stfu.dll` | DI, resiliencia, JSON, utilidades | — |
| `Resources\Samples\Piano\{Major,Minor,Tonic}\Octave1-3_*.mp3` (108 mp3 ≈20 KB) | Samples de Grand Piano (12 notas × 3 octavas × 3 conjuntos) para verificar la key de oído [1][5] | Feature P2 |

### 2.3 Otros archivos de datos (estructuras en Apéndice A)

- `Config.cache` (115 KB): caché binaria de NHibernate 3.3.1 (`BinaryFormatter`); no es un formato interoperable → ignorar para paridad.
- `midi.log`: enumera dispositivos MIDI (en esta máquina `MPK mini 3`) → soporte de controladores MIDI (alcance exacto no documentado; hipótesis: piano/decks).
- `StemSeparationEngineReport.json`: métricas de los 2 modelos de stems (ver A.7).
- `QueryLogs\*.txt`: log de SQL generado por Dapper (INSERT de `Song`/`SongSegment`/`SerializedSongStructure`) — confirma autoría del esquema y versión 11.1.0.846.
- `OfflineEvents\Analytics\*.ofl1`: payload binario comprimido/encriptado de telemetría offline (252 B).

---

## 3. Tabla maestra de features

Fuente = `disco` (observado-en-disco) · `UI` (observado-en-UI, captura §4) · `doc` (documentado-público, cita `[n]`) · `hip` (hipótesis). Prioridad: **P0** núcleo de paridad (MVP), **P1** segunda ola, **P2** diferencial/Pro.

### 3.1 Análisis (Standard = ambas ediciones)

| ID | Feature | Edición | Fuente | Comportamiento observado/esperado | Test de aceptación sugerido (Smart Mix) | Prior. |
|---|---|---|---|---|---|---|
| F-01 | Detección de tonalidad (Key) | Ambas | doc[1][3] + disco | Tonalidad en notación Camelot (p.ej. `9A`, `7B`) y nombre musical (`G-Sharp Minor`); `MainKey`+`MainKeyConfidence`, `SecondKey`+confianza en BD; algoritmo v94 | Sobre el corpus de 125 pistas ya analizadas: ≥90 % de coincidencia exacta con `MainKey` de MIKStore.db y 100 % en equivalente enarmónico (misma pitch-class) | **P0** |
| F-02 | Key por segmento + confianza | Ambas | disco | Tabla `SongSegment` (StartTime/EndTime/KeyResult/KeyConfidence/IsSingleNote) y `KeySegments` en el JSON de estructura | Reproducir segmentación: ≥1 segmento por pista; key global = moda ponderada por duración | P1 |
| F-03 | Detección de BPM/Tempo | Ambas | doc[1] + disco | `Tempo` real (p.ej. 111.999…–135.999…) en BD; TBPM entero en tags | ±0.35 BPM vs MIK en ≥95 % del corpus (el margen cubre redondeo 111.999→112) | **P0** |
| F-04 | Beat grid | Ambas | disco (`BeatGrid` v12) + doc[2] | MIK recomienda que el DJ software prepare BPM/grid primero para cues alineados [2] | (Opcional) beatgrid con error < 20 ms frente a downbeats de referencia | P1 |
| F-05 | Energy Level global (1–10) | Ambas | doc[1][5] + disco | `OverallEnergy` 4–8 en el corpus; columna UI con flecha de tendencia | Igualdad exacta ≥60 % y ±1 en ≥90 % del corpus | **P0** |
| F-06 | Energy por segmento | Ambas | disco + doc[5] | `EnergySegments` JSON (~8–12 tramos con start/end/energy); "Energy Level breakdown" en UI web [5] | Segmentos con duración 20–60 s; energía global = max o media alta de tramos; correlación S≥0.7 con MIK | P1 |
| F-07 | Cue points automáticos (≤8) | Ambas | doc[1][3] + UI | Chips `Cue 1..8` sobre la waveform; algoritmo CuePoints v14; marcas en momentos estructurales (intro/verso/drop/mix-out) [3] | 4–8 cues por pista, todos en downbeat (fase 0 del beat), 1.º cue ≈ primer beat audible; distancia mínima entre cues ≥ 16 compases | **P0** |
| F-08 | Snap Cue | Ambas | doc[5][6] + UI | Ajusta el cue más cercano a la posición de reproducción "como imán" [6] | Al pulsar snap con playhead en t, el cue activo pasa a la posición t (cuantizada a beat) | P1 |
| F-09 | Edición manual de cues | Ambas | UI | Botones `−/+` y ojo (mostrar/ocultar) junto a "Cue points" | Añadir/eliminar cue persiste tras reiniciar y respeta el límite de 8 | **P0** |
| F-10 | Análisis de volumen / clipping | Ambas | disco | Columnas `OverallVolume`, `OverallVolumeRMS1/RMS2`, `OverallVolumeLUFS`, `ClippedPeaksCount`; `ClipRepair.ax` (pipeline Platinum Notes) | Detectar clipping con cuenta de picos ≥ umbral ±20 % vs MIK; LUFS ±1 | P1 |
| F-11 | Caché de análisis por archivo | Ambas | disco | `AudioAnalysisCache3\<32-hex>.wfd` (121–125 archivos ≈ canciones) evita re-analizar | Mismo archivo re-analizado → resultado idéntico sin recomputar; invalidación por tamaño/mtime | P1 |
| F-12 | Formatos de entrada | Ambas | doc[1][4] + disco | Win: mp3, wav, aiff/aif, flac, m4a(AAC), mp4, wma; Mac añade ALAC [4]. Corpus real: aiff/mp3/wav | Decodificar los 7 formatos Win de prueba (1 archivo por formato) sin error y con misma key/BPM que MIK | **P0** |

### 3.2 Colección, navegación y UI (Standard = ambas)

| ID | Feature | Edición | Fuente | Comportamiento | Test de aceptación | Prior. |
|---|---|---|---|---|---|---|
| F-13 | Pestañas principales | Ambas | UI | `My collection` (activa), `Pro` (badge "New"), `Tag editor`, `How-to guide`, `Settings`; toggle claro/oscuro y volumen arriba a la derecha | Navegación por 5 vistas con estado persistente al reiniciar | P1 |
| F-14 | Vista de colección con tabla | Ambas | UI + disco | Columnas configurables (guardadas en `user.config`): Sequence, Cover Art, Artist, Title, Key, Tempo, Reference Key, Energy, CuePoints, Status, Comment, DateAdded, FileType, FileSize, Rating, Genre, Year, Album, Label, Remixer, Composer, Grouping, Bitrate, SampleRate, Volume, ClippedPeaks, File | Mostrar/ocultar/ordenar/redimensionar columnas persistente | **P0** |
| F-15 | Chips de key codificados por color | Ambas | UI | Cada key Camelot con color propio (p.ej. 7B rosa, 2A verde, 9A morado); UI muestra `Key` (Camelot) y `Key Name` (musical) | Mapa color↔key estable y consistente entre tabla y detalle | P2 |
| F-16 | Búsqueda | Ambas | UI | Campo "Search" sobre la vista activa | Filtrado instantáneo por artista/título/key | **P0** |
| F-17 | Filtros avanzados | Ambas | doc[6][7] + UI | 4 desplegables (key/tempo/energy/género) + "Reset" | Combinar 2 filtros reduce la tabla al subconjunto esperado | P1 |
| F-18 | Playlists anidadas + emojis | Ambas | doc[1][6] | Carpetas dentro de carpetas; campo `Emoji` en tabla `Collection`; emojis en UI (Emoji.Wpf.dll) | Crear árbol playlist>subplaylist con emoji y arrastrar pistas; persiste en BD | P1 |
| F-19 | Orden manual de pistas | Ambas | doc[6] | Reordenar canciones a mano (`Sequence` en `SongCollectionMembership`) | Drag&drop cambia `Sequence` y se conserva | P1 |
| F-20 | Vistas fijas de la barra lateral | Ambas | UI + disco | `All music (125)`, `Analysis queue`, `Recently added (0)`, `Improve tracks (121)` (badge PN) + botón `Add tracks` | Contadores correctos tras añadir/analizar | P1 |
| F-21 | Rueda Camelot interactiva | Ambas | UI + doc[7] | Panel izquierdo colapsable con rueda 1A–12A / 1B–12B coloreada [7] | Filtrar la tabla al pulsar un sector; colapsar/expander persiste | P2 |
| F-22 | Reproductor interno + waveform | Ambas | UI | Waveform azul con playhead rojo, zoom +/−, transporte, 01:00/4:21, chips de cues | Reproducir aiff/mp3/wav; seek fluido; zoom de waveform | P1 |
| F-23 | Panel Song Info (slide-in) | Ambas | doc[5][6] | Editar title/artist/album/key/energy/tempo/genre | Editar campo → actualiza BD + tag al re-analizar/re-taggear | P1 |
| F-24 | Tag editor (vista dedicada) | Ambas | UI (pestaña) + doc[5] | Segunda vista de edición masiva de tags; "second audio player in the tag editor" [7] | Edición masiva de N pistas y escritura de tags | P1 |
| F-25 | Tema claro/oscuro | Ambas | UI + doc[1] | Toggle arriba; claro por defecto en esta máquina | Cambio de tema sin reinicio; preferencia persistente | P2 |
| F-26 | Rating (estrellas) | Ambas | disco + UI | Columna `Rating` en BD y en tabla Análisis y Mashup | Rating 1–5 persistente | P2 |
| F-27 | Guías de integración in-app | Ambas | doc[2][3] | Pestaña "How-to guide" con tutoriales por plataforma DJ | (Paridad parcial) abrir guías por plataforma desde ajustes | P2 |
| F-28 | Migración desde MIK 10 | Ambas | disco | Ajuste `MIK10ImportDontAskAgain` en user.config | Importar colección MIK10 (fuera de alcance v1: no hay MIK10 local) | P2 |

### 3.3 Tags y exportación (Standard = ambas)

| ID | Feature | Edición | Fuente | Comportamiento | Test de aceptación | Prior. |
|---|---|---|---|---|---|---|
| F-29 | Escritura de tags configurable | Ambas | doc[1][3] + disco | "Tag Options": escribir Key/BPM/Energy en Comments y/o campos comunes; "Update Common Tags", "Update Custom Initial Key"; posición del comentario (`TagFormat=BeforeComments`); modo `KeyAndEnergyVerbose`; notación elegible Camelot/Standard/Sharps/Flats [2][3][6] | Escribir en un mp3 de prueba: COMM=`"9A - Energy 6"`, TKEY y TBPM correctos; no alterar otros frames | **P0** |
| F-30 | Renombrado de archivos | Ambas | disco | `FileNameFormat=Name_Key_Tempo`; archivos reales `A_Major_-_120_-_Deceive_...aiff`, `A#_Minor_-__-_Obama_...aiff` | Renombrar copia de prueba a `<Nombre>_<Key>_<Tempo>.<ext>` con espacios→`_` | P1 |
| F-31 | Limpieza de tags | Ambas | doc[5] | "Clean up your ID3 tags… delete junk" | Eliminar comentarios basura preservando TIT2/TPE1/TALB/APIC | P1 |
| F-32 | Export cues → Serato | Ambas | doc[2] | Escribe markers estilo Serato; Serato: "Rescan ID3 Tags" al final [2] | Tras export, Serato DJ Pro muestra ≤8 hotcues en posiciones idénticas (±10 ms) | **P0** |
| F-33 | Export cues → Traktor | Ambas | doc[2] + disco | Traktor cerrado durante análisis; "Check Consistency" después; key visible en columna **Key Text**; cues como `CUE_V2` en `collection.nml` [2] | Tras export + Check Consistency, `collection.nml` contiene `CUE_V2` con START≈cue MIK (±20 ms) | **P0** |
| F-34 | Export cues → rekordbox | Ambas | doc[2] | Flujo XML: exportar colección rekordbox → conectar XML en MIK → importar playlist "MIK Cue Points" [2] | Playlist de cues importable en rekordbox 7 con posiciones ±20 ms | **P0** |
| F-35 | Integración Engine DJ / Denon | Ambas | doc[2] | Tags key/energy + "Reimport Track Information"; notación Camelot en Engine [2] | Tags legibles por Engine DJ (hardware no disponible → validar solo tags) | P1 |
| F-36 | Integración VirtualDJ | Ambas | doc[2] | Usa la opción **Serato** de MIK para cues; VDJ: Batch→Reload Tags [2] | Cues visibles en VDJ tras Reload Tags | P1 |
| F-37 | Colecciones espejo por plataforma | Ambas | disco | Tabla `LibraryType` = {MixedInKey, Serato, Traktor, rekordbox} con nodo raíz `Collection` cada una | (Hipótesis de diseño) esquema multi-destino en la BD de Smart Mix | P2 |

### 3.4 Solo Pro

| ID | Feature | Edición | Fuente | Comportamiento | Test de aceptación | Prior. |
|---|---|---|---|---|---|---|
| F-38 | Mashup Mode (deck A/B + sugerencias) | Pro | doc[1][4][8] + disco | Cargar pista en Deck A → sugerencias para Deck B; filtros por energy/tempo/playlist | Dada una pista en A, sugerir ≥5 pistas compatibles (key compatible ±1 Camelot y BPM ±6 %) | P1 |
| F-39 | DJ Mix Mode (mixout ideas) | Pro | doc[1][4][8] | Sugerencias de "siguiente pista" por key/energy/BPM; guardar en playlist | Cadena generada respeta compatibilidad armónica en ≥90 % de transiciones | P1 |
| F-40 | Dos decks de audición | Pro | doc[1][8] | Reproducción sincronizada A/B para probar pares | Play simultáneo con sync de tempo/key audibles | P1 |
| F-41 | Stem separation + export | Pro | doc[1][4] + disco | Vocal/instrumental/full; 2 modelos (Medium/High) ONNX+DirectML; export a DAW; GPU ≥6 GB [4]; métricas SDR en A.7 | Separar 1 pista: SDR vocal ≥8 (Medium) medida con museval o similar; export WAV 2 stems | P2 |
| F-42 | Pitch shift / cambiar key | Pro | doc[1] + disco | `ElastiqueProV3.dll`; cambiar key y re-sugerir mashups | Shift ±2 semitonos sin artefactos audibles; tempo estable ±0.1 % | P2 |
| F-43 | Looping de secciones | Pro | doc[1] | Loop de frase vocal/instrumental durante audición | Loop seamless de 4–32 beats en cue activo | P2 |
| F-44 | Favoritos de mashups (rating) | Pro | disco + doc[8] | Tabla `Mashup` (SongA/SongB + stem + pitchshift + Rating + AddedUtc) | Guardar/valorar par A/B; persiste en BD | P2 |
| F-45 | Requisito de activación Pro | Pro | doc[8] | "Hay que cargar 100 canciones para acceder a las features Pro" [8] | N/A (nota de gating de producto) | — |

### 3.5 Infraestructura y plataforma

| ID | Feature | Edición | Fuente | Comportamiento | Test de aceptación | Prior. |
|---|---|---|---|---|---|---|
| F-46 | Soporte MIDI controllers | Ambas | disco (midi.log) | Enumera/abre dispositivos MIDI (MPK mini 3 en esta máquina); alcance no documentado (hipótesis: piano/decks) | Detectar controlador y mapear notas del piano | P2 |
| F-47 | Integración Platinum Notes | Ambas | disco + doc[5] | Columnas `HasPNTag/PNTag*`; banner `PlatinumNotesRecommender`; playlist "Improve tracks (121)" [5] | Detectar pistas "mejorables" por clipping/volumen sin PN | P2 |
| F-48 | Telemetría y crash reporting | Ambas | disco | Sentry + OfflineEvents (`.ofl1`) + AnalyticsId/InstallationId | (Opcional) no requerido para paridad | P2 |
| F-49 | Licenciamiento | Ambas | doc[4] | Compra única; VIP code; 3 ordenadores; 30 días garantía; Win10+/macOS 12.7+ [4] | N/A | — |
| F-50 | Idioma de la UI | Ambas | UI + hip | UI observada 100 % en inglés; sin evidencia de localización en disco ni en docs públicas | Decisión Smart Mix: i18n desde v1 si se quiere ventaja competitiva | P2 |

**Total: 50 features documentadas** (F-01…F-50; F-45/F-49 son notas de producto sin test).

---

## 4. Apéndice B — Observación de la UI real (captura)

**Captura:** `C:\Users\Crist\Dev\research\mik11-main-window.png` (PrintWindow de la ventana "Mixed In Key 11", 1230×1080, app recién lanzada, sin interacción).

Descripción verificada por análisis de imagen:

- **Toolbar superior:** logo "MIXED IN KEY" · pestañas **My collection** (activa, pill azul), **Pro** (badge "New"), **Tag editor**, **How-to guide**, **Settings** · derecha: toggle claro/oscuro (en claro), icono de altavoz, controles de ventana.
- **Barra izquierda:** panel **Camelot Wheel** (colapsable) con rueda completa 1A–12A (interior) y 12B–1B (exterior) con gradiente de color; botón azul **⊕ Add tracks**; sección **Playlists**: *All music (125)* (seleccionada), *Analysis queue*, *Recently added (0)*, *Improve tracks (121)* con badge "PN".
- **Encima de la tabla:** "∞ All music (125 tracks)", buscador "Search", **4 desplegables de filtro** y enlace "Reset".
- **Tabla:** columnas visibles **Cover art · Artist · Title · Key · Tempo · Key Name · Energy**; key como chip de color (7B, 2A, 9A, 3A…), Key Name musical (F, E♭m, Em, B♭m), Energy con flechita de tendencia. Fila ejemplo: Ann Lee — "2 Times" — 7B — 130 — F — 6.
- **Panel de detalle (pista cargada):** cabecera "Kimshies — Smacking The Beat (Original Mix)", tiempo `01:00 / 4:21`; waveform azul con playhead rojo y **8 chips "Cue 1"…"Cue 8"**; transporte ⏮ ▶ ⏭ y zoom +/−; metadatos: chip **1A + "G-Sharp Minor"**, Tempo 128, Energy 6, "Cue points" con botones − / + y ojo; botones de gráfica y edición.
- **Banner inferior (Standard):** "Activate the Pro version — Test mashups in 2-deck mode…" con botones "Upgrade to Pro", "View features" y enlace "Already have a Pro VIP code?"; foto promocional con × para cerrar.
- No se vieron decks, piano ni modo mashup (gating Pro), ni diálogos modales. Tema claro por defecto.

---

## 5. Advertencias y riesgos para los tests de paridad

1. **Tags contaminados por Smart Mix:** al menos un `.aiff` contiene `COMM="SM: analyzed by Smart Mix"` y `TKEY="A"` (notación inválida — MIK usa p.ej. `A`, `Am`… pero como *Initial Key* escribe Camelot o nombre completo según ajuste; `"A"` ambiguo). **No** comparar tags de esta biblioteca como si fueran suelo MIK; usar corpus limpio.
2. **La posición de cues editados no está en `MIKStore.db`** (las tablas no tienen columnas de cue): los cues viven en los tags del archivo al exportar (markers Serato/`CUE_V2` Traktor) y/o en el caché `.wfd` (formato propietario no documentado). Para comparar posiciones, exportar a Traktor y leer `collection.nml`.
3. **Doble notación de key simultánea:** UI muestra Camelot (`Key`) y musical (`Key Name`); `KeySummaryFormat=Camelot` y `ReferenceKeySummaryFormat=Standard` son ajustes distintos — reproducir ambos.
4. **Comentario por defecto observado:** `"9A - Energy 6"` (modo `KeyAndEnergyVerbose`), escrito ANTES del comentario previo (`BeforeComments`).
5. `.wfd` cache: nombre = 32 hex; NO es MD5 de la ruta (verificado); probablemente hash de contenido/ruta con xxHash (DLL presente) → para invalidar caché en Smart Mix usar (tamaño, mtime, hash contenido).

---

## 6. Navegación de la UI y preferencias (síntesis doc + observación)

- **Flujo de uso documentado** [3]: revisar guía de integración → configurar Tag Options → añadir pistas/carpetas → analizar → revisar (Key/BPM/Energy/Cues) → refrescar biblioteca DJ (proceso de cues separado del de tags).
- **Preferencias observadas en `user.config`** (valores de esta instalación): `DefaultVolume=0.7`, `TagFormat=BeforeComments`, `KeySummaryFormat=Camelot`, `ReferenceKeySummaryFormat=Standard`, `FileNameFormat=Name_Key_Tempo`, `TagDataMode=KeyAndEnergyVerbose`, `SerializedTagOutputOptions` (JSON: destino `Comments` + inclusiones Key/Energy), layout de columnas de las vistas Analyze y Mashup, geometría de ventana, `MIK10ImportDontAskAgain=True`, banner `PlatinumNotesRecommender`, `LatestAlgorithmVersion` (Key 94 / Energy 13 / CuePoints 14 / BeatGrid 12). *(Se omiten deliberadamente VIPCode, AnalyticsId, InstallationId y rutas personales.)*
- **Ajustes por plataforma DJ** [2]: "Update Common Tags", "Update Custom Initial Key", export de cues Serato/Traktor, flujo XML rekordbox, Engine DJ (notación Camelot), VirtualDJ vía opción Serato.

---

## Apéndice A — Formatos de archivo encontrados (esquemas + muestras anonimizadas)

### A.1 `MIKStore.db` — SQLite (colección MIK 11)

Ruta: `%LOCALAPPDATA%\Mixed In Key\Mixed In Key\11.0\MIKStore.db` · 2.1 MB · `_Param.SchemaVersion=11009` · 8 tablas:

```sql
-- Esquema (nombres de tabla/columna, verificados con PRAGMA):
Song( Id, File, FilePathHash, ArtistName, SongName, Comment, Tempo,
  OverallVolume, OverallEnergy, EnergySegmentsCount, StandardPitch,
  KeyResultSummary, DateAdded, ClippedPeaksCount, Artwork, LastAnalyzedUtc,
  Genre, Album, Grouping, Year, MainKey, MainKeyConfidence, SecondKey,
  SecondKeyConfidence, IsAnalyzed, HasPNTag, PNTagIsProcessed,
  PNTagAppliedClipRepair, PNTagVolumeAnalysisVersion, PNTagVolumeUnits,
  PNTagOutputVolume, LastModifiedUtc, OverallVolumeRMS1, OverallVolumeRMS2,
  OverallVolumeLUFS, DiskIsRemovable, DiskLabel, DiskSerialNumber, Label,
  Remixer, Composer, FileType, FileSize, Bitrate, SampleRate, Rating )
SongSegment( SongSegmentId, StartTime, EndTime, KeyConfidence, Volume,
  IsSingleNote, KeyResult, SongId )            -- tiempos en ticks (100 ns)
SerializedSongStructure( SongId, Data )        -- Data = JSON (ver A.2)
Collection( Id, ExternalId, Name, Emoji, Sequence, LibraryTypeId,
  IsLibrary, IsFolder, ParentFolderId )        -- playlists anidadas + emoji
LibraryType( Id, Name )   -- 1=MixedInKey, 2=Serato, 3=Traktor, 4=rekordbox
SongCollectionMembership( Id, SongId, CollectionId, Sequence )
Mashup( Id, Sequence, SongAId, SongAStemId, SongAPitchShift, SongBId,
  SongBStemId, SongBPitchShift, Rating, AddedUtc )   -- solo Pro
_Param( Key, Value )
-- Índices: IX_Song_FilePathHash, IX_Song_LastAnalyzedUtc,
--          IX_SongCollectionMembership_{SongId,CollectionId}, IX_SongSegment_SongId
```

Valores agregados (sin datos personales): 125 `Song` (todas `IsAnalyzed=1`), `MainKey`∈{1A..12A, 3B,7B,8B}, `OverallEnergy` 4–8, `Tempo` 111.999–135.999, `Comment` p.ej. `'9A - Energy 6'`, `FileType`: 112×`.aiff`, 11×`.mp3`, 2×`.wav`; `HasPNTag=0` en todas.

### A.2 `SerializedSongStructure.Data` — JSON por pista

Muestra anonimada (estructura real de una pista):

```json
{
  "EnergySegments": [
    { "StartTime": "00:00:00.4623059", "EndTime": "00:00:30.0009175", "Energy": 5 }
    // ... ~8 segmentos
  ],
  "KeySegments": [
    { "Song": null, "SongSegmentID": "00000000-0000-0000-0000-000000000000",
      "StartTime": "00:00:00.4623059", "EndTime": "00:05:46.6179104",
      "KeyResult": "9A", "IsSingleNote": false, "KeyConfidence": 0.9979 }
  ],
  "AnalysisEnergySegments": [
    { "StartTime": "00:00:00", "EndTime": "00:00:29.5376734", "Energy": 5 }
    // ... ~12 segmentos
  ]
}
```

Tiempos como `TimeSpan` .NET (`hh:mm:ss.fffffff`). Nota: **no incluye cue points**.

### A.3 `user.config` — ajustes .NET (patrón ClickOnce)

Ruta: `%LOCALAPPDATA%\Mixed_In_Key_LLC\MixedInKey.exe_Url_<hash>\11.1.0.846\user.config`. Muestra anonimada (≤20 líneas):

```xml
<configuration><userSettings><MixedInKey.Properties.Settings>
  <setting name="TagFormat"><value>BeforeComments</value></setting>
  <setting name="KeySummaryFormat"><value>Camelot</value></setting>
  <setting name="FileNameFormat"><value>Name_Key_Tempo</value></setting>
  <setting name="TagDataMode"><value>KeyAndEnergyVerbose</value></setting>
  <setting name="ReferenceKeySummaryFormat"><value>Standard</value></setting>
  <setting name="SerializedTagOutputOptions"><value>
    [{"TagKey":"Comments","Position":0,"Inclusions":{"$type":"MixedInKey.Audio.TagInclusion[], MixedInKey","$values":[
      {"$type":"MixedInKey.Audio.KeyInclusion, MixedInKey","Format":0,"Type":0},
      {"$type":"MixedInKey.Audio.EnergyInclusion, MixedInKey","Type":2}]}}]
  </value></setting>
  <setting name="LatestAlgorithmVersion" serializeAs="Xml"><value>
    <AlgorithmVersionsResult><Key>94</Key><Energy>13</Energy>
      <CuePoints>14</CuePoints><BeatGrid>12</BeatGrid></AlgorithmVersionsResult>
  </value></setting>
  <!-- omitidos: VIPCode, AnalyticsId, InstallationId, LastFileInPlayer,
       SongColumnSettingsForAnalyze (lista completa de columnas en F-14),
       MashupColumnSettings, geometría de ventana -->
</MixedInKey.Properties.Settings></userSettings></configuration>
```

### A.4 `AudioAnalysisCache3\*.wfd` — caché de análisis

~125 archivos `<32 hex>.wfd`, 200–560 KB c/u. Binario propietario (no inspeccionado en profundidad por decisión del proyecto). El nombre NO es `md5(ruta)` (verificado contra 5 pistas); probable hash xxHash (DLL `System.Data.HashFunction.xxHash.dll` presente).

### A.5 Tags en archivos de música (AIFF con chunk ID3)

AIFF analizado: chunks `FORM(AIFF) → NAME → ANNO → COMM → SSND → ID3` — **el chunk ID3 va al FINAL del archivo** (después de `SSND`). Frames ID3v2.4 observados (valores personales anonimados):

```
TIT2=<TITULO>  TPE1=<ARTISTA>  TALB=<ALBUM>  TCON=<GENERO>  TDRC=<ANIO>
TBPM='130'  TDOR/TDRL=<FECHAS>  TPUB=<SELLO>  TSRC/UFID/TSRC(<ISRC>)  TENC
WOAF/WPUB=<URL>  APIC (portada 1.88 MB)
TKEY='A'                      ← valor AMBIGUO, escrito por Smart Mix (ver §5)
COMM lang=XXX desc='' text='SM: analyzed by Smart Mix'   ← escrito por Smart Mix
```

Formato de comentario que MIK guarda/usa (BD): `'<Camelot> - Energy <n>'` (p.ej. `9A - Energy 6`). Patrón de renombrado en disco: `A_Major_-_120_-_<TITULO>_-_<ARTISTA>.aiff`. Traktor muestra la key de MIK en la columna **Key Text** y sus cues como `CUE_V2` [2].

### A.6 `collection.nml` de Traktor (formato de destino de cues)

`C:\Users\Crist\Documents\Native Instruments\Traktor 4.4.1\collection.nml` (NML VERSION=20, 13 ENTRY, 194 CUE_V2). Muestra (entrada de sonidos de fábrica, no personal):

```xml
<?xml version="1.0" encoding="utf-8" standalone="no"?>
<NML VERSION="20">
<HEAD COMPANY="www.native-instruments.com" PROGRAM="Traktor Pro 4"></HEAD>
<COLLECTION ENTRIES="13">
<ENTRY MODIFIED_DATE="2025/11/17" MODIFIED_TIME="71592" LOCK="1"
        AUDIO_ID="…" TITLE="<TITULO>" ARTIST="<ARTISTA>">
  <LOCATION DIR="/:…/:" FILE="<archivo>.mp3" VOLUME="C:" VOLUMEID="…"></LOCATION>
  <INFO BITRATE="…" LABEL="…" PLAYTIME="142" FILESIZE="5243"></INFO>
  <TEMPO BPM="136.000000" BPM_QUALITY="100.000000"></TEMPO>
  <MUSICAL_KEY VALUE="15"></MUSICAL_KEY>
  <CUE_V2 NAME="n.n." DISPL_ORDER="0" TYPE="0" START="71.622549"
          LEN="0.000000" REPEATS="-1" HOTCUE="0"></CUE_V2>
  <!-- más CUE_V2 por pista; MIK escribe los suyos al exportar -->
```

### A.7 Otros formatos menores

- `StemSeparationEngineReport.json` (muestra completa, 370 B):

```json
{"Models":[
 {"Name":"Medium","Version":1,"Error":"","InitializationTime":2.179,
  "RealtimeSpeed":17.12,"VocalSdr":8.91,"InstrumentSdr":16.37},
 {"Name":"High","Version":1,"Error":"","InitializationTime":2.19,
  "RealtimeSpeed":6.89,"VocalSdr":11.31,"InstrumentSdr":17.37}]}
```

- `midi.log`: texto plano, líneas `MakeDeviceList - Device = MPK mini 3`, `Input open`, `InputCallback` (WinMM MIDI).
- `QueryLogs\<dd_mm_yyyy>-queryLog.txt`: texto plano con SQL parametrizado de Dapper + versión `11.1.0.846`.
- `OfflineEvents\Analytics\*.ofl1`: binario opaco (telemetría offline).
- `Config.cache`: serialización `BinaryFormatter` de NHibernate 3.3.1 (caché de 2.º nivel; no interoperable).

---

## Sources

[1] https://mixedinkey.com/features — Mixed In Key 11 Features: Standard vs Pro
[2] https://mixedinkey.com/workflows/dj-software-integration-guides — DJ Software Integration Guides
[3] https://mixedinkey.com/workflows/how-to-use-mixed-in-key — How to Use Mixed In Key 11
[4] https://mixedinkey.com/faq/mixed-in-key-pro — Mixed In Key Pro FAQ
[5] https://mixedinkey.com/learn-more — Mixed In Key 11: New Version
[6] https://mixdownmag.com.au/news/mixed-in-key-announces-version-11-packed-with-new-features-fordjs-and-music-producers — Mixdown: MIK 11 announcement
[7] https://equipboard.com/items/mixed-in-key-11 — Equipboard: MIK 11 overview
[8] https://magneticmag.com/2025/04/mixed-in-key-11-pro-review-this-tool-will-change-how-you-prepare-for-sets-and-itll-make-them-better — Magnetic Mag: MIK 11 Pro review
