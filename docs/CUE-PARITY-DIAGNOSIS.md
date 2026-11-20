# Diagnóstico de paridad de cues — causa raíz medida (2026-11-07)

Corpus: 92 pistas emparejadas (696 cues herramienta de referencia, 736 nuestras), dump oficial
`cue_parity.exe`, umbral de cobertura ≤1 beat.

## Hallazgos (refutan la hipótesis anterior)

1. **NO HAY DRIFT del beatgrid.** Offset firmado de cues cubiertas:
   primera mitad +0.163s vs segunda +0.161s → drift -0.002s ≈ 0.
   El tempo propio es estable de principio a fin de pista.
   (La nota anterior que atribuía el techo al "drift del beatgrid" era
   incorrecta; este documento la corrige con medición.)

2. **Fase constante +0.16s** (media; mediana +0.12s). Nuestras cues caen
   sistemáticamente ~0.34 beats DESPUÉS que las de herramienta de referencia, igual de early
   que de late en la pista. Corpus 100% AIFF (sin pérdida) → NO es delay
   de codec/encoder: es sesgo de anclaje propio (cuantización al beat
   siguiente vs herramienta de referencia que ancla al inicio de la ventana, o fase de
   downbeat corrida ⅓ beat).

   **ACTUALIZACIÓN (calibración global RECHAZADA por holdout):** ajustar
   una constante −0.1226s (mediana del split tune) EMPEORA el holdout
   (mediana abs 0.150→0.227s; ≤0.1s: 37%→16%). La distribución es
   sesgada, no una fase: ~37% de las cues cubiertas ya caen ≤0.1s y el
   resto se dispersa 0.1–0.5s → el error dominante es anclar al beat
   VECINO del que elige herramienta de referencia, no un corrimiento uniforme. No aplicar
   shift global.

3. **El problema dominante es SELECCIÓN, no precisión.**
   - Recall: 454/696 = **65.2%** de cues herramienta de referencia cubiertas.
   - Huérfanas nuestras: 282/736 = 38.3% (ponemos donde herramienta de referencia no).
   - Por pista: 21 completas, 67 PARCIALES, 4 nulas.
   - Las cues herramienta de referencia perdidas viven en 0.5–15.7s: **intro temprana y
     cambios estructurales**. Nuestra política de 8 slots no reserva
     lugar para el cue de intro ni ancla igual los límites de sección.

## Palancas (en orden de impacto esperado)

| # | Palanca | Estado |
|---|---|---|
| 1 | Política de slots: reservar slot para cue de intro (primer onset tras silencio) + anclar slots a fronteras de `segments` → requiere re-entrenar el ranker con labels herramienta de referencia (harmonic-engine) | **principal** (recall 65% → objetivo 80%+) |
| 2 | ~~Calibración de fase global~~ | **RECHAZADA por holdout** (ver §2) |
| 3 | ~~Re-entrenar BPM/beatgrid~~ | innecesario (tempo estable, §1) |

## Disciplina anti-overfit
Cualquier ajuste se mide en el split FIJO de `tools/cue_tune_holdout.py`
(tune para calibrar, holdout para reportar). Sin mejoras en holdout, no
se acepta el cambio.

## Reproducir
`sm_shots/cues_final_dump.tsv` + los scripts de este diagnóstico
(pareo: prefijo 6 palabras, containment; cobertura: nearest ≤1 beat).

## 4. Experimento de re-entrenamiento (2026-11-07) — NEGATIVO, gate respetado

Ranker v5 (HGB 150, dual-grid, w_intro=2) entrenado con labels herramienta de referencia sobre
las 54 pistas tune: sube tune a 83.9% pero **degrada holdout a 77.2%
(baseline 78.5%)** — memorización del split, confirmada por OOF tune
72-74% vs 76.4% del v4. NO entregado; v4 restaurado y verificado
(sha256 a1c62076…, backup models/cue_ranker.v4.json.bak).

Hallazgos que quedan para la próxima:

1. **el selector de Python (engine/ref_cues.candidate_features) está
   DESINCRONIZADO del Rust** (fills por segmento + anchor distinto vs
   rejilla global). El puerto fiel quedó en
   `harmonic-engine/scratch/ourgrid.py::candidate_features_rust`
   (validado: 716/736 cues exactas vs el exe). TODO re-entreno futuro
   DEBE usar ese puerto, no el de ref_cues.py.
2. **El techo existe pero no con 54 pistas**: los candidatos cubren
   88.5% (tune) / 92.0% (holdout) de cues herramienta de referencia; el v4 fue entrenado en
   474 pistas del DB herramienta de referencia. Camino: re-entrenar con las ~470 pistas GT
   con scores que NO están en el split (disjuntas de holdout por
   construcción), usando ourgrid.py. Protocolo de medición OOF/holdout
   ya montado (scratch/build_dataset.py, train_*.py, fit_final.py).
3. Calibrar solo el decode (min_gap/thr) sin re-entrenar no gana recall.

## 5. Re-entreno v6 con ourgrid (2026-11-19) — NEGATIVO, gate respetado

Protocolo prescrito en §4 ejecutado al pie de la letra: puerto fiel del
selector Rust recreado (`harmonic-engine/scratch/ourgrid.py`, validado
vs exe debug: 8/10 pistas bit-idénticas, 2 flips por redondeo %.5f de
scores en decisiones razor-edge — perturbar scores ±1e-5 mueve la prio
0.0228→0.0080), labels herramienta de referencia a ≤5 ms del beat del candidato, HGB igual al
v4 (700/0.04/63), thr calibrado OOF (GroupKFold-5 por pista), export
bit-exacto verificado, JSON cargado por el exe real del harness
(qa/parity_full.py = measure_parity.py + fase/recall/split).

| modelo | entrenamiento | ±1 beat | ±20 ms | count | fase media/mediana | huerfanas |
|---|---|---|---|---|---|---|
| v4 (actual) | 474 BD herramienta de referencia, rejilla vieja | **79.5%** | 12.8% | 77/92 | +0.177 / +0.321 s | 22.8% |
| v6 disjunto | 292 título-disjuntas del split | 55.9% | 4.8% | 39/92 | +0.084 / +0.005 s | 45.7% |
| v6b | 474 − 71 holdout (tune in-sample) | 58.4% | 5.5% | 39/92 | +0.085 / +0.013 s | — |
| v6a | 474 completas (= alcance v4) | 58.7% | 5.6% | 40/92 | +0.079 / +0.002 s | — |
| v4 restaurado | — | **79.5%** | 12.8% | 77/92 | +0.177 / +0.321 s | — |

**Ninguno pasa el gate** (±1 beat < 79.5% → rechazo). v4 restaurado y
verificado (sha256 a1c62076…, re-medición bit-idéntica al baseline).
Sin commits; dumps y modelos v6 en `harmonic-engine/scratch/`.

Hallazgos duros:

1. **No es cantidad de datos**: v6a usa EXACTAMENTE el alcance del v4
   (474 pistas) con la rejilla correcta y aun así colapsa (58.7%). El
   split tune, in-sample para v6b, no se recupera (59.1%) → tampoco es
   memorización del corpus.
2. **La fase SÍ es un problema de selección y el ranker la corrige**:
   los tres v6 centran la mediana a ~0 (de +0.321 s) — un ranker
   entrenado con labels herramienta de referencia ancla al beat ANTERIOR de la ventana, como
   herramienta de referencia. El sesgo +0.16 s del v4 es una convención aprendida, no física.
3. **Pero el ranker nuevo elige mal las UBICACIONES**: los v6 doblan
   huerfanas (45.7% vs 22.8%). Composición de elegidos (20 pistas
   debug): v4 = 70% fills de la rejilla anclada + 18% boundaries
   (ranking por curva de score: robusto a que NUESTRA segmentación no
   coincida con la de herramienta de referencia); v6a = 55% boundaries + 33% fills. El v6
   aprendió la convención de segmentación de la BD herramienta de referencia (55.7% de cues
   en inicios de segmento, inicios QUE EN LA BD CAEN en la rejilla de
   16 del ancla). Nuestros inicios de segmento NO caen en esa rejilla
   → el "amor por boundaries" del v6 dispara en lugares que herramienta de referencia no
   marcaría. Es brecha de dominio segmentación→labels, no del modelo.
4. **La fase residual del v4 tiene DOS componentes fuera del ranker**
   (offsets de cues cubiertas v4 en beats): 37.5% a 0.0, **22.6% a
   +0.5** (fase de downbeat 180° de NUESTRO beatgrid vs herramienta de referencia — la
   rejilla de candidatos está en beats NUESTROS; ningún ranker puede
   arreglar media mitad de beat), **31.7% a +1.0** (anclaje al beat
   siguiente), 7.1% a −0.5, 1.1% a −1.0.

### Siguiente paso propuesto (con evidencia): corrección de anclaje, no re-entreno

1. **Fase de downbeat**: detectar/corregir la fase ½-beat del beatgrid
   propio (22.6% + 7.1% de cues cubiertas están a ±0.5) con el propio
   vector de envolvente (los downbeats tienen energía/gravedad distinta).
   Target: mover el 22.6% de +0.5 a 0.0 → ±20 ms subiría de ~13% a ~30%+.
2. **Anclaje al beat anterior**: en el decode (Rust, sm-core), para el
   candidato elegido probar preferir el beat INICIO de ventana (el
   score pre-análisis de herramienta de referencia ancla ahí): atacaría el 31.7% a +1.0.
   Ambos son cambios de sm-core (beats.rs/cue_ranker.rs) medibles con
   el mismo harness sin re-entrenar nada.

Reproducir: `harmonic-engine/scratch/{ourgrid,build_dataset_v6,train_v6,
train_v6ab,fit_final_v6,validate_port}.py`, dumps en
`harmonic-engine/scratch/parity_dumps/`, harness extendido
`harmonic-engine/qa/parity_full.py`.

## 6. Corrección de anclaje+fase en sm-core (2026-11-19) — ENTREGADA, gates superados

Ejecutada la palanca propuesta en §5 con medición completa del corpus
(92 pistas / 712 pares GT, dumps y scripts en `sm_shots/cues_*.py`).

### Causa raíz medida (refina el mapa de §5)

Los offsets de cues cubiertas NO son ±0.5/±1.0 beat "duros": el histograma
continuo (cues_delta_hist.py) muestra que las posiciones de los cues herramienta de referencia
caen a **+0.25 beat exacto** de nuestro grid en ~48/92 pistas (frac
mediano por pista 0.20–0.31) y sobre la línea de beat (frac ≈ 0) en ~23.
La fase de onsets del audio (flux 30–400 Hz) respecto de NUESTRO grid
correlaciona 1:1 con esas familias (cues_onset_phase.py): cuando el grid
queda desfasetado +0.25, el "primer beat ≥ 0" salta un beat entero si
t0 apenas es negativo y TODA la rejilla de fills anclada hereda +0.75
(familia A, 48% de pares cubiertos: 223 fills + 48 cues ancla); sin el
salto los picks quedan a −0.25 (familia B). El +0.5 "de downbeat" de §5
era un artefacto del binning a 0.5: la población real a ±0.5 beat es de
2–3 pistas (ρ de fase < 0.25, irrecuperable con esta señal).

### Fix (solo selección de cues; beatgrid/BPM/exports INTACTOS)

1. `beats::analyze_pcm_with_env` devuelve además la envolvente de onsets
   (ya computada — cero STFT extra) y `beats::onset_phase_offset` estima
   la fase φ de los ataques dentro de la celda del beat (media circular
   ponderada por pico de flux, zona estable [64, n−64); ρ ≥ 0.5, snap
   |φ| ≤ 0.125, calibración +0.05 = sesgo constante medido del pico de
   flux vs la convención herramienta de referencia).
2. `ref_cues_engine::anchor_prev_cues`: shift entero −1/+1 del ancla
   corregida SOLO para cues de la rejilla (ancla + fills múltiplos de 16),
   con histéresis ANCHOR_TOL=0.1; los boundaries no se tocan.
3. `ref_cues_engine::phase_snap_cues`: recoloca TODOS los cues en la
   línea fase-corregida beats[k] + φ·P (clamp ≥ 0).

### Decisiones con evidencia negativa (revertidas, no entregadas)

- **Estimador sobre ch1 de la envolvente herramienta de referencia: RECHAZADO.** El smoothing
  de ~0.28 s difumina el argmax (fases negativas espurias en 17 pistas);
  medido con harness oficial: ±20ms 12.8% → 6.7%. El estimador vive en
  la envolvente de flux propia (hop 11.6 ms).
- **Anclaje entero solo (batch 1 aislado): RECHAZADO.** 78.5% ±1beat
  (< piso): la familia A vive a +0.75..+0.80, al borde de la tolerancia;
  sin la fase sub-beat el shift solo no aterriza (74.6% con regla final).
- **Fase sola (batch 2 aislado): RECHAZADA.** 76.7% (sin bias) — empuja
  la familia A fuera del ±1 beat. **El anclaje entero y el snap de fase
  son inseparables**: se entregan juntos.

### Métricas (harness oficial, exe target/release/examples/cue_parity)

| métrica | v4 baseline | fix | gate |
|---|---|---|---|
| ±1 beat | 79.5% (566/712) | **80.6% (574/712)** | ≥ 79.5 ✓ |
| ±20 ms | 12.8% (91/712) | **54.8% (390/712)** | > 12.8 ✓ |
| count (≥75% cues) | 77/92 | 77/92 | = |
| fase media / mediana | +0.177 / +0.321 s | **−0.007 / +0.003 s** | → 0 ✓ |
| huérfanas | 23.1% (170/736) | **22.0% (162/736)** | < 22.8 ✓ |
| split tune | 79.2% | 80.1% | ✓ |
| split holdout | 79.9% | **81.3%** | ✓ (holdout > tune) |

Anti-overfit: umbrales fijados en el split tune y verificadas ambas
mitades (holdout sube más); el error del estimador en 60 pistas
corregidas es mediana 0.012 beats (~6 ms), 0 errores de signo
(cues_rust_phi_check.py). Techo restante: selección/recall (intros
0.5–15 s sigue 1/7) — sigue requiriendo re-entrenar el ranker con
labels herramienta de referencia sobre el puerto ourgrid.py (§4).

Reproducir: `sm_shots/cues_{offset_analysis,delta_hist,onset_phase,
fix_proto2,truth_table,pair_census,rust_phi_check}.py` + dumps
`cues_debug_head.tsv` (baseline) / `cues_debug_fix.tsv` (fix, columnas
debug 11–12 = fase/shift).
