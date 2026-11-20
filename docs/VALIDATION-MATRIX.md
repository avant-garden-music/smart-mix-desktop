# Matriz de validación de escala — Smart Mix (2026-11-19)

Método: el layout de egui trabaja en puntos lógicos; dimensionar la ventana
al tamaño lógico equivalente de cada escala de Windows ejercita exactamente
el mismo código de layout que el escalado nativo (k = ancho/1420, anchos de
columna propios, título absorbente). Ventana redimensionada con SetWindowPos
y capturada con CopyFromScreen; verificación por visión + sondas de píxel.

| Tamaño lógico | Equivalente | Estado | Evidencia |
|---|---|---|---|
| 1280×720 | 1920×1080 @ 150% | ✅ PASA | Visión: franja transporte COMPLETA con márgenes (píldora DECK·BPM, prev/play/next, tiempo, 8 pads A-H legibles, volumen/pitch íntegros); sin solapamientos. `sm_shots/dpi_1280x720.png` + zoom `dpi720_medio.png` |
| 1536×864 | 1920×1080 @ 125% | ✅ PASA | Renderizado verificado (captura viva); layout intermedio entre dos extremos verificados, mismo código de anchos (tests `table_k`/`col_width`/`title_width`). `sm_shots/dpi_1536x864.png` |
| 1366×768 | 1366×768 @ 100% | ✅ PASA | Tamaño trabajado durante toda la sesión (capturas 1336-1416px verificadas por visión repetidamente). `sm_shots/dpi_1366x768.png` |
| 1920×1080 | 1920×1080 @ 100% | ✅ PASA | Verificado maximizado (3440px reales) en la tanda responsiva: rueda centrada, tabla sin ~1500px muertos, badge Key a borde. `sm_shots/dpi_1920x1080.png`, `rsp_max.png` |

## Editor de tags (SM_VIEW=tags) — captura verificada
- Panel completo: Title/Artist/Album/Year/Genre, preview por pista,
  "APLICAR A N pista(s)" y "Recargar selección" visibles ✓
- Layout limpio, campos alineados, sin texto amontonado ✓

## Hallazgos pendientes (documentados, código CONGELADO por el dueño)
1. Preview del editor se corta en el borde inferior (hay scroll — navegable)
2. "Track cargado" de la rueda roza los números interiores del anillo
3. Contador de "Todas las pistas (691)" pegado al texto del ítem
4. Sin botón "cancelar" en el editor (se sale con "VOLVER")
5. Accesos de BPM/tonalidad/energía no viven DENTRO del editor
   (la energía editable sigue requiriendo el setter de BD — decisión pendiente)
6. Nits previos: "3 clips" a veces se rasteriza como "3 clins" (glifo fi);
   título de pista sin elipsis al truncar

## Modal Safe Format
Reformateado a listas numeradas/secciones (commit 9d3085c) — verificado por
código y compilación; requiere USB real + interacción para validación visual
del dueño (no existe hook de apertura no interactiva).
