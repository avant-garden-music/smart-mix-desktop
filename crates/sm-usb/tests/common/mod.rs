//! Helpers compartidos por los tests de integración de sm-usb — espejo de
//! los helpers de `tests/test_usb.py` (make_file, snapshot,
//! assert_trees_identical, fixture fake_usb).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use sm_usb::sha256_hex;

/// xorshift64 determinista: bytes "aleatorios" reproducibles por test.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            let x = self.0;
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            let x = x ^ self.0;
            out.extend_from_slice(&x.to_le_bytes());
        }
        out.truncate(n);
        out
    }
}

/// Directorio temporal único por test (limpio al entrar).
pub fn tmpdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sm-usb-test-{}-{name}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("limpiar tmpdir previa");
    }
    fs::create_dir_all(&dir).expect("crear tmpdir");
    dir
}

pub fn make_file(path: &Path, data: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("crear padre");
    }
    fs::write(path, data).expect("escribir archivo");
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// relpath POSIX → sha256 para TODOS los archivos bajo root (basura
/// incluida), igual que `snapshot` del .py.
pub fn snapshot(root: &Path) -> BTreeMap<String, String> {
    let mut files = Vec::new();
    walk_files(root, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/");
            let data = fs::read(&p).expect("leer para snapshot");
            (rel, sha256_hex(&data))
        })
        .collect()
}

/// Byte a byte: cada relpath existe en ambos árboles con el mismo hash.
pub fn assert_trees_identical(a: &Path, b: &Path, relpaths: &[String]) {
    for rel in relpaths {
        let pa = rel_to_path(a, rel);
        let pb = rel_to_path(b, rel);
        assert!(pa.is_file(), "missing in {}: {rel}", a.display());
        assert!(pb.is_file(), "missing in {}: {rel}", b.display());
        let da = fs::read(&pa).expect("leer a");
        let db = fs::read(&pb).expect("leer b");
        assert_eq!(da, db, "contenido difiere: {rel}");
        assert_eq!(sha256_hex(&da), sha256_hex(&db), "hash difiere: {rel}");
    }
}

/// Convierte un relpath POSIX en ruta nativa bajo `base` (los manifiestos
/// siempre usan `/`).
pub fn rel_to_path(base: &Path, rel: &str) -> PathBuf {
    let mut p = base.to_path_buf();
    for part in rel.split('/') {
        p.push(part);
    }
    p
}

/// USB falso del .py: música, librerías DJ (PIONEER + EngineLibrary) y
/// basura del SO. 7 archivos manifestables, 2 pistas.
pub fn fake_usb(tmp: &Path) -> PathBuf {
    let root = tmp.join("USB");
    make_file(&root.join("Music/Techno/Track A.MP3"), b"audio-a".repeat(101).as_slice());
    make_file(&root.join("Music/House/deep/bEEt.Wav"), b"audio-b".repeat(77).as_slice());
    make_file(&root.join("playlists/warmup.m3u"), b"#EXTM3U\n");
    make_file(&root.join("PIONEER/USBANLZ/000.DAT"), b"anlz".repeat(50).as_slice());
    make_file(&root.join("PIONEER/ARTWORK/cover.jpg"), b"jpgdata".repeat(20).as_slice());
    make_file(&root.join("PIONEER/rekordbox.dbs"), b"dbs".repeat(40).as_slice());
    make_file(&root.join("EngineLibrary/m.db"), b"engine".repeat(30).as_slice());
    // Basura del SO que debe quedar fuera del manifiesto.
    make_file(&root.join(".DS_Store"), b"junk");
    make_file(&root.join("._Track A.MP3"), b"junk");
    make_file(&root.join(".hidden/secret.txt"), b"junk");
    make_file(
        &root.join("System Volume Information/WPSettings.dat"),
        b"junk",
    );
    root
}

/// Archivo sintético para inyectar en el manifiesto sin tocar el disco
/// (los tests de nombre inválido/emoji/oversize del .py usan archivos
/// reales, pero `<>*?` no puede existir en Windows/NTFS).
pub fn synthetic_entry(relpath: &str, size: u64) -> sm_usb::FileEntry {
    sm_usb::FileEntry {
        relpath: relpath.to_string(),
        size,
        sha256: "0".repeat(64),
    }
}
