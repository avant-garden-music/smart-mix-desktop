//! Orquestador del flujo: scan → validar → backup → verificar → formato →
//! restore. Port de `core/usb/workflow.py`.
//!
//! Modelo de seguridad (idéntico al .py):
//! - `dry_run = true` es el DEFAULT: no se escribe nada en ningún sitio.
//! - La librería en sí NUNCA ejecuta el comando de formato. Un caller
//!   puede inyectar un `format_executor` (responsabilidad del CLI/GUI);
//!   solo corre tras verificar los hashes del backup Y que el usuario
//!   pasó el token de confirmación del plan dos veces (doble
//!   confirmación).
//! - Tras un formato exitoso, el backup se restaura en la unidad con
//!   rutas relativas idénticas y se re-verifica.

use std::path::{Path, PathBuf};

use crate::backup::{backup_drive, verify_backup, BackupMode, BackupResult};
use crate::fat32::{validate_fat32, Fat32Warning};
use crate::formatter::{
    check_double_confirmation, prepare_format_command, FormatPlan,
};
use crate::restore::{restore_backup, RestoreResult};
use crate::scanner::{scan_drive, ProgressCb, ScanResult};
use crate::strings::t;

/// Hook opcional que ejecuta el formato real. Recibe el plan preparado,
/// devuelve `true` en éxito. La librería jamás provee uno.
pub type FormatExecutor<'a> = &'a mut (dyn FnMut(&FormatPlan) -> bool + 'a);

/// Confirmaciones del usuario: tokens ya recogidos o un proveedor que los
/// calcula a partir del plan (≙ `Sequence[str] | Callable` del .py).
pub enum Confirmations<'a> {
    Tokens(Vec<String>),
    Provider(&'a mut (dyn FnMut(&FormatPlan) -> Vec<String> + 'a)),
}

impl Default for Confirmations<'_> {
    fn default() -> Self {
        Confirmations::Tokens(Vec::new())
    }
}

impl Confirmations<'_> {
    fn resolve(&mut self, plan: &FormatPlan) -> Vec<String> {
        match self {
            Confirmations::Tokens(v) => v.clone(),
            Confirmations::Provider(f) => f(plan),
        }
    }
}

/// Error cuando una precondición de seguridad bloquea el workflow
/// (≙ `WorkflowError(RuntimeError)` del .py; mensaje localizado dentro).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowError(pub String);

impl std::fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WorkflowError {}

#[derive(Debug, Clone)]
pub struct WorkflowResult {
    pub dry_run: bool,
    pub scan: ScanResult,
    pub fat32_warnings: Vec<Fat32Warning>,
    pub backup: Option<BackupResult>,
    pub backup_verified: bool,
    pub format_plan: Option<FormatPlan>,
    pub restore: Option<RestoreResult>,
    pub steps: Vec<String>, // log localizado
}

/// Opciones del workflow (≙ los keyword args de `run_workflow`; los
/// defaults replican el .py, en particular `dry_run = true`).
pub struct WorkflowOptions<'a> {
    pub mode: Option<BackupMode>,
    pub dry_run: bool,
    pub os_name: Option<&'a str>,
    pub format_device: Option<&'a str>,
    pub label: &'a str,
    pub size_gb: Option<f64>,
    pub confirmations: Confirmations<'a>,
    pub format_executor: Option<FormatExecutor<'a>>,
    pub restore_after_format: bool,
    pub restore_root: Option<&'a Path>,
}

impl Default for WorkflowOptions<'_> {
    fn default() -> Self {
        WorkflowOptions {
            mode: None,
            dry_run: true,
            os_name: None,
            format_device: None,
            label: "HARMONIC",
            size_gb: None,
            confirmations: Confirmations::default(),
            format_executor: None,
            restore_after_format: true,
            restore_root: None,
        }
    }
}

/// Ejecuta el flujo a-b-c-d-e sobre `drive`, escribiendo backups en
/// `backup_dest`.
///
/// Pasos: (a) scan+manifiesto, (b) avisos FAT32, (c) backup, (d)
/// verificación de hashes, (e) preparar comando de formato [+ executor
/// inyectable opcional con doble confirmación, luego restore]).
pub fn run_workflow(
    drive: &Path,
    backup_dest: &Path,
    mut opts: WorkflowOptions<'_>,
    progress: Option<ProgressCb<'_>>,
) -> Result<WorkflowResult, WorkflowError> {
    let mut progress = progress;
    let mut steps: Vec<String> = Vec::new();
    // Adaptador: re-expone el callback opcional como un solo closure para
    // re-prestarlo en cada paso (un `&mut dyn` no se puede re-prestar
    // directamente por invarianza).
    let mut emit = |msg: &str, frac: f64| {
        if let Some(p) = progress.as_mut() {
            p(msg, frac);
        }
    };

    // (a) scan
    let scan = scan_drive(drive, Some(&mut emit as ProgressCb))
        .map_err(|e| WorkflowError(e.to_string()))?;
    steps.push(t(
        "usb.workflow.step_scan",
        &[("n", &scan.files.len().to_string())],
    ));
    if scan.files.is_empty() {
        return Err(WorkflowError(t("usb.workflow.nothing_to_backup", &[])));
    }

    // (b) validaciones pre-formato (avisos, nunca bloquean)
    let fat32_warnings = validate_fat32(&scan);
    steps.push(t(
        "usb.workflow.step_validate",
        &[("n", &fat32_warnings.len().to_string())],
    ));

    // (e-preview) preparar el comando si se dio dispositivo
    let plan: Option<FormatPlan> = match opts.format_device {
        Some(dev) => Some(
            prepare_format_command(dev, opts.os_name, opts.label, opts.size_gb)
                .map_err(|e| WorkflowError(e.to_string()))?,
        ),
        None => None,
    };

    if opts.dry_run {
        steps.push(t(
            "usb.workflow.dry_run",
            &[("dest", &backup_dest.to_string_lossy())],
        ));
        if let Some(p) = &plan {
            steps.push(t("usb.workflow.step_format", &[("command", &p.command)]));
        }
        return Ok(WorkflowResult {
            dry_run: true,
            scan,
            fat32_warnings,
            backup: None,
            backup_verified: false,
            format_plan: plan,
            restore: None,
            steps,
        });
    }

    // (c) backup
    let backup = backup_drive(&scan, backup_dest, opts.mode, Some(&mut emit as ProgressCb));
    steps.push(t(
        "usb.workflow.step_backup",
        &[("n", &backup.manifest.len().to_string())],
    ));

    // (d) verificar hashes del backup antes de nada destructivo
    let mismatches = verify_backup(&backup, Some(&mut emit as ProgressCb));
    if !mismatches.is_empty() {
        return Err(WorkflowError(t(
            "usb.workflow.backup_corrupt",
            &[("n", &mismatches.len().to_string())],
        )));
    }
    steps.push(t(
        "usb.workflow.step_verify",
        &[("n", &backup.manifest.len().to_string())],
    ));

    let mut result = WorkflowResult {
        dry_run: false,
        scan,
        fat32_warnings,
        backup: Some(backup),
        backup_verified: true,
        format_plan: plan,
        restore: None,
        steps,
    };

    let plan = match &result.format_plan {
        None => {
            result.steps.push(t("usb.workflow.no_format_device", &[]));
            return Ok(result);
        }
        Some(p) => p.clone(),
    };

    result
        .steps
        .push(t("usb.workflow.step_format", &[("command", &plan.command)]));
    let executor = match opts.format_executor.as_mut() {
        None => {
            // Solo backup: el usuario ejecuta el comando a mano y llama a
            // restore aparte (o re-corre con un executor).
            result.steps.push(t("usb.workflow.format_skipped", &[]));
            return Ok(result);
        }
        Some(exec) => exec,
    };

    // La doble confirmación puerta el executor inyectado.
    let entered = opts.confirmations.resolve(&plan);
    let entered_refs: Vec<&str> = entered.iter().map(|s| s.as_str()).collect();
    if !check_double_confirmation(&plan, &entered_refs) {
        return Err(WorkflowError(t(
            "usb.workflow.confirmation_missing",
            &[("token", &plan.confirm_token)],
        )));
    }
    if !executor(&plan) {
        return Err(WorkflowError(t("usb.workflow.format_failed", &[])));
    }

    if opts.restore_after_format {
        let target: PathBuf = match opts.restore_root {
            Some(r) => r.to_path_buf(),
            None => drive.to_path_buf(),
        };
        let backup = result.backup.as_ref().expect("backup ya hecho");
        let restore =
            restore_backup(backup, &target, Some(&mut emit as ProgressCb));
        result.steps.push(t(
            "usb.workflow.step_restore",
            &[("n", &restore.files_restored.to_string())],
        ));
        result.restore = Some(restore);
    }
    Ok(result)
}
