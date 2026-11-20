//! Destino de exportación directa a USB: resolución pura de la carpeta
//! destino (`<root>/SmartMix/`) y avisos FAT32 de nombres largos para
//! equipos DJ. La enumeración de unidades (hardware) vive en la app y se
//! mantiene delgada: aquí solo la lógica testeable sin dispositivo.

use std::path::{Path, PathBuf};

use crate::fat32::Fat32Warning;
use crate::strings::t;

/// Límite práctico de nombre de archivo para equipos DJ sobre FAT32:
/// más allá de esto algunos reproductores truncan o no listan el archivo.
pub const FAT32_DJ_NAME_LIMIT: usize = 31;

/// Carpeta canónica de exportación en una unidad: `<root>/SmartMix/`.
pub fn smartmix_dir(root: &Path) -> PathBuf {
    root.join("SmartMix")
}

/// Solo el componente final (defensivo: acepta nombres o rutas completas).
fn file_name_of(name: &str) -> String {
    Path::new(name)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string())
}

/// Avisos FAT32 de una exportación: nombres de archivo de más de
/// [`FAT32_DJ_NAME_LIMIT`] caracteres. Fuera de FAT32 no hay aviso (el
/// límite es del equipo DJ leyendo FAT32, no del formato en sí).
pub fn usb_export_warnings(is_fat32: bool, names: &[String]) -> Vec<Fat32Warning> {
    if !is_fat32 {
        return Vec::new();
    }
    names
        .iter()
        .map(|n| file_name_of(n))
        .filter(|n| n.chars().count() > FAT32_DJ_NAME_LIMIT)
        .map(|n| Fat32Warning {
            code: "name_gt_31",
            relpath: n.clone(),
            message: t("usb.fat32.name_gt_31", &[("path", &n)]),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_of_toma_el_componente_final() {
        assert_eq!(file_name_of("C:/music/tema.flac"), "tema.flac");
        assert_eq!(file_name_of("tema.flac"), "tema.flac");
        // separador nativo: en linux/macos '\\' no es separador de ruta
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(file_name_of(&format!("E:{sep}a{sep}b.mp3")), "b.mp3");
    }
}
