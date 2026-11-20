# Diseño visual — decisión "estructura del kit + piel Avant Garden"

**Decisión del dueño, 2026-11-09.** La vista Studio conserva la
**ESTRUCTURA** del kit GPT (`smart-mix-hermes-kit`, 2026-11-07: appbar,
drawer de tags 304px, rueda en el sidebar, tabla, popover, transporte) pero
la **PIEL** vuelve a la identidad **Avant Garden** del repo (paleta
handoff-v1 §3 + fuentes embebidas), con la **escala tipográfica herramienta de referencia**
calibrada contra herramienta de referencia en vivo antes del kit.

## Por qué

El kit traía una paleta propia (verde carbón + lima + lila azulado,
`design/tokens.json`) y una escala reducida (base 12px, filas 33px), con
Segoe UI del sistema como proportional. El dueño dictaminó: *"todo se ve
muy pequeño"* y la identidad visual debe ser Avant Garden. La estructura
(componentes estandarizados de `studio.rs`, layout de paneles) funcionaba y
se queda; colores/fuentes/branding/escala vuelven a AG.

## Qué se cambió (y dónde)

| Capa | Antes (kit) | Ahora (AG) |
|---|---|---|
| `theme::dark()/light()` | paleta Studio (bg #121411, lime #D1F25E…) | paleta AG handoff-v1 §3 (bg #0B0B0D, lavanda #DDC9FF, cian #1FD3FA, magenta #F50077…) — restaurada de `1e683ea` |
| `studio.rs` tokens | CHROME #0D0F0C, LIME #D1F25E, LILAC #C7B7ED… | CHROME #0E0E10, acento = lavanda #DDC9FF (`LIME`/`LILAC`), SIDE #131315, chip KEY = Container #201F21, A(menor)=cian/B(mayor)=lavanda (§8) |
| Fuentes UI | Segoe UI del sistema insertada en `install()` | **Sin Segoe UI**: Barlow Semi Condensed (`ag_body`) proportional + Sora (`ag_heading`), como antes del kit |
| Branding appbar | icono de onda + "SMART MIX" dibujados | `theme::brand_lockup` (lockup oficial Penpot embebido) dentro de la MISMA zona 128x30 del kit |
| Exportar | botón en la cabecera de pista | botón **Exportar** en el appbar, inmediatamente a la derecha de **Analizar** (exporta la selección); en la cabecera queda solo el menú … |
| Escala | FONT_BODY 12 / SMALL 11, filas 33, botones 30 | **Escala herramienta de referencia**: FONT_BODY 15 / SMALL 13.5, filas de tabla 36, botones 33, transporte 42 (play), icon_button 33, nav 34, header 34, status 30 |

## Escala tipográfica final (herramienta de referencia)

- Cuerpo de UI (botones, tabla, menús, sidebar): **15px** (`studio::FONT_BODY`)
- Texto pequeño (métricas, timeline, meta): **13.5px** (`studio::FONT_SMALL`)
- Título de pista en reproducción: **26px** (antes 24)
- BPM/KEY grandes del deck: **25px** (decimales 17px; antes 23/15)
- Tiempo del transporte: **18px**; centro de la rueda: **26px**
- Pads de cue: 13px sobre pads de 42px; filas de tabla **36px**
- Estilo global egui: `interact_size 36x33`, `button_padding 12x7`
  (`theme::install`)

## Qué NO se tocó

- **Waveform** (`widgets.rs`): sigue sobre su lienzo propio `#0B0B0D`
  (`theme::WAVE_BG`) con sus bandas azul/naranja/blanco y playhead magenta
  intactos — renderer prohibido por decisión del proyecto.
- Estructura del kit: appbar, drawer de tags (304px), rueda harmónica en el
  sidebar, tabla con columnas del kit, popover de herramientas.
- `sm-core` / `sm-db` / `sm-export` / `sm-model`.

## Bugs de comportamiento corregidos junto al re-skin

1. **Cue = seek + play garantizado** (comportamiento CDJ real): antes
   `A::Cue` hacía seek + toggle → pausaba si estaba sonando. Ahora
   `studio_seek(t) + studio_play()` (`app.rs`); `DeckCmd::Play` es
   idempotente. Resolución de letra→tiempo extraída a
   `cue_time_by_letter` con test.
2. **Doble clic en filas 2+** no cambiaba la pista cargada: causa raíz en
   egui 0.27 — el hit-test (`hit_test.rs: hit_test_on_close`) entrega el
   clic al ÚLTIMO widget click-sensed bajo el puntero, y el `egui::Button`
   del título de la celda es más interior que la celda → `row.response()`
   jamás veía `clicked()`/`double_clicked()` sobre el título y
   `Action::LoadAndPlay` no se emitía. El library pre-kit leía el doble
   clic en la respuesta del propio botón; el port del kit lo movió solo a
   `row.response()`. Ahora se lee en el botón de título/artista Y en la
   fila (test de regresión con `egui::Context` real en `studio_view.rs`).
