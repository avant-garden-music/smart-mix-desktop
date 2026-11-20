//! Notación Camelot y compatibilidad armónica.
//!
//! La rueda de herramienta de referencia mapea cada mayor a su **menor paralela** (no a la
//! relativa de la Camelot estándar): `num = (8 + 7·(pc + 3·minor)) mod 12`
//! con 0→12 — [VERIF] contra la tabla `_key_table` del binario herramienta de referencia
//! (harmonic-engine docs/ref_detector_spec.md §mapa).

use serde::{Deserialize, Serialize};

/// Clave Camelot: (número 1..=12, letra 'A' menor / 'B' mayor).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CamelotKey {
    pub num: u8,
    pub minor: bool,
}

impl CamelotKey {
    pub fn letter(&self) -> char {
        if self.minor { 'A' } else { 'B' }
    }
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let bytes = s.as_bytes();
        if bytes.len() < 2 {
            return None;
        }
        let num: u8 = s[..bytes.len() - 1].parse().ok()?;
        let letter = s[bytes.len() - 1..].to_ascii_uppercase();
        let minor = match letter.as_str() {
            "A" => true,
            "B" => false,
            _ => return None,
        };
        if (1..=12).contains(&num) {
            Some(CamelotKey { num, minor })
        } else {
            None
        }
    }
    /// pc (0=C) y modalidad desde el código Camelot, usando el mapa de herramienta de referencia.
    pub fn pitch_class(&self) -> u8 {
        // inverso de num = (8 + 7·(pc + 3·minor)) mod 12 (0→12)
        let n = if self.num == 12 { 0 } else { self.num as i32 };
        let off = if self.minor { 3 } else { 0 };
        let v = (12 + n - 8 - 7 * off).rem_euclid(12);
        let pc = (v * 7).rem_euclid(12); // inverso modular de 7 mod 12 es 7 (7·7=49≡1)
        pc as u8
    }
    pub fn from_pitch_class(pc: u8, minor: bool) -> Self {
        let off = if minor { 3 } else { 0 };
        let num = (8 + 7 * (pc as i32 + off)) % 12;
        CamelotKey { num: if num == 0 { 12 } else { num as u8 }, minor }
    }
}

/// Código "8A" / "11B" desde (pitch class, menor).
pub fn camelot_code(pc: u8, minor: bool) -> String {
    let k = CamelotKey::from_pitch_class(pc, minor);
    format!("{}{}", k.num, k.letter())
}

const PITCH_NAMES: [&str; 12] =
    ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// Nombre legible ("A minor" para 8A, "C major" para 8B).
pub fn camelot_display_name(code: &str) -> String {
    match CamelotKey::parse(code) {
        Some(k) => {
            let pc = k.pitch_class();
            format!("{} {}", PITCH_NAMES[pc as usize], if k.minor { "minor" } else { "major" })
        }
        None => code.to_string(),
    }
}

/// Grado de compatibilidad armónica entre dos claves (0 = incompatible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarmonicRelation {
    Same,
    /// ±1 mismo lado de la rueda.
    Adjacent,
    /// Mismo número, lado opuesto (relativa herramienta de referencia).
    Relative,
}

impl HarmonicRelation {
    pub fn label(&self) -> &'static str {
        match self {
            HarmonicRelation::Same => "misma clave",
            HarmonicRelation::Adjacent => "±1",
            HarmonicRelation::Relative => "relativa",
        }
    }
}

pub fn relation(a: &str, b: &str) -> Option<HarmonicRelation> {
    let (a, b) = (CamelotKey::parse(a)?, CamelotKey::parse(b)?);
    if a.num == b.num && a.minor == b.minor {
        Some(HarmonicRelation::Same)
    } else if a.num == b.num && a.minor != b.minor {
        Some(HarmonicRelation::Relative)
    } else {
        let dn = ((a.num as i16 - b.num as i16).abs() % 12) as u8;
        if (dn == 1 || dn == 11) && a.minor == b.minor {
            Some(HarmonicRelation::Adjacent)
        } else {
            None
        }
    }
}

pub fn is_compatible(a: &str, b: &str) -> bool {
    relation(a, b).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camelot_roundtrip() {
        for pc in 0..12u8 {
            for minor in [false, true] {
                let code = camelot_code(pc, minor);
                let k = CamelotKey::parse(&code).unwrap();
                assert_eq!(k.pitch_class(), pc, "roundtrip {code}");
                assert_eq!(k.minor, minor);
            }
        }
    }

    #[test]
    fn known_pairs() {
        // C major → 8B, A minor → 8A (mapa herramienta de referencia: mayor↔menor paralela)
        assert_eq!(camelot_code(0, false), "8B");
        assert_eq!(camelot_code(9, true), "8A");
        assert_eq!(camelot_display_name("8B"), "C major");
        assert_eq!(camelot_display_name("8A"), "A minor");
        assert_eq!(camelot_code(7, false), "9B"); // G major
        assert_eq!(camelot_code(2, true), "7A"); // D minor
        assert_eq!(camelot_code(11, false), "1B"); // B major
    }

    #[test]
    fn compatibility() {
        assert!(is_compatible("8A", "8A"));
        assert!(is_compatible("8A", "8B"));
        assert!(is_compatible("8A", "7A"));
        assert!(is_compatible("8A", "9A"));
        assert!(!is_compatible("8A", "9B"));
        assert!(!is_compatible("8A", "10A"));
        assert_eq!(relation("8A", "8B"), Some(HarmonicRelation::Relative));
        assert_eq!(relation("8A", "9A"), Some(HarmonicRelation::Adjacent));
    }
}
