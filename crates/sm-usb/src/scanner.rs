//! Escáner de unidades: manifiesto SHA-256, detección de librerías DJ y
//! exclusión de basura del SO. Port de `core/usb/scanner.py`.
//!
//! El manifiesto es la única fuente de verdad de todo el flujo USB Safe
//! Format: las rutas relativas son estilo POSIX y conservan mayúsculas, y
//! cada archivo lleva su SHA-256 para que backup/restore se pueda verificar
//! byte a byte.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::strings::t;

const CHUNK: usize = 1024 * 1024;

/// Callback de progreso: recibe (mensaje de etapa localizado, fracción
/// 0.0..1.0). Equivalente a `Progress = Callable[[str, float], None]`.
pub type ProgressCb<'a> = &'a mut (dyn FnMut(&str, f64) + 'a);

/// Extensiones consideradas pista de audio (mismo conjunto que el .py).
pub const AUDIO_EXTENSIONS: &[&str] = &[
    ".mp3", ".wav", ".flac", ".aiff", ".aif", ".m4a", ".aac", ".ogg", ".wma", ".alac",
];

/// Ruido del SO/indexador que jamás debe llegar al manifiesto.
pub const EXCLUDED_NAMES: &[&str] = &[".DS_Store", "System Volume Information", "$RECYCLE.BIN"];

pub const PIONEER_DIR: &str = "PIONEER";
pub const PIONEER_ITEMS: &[&str] = &["rekordbox.dbs", "ARTWORK", "USBANLZ"];
pub const ENGINE_DIR: &str = "EngineLibrary";

/// Error de escaneo. Python lanza `ValueError`; aquí el caso observable
/// (raíz que no es directorio) viaja en `ScanError::NotADir` con el mismo
/// mensaje localizado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanError {
    NotADir(String),
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::NotADir(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for ScanError {}

/// SHA-256 de un archivo en streaming (bloques de 1 MiB, seguro para
/// archivos grandes). Equivalente a `hash_file`.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut f = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// SHA-256 hexdigest de bytes en memoria.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

/// Tamaño legible (formato puro, sin localizar). Réplica exacta de
/// `fmt_bytes`: B sin decimales, el resto con uno, cortes en 1024.
pub fn fmt_bytes(n: u64) -> String {
    let mut size = n as f64;
    for unit in ["B", "KB", "MB", "GB", "TB"] {
        if size < 1024.0 || unit == "TB" {
            return if unit == "B" {
                format!("{size:.0} {unit}")
            } else {
                format!("{size:.1} {unit}")
            };
        }
        size /= 1024.0;
    }
    format!("{n} B")
}

/// Un archivo del manifiesto: relpath POSIX (mayúsculas conservadas),
/// tamaño y hash. Serializa a exactamente el JSON del manifiesto Python
/// (`relpath`/`size`/`sha256`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub relpath: String,
    pub size: u64,
    pub sha256: String,
}

/// Librerías DJ halladas en la raíz de la unidad (nombres reales,
/// conservando mayúsculas).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecialFolders {
    pub pioneer: Option<String>,
    pub pioneer_items: Vec<String>,
    pub engine: Option<String>,
}

impl SpecialFolders {
    pub fn detected(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(p) = &self.pioneer {
            out.push(p.clone());
        }
        if let Some(e) = &self.engine {
            out.push(e.clone());
        }
        out
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanResult {
    pub root: PathBuf,
    pub files: Vec<FileEntry>,
    pub total_bytes: u64,
    pub special: SpecialFolders,
    pub loose_root_tracks: Vec<String>,
    pub errors: Vec<String>,
}

impl ScanResult {
    /// Pistas de audio del manifiesto (extensión de audio, case-insensitive).
    pub fn track_count(&self) -> usize {
        self.files
            .iter()
            .filter(|e| is_audio(&e.relpath))
            .count()
    }

    pub fn relpaths(&self) -> Vec<String> {
        self.files.iter().map(|e| e.relpath.clone()).collect()
    }
}

/// Extensión en minúsculas CON punto (`.mp3`); vacía si no hay.
fn ext_lower(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
        .unwrap_or_default()
}

/// `True` si `name` termina en extensión de audio (case-insensitive).
pub fn is_audio(name: &str) -> bool {
    AUDIO_EXTENSIONS.contains(&ext_lower(name).as_str())
}

/// Basura del SO, AppleDouble y entradas ocultas (empiezan por `.`).
fn is_excluded(name: &str) -> bool {
    EXCLUDED_NAMES.contains(&name) || name.starts_with("._") || name.starts_with('.')
}

/// Ruta relativa estilo POSIX (`Music/Techno/Track A.MP3`).
fn rel_posix(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("la ruta siempre cuelga de root")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Recolección determinista espejo de `os.walk` con `sorted`: por cada
/// directorio, primero sus archivos ordenados, luego recursión en cada
/// subdirectorio ordenado. Errores de lectura de un directorio se ignoran
/// (igual que `os.walk(onerror=None)`).
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut dirs: Vec<(String, PathBuf)> = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_excluded(&name) {
            continue;
        }
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => dirs.push((name, entry.path())),
            Ok(_) => files.push((name, entry.path())),
            Err(_) => continue,
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    dirs.sort_by(|a, b| a.0.cmp(&b.0));
    out.extend(files.into_iter().map(|(_, p)| p));
    for (_, d) in dirs {
        collect_files(&d, out);
    }
}

/// Detecta `PIONEER/` (rekordbox.dbs, ARTWORK/, USBANLZ/) y
/// `EngineLibrary/` en la raíz. Matching case-insensitive (los vendors
/// shippean en mayúsculas, los usuarios renombran); los nombres
/// reportados son los reales del disco.
fn detect_special(root: &Path) -> SpecialFolders {
    let mut special = SpecialFolders::default();
    let Ok(rd) = fs::read_dir(root) else {
        return special;
    };
    for entry in rd.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let low = name.to_lowercase();
        if low == PIONEER_DIR.to_lowercase() && special.pioneer.is_none() {
            special.pioneer = Some(name);
            let wanted: Vec<String> = PIONEER_ITEMS.iter().map(|i| i.to_lowercase()).collect();
            if let Ok(children) = fs::read_dir(entry.path()) {
                for child in children.flatten() {
                    let cn = child.file_name().to_string_lossy().to_lowercase();
                    if wanted.contains(&cn) {
                        special
                            .pioneer_items
                            .push(child.file_name().to_string_lossy().to_string());
                    }
                }
            }
        } else if low == ENGINE_DIR.to_lowercase() && special.engine.is_none() {
            special.engine = Some(name);
        }
    }
    special
}

/// Inventario de `root`: manifiesto con SHA-256 por archivo + carpetas
/// especiales. Recorre el árbol deterministicamente (ordenado), saltando
/// la basura del SO. Los errores de archivos individuales se colectan
/// (localizados) en lugar de abortar el escaneo.
pub fn scan_drive(
    root: &Path,
    mut progress: Option<ProgressCb<'_>>,
) -> Result<ScanResult, ScanError> {
    if !root.is_dir() {
        return Err(ScanError::NotADir(t(
            "usb.scan.not_a_dir",
            &[("root", &root.to_string_lossy())],
        )));
    }
    if let Some(p) = progress.as_mut() {
        p(
            &t("usb.scan.start", &[("root", &root.to_string_lossy())]),
            0.0,
        );
    }

    // Primera pasada: recolectar rutas para que la fracción sea exacta.
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_files(root, &mut paths);

    let mut files: Vec<FileEntry> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut total: u64 = 0;
    for (i, path) in paths.iter().enumerate() {
        let rel = rel_posix(root, path);
        let entry = (|| -> std::io::Result<FileEntry> {
            let size = fs::metadata(path)?.len();
            let sha = hash_file(path)?;
            Ok(FileEntry {
                relpath: rel.clone(),
                size,
                sha256: sha,
            })
        })();
        match entry {
            Ok(e) => {
                total += e.size;
                files.push(e);
            }
            Err(exc) => errors.push(t(
                "usb.scan.read_error",
                &[("path", &rel), ("error", &exc.to_string())],
            )),
        }
        if let Some(p) = progress.as_mut() {
            let frac = if paths.is_empty() {
                1.0
            } else {
                (i + 1) as f64 / paths.len() as f64
            };
            p(&t("usb.scan.progress", &[("n", &(i + 1).to_string())]), frac);
        }
    }

    let loose = files
        .iter()
        .filter(|e| !e.relpath.contains('/') && is_audio(&e.relpath))
        .map(|e| e.relpath.clone())
        .collect();

    if let Some(p) = progress.as_mut() {
        p(
            &t(
                "usb.scan.title",
                &[
                    ("n", &files.len().to_string()),
                    ("bytes", &fmt_bytes(total)),
                ],
            ),
            1.0,
        );
    }

    Ok(ScanResult {
        root: root.to_path_buf(),
        files,
        total_bytes: total,
        special: detect_special(root),
        loose_root_tracks: loose,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_bytes_replica_python() {
        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1023), "1023 B");
        assert_eq!(fmt_bytes(1024), "1.0 KB");
        assert_eq!(fmt_bytes(1536), "1.5 KB");
        assert_eq!(fmt_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(fmt_bytes((1024.0 * 1024.0 * 1024.0 * 1.5) as u64), "1.5 GB");
    }

    #[test]
    fn extension_de_audio_case_insensitive() {
        assert!(is_audio("Track A.MP3"));
        assert!(is_audio("bEEt.Wav"));
        assert!(!is_audio("warmup.m3u"));
        assert!(!is_audio("sinext"));
    }
}
