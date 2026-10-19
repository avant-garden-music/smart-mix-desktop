//! Navegación USB (SOLO LECTURA): unidades extraíbles + playlists detectadas.
//!
//! Nunca escribe ni formatea dispositivos. Detección:
//!   Windows  → PowerShell Get-Volume (DriveType Removable)
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
}

/// Enumera unidades extraíbles con sus playlists (llamada bajo demanda:
/// lanza un PowerShell en Windows ~200 ms).
pub fn scan_drives() -> Vec<UsbDrive> {
    let mut out = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(out_ps) = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-Volume | Where-Object { $_.DriveType -eq 'Removable' -and $_.DriveLetter } | ForEach-Object { '{0}|{1}|{2}|{3}' -f $_.DriveLetter, $_.FileSystemLabel, $_.Size, $_.SizeRemaining }",
            ])
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
                let root = PathBuf::from(format!("{letter}:\\"));
                out.push(build_drive(letter, root, label, size, free));
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
                        out.push(build_drive(name.clone(), root, name, 0.0, 0.0));
                    }
                }
            }
        }
    }
    out
}

fn build_drive(
    letter_or_name: String,
    root: PathBuf,
    label: String,
    size: f64,
    free: f64,
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
        let d = build_drive("T".into(), tmp.clone(), "test".into(), 0.0, 0.0);
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
    fn scan_drives_no_panic() {
        // en CI no hay USB: debe devolver vacío sin pánico
        let _ = scan_drives();
    }
}
