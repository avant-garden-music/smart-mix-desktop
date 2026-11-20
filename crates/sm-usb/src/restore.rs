//! Restore: copia el backup de vuelta con rutas relativas IDÉNTICAS +
//! verificación de hash. Port de `core/usb/restore.py`.
//!
//! Cada archivo restaurado se hashea mientras se escribe y se compara con
//! el manifiesto capturado en el escaneo, de modo que un roundtrip bit a
//! bit queda garantizado (o reportado en `RestoreResult.hash_failures`).

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::backup::{rel_to_path, BackupMode, BackupResult};
use crate::scanner::{FileEntry, ProgressCb};
use crate::strings::t;

const CHUNK: usize = 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct RestoreResult {
    pub dest: PathBuf,
    pub files_restored: usize,
    pub manifest: Vec<FileEntry>,
    pub hash_failures: Vec<String>,
    pub errors: Vec<String>,
}

impl RestoreResult {
    pub fn ok(&self) -> bool {
        self.hash_failures.is_empty() && self.errors.is_empty()
    }
}

enum StreamOutcome {
    Ok,
    HashMismatch,
}

/// Stream de src → target hasheando en vuelo; Err ≙ OSError del .py.
fn restore_stream<R: Read>(
    src: &mut R,
    target: &Path,
    expected_sha: &str,
) -> std::io::Result<StreamOutcome> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut h = Sha256::new();
    let mut out = fs::File::create(target)?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n])?;
    }
    let hex = format!("{:x}", h.finalize());
    Ok(if hex != expected_sha {
        StreamOutcome::HashMismatch
    } else {
        StreamOutcome::Ok
    })
}

/// Restaura el backup completo en `dest` preservando las rutas relativas.
pub fn restore_backup(
    backup: &BackupResult,
    dest: &Path,
    mut progress: Option<ProgressCb<'_>>,
) -> RestoreResult {
    let mut result = RestoreResult {
        dest: dest.to_path_buf(),
        manifest: backup.manifest.clone(),
        ..Default::default()
    };
    if let Some(p) = progress.as_mut() {
        p(
            &t(
                "usb.restore.start",
                &[
                    ("n", &backup.manifest.len().to_string()),
                    ("dest", &dest.to_string_lossy()),
                ],
            ),
            0.0,
        );
    }

    let total = backup.manifest.len();
    match backup.mode {
        BackupMode::Zip => {
            let opened =
                fs::File::open(&backup.location).ok().and_then(|f| zip::ZipArchive::new(f).ok());
            let mut zf = match opened {
                Some(zf) => zf,
                None => {
                    for entry in &backup.manifest {
                        result.errors.push(t(
                            "usb.restore.missing_file",
                            &[("path", &entry.relpath), ("error", "not in zip")],
                        ));
                    }
                    return finish_restore(result, progress);
                }
            };
            for (i, entry) in backup.manifest.iter().enumerate() {
                match zf.by_name(&entry.relpath) {
                    Err(zip::result::ZipError::FileNotFound) => {
                        result.errors.push(t(
                            "usb.restore.missing_file",
                            &[("path", &entry.relpath), ("error", "not in zip")],
                        ));
                    }
                    Ok(mut fh) => handle_entry(&mut fh, entry, dest, &mut result),
                    Err(exc) => result.errors.push(t(
                        "usb.restore.missing_file",
                        &[("path", &entry.relpath), ("error", &exc.to_string())],
                    )),
                }
                if let Some(p) = progress.as_mut() {
                    let frac = if total == 0 {
                        1.0
                    } else {
                        (i + 1) as f64 / total as f64
                    };
                    p(&t("usb.restore.progress", &[("path", &entry.relpath)]), frac);
                }
            }
        }
        BackupMode::Folder => {
            for (i, entry) in backup.manifest.iter().enumerate() {
                let src = rel_to_path(&backup.location, &entry.relpath);
                if !src.is_file() {
                    result.errors.push(t(
                        "usb.restore.missing_file",
                        &[("path", &entry.relpath), ("error", "not found")],
                    ));
                } else {
                    match fs::File::open(&src) {
                        Ok(mut fh) => handle_entry(&mut fh, entry, dest, &mut result),
                        Err(exc) => result.errors.push(t(
                            "usb.restore.missing_file",
                            &[("path", &entry.relpath), ("error", &exc.to_string())],
                        )),
                    }
                }
                if let Some(p) = progress.as_mut() {
                    let frac = if total == 0 {
                        1.0
                    } else {
                        (i + 1) as f64 / total as f64
                    };
                    p(&t("usb.restore.progress", &[("path", &entry.relpath)]), frac);
                }
            }
        }
    }

    finish_restore(result, progress)
}

fn handle_entry<R: Read>(
    source: &mut R,
    entry: &FileEntry,
    dest: &Path,
    result: &mut RestoreResult,
) {
    let target = rel_to_path(dest, &entry.relpath);
    match restore_stream(source, &target, &entry.sha256) {
        Ok(StreamOutcome::Ok) => result.files_restored += 1,
        Ok(StreamOutcome::HashMismatch) => {
            result.hash_failures.push(entry.relpath.clone());
            result.errors.push(t(
                "usb.restore.hash_mismatch",
                &[("path", &entry.relpath)],
            ));
        }
        Err(exc) => result.errors.push(t(
            "usb.restore.missing_file",
            &[("path", &entry.relpath), ("error", &exc.to_string())],
        )),
    }
}

fn finish_restore(
    result: RestoreResult,
    mut progress: Option<ProgressCb<'_>>,
) -> RestoreResult {
    if let Some(p) = progress.as_mut() {
        p(
            &t(
                "usb.restore.done",
                &[("n", &result.files_restored.to_string())],
            ),
            1.0,
        );
    }
    result
}
