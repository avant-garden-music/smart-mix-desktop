//! sm-library — escaneo de disco, lectura de etiquetas (lofty) y
//! deduplicación por metadatos.
//!
//! Reglas de duplicados portadas de `core/dedupe/detector.py` de
//! harmonic-engine, adaptadas a metadatos: mismo artist+title normalizado
//! con duración similar (±3 s) — union-find como en el original.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use lofty::{Accessor, AudioFile, PictureType, TaggedFileExt};
use sm_model::TrackMeta;
use walkdir::WalkDir;

/// Extensiones de audio soportadas por el escaneo (case-insensitive).
pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "wav", "wave", "flac", "aiff", "aif", "aifc", "m4a", "mp4", "m4b", "ogg", "oga",
    "opus", "aac",
];

/// Resultado de leer un archivo: metadatos normalizados + portada embebida.
#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub meta: TrackMeta,
    pub cover: Option<Vec<u8>>,
}

/// Grupo de duplicados: `(id, path, duration_s)` por pista.
#[derive(Debug, Clone, PartialEq)]
pub struct DuplicateGroup {
    pub reason: String,
    pub tracks: Vec<(i64, String, f64)>,
}

/// Escaneo recursivo de `root` buscando archivos de audio.
/// Ignora directorios ocultos (incluido `.git`) y devuelve rutas ordenadas.
pub fn scan_root(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !is_hidden_dir(e))
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        // resource forks de macOS (._foo.mp3): no son audio real
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("._") || name == ".DS_Store" {
            continue;
        }
        let ext_match = entry
            .path()
            .extension()
            .map(|e| {
                let e = e.to_string_lossy().to_ascii_lowercase();
                AUDIO_EXTS.contains(&e.as_str())
            })
            .unwrap_or(false);
        if ext_match {
            out.push(entry.into_path());
        }
    }
    out.sort();
    out
}

fn is_hidden_dir(e: &walkdir::DirEntry) -> bool {
    if e.depth() == 0 {
        return false; // la raíz explícita siempre se recorre
    }
    e.file_name()
        .to_str()
        .map(|n| n.starts_with('.'))
        .unwrap_or(false)
}

/// Lee etiquetas y propiedades de audio con lofty. El formato sale de la
/// extensión; tamaño/mtime de `fs::metadata`. Sin tags válidos lofty puede
/// seguir parseando propiedades; si el archivo es ilegible, propaga error.
pub fn read_metadata(path: &Path) -> Result<ScannedFile> {
    let fs_meta = std::fs::metadata(path)
        .with_context(|| format!("stat de {}", path.display()))?;
    let file_size = fs_meta.len();
    let file_modified_unix = fs_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let format = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    let tagged = lofty::read_from_path(path)
        .with_context(|| format!("leyendo etiquetas de {}", path.display()))?;

    let props = tagged.properties();
    let duration_s = props.duration().as_secs_f64();
    let bitrate_kbps = props.audio_bitrate().or(props.overall_bitrate()).filter(|b| *b > 0);
    let sample_rate = props.sample_rate().filter(|s| *s > 0).map(|s| s as u32);
    let channels = props.channels().filter(|c| *c > 0).map(|c| c as u16);

    let tag = tagged.primary_tag();
    let get = |f: &dyn Fn(&lofty::Tag) -> Option<std::borrow::Cow<'_, str>>| {
        tag.and_then(|t| f(t)).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    };
    let title_tag = get(&|t| t.title());
    let artist = get(&|t| t.artist());
    let album = get(&|t| t.album());
    let genre = get(&|t| t.genre());
    let year = tag.and_then(|t| t.year()).map(|y| y as i32);
    let track_no = tag.and_then(|t| t.track()).map(|n| n as i32);

    // fallback: nombre de archivo → "N - Artist - Title" / "Artist - Title"
    // (como herramienta de referencia muestra pistas sin tags)
    let mut title = title_tag;
    let mut artist = artist;
    if title.is_none() {
        if let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string()) {
            let parts: Vec<&str> = stem.splitn(3, " - ").collect();
            match parts.len() {
                3 if parts[0].chars().next().is_some_and(|c| c.is_ascii_digit()) => {
                    artist = artist.or_else(|| Some(parts[1].trim().to_string()));
                    title = Some(parts[2].trim().to_string());
                }
                2 => {
                    artist = artist.or_else(|| Some(parts[0].trim().to_string()));
                    title = Some(parts[1].trim().to_string());
                }
                _ => title = Some(stem),
            }
        }
    }

    let cover = tag
        .and_then(|t| {
            t.get_picture_type(PictureType::CoverFront)
                .or_else(|| t.pictures().first())
        })
        .map(|p| p.data().to_vec());

    Ok(ScannedFile {
        meta: TrackMeta {
            path: path.to_string_lossy().to_string(),
            title: title.unwrap_or_default(),
            artist: artist.unwrap_or_default(),
            album: album.unwrap_or_default(),
            genre: genre.unwrap_or_default(),
            year,
            track_no,
            duration_s,
            bitrate_kbps,
            sample_rate,
            channels,
            format,
            file_size,
            file_modified_unix,
        },
        cover,
    })
}

/// Importa un M3U/M3U8: ignora comentarios `#...`, resuelve rutas relativas
/// contra el directorio del propio archivo y devuelve las absolutas.
pub fn import_m3u(path: &Path) -> Result<Vec<String>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("leyendo {}", path.display()))?;
    let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let p = Path::new(line);
        let resolved = if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
        out.push(resolved.to_string_lossy().to_string());
    }
    Ok(out)
}

/// Duplicados por metadatos (portado de `core/dedupe/detector.py`, regla de
/// tags): mismo artist+title normalizado Y duración similar ±3 s.
/// Agrupado por union-find; `reason` = `"artist+title"`.
pub fn find_duplicates(metas: &[(i64, TrackMeta)]) -> Vec<DuplicateGroup> {
    const DUR_TOL: f64 = 3.0;

    let mut by_key: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (i, (_, m)) in metas.iter().enumerate() {
        let na = normalize(&m.artist);
        let nt = normalize(&m.title);
        if na.is_empty() || nt.is_empty() {
            continue; // sin tags no hay señal de duplicado
        }
        by_key.entry((na, nt)).or_default().push(i);
    }

    let mut dsu = Dsu::new(metas.len());
    for idxs in by_key.values() {
        for xi in 0..idxs.len() {
            for yi in xi + 1..idxs.len() {
                let (a, b) = (idxs[xi], idxs[yi]);
                if (metas[a].1.duration_s - metas[b].1.duration_s).abs() <= DUR_TOL {
                    dsu.union(a, b);
                }
            }
        }
    }

    let mut comps: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..metas.len() {
        comps.entry(dsu.find(i)).or_default().push(i);
    }
    let mut groups: Vec<DuplicateGroup> = comps
        .into_values()
        .filter(|m| m.len() >= 2)
        .map(|members| {
            let mut tracks: Vec<(i64, String, f64)> = members
                .iter()
                .map(|&i| (metas[i].0, metas[i].1.path.clone(), metas[i].1.duration_s))
                .collect();
            tracks.sort_by(|a, b| a.1.cmp(&b.1));
            DuplicateGroup { reason: "artist+title".to_string(), tracks }
        })
        .collect();
    groups.sort_by(|a, b| {
        b.tracks.len().cmp(&a.tracks.len()).then_with(|| a.tracks[0].1.cmp(&b.tracks[0].1))
    });
    groups
}

// -- helpers privados (misma normalización que sm-db) -----------------------

/// minúsculas, solo alfanuméricos, espacios colapsados.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for ch in s.chars().flat_map(|c| c.to_lowercase()) {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim_end().to_string()
}

struct Dsu {
    parent: Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect() }
    }
    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra.max(rb)] = ra.min(rb);
        }
    }
}
