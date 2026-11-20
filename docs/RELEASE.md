# RELEASE.md — Primer instalador Windows (MSI) de Smart Mix

Estado: **MSI 0.1.0 construido y validado** (`target/wix/SmartMix-0.1.0-win64.msi`, ~16.8 MB).
Instalación **per-user sin admin** (sin prompt de UAC en Windows 7+).

Todo el pipeline vive en `scripts/build-msi.sh`; la definición del instalador en
`crates/sm-app/wix/main.wxs`.

---

## 1. Toolchain detectado / usado

| Pieza | Versión | Ubicación | Notas |
|---|---|---|---|
| `cargo wix` | 0.3.9 | `~/.cargo/bin/cargo-wix.exe` | ya instalado (`cargo install cargo-wix`) |
| WiX Toolset (candle + light) | 3.14.1.8722 | `%LOCALAPPDATA%\wix314` | **portable** — descargado de la release oficial `wix3141rtm` (`wix314-binaries.zip`, GitHub `wixtoolset/wix3`), solo descomprimido, sin instalador ni admin |

Nota sobre `cargo wix` 0.3.9: este fork **no tiene subcomando `build`**; compilar
es la acción por defecto (`cargo wix --no-build ...`). Además el `UpgradeCode`
que imprime su plantilla es **aleatorio por invocación** — nunca usarlo.

## 2. Comandos exactos (reproducibles)

Todo en uno:

```bash
scripts/build-msi.sh          # build + staging + MSI
```

Equivalente a mano (desde la raíz del repo):

```bash
cargo build --release -p sm-app
# staging: el wxs toma todo de target/release
mkdir -p target/release/models
cp models/energy_global.onnx models/energy_segment.onnx models/key_linear.json \
   models/key_model_v2.onnx models/key_model_v4.onnx models/key_model_v4_nomap.onnx \
   models/cue_ranker.json target/release/models/
cp assets/brand/app-icons/smart-mix.ico target/release/smart-mix.ico
cargo wix -b "$LOCALAPPDATA/wix314" --no-build -p sm-app -n SmartMix \
   -o target/wix/SmartMix-0.1.0-win64.msi
```

## 3. Qué contiene el MSI

- `smart-mix.exe` (release, ~40 MB — build actual al 2026-11-19, incluye c7ea6d3).
- `models/` (7 archivos, ~7.4 MB): la app en build instalado resuelve
  `exe_dir/models` (`default_models_dir()` en `sm-core/pipeline.rs`); sin ellos
  degrada key/energía al fallback.
- Shortcut en Menú Inicio (carpeta "Smart Mix") + Desktop, con el icono de marca.
- Icono en "Agregar o quitar programas" (`ARPPRODUCTICON`).
- `smart-mix.ico` se genera desde los PNG oficiales
  (`assets/brand/app-icons/smart-mix-app-icon-{16..256}.png`, contenedor ICO con
  entradas PNG); el archivo queda versionado en
  `assets/brand/app-icons/smart-mix.ico`.
- Sin EULA: el proyecto es `LicenseRef-Proprietary` y no hay LICENSE en el repo.
  El diálogo de licencia está saltado (WixUI_InstallDir + Publish Order=2).

## 4. Per-user sin admin: cómo funciona y por qué el MSI "real" pide admin

- El paquete declara **`ALLUSERS=2` + `MSIINSTALLPERUSER=1`** (mecanismo MSI 5.0;
  `InstallerVersion=500`). Ojo: `InstallScope='perUser'` de WiX3 **no** escribe
  estas propiedades por sí solo — van authoradas a mano y verificadas en la
  Property table del MSI final.
- Con `MSIINSTALLPERUSER=1`, el servicio Windows Installer **remapea**
  `ProgramFiles64Folder` → `%LOCALAPPDATA%\Programs`, y ProgramMenu/Desktop a
  los del usuario. Instalar y desinstalar no toca `C:\Program Files` ni HKLM →
  **cero UAC**.
- Un MSI per-machine clásico sí exige admin: escribir en `C:\Program Files`,
  HKLM y "Todos los usuarios" requiere elevación — no es un capricho del MSI,
  es ACL de Windows. La vía sin admin es exactamente este modo per-user.
- Componentes con KeyPath en **HKCU** (patrón estándar per-user; keypath en HKLM
  pediría admin). El warning ICE57 de candle/light es esperado y benigno en
  este esquema dual.

## 5. Identidad del producto (NO cambiar jamás)

| Campo | Valor |
|---|---|
| UpgradeCode | `E766609D-F00E-4F39-AE2F-87D474BAF0E4` |
| Component MainExecutable | `C8CA4559-9CE1-4343-A83E-98D58EF1A16A` |
| Component Models | `CD75ABC8-CC18-4200-96A2-F0BB259F9D41` |
| Component StartMenuShortcut | `4EF96C97-4A93-4206-9207-BC26801FB5A5` |
| Component DesktopShortcut | `64BB6CF3-E6ED-405B-A195-FB1E3CE1640F` |

El UpgradeCode es la identidad para MajorUpgrade entre versiones: **una vez
publicado un release, cambiarlo instala "otra app" y deja la vieza huérfana.**
`ProductCode` es `*` (nuevo por build, correcto). `MajorUpgrade` con
`AllowSameVersionUpgrades` permite reinstalar la misma versión.

## 6. Validación realizada (2026-11-19)

- Property table (COM `WindowsInstaller.Installer`): `ALLUSERS=2`,
  `MSIINSTALLPERUSER=1`, UpgradeCode/ProductVersion/ProductName correctos.
- Upgrade table: dos filas (detect + remove) sobre el UpgradeCode estable.
- File table: exe + 7 modelos con tamaños exactos.
- **`msiexec /a` (administrative install) exit 0** — extrae el árbol completo:
  `Smart Mix/smart-mix.exe` + `Smart Mix/models/{7 archivos}`.
- NO se instaló en la máquina (la prueba real va en VM/otro equipo).

## 7. Firmado — DECISIÓN PENDIENTE

El MSI está **sin firmar**. Consecuencia: SmartScreen/Defender mostrará
"Windows protegió tu PC / editor desconocido" al abrirlo ("Más información →
Ejecutar de todas formas" funciona, pero no es presentable a terceros).

Opciones:

| Opción | Costo aprox. | Efecto SmartScreen |
|---|---|---|
| Sin firmar (hoy) | $0 | warning rojo siempre |
| Certificado OV (Sectigo/Certum open-source…) | $20–300/año | warning hasta ganar reputación (días-semanas de descargas) |
| **Certificado EV** (token hardware) | $300–500/año | reputación **inmediata**, sin warning |
| Azure Trusted Signing | ~$10/mes | sin warning (validación de identidad individual/org) |
| Autofirmado | $0 | **peor** para usuarios finales: warning siempre + "editor desconocido" |

`cargo wix sign` ya soporta firmar con signtool (usa `/a`, timestamp Comodo).
Falta: elegir certificado → **pendiente decisión de negocio**. Hasta entonces,
distribuir el hash SHA-256 junto al MSI para verificación manual
(`scripts/build-msi.sh` lo imprime).

## 8. Limitaciones conocidas / próximos pasos

1. **El exe no tiene icono embebido**: el taskbar muestra el icono por defecto
   (los shortcuts y ARP sí usan el de marca). Arreglarlo requiere
   `winresource` como build-dependency en `sm-app` + `build.rs` (toca `src/`,
   fuera del alcance de esta tarea).
2. **Diálogos del instalador en inglés** (WixUI en-us): para español hace falta
   un `.wxl` + `-l es-es` en `cargo wix` (futuro).
3. La app muestra idioma según el sistema; el MSI declara `Language=1033`.
4. Un `[env | release]` con CI (GitHub Actions) puede reproducir esto con
   `wix314-binaries.zip` cacheado — candidato a workflow propio.
5. Instalar en una VM/PC limpio para smoke test real (aquí solo se validó con
   `msiexec /a` + tablas).
