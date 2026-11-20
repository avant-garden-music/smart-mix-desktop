# Roadmap — Smart Mix

Estado: cierre de producto (v0.1). Este archivo separa lo **planeado** de lo
**prohibido de construir por ahora** — nada de esta página entra en un ciclo
de desarrollo sin decisión explícita del dueño.

## Planeado — no construir

### DIGGING (modo descubrimiento)

Un modo aparte para *descubrir* música, separado del flujo de trabajo del DJ:

- **Huecos armónicos de la biblioteca**: para cada clave de la rueda, qué
  zonas de `compatible_set` no tienen pistas — "te faltan 8B y 5A para
  cerrar el círculo". Reutiliza el grafo de compatibilidad existente, sin
  nuevo análisis.
- **"Como tu playlist X"**: dada una playlist, pistas de la biblioteca (no
  incluidas) que encajen armónica y energéticamente con su perfil.
- **Charts con filtro propio**: top pistas por energía/BPM/rating dentro de
  un filtro del usuario — nunca la mezcla de charts externas con la
  biblioteca local.
- **Previews de 30 s + enlace afiliado**: vía **iTunes Search API v1**
  (gratuita, sin auth). La pista ajena NUNCA entra a la biblioteca, decks,
  playlists ni export — solo se escucha el preview y sale al enlace.
- **Entrada intencional**: DIGGING vive detrás de una entrada explícita de
  menú. JAMÁS se cuela en el flujo export ni en sugerencias automáticas del
  flujo normal.

### Restos del motor

- **Selector de cues**: hoy el pipeline fija el set automático; falta el
  selector que elija entre perfiles (p. ej. intro-first vs. Energy-peak)
  antes de escribir. Depende del trabajo en curso de `models/`.
- **Migración de egui**: objc2 0.4.1 (arrastrada por eframe 0.27) no
  compila con rustc ≥ 1.88 en macOS — el CI clava la toolchain 1.88.0.
  Subir egui/eframe elimina el pin; es la primera tarea técnica del
  siguiente ciclo.

## Fuera de alcance permanente

- Escribir en la nube, cuentas de usuario, telemetría.
- Mezcla automática/autosync en vivo (la app asiste, no mezcla por el DJ).
