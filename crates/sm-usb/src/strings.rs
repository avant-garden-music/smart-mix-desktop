//! Strings i18n del módulo USB — port de `core/usb/STRINGS.md`.
//!
//! En esta fase el crate devuelve SIEMPRE el texto fuente EN con los
//! `{placeholders}` interpolados; las claves son idénticas a las del módulo
//! Python (`usb.scan.title`, …) para que la app pueda traducirlas después
//! (contrato: `core/usb/STRINGS.md`, copiado junto a este crate).
//!
//! ```ignore
//! use sm_usb::strings::t;
//! assert!(t("usb.scan.title", &[("n", "42"), ("bytes", "1.0 KB")])
//!     .starts_with("Found 42 files"));
//! ```

/// Interpola `{name}` con los parámetros dados (estilo `str.format` de
/// Python). Los placeholders sin parámetro quedan literales; una clave
/// desconocida se devuelve tal cual (bug visible, nunca pánico).
pub fn t(key: &str, params: &[(&str, &str)]) -> String {
    match template(key) {
        Some(tmpl) => interpolate(tmpl, params),
        None => key.to_string(),
    }
}

/// Todas las claves del contrato STRINGS.md (para el test de sincronía).
pub fn all_keys() -> Vec<&'static str> {
    vec![
        // usb.scan
        "usb.scan.not_a_dir",
        "usb.scan.start",
        "usb.scan.progress",
        "usb.scan.read_error",
        "usb.scan.title",
        // usb.backup
        "usb.backup.start",
        "usb.backup.progress",
        "usb.backup.done",
        "usb.backup.verify_start",
        "usb.backup.verify_done",
        "usb.backup.hash_mismatch",
        "usb.backup.missing_file",
        "usb.backup.read_error",
        // usb.fat32
        "usb.fat32.oversize",
        "usb.fat32.invalid_chars",
        "usb.fat32.emoji",
        "usb.fat32.name_too_long",
        "usb.fat32.name_gt_31",
        "usb.fat32.loose_track",
        // usb.format
        "usb.format.bad_device.windows",
        "usb.format.bad_device.macos",
        "usb.format.bad_device.linux",
        "usb.format.label_truncated",
        "usb.format.linux_umount",
        "usb.format.windows_large",
        "usb.format.needs_admin",
        "usb.format.wipe_warning",
        "usb.format.unsupported_os",
        // usb.restore
        "usb.restore.start",
        "usb.restore.progress",
        "usb.restore.done",
        "usb.restore.hash_mismatch",
        "usb.restore.missing_file",
        // usb.workflow
        "usb.workflow.step_scan",
        "usb.workflow.step_validate",
        "usb.workflow.step_backup",
        "usb.workflow.step_verify",
        "usb.workflow.step_format",
        "usb.workflow.step_restore",
        "usb.workflow.dry_run",
        "usb.workflow.backup_corrupt",
        "usb.workflow.confirmation_missing",
        "usb.workflow.format_failed",
        "usb.workflow.format_skipped",
        "usb.workflow.no_format_device",
        "usb.workflow.nothing_to_backup",
        // usb.report
        "usb.report.title",
        "usb.report.dry_run_note",
        "usb.report.special",
        "usb.report.tracks",
        "usb.report.restored",
        "usb.report.playlists_cues",
        "usb.report.bytes",
        "usb.report.fat32_warnings",
        "usb.report.errors",
        "usb.report.no_errors",
    ]
}

/// Texto fuente EN de cada clave (byte a byte de STRINGS.md).
fn template(key: &str) -> Option<&'static str> {
    Some(match key {
        // usb.scan
        "usb.scan.not_a_dir" => "Not a folder or not mounted: {root}",
        "usb.scan.start" => "Scanning drive {root}",
        "usb.scan.progress" => "Scanning: {n} files found",
        "usb.scan.read_error" => "Could not read {path}: {error}",
        "usb.scan.title" => "Found {n} files ({bytes})",
        // usb.backup
        "usb.backup.start" => "Backing up {n} files to {dest}",
        "usb.backup.progress" => "Copying {path}",
        "usb.backup.done" => "Backup complete: {n} files ({bytes})",
        "usb.backup.verify_start" => "Verifying backup hashes",
        "usb.backup.verify_done" => "Backup verification finished: {n} mismatches",
        "usb.backup.hash_mismatch" => "SHA-256 mismatch in backup for {path}",
        "usb.backup.missing_file" => "Missing from backup: {path}",
        "usb.backup.read_error" => "Could not back up {path}: {error}",
        // usb.fat32
        "usb.fat32.oversize" => "{path} is {size} bytes; FAT32 files max out at {max} bytes (4 GB). Re-encode to FLAC or ALAC, or split the file.",
        "usb.fat32.invalid_chars" => "{path} contains characters not allowed on FAT32: {chars}",
        "usb.fat32.emoji" => "{path} contains emojis or symbols that may not survive a FAT32 format",
        "usb.fat32.name_too_long" => "{path} has a name longer than 255 characters",
        "usb.fat32.name_gt_31" => "{path}: name is longer than 31 characters - some FAT32 DJ gear truncates or skips it",
        "usb.fat32.loose_track" => "{path} is loose in the drive root; moving it into a subfolder is recommended",
        // usb.format
        "usb.format.bad_device.windows" => "Windows device must be a drive letter like E or E: (got \"{device}\")",
        "usb.format.bad_device.macos" => "macOS device must be a disk identifier like disk4 or /dev/disk4 (got \"{device}\")",
        "usb.format.bad_device.linux" => "Linux device must be a block device like /dev/sdb (got \"{device}\")",
        "usb.format.label_truncated" => "Volume label \"{label}\" was truncated to \"{new}\" (FAT32 allows 11 characters)",
        "usb.format.linux_umount" => "Unmount the device first: umount {device}",
        "usb.format.windows_large" => "The volume is larger than 32 GB: the Windows GUI cannot format FAT32 here. Use the PowerShell command provided (diskpart is the equivalent alternative).",
        "usb.format.needs_admin" => "Administrator elevation is required: run the command from an elevated prompt (Windows: \"Run as administrator\"; macOS/Linux: sudo).",
        "usb.format.wipe_warning" => "WARNING: this ERASES everything on {device}. Only run it after the backup has been verified.",
        "usb.format.unsupported_os" => "Unsupported operating system: {os}",
        // usb.restore
        "usb.restore.start" => "Restoring {n} files to {dest}",
        "usb.restore.progress" => "Restoring {path}",
        "usb.restore.done" => "Restore complete: {n} files",
        "usb.restore.hash_mismatch" => "SHA-256 mismatch after restore: {path}",
        "usb.restore.missing_file" => "Could not restore {path}: {error}",
        // usb.workflow
        "usb.workflow.step_scan" => "[1/5] Scanned drive: {n} files",
        "usb.workflow.step_validate" => "[2/5] FAT32 validation: {n} warnings",
        "usb.workflow.step_backup" => "[3/5] Backup: {n} files copied",
        "usb.workflow.step_verify" => "[4/5] Backup verified: {n} hashes OK",
        "usb.workflow.step_format" => "[5/5] Format prepared: {command}",
        "usb.workflow.step_restore" => "Restore complete: {n} files back on the drive",
        "usb.workflow.dry_run" => "Dry run: nothing was written. A real run would back up to {dest}",
        "usb.workflow.backup_corrupt" => "Backup verification failed with {n} mismatches: aborting before the format",
        "usb.workflow.confirmation_missing" => "Double confirmation required: type the token {token} TWICE to authorize the format",
        "usb.workflow.format_failed" => "The format command reported failure; nothing was restored",
        "usb.workflow.format_skipped" => "Format not executed here: run the prepared command manually, then restore",
        "usb.workflow.no_format_device" => "No format device given: backup-only run",
        "usb.workflow.nothing_to_backup" => "The drive has no files to back up",
        // usb.report
        "usb.report.title" => "USB Safe Format — summary",
        "usb.report.dry_run_note" => "(dry run: counts are from the scan only)",
        "usb.report.special" => "DJ libraries detected: {names}",
        "usb.report.tracks" => "Tracks: {restored}/{n} restored",
        "usb.report.restored" => "Files restored: {n}/{total}",
        "usb.report.playlists_cues" => "Playlists/cues metadata restored (PIONEER/EngineLibrary): {n}",
        "usb.report.bytes" => "Total data: {size}",
        "usb.report.fat32_warnings" => "FAT32 warnings before format: {n}",
        "usb.report.errors" => "Errors: {n}",
        "usb.report.no_errors" => "No errors",
        _ => return None,
    })
}

/// Sustituye cada `{name}` presente en `params`; el resto queda literal.
fn interpolate(tmpl: &str, params: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(tmpl.len() + 32);
    let mut rest = tmpl;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(j) = after.find('}') {
            let name = &after[..j];
            match params.iter().find(|(k, _)| *k == name) {
                Some((_, v)) => out.push_str(v),
                None => {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
            }
            rest = &after[j + 1..];
        } else {
            out.push('{');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpola_placeholders() {
        assert_eq!(
            t("usb.scan.title", &[("n", "42"), ("bytes", "1.0 KB")]),
            "Found 42 files (1.0 KB)"
        );
    }

    #[test]
    fn placeholder_sin_parametro_queda_literal() {
        assert_eq!(t("usb.scan.start", &[]), "Scanning drive {root}");
    }

    #[test]
    fn clave_desconocida_se_devuelve_tal_cual() {
        // clave construida en runtime para no introducir un literal que el
        // test de sincronía con STRINGS.md pudiera confundir con clave real
        let key = String::from("usb") + ".noexiste.nada";
        assert_eq!(t(&key, &[("n", "1")]), key);
    }

    #[test]
    fn todas_las_claves_tienen_plantilla() {
        let keys = all_keys();
        assert_eq!(keys.len(), 56);
        for k in keys {
            assert!(template(k).is_some(), "sin plantilla: {k}");
        }
    }
}
