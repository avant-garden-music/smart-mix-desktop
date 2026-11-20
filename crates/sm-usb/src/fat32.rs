//! Validadores de compatibilidad FAT32 — solo advierten, NUNCA renombran
//! ni modifican. Port de `core/usb/fat32.py`.
//!
//! Tres familias de chequeos, todos devolviendo `Fat32Warning` para que el
//! CLI/GUI los muestre antes de que el usuario decida nada:
//!
//! 1. Archivos más grandes que el límite por archivo de FAT32 (4 GiB − 1)
//!    — sugiere recodificar a FLAC/ALAC o partir.
//! 2. Nombres que podrían no sobrevivir en FAT32: caracteres \ / : * ? " < > |,
//!    caracteres de control, emojis/símbolos o componentes > 255 caracteres.
//! 3. Archivos de audio sueltos en la raíz de la unidad — sugiere
//!    organizarlos en subcarpetas.

use crate::scanner::ScanResult;
use crate::strings::t;

/// Tamaño máximo de archivo en FAT32: 4 GiB menos un byte.
pub const FAT32_MAX_FILESIZE: u64 = 4 * 1024 * 1024 * 1024 - 1;

/// Caracteres prohibidos en nombres largos vfat/FAT32.
pub const INVALID_CHARS: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

pub const MAX_NAME_LEN: usize = 255;

/// Rangos de codepoints que se comportan mal entre herramientas FAT32 y
/// equipo de DJ.
const EMOJI_RANGES: &[(u32, u32)] = &[
    (0x1F000, 0x1FAFF), // bloques emoji (plano suplementario)
    (0x2600, 0x27BF),   // símbolos misc y dingbats
    (0x2B00, 0x2BFF),   // cola del bloque flechas/estrellas
    (0xFE00, 0xFE0F),   // selectores de variación
    (0x200D, 0x200D),   // zero-width joiner
];

fn is_emoji(ch: char) -> bool {
    let code = ch as u32;
    EMOJI_RANGES.iter().any(|(lo, hi)| *lo <= code && code <= *hi)
}

/// Aviso FAT32: código legible por máquina (`oversize` | `invalid_chars` |
/// `emoji` | `name_too_long` | `loose_track`), ruta relativa y mensaje ya
/// localizado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fat32Warning {
    pub code: &'static str,
    pub relpath: String,
    pub message: String,
}

/// Función pura sobre el manifiesto: devuelve avisos, no cambia nada.
pub fn validate_fat32(scan: &ScanResult) -> Vec<Fat32Warning> {
    let mut warnings = Vec::new();
    for entry in &scan.files {
        if entry.size > FAT32_MAX_FILESIZE {
            warnings.push(Fat32Warning {
                code: "oversize",
                relpath: entry.relpath.clone(),
                message: t(
                    "usb.fat32.oversize",
                    &[
                        ("path", &entry.relpath),
                        ("size", &entry.size.to_string()),
                        ("max", &FAT32_MAX_FILESIZE.to_string()),
                    ],
                ),
            });
        }
        for part in entry.relpath.split('/') {
            // Set ordenado y único de caracteres inválidos (orden de codepoint,
            // igual que sorted({...}) del .py).
            let mut bad: Vec<char> = part
                .chars()
                .filter(|ch| INVALID_CHARS.contains(ch) || (*ch as u32) < 0x20)
                .collect();
            bad.sort_unstable();
            bad.dedup();
            if !bad.is_empty() {
                let chars: String = bad.into_iter().collect();
                warnings.push(Fat32Warning {
                    code: "invalid_chars",
                    relpath: entry.relpath.clone(),
                    message: t(
                        "usb.fat32.invalid_chars",
                        &[("path", &entry.relpath), ("chars", &chars)],
                    ),
                });
                break;
            }
            if part.chars().any(is_emoji) {
                warnings.push(Fat32Warning {
                    code: "emoji",
                    relpath: entry.relpath.clone(),
                    message: t("usb.fat32.emoji", &[("path", &entry.relpath)]),
                });
                break;
            }
            if part.chars().count() > MAX_NAME_LEN {
                warnings.push(Fat32Warning {
                    code: "name_too_long",
                    relpath: entry.relpath.clone(),
                    message: t("usb.fat32.name_too_long", &[("path", &entry.relpath)]),
                });
                break;
            }
        }
    }
    for rel in &scan.loose_root_tracks {
        warnings.push(Fat32Warning {
            code: "loose_track",
            relpath: rel.clone(),
            message: t("usb.fat32.loose_track", &[("path", rel)]),
        });
    }
    warnings
}
