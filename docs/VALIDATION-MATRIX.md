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
- Cierre: botón CANCELAR (descarta campos no aplicados) y aire inferior en
  el preview ✓

## Hallazgos pendientes — CIERRE (2026-11-20)
1. ~~Preview del editor se corta en el borde inferior~~ → resuelto: aire
   inferior (10px) dentro del scroll del preview (y del preview de basura)
2. "Track cargado" de la rueda roza los números interiores del anillo —
   sigue pendiente (cosmético, rueda)
3. ~~Contador de "Todas las pistas" pegado al texto~~ → resuelto: el
   contador del encabezado de colección vive en un chip separado (píldora
   RAISED, mismo lenguaje que los contadores del sidebar)
4. ~~Sin botón "cancelar" en el editor~~ → resuelto: CANCELAR en el Tag
   Editor F-24 (i18n es/en, clave `studio.cancel` — que además estaba usada
   sin definir en el drawer del studio)
5. ~~La energía editable requiere el setter de BD~~ → resuelto:
   `sm-db::set_energy` (UPDATE, sin cambio de esquema) + combo ACTIVO en el
   drawer del studio (1-10, solo entra al patch si el usuario lo cambia)
6. "3 clips" a veces se rasteriza como "3 clins" — investigado en el cierre:
   egui/epaint NO aplica ligaduras (fi/fl); el label usa Barlow Semi
   Condensed 14px. No reproducible en captura de esta máquina (ver
   `docs/ROADMAP.md` / notas de cierre). Elipsis de truncado → resuelto:
   `studio::ellipsize` con "…" en título/artista de la tabla (el deck ya
   usaba `Label::truncate` de egui, que elide)

## Modal Safe Format
Reformateado a listas numeradas/secciones (commit 9d3085c) — verificado por
código y compilación; requiere USB real + interacción para validación visual
del dueño (no existe hook de apertura no interactiva).
