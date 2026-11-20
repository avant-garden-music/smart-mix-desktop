# Procedencia y límites (PROVENANCE)

Este documento separa hechos verificados de hipótesis, y delimita qué código
y datos entran en el producto.

## Referencia del motor

- Repositorio de investigación: `ImSeenOne/harmonic-engine` @ `0f3e1023`
  (privado, propio).
- El prototipo Python fue validado contra la DB real de herramienta comercial de referencia 11
  (768 pistas) por el proyecto harmonic-engine. **Smart Mix porta ese
  prototipo a Rust**; las métricas de la tabla del README son del prototipo,
  verificadas en Rust por paridad numérica (p. ej. comb tempo: Python
  116.4431 vs Rust 116.4430 sobre señal sintética idéntica).
- Constantes del motor (FFT 4096/hop 2048, refinado cúbico, EQ 24 bandas,
  CALIB_GAIN ×1.2388, histograma 1000 bins, etc.) están documentadas como
  [VERIF] en `harmonic-engine/docs/*.md`. No se ha descompilado ningún
  binario en este proyecto.

## Modelos incluidos

| Archivo | Origen | Estado |
|---|---|---|
| `models/energy_global.onnx` / `energy_segment.onnx` | entrenados por harmonic-engine (HGB) contra labels de su DB | uso directo vía ort |
| `models/key_model_v4.onnx` | RegLog 112→24 entrenado por harmonic-engine | el runtime ort rechaza su nodo ZipMap (double); **v4 además colapsaba a tonalidad menor** (corpus de entrenamiento ~96% minor — QA-F-015), ver abajo |
| `models/key_linear.json` | **v5 (2026-11-02)**: RegLog 112→24 reentrenada sobre 588 pistas etiquetadas por el renombrado de herramienta de referencia (313 mayores/275 menores) + augmentación por rotación de semitonos (12×) | clasificador primario. Medido (CV 5-fold, letra-consciente): **exacta 75.8% ±, letra 82.3%** (v4: 36.2%/46.8%, mayores 0%). Corpus discriminante y scripts: `sm_shots/{gen_labels_588,train_key_v5}.py` |

El clasificador 1–10 de energía y el de tonalidad **de herramienta de referencia viven en su
servidor cloud**; aquí se usan sustitutos propios entrenados por
harmonic-engine (90.5% CV en tonalidad). Ampliar el corpus de entrenamiento
mejora la exactitud.

## Límites conocidos honestos

- Cue points: la regla del cloud de herramienta de referencia es parcialmente desconocida
  ("recorte exacto a 8", §8.3 de la spec energía). Se implementa la regla
  empírica validada contra 5843 cues reales → etiquetado [APROX].
- ZCLIPPEDPEAKCOUNT: conteo crudo |x| ≥ 0.997 [APROX] (la regla exacta es
  UNKNOWN).
- Paridad funcional con herramienta de referencia: ver `docs/FEATURES.md`. Lo marcado ⏳ no se
  declara terminado.
- No se ha incluido ni descompilado código, assets ni identidad de terceros.
  Los formatos de exportación (NML, rekordbox.xml, crate de Serato,
  EngineLibrary) se implementaron desde la documentación pública y la
  ingeniería propia del proyecto harmonic-engine (core/tagger).

## Tipografía (SIL OFL 1.1) — decisión 2026-10-19

Selección definitiva open source (sustituye a Termina y Tusker Grotesk,
que quedaron REMOVIDAS del repo por no ser OFL):

| Rol | Familia | Archivo | Origen |
|---|---|---|---|
| Display / acentos de marca | Dela Gothic One | `assets/DelaGothicOne-Regular.ttf` | Google Fonts (OFL) |
| Heading / encabezados | Sora 700 | `assets/fonts/sora/sora-v17-latin-700.ttf` | Google Fonts (OFL) |
| Body / interfaz | Barlow Semi Condensed 500 | `assets/fonts/barlow-semi-condensed/…500.ttf` | Google Fonts (OFL) |
| Editorial / títulos grandes | Big Shoulders Display 700 | `assets/fonts/big-shoulders-display/bsd-700.ttf` | Google Fonts (OFL) |
| Datos numéricos (BPM/LUFS/duración) | JetBrains Mono 400 | `assets/fonts/jetbrains-mono/…400.ttf` | Google Fonts (OFL) |

Todas bajo SIL Open Font License 1.1; los pesos adicionales conservados en
`assets/fonts/` son para uso futuro de la jerarquía tipográfica.
