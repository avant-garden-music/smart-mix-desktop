//! Navegación USB (SOLO LECTURA): unidades extraíbles + playlists detectadas.
//!
//! Nunca escribe ni formatea dispositivos. Detección:
//!   Windows  → UNA invocación de PowerShell (Get-Partition → Get-Volume +
//!             Get-Disk): letra|label|size|free|fs|driveType|busType. Se
//!             aceptan bus USB y Removable con media — muchos USB modernos
//!             (SanDisk/SSD) se reportan como `Fixed` en Windows y un filtro
//!             por DriveType Removable los ocultaba (fix usb-scan).
//!   macOS    → /Volumes/*
//!   Linux    → /media/$USER/* y /run/media/$USER/*
//! Playlists: crates de Serato (`_Serato_/Subcrates/*.crate`, parseadas con
//! el lector TLV de sm-export) y presencia de exportaciones rekordbox
//! (`PIONEER/`) / Engine DJ (`EngineLibrary/`).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct UsbDrive {
    pub letter_or_name: String,
    pub root: PathBuf,
    pub label: String,
    pub size_gb: f64,
    pub free_gb: f64,
    pub serato_crates: Vec<(String, Vec<String>)>, // (nombre, rutas)
    pub has_pioneer: bool,
    pub has_engine: bool,
    /// Sistema de archivos del volumen ("FAT32", "exFAT", "NTFS"...;
    /// vacío si la plataforma no lo reporta).
    pub filesystem: String,
}

impl UsbDrive {
    /// true si la unidad está formateada en FAT32 (case-insensitive).
    pub fn is_fat32(&self) -> bool {
        self.filesystem.eq_ignore_ascii_case("FAT32")
    }
}

/// Criterio puro de unidad exportable (fix usb-scan). Acepta una unidad si:
/// - su BUS es USB (ground truth del hardware: la SanDisk 256GB "TYPO" se
///   reporta como `Fixed` en Windows y el filtro viejo por DriveType
///   `Removable` la ocultaba), o
/// - su DriveType es `Removable` (fallback si Get-Disk no reportó bus),
/// y además tiene tamaño > 0 (descarta lectores de tarjetas SIN media,
/// que aparecen como Removable de 0 bytes).
pub fn is_exportable_drive(drive_type: &str, bus_type: &str, size_bytes: f64) -> bool {
    let usb_bus = bus_type.eq_ignore_ascii_case("usb");
    let removable = drive_type.eq_ignore_ascii_case("removable");
    (usb_bus || removable) && size_bytes > 0.0
}

/// Enumera unidades USB exportables con sus playlists. Windows: UNA
/// invocación de PowerShell (~1.5 s medido) — llamar SIEMPRE desde un hilo
/// de fondo (`App::spawn_device_scan`); en el hilo UI congelaba la app.
pub fn scan_drives() -> Vec<UsbDrive> {
    let t0 = std::time::Instant::now();
    let mut out = Vec::new();
    #[cfg(target_os = "windows")]
    {
        // letter|label|size|free|fs|driveType|busType para TODOS los volúmenes
        // con letra (Get-Partition→Get-Disk da el bus real; no requiere admin).
        const PS_SCAN: &str = "Get-Partition | Where-Object DriveLetter | ForEach-Object { $v = Get-Volume -DriveLetter $_.DriveLetter -ErrorAction SilentlyContinue; if ($v) { $d = Get-Disk -Number $_.DiskNumber -ErrorAction SilentlyContinue; '{0}|{1}|{2}|{3}|{4}|{5}|{6}' -f $v.DriveLetter, $v.FileSystemLabel, $v.Size, $v.SizeRemaining, $v.FileSystem, $v.DriveType, $d.BusType } }";
        if let Ok(out_ps) = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", PS_SCAN])
            .output()
        {
            let txt = String::from_utf8_lossy(&out_ps.stdout);
            for line in txt.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() < 4 {
                    continue;
                }
                let letter = parts[0].trim().to_string();
                let label = parts[1].trim().to_string();
                let size: f64 = parts[2].trim().parse().unwrap_or(0.0);
                let free: f64 = parts[3].trim().parse().unwrap_or(0.0);
                if letter.is_empty() {
                    continue;
                }
                let fs = parts
                    .get(4)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let drive_type = parts
                    .get(5)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let bus_type = parts
                    .get(6)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let take = is_exportable_drive(&drive_type, &bus_type, size);
                eprintln!(
                    "[usb-scan] {letter}: type={drive_type} bus={bus_type} fs={fs} size={size:.0}B label={label:?} -> {}",
                    if take { "ACEPTADA" } else { "descartada" }
                );
                if take {
                    let root = PathBuf::from(format!("{letter}:\\"));
                    out.push(build_drive(letter, root, label, size, free, fs));
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let user = std::env::var("USER").unwrap_or_default();
        for base in [
            "/Volumes",
            &format!("/media/{user}"),
            &format!("/run/media/{user}"),
        ] {
            if let Ok(rd) = std::fs::read_dir(base) {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        let name = e
                            .file_name()
                            .to_string_lossy()
                            .trim_start_matches('/')
                            .to_string();
                        let root = e.path();
                        out.push(build_drive(
                            name.clone(),
                            root,
                            name,
                            0.0,
                            0.0,
                            String::new(),
                        ));
                    }
                }
            }
        }
    }
    eprintln!(
        "[usb-scan] {} unidad(es) aceptadas en {:?}",
        out.len(),
        t0.elapsed()
    );
    out
}

fn build_drive(
    letter_or_name: String,
    root: PathBuf,
    label: String,
    size: f64,
    free: f64,
    filesystem: String,
) -> UsbDrive {
    let serato_crates = read_serato_crates(&root);
    let has_pioneer = root.join("PIONEER").is_dir();
    let has_engine = root.join("EngineLibrary").is_dir();
    UsbDrive {
        letter_or_name,
        root,
        label,
        size_gb: size / 1e9,
        free_gb: free / 1e9,
        serato_crates,
        has_pioneer,
        has_engine,
        filesystem,
    }
}

/// Lee los crates de Serato del dispositivo (TLV del proyecto).
pub fn read_serato_crates(root: &Path) -> Vec<(String, Vec<String>)> {
    let sub = root.join("_Serato_").join("Subcrates");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&sub) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("crate") {
                continue;
            }
            let name = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if let Ok(data) = std::fs::read(&p) {
                if let Some(paths) = sm_export::serato::parse_crate_paths(&data) {
                    out.push((name, paths));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crates_vacio_si_no_hay_serato() {
        let tmp = std::env::temp_dir().join(format!("sm_usb_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let d = build_drive(
            "T".into(),
            tmp.clone(),
            "test".into(),
            0.0,
            0.0,
            String::new(),
        );
        assert!(d.serato_crates.is_empty());
        assert!(!d.has_pioneer);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn detecta_crate_serato() {
        let tmp = std::env::temp_dir().join(format!("sm_usb_serato_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let entries = vec!["C:/music/a.mp3".to_string(), "C:/music/b.mp3".to_string()];
        sm_export::write_serato_crates(&[("Test".into(), entries.clone())], &tmp).unwrap();
        // el lector USB debe encontrar el crate generado por el exportador
        let found = read_serato_crates(&tmp);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "Test");
        assert_eq!(found[0].1, entries);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn fat32_deteccion_case_insensitive() {
        let tmp = std::env::temp_dir();
        let d = build_drive("T".into(), tmp, "x".into(), 0.0, 0.0, "Fat32".into());
        assert!(d.is_fat32());
        let mut d2 = d.clone();
        d2.filesystem = "exFAT".into();
        assert!(!d2.is_fat32());
        d2.filesystem = String::new();
        assert!(!d2.is_fat32());
    }

    #[test]
    fn scan_drives_no_panic() {
        // en CI no hay USB: debe devolver vacío sin pánico
        let _ = scan_drives();
    }

    #[test]
    fn sandisk_fixed_usb_es_exportable() {
        // caso real medido (fix usb-scan): G: 'TYPO' 256GB SanDisk se reporta
        // como DriveType=Fixed con BusType=USB — el filtro viejo la ocultaba.
        assert!(is_exportable_drive("Fixed", "USB", 250_060_177_408.0));
        assert!(is_exportable_drive("fixed", "usb", 64e9)); // case-insensitive
    }

    #[test]
    fn discos_internos_se_rechazan() {
        // C:/D: reales de esta máquina: Fixed + SATA
        assert!(!is_exportable_drive("Fixed", "SATA", 2e12));
        assert!(!is_exportable_drive("Fixed", "NVMe", 1e12));
        // sin info de bus, un Fixed no es exportable per se
        assert!(!is_exportable_drive("Fixed", "", 500e9));
    }

    #[test]
    fn lectores_de_tarjetas_sin_media_se_rechazan() {
        // E:/F: reales: Removable + USB pero Size=0 (sin tarjeta insertada)
        assert!(!is_exportable_drive("Removable", "USB", 0.0));
    }

    #[test]
    fn removable_con_media_sin_info_bus_se_acepta() {
        // fallback si Get-Disk no reporta bus: pendrive clásico con datos
        assert!(is_exportable_drive("Removable", "", 32e9));
    }

    #[test]
    fn scan_drives_diag_log() {
        // evidencia del fix (sin asserts: en CI no hay USB). Correr con
        // --nocapture para ver el listado [usb-scan] de ESTA máquina.
        let drives = scan_drives();
        for d in &drives {
            eprintln!(
                "[usb-scan] ok {} label={:?} fs={} libre={:.1}/{:.1}GB serato={} pioneer={} engine={}",
                d.letter_or_name,
                d.label,
                d.filesystem,
                d.free_gb,
                d.size_gb,
                d.serato_crates.len(),
                d.has_pioneer,
                d.has_engine
            );
        }
    }
}
