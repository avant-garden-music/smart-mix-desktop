//! Crates de Serato DJ (`_Serato_/Subcrates/*.crate`).
//!
//! Formato canónico del proyecto (documentado en
//! `core/playlists/readers_native.py`, validado por
//! `tests/test_playlists_native.py` y por el importador de Mixxx /
//! pyserato): secuencia TLV `[tag ASCII 4][longitud u32 BE][datos]`:
//!
//! ```text
//! vrsn │ 4+4 bytes │ "2.0" en UTF-16BE (cabecera de versión)
//! otrk │ por pista │ payload = chunk anidado ptrk
//! ptrk │ anidado   │ ruta de la pista en UTF-16BE (sin BOM)
//! ```
//!
//! Nota: `core/tagger/serato.py` escribe los *hot cues* GEOB "Serato
//! Markers2" DENTRO del archivo de audio (otro canal, manejado por el
//! tagger); las *crates* de playlist usan este formato TLV. No existe
//! ninguna estructura "nidb" en el proyecto: la forma canónica es exactamente
//! `vrsn` + un `otrk{ptrk}` por pista (sin serialización estilo `repr`).
//!
//! Las rutas se escriben absolutas con separador `/` (el dialecto que el
//! propio lector del proyecto consume sin transformar).

use std::fs;
use std::path::Path;

use anyhow::{bail, Result};

/// Escribe una crate `.crate` por entrada en
/// `{serato_root}/_Serato_/Subcrates/{nombre}.crate`.
///
/// `crates` = [(nombre de crate, rutas de pista en orden)]. El layout
/// `_Serato_/Subcrates/` es el que usa Serato en USB y el que recorre
/// `read_serato` del proyecto.
pub fn write_serato_crates(
    crates: &[(String, Vec<String>)],
    serato_root: impl AsRef<Path>,
) -> Result<()> {
    let subcrates = serato_root.as_ref().join("_Serato_").join("Subcrates");
    fs::create_dir_all(&subcrates)?;
    for (name, paths) in crates {
        let name = name.trim();
        if name.is_empty() || name.contains(['/', '\\', ':']) {
            bail!("nombre de crate inválido: {name:?}");
        }
        let mut out = Vec::new();
        out.extend_from_slice(&chunk(b"vrsn", &utf16_be("2.0")));
        for p in paths {
            let path = p.replace('\\', "/");
            out.extend_from_slice(&chunk(b"otrk", &chunk(b"ptrk", &utf16_be(&path))));
        }
        fs::write(subcrates.join(format!("{name}.crate")), out)?;
    }
    Ok(())
}

/// Campo TLV de crate: `[tag ASCII 4][len u32 BE][datos]`.
fn chunk(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(tag);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Texto → UTF-16BE sin BOM (la codificación de las crates).
fn utf16_be(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for unit in s.encode_utf16() {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out
}

/// Parser TLV mínimo para tests/verificación: devuelve los payloads `ptrk`
/// (rutas) de una crate en orden. `None` si no empieza con `vrsn`.
pub fn parse_crate_paths(data: &[u8]) -> Option<Vec<String>> {
    let mut paths = Vec::new();
    let mut rest = data;
    let mut first = true;
    while rest.len() >= 8 {
        let tag = &rest[..4];
        let len = u32::from_be_bytes([rest[4], rest[5], rest[6], rest[7]]) as usize;
        let payload = rest.get(8..8 + len)?;
        if first {
            if tag != b"vrsn" {
                return None;
            }
            first = false;
        } else if tag == b"otrk" {
            // otrk anida un ptrk (formato canónico del proyecto)
            if payload.len() >= 8 && &payload[..4] == b"ptrk" {
                let plen =
                    u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
                let pbytes = payload.get(8..8 + plen)?;
                paths.push(utf16_be_to_string(pbytes));
            }
        }
        rest = &rest[8 + len..];
    }
    Some(paths)
}

fn utf16_be_to_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_tlv_roundtrip() {
        let mut data = Vec::new();
        data.extend_from_slice(&chunk(b"vrsn", &utf16_be("2.0")));
        data.extend_from_slice(&chunk(
            b"otrk",
            &chunk(b"ptrk", &utf16_be("C:/Music/a.mp3")),
        ));
        assert_eq!(
            parse_crate_paths(&data),
            Some(vec!["C:/Music/a.mp3".to_string()])
        );
        // sin cabecera vrsn → None (crate inválida)
        assert_eq!(parse_crate_paths(&chunk(b"otrk", &chunk(b"ptrk", b""))), None);
    }
}
