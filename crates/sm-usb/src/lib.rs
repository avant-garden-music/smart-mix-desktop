//! sm-usb — USB Safe Format: backup → formato FAT32 → restore.
//! Port fiel de `harmonic-engine/core/usb` (Python) al workspace Smart Mix.
//!
//! API pública pensada para CLI y GUI: funciones puras más callbacks de
//! progreso opcionales (`FnMut(&str, f64)` con mensaje de etapa localizado
//! y fracción 0.0–1.0). Las claves i18n son idénticas a las del módulo
//! Python (`usb.scan.title`, …, contrato en `STRINGS.md` junto a este
//! crate); en esta fase la librería devuelve el texto fuente EN y la app
//! traduce.
//!
//! Flujo típico:
//!
//! ```ignore
//! use sm_usb::{run_workflow, WorkflowOptions, build_report_from_workflow, format_report};
//!
//! let result = run_workflow(&usb_root, &backup_dir, WorkflowOptions::default())?; // dry-run
//! let result = run_workflow(
//!     &usb_root, &backup_dir,
//!     WorkflowOptions {
//!         dry_run: false,
//!         os_name: Some("linux"),
//!         format_device: Some("/dev/sdb"),
//!         format_executor: Some(&mut my_executor), // el caller lo ejecuta
//!         confirmations: Confirmations::Tokens(vec![token.clone(), token]),
//!         ..Default::default()
//!     },
//! )?;
//! println!("{}", format_report(&build_report_from_workflow(&result)));
//! ```
//!
//! Garantías de seguridad (idénticas al .py):
//! - `dry_run = true` por defecto; en ese modo no se escribe NADA.
//! - Este crate NUNCA ejecuta el comando de formato: `formatter` solo lo
//!   prepara y devuelve un token de confirmación que debe suministrarse
//!   DOS veces (doble confirmación) antes de que el caller pueda
//!   ejecutarlo.
//! - Los backups se verifican por hash (SHA-256) antes de que el workflow
//!   continúe y los restores conservan las rutas relativas exactas,
//!   re-verificando cada archivo.
pub mod backup;
pub mod export_dest;
pub mod fat32;
pub mod formatter;
pub mod report;
pub mod restore;
pub mod scanner;
pub mod strings;
pub mod workflow;

pub use backup::{
    backup_drive, backup_folder, backup_zip, verify_backup, BackupMode, BackupResult,
    MANIFEST_NAME,
};
pub use export_dest::{smartmix_dir, usb_export_warnings, FAT32_DJ_NAME_LIMIT};
pub use fat32::{validate_fat32, Fat32Warning, FAT32_MAX_FILESIZE};
pub use formatter::{
    check_double_confirmation, prepare_format_command, FormatError, FormatPlan,
};
pub use report::{build_report, build_report_from_workflow, format_report, Report, ReportInput};
pub use restore::{restore_backup, RestoreResult};
pub use scanner::{
    fmt_bytes, hash_file, is_audio, scan_drive, sha256_hex, FileEntry, ProgressCb, ScanError,
    ScanResult, SpecialFolders, AUDIO_EXTENSIONS, EXCLUDED_NAMES,
};
pub use workflow::{
    run_workflow, Confirmations, FormatExecutor, WorkflowError, WorkflowOptions, WorkflowResult,
};

/// Versión del port (≙ `core.usb.__version__`).
pub const VERSION: &str = "1.0.0";
