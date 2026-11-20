//! Resumen legible de una corrida USB Safe Format. Port de
//! `core/usb/report.py`.
//!
//! Cuenta pistas, archivos restaurados y los metadatos de
//! playlists/cues que volvieron de las librerías DJ (PIONEER/ con
//! rekordbox.dbs, ARTWORK/ y USBANLZ/, más EngineLibrary/).

use crate::backup::BackupResult;
use crate::restore::RestoreResult;
use crate::scanner::{fmt_bytes, is_audio, FileEntry, ScanResult};
use crate::strings::t;
use crate::workflow::WorkflowResult;

/// Subárboles que cargan metadatos de playlists/cues/artwork para
/// equipo DJ.
const METADATA_PREFIXES: &[&str] = &[
    "pioneer/usbanlz/",
    "pioneer/rekordbox.dbs",
    "pioneer/artwork/",
    "enginelibrary/",
];

fn is_metadata(relpath: &str) -> bool {
    let low = relpath.to_lowercase();
    METADATA_PREFIXES.iter().any(|p| low.starts_with(p))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub files_total: usize,
    pub files_restored: usize,
    pub bytes_total: u64,
    pub bytes_restored: u64,
    pub tracks_total: usize,
    pub tracks_restored: usize,
    pub playlists_cues_restored: usize,
    pub fat32_warnings: usize,
    pub dry_run: bool,
    pub special: Vec<String>,
    pub errors: Vec<String>,
}

/// Entradas de `build_report`: piezas explícitas o un workflow entero
/// (≙ los parámetros de `build_report` del .py).
#[derive(Default)]
pub struct ReportInput<'a> {
    pub scan: Option<&'a ScanResult>,
    pub backup: Option<&'a BackupResult>,
    pub restore: Option<&'a RestoreResult>,
    pub dry_run: bool,
    pub fat32_warning_count: usize,
    pub errors: Vec<String>,
}

impl<'a> ReportInput<'a> {
    /// Agregado desde un `WorkflowResult` (≙ `build_report(workflow_result=…)`).
    pub fn from_workflow(wf: &'a WorkflowResult) -> Self {
        let mut errors = wf.scan.errors.clone();
        if let Some(backup) = &wf.backup {
            errors.extend(backup.warnings.iter().cloned());
        }
        if let Some(restore) = &wf.restore {
            errors.extend(restore.errors.iter().cloned());
        }
        ReportInput {
            scan: Some(&wf.scan),
            backup: wf.backup.as_ref(),
            restore: wf.restore.as_ref(),
            dry_run: wf.dry_run,
            fat32_warning_count: wf.fat32_warnings.len(),
            errors,
        }
    }
}

/// Agrega un reporte desde piezas explícitas o de un workflow.
pub fn build_report(input: &ReportInput) -> Report {
    let mut report = Report {
        dry_run: input.dry_run,
        fat32_warnings: input.fat32_warning_count,
        errors: input.errors.clone(),
        ..Default::default()
    };
    if let Some(scan) = input.scan {
        report.files_total = scan.files.len();
        report.bytes_total = scan.total_bytes;
        report.tracks_total = scan.track_count();
        report.special = scan.special.detected();
    }
    if let Some(restore) = input.restore {
        let restored_entries: &[FileEntry] = &restore.manifest;
        if !restored_entries.is_empty() {
            report.files_restored = restore.files_restored;
            report.tracks_restored = restored_entries
                .iter()
                .filter(|e| is_audio(&e.relpath))
                .count();
            report.playlists_cues_restored = restored_entries
                .iter()
                .filter(|e| is_metadata(&e.relpath))
                .count();
            report.bytes_restored = restored_entries.iter().map(|e| e.size).sum();
        }
    }
    report
}

/// Atajo equivalente a `build_report(workflow_result=…)` del .py.
pub fn build_report_from_workflow(wf: &WorkflowResult) -> Report {
    build_report(&ReportInput::from_workflow(wf))
}

/// Renderiza el reporte como texto localizado, un hecho por línea.
pub fn format_report(report: &Report) -> String {
    let mut lines = vec![t("usb.report.title", &[])];
    if report.dry_run {
        lines.push(t("usb.report.dry_run_note", &[]));
    }
    if !report.special.is_empty() {
        lines.push(t(
            "usb.report.special",
            &[("names", &report.special.join(", "))],
        ));
    }
    lines.push(t(
        "usb.report.tracks",
        &[
            ("n", &report.tracks_total.to_string()),
            ("restored", &report.tracks_restored.to_string()),
        ],
    ));
    lines.push(t(
        "usb.report.restored",
        &[
            ("n", &report.files_restored.to_string()),
            ("total", &report.files_total.to_string()),
        ],
    ));
    lines.push(t(
        "usb.report.playlists_cues",
        &[("n", &report.playlists_cues_restored.to_string())],
    ));
    lines.push(t(
        "usb.report.bytes",
        &[("size", &fmt_bytes(report.bytes_total))],
    ));
    lines.push(t(
        "usb.report.fat32_warnings",
        &[("n", &report.fat32_warnings.to_string())],
    ));
    if !report.errors.is_empty() {
        lines.push(t("usb.report.errors", &[("n", &report.errors.len().to_string())]));
        lines.extend(report.errors.iter().map(|e| format!("  - {e}")));
    } else {
        lines.push(t("usb.report.no_errors", &[]));
    }
    lines.join("\n")
}
