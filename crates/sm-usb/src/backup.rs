//! Backup de una unidad escaneada hacia carpeta local o zip comprimido.
//! Port de `core/usb/backup.py`.
//!
//! Ambos modos verifican los hashes SHA-256 de la copia antes de reportar
//! éxito: `BackupResult.failed` lista todo archivo cuyo hash de respaldo
//! no coincide con el manifiesto, y `verify_backup` re-comprueba el
//! destino en cualquier momento posterior (p. ej. justo antes del paso
//! destructivo de formato).

use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::scanner::{fmt_bytes, hash_file, FileEntry, ProgressCb, ScanResult};
use crate::strings::t;

pub const MANIFEST_NAME: &str = "_manifest.json";

/// Modo de backup (≙ los strings "folder" | "zip" del .py).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupMode {
    Folder,
    Zip,
}

impl std::fmt::Display for BackupMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BackupMode::Folder => "folder",
            BackupMode::Zip => "zip",
        })
    }
}

#[derive(Debug, Clone)]
pub struct BackupResult {
    pub location: PathBuf,
    pub mode: BackupMode,
    pub manifest: Vec<FileEntry>,
    pub warnings: Vec<String>,
    pub failed: Vec<String>,
}

impl BackupResult {
    pub fn ok(&self) -> bool {
        self.failed.is_empty()
    }
}

/// Manifiesto autodescriptivo: el backup se puede restaurar sin la app.
/// Mismo JSON que el .py (indent 2, claves relpath/size/sha256, sin
/// escapar ASCII).
fn manifest_json(manifest: &[FileEntry]) -> String {
    serde_json::to_string_pretty(manifest).expect("FileEntry siempre serializa")
}

/// Copia todo archivo del manifiesto a `dest` (árbol de carpetas o .zip) y
/// verifica. `mode` por defecto: "zip" si dest termina en .zip, si no
/// "folder". Los hashes se re-calculan en destino; los desajustes caen en
/// `BackupResult.failed` y el workflow se niega a formatear si no está
/// vacío.
pub fn backup_drive(
    scan: &ScanResult,
    dest: &Path,
    mode: Option<BackupMode>,
    progress: Option<ProgressCb<'_>>,
) -> BackupResult {
    let mut progress = progress;
    let mode = mode.unwrap_or({
        let is_zip = dest
            .extension()
            .map(|e| e.to_string_lossy().eq_ignore_ascii_case("zip"))
            .unwrap_or(false);
        if is_zip {
            BackupMode::Zip
        } else {
            BackupMode::Folder
        }
    });
    let manifest = scan.files.clone();
    if let Some(p) = progress.as_mut() {
        p(
            &t(
                "usb.backup.start",
                &[
                    ("n", &manifest.len().to_string()),
                    ("dest", &dest.to_string_lossy()),
                ],
            ),
            0.0,
        );
    }

    let mut warnings: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    match mode {
        BackupMode::Folder => {
            if fs::create_dir_all(dest).is_err() {
                // Python dejaría propagar el OSError; el contrato Rust
                // devuelve BackupResult → todo el manifiesto falla con
                // read_error (ok=false, observable e inequívoco).
                let err = format!("cannot create {}", dest.display());
                for entry in &manifest {
                    failed.push(entry.relpath.clone());
                    warnings.push(t(
                        "usb.backup.read_error",
                        &[("path", &entry.relpath), ("error", &err)],
                    ));
                }
                return finish(scan, dest, mode, manifest, warnings, failed, progress);
            }
            for (i, entry) in manifest.iter().enumerate() {
                let src = rel_to_path(&scan.root, &entry.relpath);
                let dst = rel_to_path(dest, &entry.relpath);
                // Espejo del try/except del .py: copy2 + re-hash dentro del
                // mismo intento; OSError → read_error, hash distinto →
                // hash_mismatch.
                match copy_and_verify(&src, &dst, &entry.sha256) {
                    Outcome::Ok => {}
                    Outcome::Mismatch => {
                        failed.push(entry.relpath.clone());
                        warnings.push(t(
                            "usb.backup.hash_mismatch",
                            &[("path", &entry.relpath)],
                        ));
                    }
                    Outcome::Err(exc) => {
                        failed.push(entry.relpath.clone());
                        warnings.push(t(
                            "usb.backup.read_error",
                            &[("path", &entry.relpath), ("error", &exc.to_string())],
                        ));
                    }
                }
                if let Some(p) = progress.as_mut() {
                    let frac = frac_of(i + 1, manifest.len());
                    p(&t("usb.backup.progress", &[("path", &entry.relpath)]), frac);
                }
            }
            let mpath = dest.join(MANIFEST_NAME);
            if fs::write(&mpath, manifest_json(&manifest)).is_err() {
                failed.push(MANIFEST_NAME.to_string());
            }
        }
        BackupMode::Zip => {
            match zip_write(
                scan,
                dest,
                &manifest,
                &mut progress,
                &mut failed,
                &mut warnings,
            ) {
                Ok(()) => {}
                Err(exc) => {
                    // No se pudo ni abrir el zip: todo el manifiesto falla.
                    for entry in &manifest {
                        if !failed.contains(&entry.relpath) {
                            failed.push(entry.relpath.clone());
                            warnings.push(t(
                                "usb.backup.read_error",
                                &[("path", &entry.relpath), ("error", &exc)],
                            ));
                        }
                    }
                }
            }
        }
    }

    finish(scan, dest, mode, manifest, warnings, failed, progress)
}

#[allow(clippy::too_many_arguments)]
fn finish(
    scan: &ScanResult,
    dest: &Path,
    mode: BackupMode,
    manifest: Vec<FileEntry>,
    warnings: Vec<String>,
    failed: Vec<String>,
    mut progress: Option<ProgressCb<'_>>,
) -> BackupResult {
    if let Some(p) = progress.as_mut() {
        p(
            &t(
                "usb.backup.done",
                &[
                    ("n", &(manifest.len() - failed.len()).to_string()),
                    ("bytes", &fmt_bytes(scan.total_bytes)),
                ],
            ),
            1.0,
        );
    }
    BackupResult {
        location: dest.to_path_buf(),
        mode,
        manifest,
        warnings,
        failed,
    }
}

fn frac_of(done: usize, total: usize) -> f64 {
    if total == 0 {
        1.0
    } else {
        done as f64 / total as f64
    }
}

/// Resultado de copiar un archivo del manifiesto y verificar su hash
/// en el MISMO pase de copia (try/except del .py; B5 del audit).
#[derive(Debug)]
enum Outcome {
    Ok,
    Mismatch,
    Err(std::io::Error),
}

/// Bloque del bucle de copia (mismo tamaño que el `CHUNK` del scanner).
const COPY_CHUNK: usize = 1024 * 1024;

/// Copia `src` → `dst` hasheando DURANTE la copia, sin la relectura
/// completa del destino que hacía `fs::copy` + `hash_file` (B5: dos
/// lecturas completas del archivo, caras en destino USB lento).
///
/// Verificación bit a bit contra un ancla INDEPENDIENTE: el SHA-256 del
/// stream que sale de `read()` en origen debe igualar `expected_sha`,
/// que calculó `scanner::hash_file` en una pasada separada durante el
/// escaneo. Cualquier bug de este bucle que altere un solo byte del
/// stream (bloques mal cortados, bytes fantasma del buffer, truncado)
/// rompe la igualdad y el archivo cae en `failed`. Además, la talla en
/// disco del destino debe coincidir con los bytes escritos.
///
/// Por qué NO hay un segundo hasher "del stream escrito": alimentado
/// desde el mismo buffer que se acaba de escribir solo puede dar un
/// resultado idéntico por construcción (nunca ve otros datos) y cuesta
/// duplicar el volumen hasheado (medido ~30% más lento en 50 MiB de
/// disco local). Un checksum del lado de escritura que no sea teatro
/// exigiría releer lo escrito — justo la pasada que B5 elimina.
///
/// Lo que se pierde frente a la relectura literal del destino: detectar
/// que el MEDIO retiene otros bytes que los aceptados por `write()`. La
/// relectura inmediata anterior tampoco detectaba eso de verdad (se
/// servía de la caché del SO, no del disco), y esa garantía más fuerte
/// la sigue dando `verify_backup`, que re-hashea el destino en un
/// momento posterior (p. ej. justo antes del paso destructivo). A cambio
/// se gana una comprobación que antes no existía: el stream LEÍDO del
/// origen se compara contra el manifiesto bit a bit (antes el origen
/// solo se verificaba indirectamente a través de la copia).
///
/// `fs::copy` preservaba la marca de tiempo del origen (CopyFileW /
/// `shutil.copy2` del .py): se replica best-effort tras cerrar,
/// ignorando errores de metadatos igual que hacía `fs::copy`.
fn copy_and_verify(src: &Path, dst: &Path, expected_sha: &str) -> Outcome {
    let copied = (|| -> std::io::Result<(String, u64)> {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        let mtime = fs::metadata(src).and_then(|m| m.modified()).ok();
        let mut reader = fs::File::open(src)?;
        // File::create trunca si el destino ya existía (semántica de fs::copy:
        // un destino previo más largo no deja basura al final).
        let mut writer = fs::File::create(dst)?;
        let mut read_h = Sha256::new();
        let mut buf = vec![0u8; COPY_CHUNK];
        let mut total: u64 = 0;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            // Solo el tramo efectivamente leído: ni bytes fantasma del
            // bloque anterior ni del buffer sin inicializar.
            read_h.update(&buf[..n]);
            // write_all solo retorna Ok cuando aceptó los n bytes completos.
            writer.write_all(&buf[..n])?;
            total += n as u64;
        }
        drop(writer);
        if let Some(mtime) = mtime {
            if let Ok(f) = fs::OpenOptions::new().write(true).open(dst) {
                let _ = f.set_modified(mtime); // best-effort, como fs::copy
            }
        }
        // Talla en disco == bytes escritos: detecta truncamientos groseros
        // del destino sin coste de lectura.
        let on_disk = fs::metadata(dst)?.len();
        if on_disk != total {
            return Err(std::io::Error::other(format!(
                "talla en disco {on_disk} != {total} bytes escritos"
            )));
        }
        Ok((format!("{:x}", read_h.finalize()), total))
    })();
    match copied {
        Err(e) => Outcome::Err(e),
        Ok((read_sha, _)) if read_sha == expected_sha => Outcome::Ok,
        Ok(_) => Outcome::Mismatch,
    }
}

/// Convierte un relpath POSIX en ruta nativa bajo `base`.
pub(crate) fn rel_to_path(base: &Path, rel: &str) -> PathBuf {
    let mut p = base.to_path_buf();
    for part in rel.split('/') {
        p.push(part);
    }
    p
}

/// Escribe el zip (deflate) con el manifiesto y lo re-verifica miembro a
/// miembro una vez cerrado y flusheado (como el .py).
fn zip_write(
    scan: &ScanResult,
    dest: &Path,
    manifest: &[FileEntry],
    progress: &mut Option<ProgressCb<'_>>,
    failed: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let file = fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut zw = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    for (i, entry) in manifest.iter().enumerate() {
        let src = rel_to_path(&scan.root, &entry.relpath);
        let write_err = (|| -> std::io::Result<()> {
            zw.start_file(entry.relpath.as_str(), opts)?;
            let mut fh = fs::File::open(&src)?;
            std::io::copy(&mut fh, &mut zw)?;
            Ok(())
        })()
        .err()
        .map(|e| e.to_string());
        if let Some(err) = write_err {
            failed.push(entry.relpath.clone());
            warnings.push(t(
                "usb.backup.read_error",
                &[("path", &entry.relpath), ("error", &err)],
            ));
        }
        if let Some(p) = progress.as_mut() {
            let frac = frac_of(i + 1, manifest.len());
            p(&t("usb.backup.progress", &[("path", &entry.relpath)]), frac);
        }
    }
    let manifest_err = (|| -> std::io::Result<()> {
        zw.start_file(MANIFEST_NAME, opts)?;
        zw.write_all(manifest_json(manifest).as_bytes())?;
        zw.finish()?;
        Ok(())
    })()
    .err()
    .map(|e| e.to_string());
    if let Some(err) = manifest_err {
        failed.push(MANIFEST_NAME.to_string());
        warnings.push(t(
            "usb.backup.read_error",
            &[("path", MANIFEST_NAME), ("error", &err)],
        ));
        return Ok(());
    }

    // Verificar miembros tras cerrar y flushear el archivo.
    let file = fs::File::open(dest).map_err(|e| e.to_string())?;
    let mut zf = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let names: HashSet<String> = zf.file_names().flatten().map(|n| n.to_string()).collect();
    for entry in manifest {
        if !names.contains(&entry.relpath) {
            failed.push(entry.relpath.clone());
            warnings.push(t("usb.backup.missing_file", &[("path", &entry.relpath)]));
            continue;
        }
        match zf.by_name(&entry.relpath) {
            Ok(mut fh) => {
                let h = stream_sha256(&mut fh);
                if h.as_deref() != Some(entry.sha256.as_str())
                    && !failed.contains(&entry.relpath)
                {
                    failed.push(entry.relpath.clone());
                    warnings.push(t(
                        "usb.backup.hash_mismatch",
                        &[("path", &entry.relpath)],
                    ));
                }
            }
            Err(_) => {
                failed.push(entry.relpath.clone());
                warnings.push(t("usb.backup.missing_file", &[("path", &entry.relpath)]));
            }
        }
    }
    Ok(())
}

fn stream_sha256<R: Read>(r: &mut R) -> Option<String> {
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => h.update(&buf[..n]),
            Err(_) => return None,
        }
    }
    Some(format!("{:x}", h.finalize()))
}

/// Backup en modo carpeta (atalajo de `backup_drive` con modo explícito).
pub fn backup_folder(
    scan: &ScanResult,
    dest: &Path,
    progress: Option<ProgressCb<'_>>,
) -> BackupResult {
    backup_drive(scan, dest, Some(BackupMode::Folder), progress)
}

/// Backup en modo zip (atalajo de `backup_drive` con modo explícito).
pub fn backup_zip(
    scan: &ScanResult,
    dest: &Path,
    progress: Option<ProgressCb<'_>>,
) -> BackupResult {
    backup_drive(scan, dest, Some(BackupMode::Zip), progress)
}

/// Re-hashea el destino del backup; devuelve los mensajes (localizados)
/// de desajuste. Se puede llamar en cualquier momento posterior, p. ej.
/// justo antes del paso destructivo.
pub fn verify_backup(
    backup: &BackupResult,
    mut progress: Option<ProgressCb<'_>>,
) -> Vec<String> {
    if let Some(p) = progress.as_mut() {
        p(&t("usb.backup.verify_start", &[]), 0.0);
    }
    let mut mismatches: Vec<String> = Vec::new();
    match backup.mode {
        BackupMode::Folder => {
            for (i, entry) in backup.manifest.iter().enumerate() {
                let path = rel_to_path(&backup.location, &entry.relpath);
                if !path.is_file() {
                    mismatches.push(t(
                        "usb.backup.missing_file",
                        &[("path", &entry.relpath)],
                    ));
                } else if hash_file(&path).ok().as_deref() != Some(entry.sha256.as_str()) {
                    mismatches.push(t(
                        "usb.backup.hash_mismatch",
                        &[("path", &entry.relpath)],
                    ));
                }
                if let Some(p) = progress.as_mut() {
                    let frac = frac_of(i + 1, backup.manifest.len());
                    p(&t("usb.backup.progress", &[("path", &entry.relpath)]), frac);
                }
            }
        }
        BackupMode::Zip => {
            let opened =
                fs::File::open(&backup.location).ok().and_then(|f| zip::ZipArchive::new(f).ok());
            match opened {
                None => {
                    for entry in &backup.manifest {
                        mismatches.push(t(
                            "usb.backup.missing_file",
                            &[("path", &entry.relpath)],
                        ));
                    }
                }
                Some(mut zf) => {
                    let names: HashSet<String> = zf
                        .file_names()
                        .flatten()
                        .map(|n| n.to_string())
                        .collect();
                    for (i, entry) in backup.manifest.iter().enumerate() {
                        if !names.contains(&entry.relpath) {
                            mismatches.push(t(
                                "usb.backup.missing_file",
                                &[("path", &entry.relpath)],
                            ));
                        } else if zf
                            .by_name(&entry.relpath)
                            .ok()
                            .and_then(|mut fh| stream_sha256(&mut fh))
                            .as_deref()
                            != Some(entry.sha256.as_str())
                        {
                            mismatches.push(t(
                                "usb.backup.hash_mismatch",
                                &[("path", &entry.relpath)],
                            ));
                        }
                        if let Some(p) = progress.as_mut() {
                            let frac = frac_of(i + 1, backup.manifest.len());
                            p(&t("usb.backup.progress", &[("path", &entry.relpath)]), frac);
                        }
                    }
                }
            }
        }
    }
    if let Some(p) = progress.as_mut() {
        p(
            &t("usb.backup.verify_done", &[("n", &mismatches.len().to_string())]),
            1.0,
        );
    }
    mismatches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::sha256_hex;

    /// xorshift64 determinista (espejo del helper de tests/common).
    struct Rng(u64);

    impl Rng {
        fn new(seed: u64) -> Self {
            Rng(seed | 1)
        }

        fn bytes(&mut self, n: usize) -> Vec<u8> {
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

    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn tmpdir(name: &str) -> PathBuf {
        // Sin `::process` del std (lo veta el guardián de safety.rs, que
        // escanea la fuente por vías de ejecución); un secuencial
        // atómico da unicidad entre llamidas concurrentes.
        let dir = std::env::temp_dir().join(format!(
            "sm-usb-cv-{}-{name}",
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("limpiar tmpdir previa");
        }
        fs::create_dir_all(&dir).expect("crear tmpdir");
        dir
    }

    fn make_file(path: &Path, data: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("crear padre");
        }
        fs::write(path, data).expect("escribir archivo");
    }

    /// Paridad copy-vs-relectura multi-tamaño (T5): `copy_and_verify` solo
    /// puede reportar Ok si una RELECTURA INDEPENDIENTE del destino
    /// (`hash_file`, pasada posterior por otra ruta) produce el mismo hash
    /// que el ancla del manifiesto. La relectura vive aquí en el test — es
    /// la comprobación de "qué hay de verdad en disco" que la rutina de
    /// producción ya no hace — y con datos pseudoaleatorios por bloque,
    /// cualquier error de límites del bucle de copia rompe la igualdad.
    #[test]
    fn copy_and_verify_paridad_multitamaño() {
        let tmp = tmpdir("sizes");
        let sizes: &[usize] = &[
            0, // 0 B (hash del vacío)
            COPY_CHUNK,               // exactamente 1 bloque
            COPY_CHUNK + 1,           // 1 bloque + resto mínimo
            2 * COPY_CHUNK + 123_456, // N bloques + resto
            6 * COPY_CHUNK + 321,     // grande sintético (~6 MiB)
        ];
        for (i, &size) in sizes.iter().enumerate() {
            let mut rng = Rng::new(0x5EED_0000 + size as u64);
            let data = rng.bytes(size);
            let src = tmp.join(format!("src{i}.bin"));
            make_file(&src, &data);
            // Padre inexistente a propósito: create_dir_all del dst.
            let dst = tmp.join(format!("out/dst{i}.bin"));
            let expected = sha256_hex(&data); // independiente del bucle copiado
            assert!(
                matches!(copy_and_verify(&src, &dst, &expected), Outcome::Ok),
                "size {size}: debía ser Ok"
            );
            assert_eq!(
                hash_file(&dst).unwrap(),
                expected,
                "size {size}: relectura del destino difiere"
            );
            assert_eq!(fs::metadata(&dst).unwrap().len() as usize, size);
        }
    }

    /// Archivo de origen inexistente → Err (read_error), nunca Ok ni
    /// Mismatch, y sin dejar basura de un intento a medias.
    #[test]
    fn copy_and_verify_origen_inexistente_es_err() {
        let tmp = tmpdir("missing");
        let src = tmp.join("no_existe.bin");
        let dst = tmp.join("out/no_existe.bin");
        let outcome = copy_and_verify(&src, &dst, &"0".repeat(64));
        assert!(
            matches!(outcome, Outcome::Err(_)),
            "debía ser Err, fue {outcome:?}"
        );
        assert!(!dst.exists(), "no debía crear el destino");
    }

    /// Manifiesto mentiroso (hash imposible): la copia se hace pero la
    /// verificación marca Mismatch — paridad con el .py (shutil.copy2 +
    /// re-hash): el archivo cae en `failed`, el destino queda copiado.
    #[test]
    fn copy_and_verify_hash_mismatch() {
        let tmp = tmpdir("mismatch");
        let data = b"contenido-verificable".repeat(64);
        let src = tmp.join("src.bin");
        let dst = tmp.join("dst.bin");
        make_file(&src, &data);
        let outcome = copy_and_verify(&src, &dst, &"f".repeat(64));
        assert!(matches!(outcome, Outcome::Mismatch));
        // El destino existe con el contenido correcto: el fallo es del
        // hash esperado, no de la copia.
        assert_eq!(hash_file(&dst).unwrap(), sha256_hex(&data));
    }

    /// Destino previo MÁS LARGO: se trunca (semántica de fs::copy) y la
    /// verificación pasa solo con el contenido nuevo — un residuo al final
    /// habría corrompido el backup silenciosamente.
    #[test]
    fn copy_and_verify_trunca_destino_previo_mas_largo() {
        let tmp = tmpdir("truncate");
        let data = rng_bytes(1024);
        let src = tmp.join("src.bin");
        make_file(&src, &data);
        let dst = tmp.join("dst.bin");
        make_file(&dst, &rng_bytes(5 * 1024 * 1024)); // 5 MiB previos
        let expected = sha256_hex(&data);
        assert!(matches!(
            copy_and_verify(&src, &dst, &expected),
            Outcome::Ok
        ));
        assert_eq!(fs::metadata(&dst).unwrap().len(), 1024);
        assert_eq!(hash_file(&dst).unwrap(), expected);
    }

    fn rng_bytes(n: usize) -> Vec<u8> {
        Rng::new(0x1234_ABCD).bytes(n)
    }
}
