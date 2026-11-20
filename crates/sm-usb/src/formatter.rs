//! PREPARADOR del comando de formato — este módulo NUNCA ejecuta nada.
//! Port de `core/usb/formatter.py`.
//!
//! `prepare_format_command` construye el comando shell exacto para
//! formatear un dispositivo como FAT32 en cada SO, más notas localizadas
//! que explican elevación y riesgos. Ejecutarlo es responsabilidad
//! EXCLUSIVA de la capa CLI/GUI, tras `check_double_confirmation`: el
//! usuario debe teclear el token del plan DOS veces.
//!
//! Específico por SO:
//! - Windows: los volúmenes de más de 32 GB no se pueden formatear FAT32
//!   desde la GUI, así que los tamaños grandes/desconocidos van siempre
//!   por PowerShell `Format-Volume`.
//! - macOS: `diskutil eraseDisk "MS-DOS FAT32"`.
//! - Linux: `mkfs.vfat -F 32`.

use sha2::{Digest, Sha256};

use crate::strings::t;

pub const FAT32_LABEL_MAX: usize = 11;

/// Caracteres que no pueden aparecer en una etiqueta de volumen FAT32.
pub const INVALID_LABEL_CHARS: &[char] = &['\\', '/', ':', '*', '?', '<', '>', '|'];

const WINDOWS_LARGE_GB: f64 = 32.0;

/// Error al preparar el formato. `UnsupportedOs` ≙ `UnsupportedOSError`
/// (subclase de ValueError en Python); `BadDevice` ≙ `ValueError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    UnsupportedOs(String),
    BadDevice(String),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::UnsupportedOs(msg) | FormatError::BadDevice(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for FormatError {}

fn normalize_os(os_name: Option<&str>) -> Result<&'static str, FormatError> {
    // None → SO actual (Python usa platform.system(); consts::OS ya da la
    // clave normalizada "windows"/"macos"/"linux").
    let raw = os_name.unwrap_or(std::env::consts::OS);
    let key = os_name
        .unwrap_or(std::env::consts::OS)
        .trim()
        .to_lowercase();
    match key.as_str() {
        "windows" => Ok("windows"),
        "darwin" | "mac os x" | "macos" => Ok("macos"),
        "linux" => Ok("linux"),
        _ => Err(FormatError::UnsupportedOs(t(
            "usb.format.unsupported_os",
            &[("os", raw)],
        ))),
    }
}

/// Mayúsculas, sin caracteres inválidos para FAT32, truncado a 11
/// (con nota que lo explica).
fn sanitize_label(label: &str, notes: &mut Vec<String>) -> String {
    let mut clean: String = label
        .to_uppercase()
        .chars()
        .filter(|ch| {
            (*ch as u32) >= 0x20 && *ch != '"' && *ch != '\\' && !INVALID_LABEL_CHARS.contains(ch)
        })
        .collect();
    if clean.is_empty() {
        clean = "USB".to_string();
    }
    if clean.chars().count() > FAT32_LABEL_MAX {
        let new: String = clean.chars().take(FAT32_LABEL_MAX).collect();
        notes.push(t(
            "usb.format.label_truncated",
            &[("label", label), ("new", &new)],
        ));
        clean = new;
    }
    clean
}

/// `r?disk\d+` completo (fullmatch del .py, sin crate de regex).
fn is_disk_id(s: &str) -> bool {
    let rest = s.strip_prefix('r').unwrap_or(s);
    match rest.strip_prefix("disk") {
        Some(digits) => !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()),
        None => false,
    }
}

/// Comando de formato FAT32 preparado (NUNCA ejecutado) + metadatos de
/// seguridad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatPlan {
    pub os_name: String,
    pub device: String,
    pub command: String,
    pub shell: String, // "cmd" | "powershell" | "bash"
    pub needs_admin: bool,
    pub notes: Vec<String>, // localizadas
    pub confirm_token: String,
    pub label: String,
}

/// Construye el comando de formato FAT32 para `device`; devuelve un
/// `FormatPlan`.
///
/// `size_gb` (tamaño del volumen) solo importa en Windows, donde elige
/// entre `format` (volúmenes pequeños) y `Format-Volume` de PowerShell
/// (grandes o desconocidos).
pub fn prepare_format_command(
    device: &str,
    os_name: Option<&str>,
    label: &str,
    size_gb: Option<f64>,
) -> Result<FormatPlan, FormatError> {
    let os_key = normalize_os(os_name)?;
    let mut notes: Vec<String> = Vec::new();
    let lab = sanitize_label(label, &mut notes);
    let device = device.trim();

    let (dev, command, shell) = match os_key {
        "windows" => {
            let first = device.chars().next();
            if device.is_empty() || !first.map(|c| c.is_alphabetic()).unwrap_or(false) {
                return Err(FormatError::BadDevice(t(
                    "usb.format.bad_device.windows",
                    &[("device", device)],
                )));
            }
            let letter = first.unwrap().to_uppercase().to_string();
            let dev = format!("{letter}:");
            // Tamaño desconocido también va por PowerShell: sirve para
            // cualquier tamaño.
            let large = size_gb.is_none_or(|gb| gb > WINDOWS_LARGE_GB);
            if large {
                notes.push(t("usb.format.windows_large", &[]));
                let command = format!(
                    "powershell -NoProfile -Command \"Format-Volume -DriveLetter {letter} \
                     -FileSystem FAT32 -NewFileSystemLabel {lab} -Force\""
                );
                (dev, command, "powershell".to_string())
            } else {
                let command = format!("format {letter}: /FS:FAT32 /Q /V:{lab} /Y");
                (dev, command, "cmd".to_string())
            }
        }
        "macos" => {
            let candidate = device.strip_prefix("/dev/").unwrap_or(device);
            if !is_disk_id(candidate) {
                return Err(FormatError::BadDevice(t(
                    "usb.format.bad_device.macos",
                    &[("device", device)],
                )));
            }
            let dev = format!("/dev/{candidate}");
            let command = format!("diskutil eraseDisk \"MS-DOS FAT32\" {lab} {dev}");
            (dev, command, "bash".to_string())
        }
        _ => {
            // linux
            if !device.starts_with("/dev/") || device == "/dev/" {
                return Err(FormatError::BadDevice(t(
                    "usb.format.bad_device.linux",
                    &[("device", device)],
                )));
            }
            let dev = device.to_string();
            notes.push(t("usb.format.linux_umount", &[("device", &dev)]));
            let command = format!("mkfs.vfat -F 32 -n {lab} {dev}");
            (dev, command, "bash".to_string())
        }
    };

    notes.push(t("usb.format.wipe_warning", &[("device", &dev)]));
    notes.push(t("usb.format.needs_admin", &[]));
    // Determinista a propósito: el CLI puede mostrarlo en un dry run y
    // pedir al usuario teclearlo dos veces en la ejecución real. Es un
    // ritual de confirmación, no un secreto de autenticación.
    let token_source = format!("{dev}|{lab}|harmonic-usb-format");
    let mut h = Sha256::new();
    h.update(token_source.as_bytes());
    let token: String = format!("{:x}", h.finalize()).chars().take(16).collect();
    Ok(FormatPlan {
        os_name: os_key.to_string(),
        device: dev,
        command,
        shell,
        needs_admin: true,
        notes,
        confirm_token: token,
        label: lab,
    })
}

/// `true` solo cuando el token exacto aparece al menos dos veces en
/// `entered`.
pub fn check_double_confirmation(plan: &FormatPlan, entered: &[&str]) -> bool {
    entered.iter().filter(|e| **e == plan.confirm_token).count() >= 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::sha256_hex;

    #[test]
    fn token_determinista_y_ligado_a_etiqueta() {
        let a = prepare_format_command("/dev/sdb", Some("linux"), "L1", None).unwrap();
        let b = prepare_format_command("/dev/sdb", Some("linux"), "L1", None).unwrap();
        let c = prepare_format_command("/dev/sdb", Some("linux"), "OTHER", None).unwrap();
        assert_eq!(a.confirm_token, b.confirm_token);
        assert_ne!(a.confirm_token, c.confirm_token);
        // El token del .py: sha256("{dev}|{lab}|harmonic-usb-format")[:16]
        assert_eq!(
            a.confirm_token,
            sha256_hex(b"/dev/sdb|L1|harmonic-usb-format").chars().take(16).collect::<String>()
        );
    }

    #[test]
    fn doble_confirmacion_exige_dos_tokens_exactos() {
        let plan = prepare_format_command("/dev/sdb", Some("linux"), "HARMONIC", None).unwrap();
        let tok = plan.confirm_token.as_str();
        assert!(!check_double_confirmation(&plan, &[]));
        assert!(!check_double_confirmation(&plan, &[tok]));
        assert!(!check_double_confirmation(&plan, &["wrong", "wrong"]));
        let tok_up = plan.confirm_token.to_uppercase();
        assert!(!check_double_confirmation(&plan, &[&tok_up, tok]));
        assert!(check_double_confirmation(&plan, &[tok, tok]));
        assert!(check_double_confirmation(&plan, &["x", tok, tok]));
    }
}
