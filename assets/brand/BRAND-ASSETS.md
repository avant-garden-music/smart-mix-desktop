# Smart Mix Brand Identity v1 assets

Status: INTEGRATED into the Rust application (commit 144574c, 2026-11-05) by product-owner instruction. Header lockup 34px + window icon shipped; vertical lockup rasterized for future About.

## Approved source

`../../smart-mix-symbol-master.svg`

This Recraft SVG is the immutable approved master. It contains C2PA metadata, a painted `#0B0B0D` background, one `#DDC9FF` symbol path and two `#0B0B0D` interior paths. It is preserved without modification.

## Transparent vector exports

- `export/smart-mix-symbol.svg` — lavender `#DDC9FF`, transparent canvas and counters.
- `export/smart-mix-symbol-white.svg` — white, transparent canvas and counters.
- `export/smart-mix-symbol-black.svg` — `#0B0B0D`, transparent canvas and counters.
- `export/smart-mix-wordmark.svg` — Sora 700 converted to vector outlines.
- `export/smart-mix-logo-horizontal.svg` — approved symbol plus outlined wordmark.
- `export/smart-mix-logo-vertical.svg` — approved symbol above outlined wordmark.

The transparent symbol variants preserve the three approved geometry subpaths as one compound vector path using `fill-rule="evenodd"`. No raster image, painted background or painted counter remains.

## App icons

PNG application icons are generated from the approved symbol on the official `#0B0B0D` background.

Standard layout: symbol occupies 72% of icon height.

Optical small layout: 16 px and 24 px exports use 84% symbol height because the standard layout loses counter clarity at those sizes. This changes only scale and placement; the master path geometry is unchanged.

Expected sizes:

- 16×16
- 24×24
- 32×32
- 48×48
- 64×64
- 128×128
- 256×256
- 512×512
- 1024×1024

## Typography

Editable Penpot wordmark: Sora 700 with optical tracking.

Exported lockups: Sora 700 converted to paths to avoid runtime font dependencies. See `licenses/FONT-LICENSES.md` and `licenses/OFL-1.1.txt`.
