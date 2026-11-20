//! Renombrado de archivos estilo herramienta comercial de referencia (F-30).
//!
//! Port fiel de `core/tagger/rename.py` (harmonic-engine): sanitización de
//! tokens (caracteres ilegales de filesystem fuera, `\s+` → `_`, bordes
//! `._` fuera), colapso de runs de separadores `_-_`, colisiones con
//! sufijo `' (N)'` antes de la extensión y preservación de la extensión
//! original. Ejemplo real del corpus del usuario:
//! `Ab_Major_-_118_-_Gold_&_Silber_(Original_Mi.aiff`.
//!
//! Seguridad (F-30 llevaba años sin implementarse a propósito):
//!
//! * [`plan_renames`] y [`preview_renames`] NUNCA escriben en disco; solo
//!   [`apply_renames`] ejecuta, con `std::fs::rename` (atómico en el mismo
//!   volumen) y sin sobrescribir: destino existente → `skipped_conflict`.
//! * El texto literal del patrón también pasa por el filtro de caracteres
//!   ilegales y los runs `..` se colapsan a `.`: un `Custom("…")` no puede
//!   inyectar separadores de ruta ni `..` (path traversal imposible).
//! * Los no-ops (nombre final igual al actual, exacto o solo en
//!   mayúsculas) quedan fuera del plan — `rename.py` tampoco los ejecuta.
//! * Los orígenes del lote se reservan como nombres ocupados: un plan no
//!   puede mandar un archivo A sobre el nombre actual de un archivo B que
//!   aún no se renombra (escenarios de swap quedan con sufijo).
//!
//! Desviaciones deliberadas sobre el `.py` (endurecimiento, no relajación):
//! filtro de ilegales + colapso de `..` aplicados también al literal del
//! patrón, y BPM no finito tratado como ausente.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::values::camelot_to_musical;

/// Plantilla KeyBpmStyle: `Key_Mode_-_Tempo_-_Title_-_Artist`.
///
/// Equivalente al `DEFAULT_TEMPLATE` de rename.py (`{Key}_-_{BPM}_-_{Title}_-_{Artist}`
/// con `Key = 'F Major'` → `F_Major`): aquí la tonalidad entra partida en
/// `{key}` (tónica, p. ej. `F#`) y `{mode}` (`Major`/`Minor`).
const KeyBpmStyle: &str = "{key}_{mode}_-_{bpm}_-_{title}_-_{artist}";

/// Plantilla CamelotSimple: código Camelot en lugar de tonalidad musical.
const CamelotSimple: &str = "{camelot}_-_{bpm}_-_{title}_-_{artist}";

/// Metadatos que alimentan el renombrado (viene de la BD/análisis).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RenameInfo {
    pub title: String,
    pub artist: String,
    /// Código Camelot (`7A`); vacío o inválido → campos de key vacíos.
    pub key_camelot: String,
    /// `None` (o no finito) → `{bpm}` vacío; si no, entero redondeado
    /// (half-to-even, como `int(round(bpm))` de Python).
    pub bpm: Option<f64>,
}

/// Patrón de renombrado.
#[derive(Debug, Clone, PartialEq)]
pub enum RenamePattern {
    /// `Key_Mode_-_Tempo_-_Title_-_Artist` (default de rename.py).
    KeyBpmStyle,
    /// Igual que KeyBpmStyle con código Camelot (`7A`) en vez de `D_Minor`.
    CamelotSimple,
    /// Plantilla libre con placeholders `{key}`, `{mode}`, `{bpm}`,
    /// `{title}`, `{artist}` (también `{camelot}`; mayúsculas da igual,
    /// desconocidos → vacío, como `_MissingAsEmpty` en el .py).
    Custom(String),
}

impl RenamePattern {
    fn template(&self) -> &str {
        match self {
            RenamePattern::KeyBpmStyle => KeyBpmStyle,
            RenamePattern::CamelotSimple => CamelotSimple,
            RenamePattern::Custom(s) => s,
        }
    }
}

/// Un renombrado planificado (nunca se ejecuta hasta `apply_renames`).
#[derive(Debug, Clone, PartialEq)]
pub struct RenamePlan {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// Fallo individual durante `apply_renames` (el lote continúa).
#[derive(Debug, Clone, PartialEq)]
pub struct RenameFailure {
    pub from: PathBuf,
    pub to: PathBuf,
    pub error: String,
}

/// Resultado de aplicar un lote de renombrados.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RenameReport {
    pub applied: usize,
    /// Destino existente (o igual al origen): no se toca, jamás se pisa.
    pub skipped_conflict: usize,
    pub errors: Vec<RenameFailure>,
}

/// Planifica los renombrados de un lote SIN tocar el disco.
///
/// `dest_dir` cambia el directorio destino (por defecto, el mismo del
/// origen). Extensiones preservadas tal cual (`.MP3` sigue `.MP3`).
/// Colisiones (en disco o con otros nombres del lote) → sufijo `' (2)'`,
/// `' (3)'`… antes de la extensión, como `resolve_collision` en rename.py.
pub fn plan_renames(
    entries: &[(&Path, &RenameInfo)],
    pattern: &RenamePattern,
    dest_dir: Option<&Path>,
) -> Result<Vec<RenamePlan>> {
    if let Some(dir) = dest_dir {
        if !dir.is_dir() {
            bail!(
                "dest_dir no existe o no es un directorio: {}",
                dir.display()
            );
        }
    }
    let template = pattern.template();
    // Validar la plantilla aunque el lote venga vacío.
    render_filename(template, &[])?;

    // Nombres reservados: orígenes del lote + destinos ya planificados.
    let mut reserved: HashSet<String> = entries.iter().map(|(p, _)| normcase(p)).collect();

    let mut plan = Vec::with_capacity(entries.len());
    for (from, info) in entries {
        let file_name = from
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if file_name.is_empty() {
            continue;
        }
        let (stem, ext) = split_name(&file_name);
        let name = render_filename(template, &fields_for(info, stem))?;
        // No-op: nombre final vacío o igual al actual (exacto o
        // mayúsculas) — rename.py reporta renamed=False y no ejecuta.
        if name.is_empty() || name == stem || normcase(&name) == normcase(stem) {
            continue;
        }
        let dir: &Path = dest_dir.unwrap_or_else(|| from.parent().unwrap_or_else(|| Path::new("")));
        let nc_from = normcase(from);
        let mut target = dir.join(format!("{name}{ext}"));
        let mut n = 1usize;
        while reserved.contains(&normcase(&target))
            || (target.exists() && normcase(&target) != nc_from)
        {
            n += 1;
            if n > 9999 {
                bail!(
                    "colisiones de nombre sin fin planificando {}",
                    from.display()
                );
            }
            target = dir.join(format!("{name} ({n}){ext}"));
        }
        reserved.insert(normcase(&target));
        plan.push(RenamePlan {
            from: from.to_path_buf(),
            to: target,
        });
    }
    Ok(plan)
}

/// Preview para UI: `"origen -> destino"` por línea; error del plan como
/// única línea `error: …`. No escribe nada en disco.
pub fn preview_renames(
    entries: &[(&Path, &RenameInfo)],
    pattern: &RenamePattern,
    dest_dir: Option<&Path>,
) -> Vec<String> {
    match plan_renames(entries, pattern, dest_dir) {
        Ok(plan) => plan
            .iter()
            .map(|r| format!("{} -> {}", r.from.display(), r.to.display()))
            .collect(),
        Err(e) => vec![format!("error: {e}")],
    }
}

/// Ejecuta el plan con `std::fs::rename` (atómico en el mismo volumen).
///
/// NUNCA sobrescribe: destino existente → `skipped_conflict`. Origen
/// desaparecido → `errors` y el lote continúa. `progress` se llama con el
/// índice de cada elemento antes de procesarlo y con `plan.len()` al final.
///
/// Nota: el chequeo de existencia y el rename no son atómicos entre sí
/// (TOCTOU); el apply es secuencial y de un solo proceso, que es el uso
/// previsto (lote de la UI).
pub fn apply_renames(plan: &[RenamePlan], mut progress: impl FnMut(usize)) -> Result<RenameReport> {
    let mut report = RenameReport::default();
    for (i, r) in plan.iter().enumerate() {
        progress(i);
        if !r.from.is_file() {
            report.errors.push(RenameFailure {
                from: r.from.clone(),
                to: r.to.clone(),
                error: "el origen no existe (o no es un archivo)".to_string(),
            });
        } else if normcase(&r.from) == normcase(&r.to) || r.to.exists() {
            report.skipped_conflict += 1;
        } else {
            match std::fs::rename(&r.from, &r.to) {
                Ok(()) => report.applied += 1,
                Err(e) => report.errors.push(RenameFailure {
                    from: r.from.clone(),
                    to: r.to.clone(),
                    error: e.to_string(),
                }),
            }
        }
    }
    progress(plan.len());
    Ok(report)
}

// ------------------------------------------------------------- internals

/// `os.path.normcase` de Windows: minúsculas y `/` → `\`.
fn normcase(p: impl AsRef<Path>) -> String {
    p.as_ref().to_string_lossy().to_lowercase().replace('/', "\\")
}

/// `sanitize_token` de rename.py: ilegales fuera, `\s+` → `_`, bordes `._` fuera.
fn sanitize_token(value: &str) -> String {
    let filtered = strip_illegal(value);
    let mut out = String::with_capacity(filtered.len());
    let mut pending_ws = false;
    for c in filtered.trim().chars() {
        if c.is_whitespace() {
            pending_ws = true;
        } else {
            if pending_ws {
                out.push('_');
                pending_ws = false;
            }
            out.push(c);
        }
    }
    out.trim_matches(|c| c == '_' || c == '.').to_string()
}

/// `[\\/:*?\"<>|]` + controles `\x00-\x1f` fuera (el `_ILLEGAL` del .py).
fn strip_illegal(s: &str) -> String {
    s.chars()
        .filter(|&c| {
            !matches!(c, '\\' | '/' | ':' | '"' | '*' | '?' | '<' | '>' | '|') && (c as u32) > 0x1f
        })
        .collect()
}

/// ¿Empieza un `"_-_"` (guión bajo, guion, guion bajo) en `i`?
fn is_sep_run(b: &[char], i: usize) -> bool {
    i + 3 <= b.len() && b[i] == '_' && b[i + 1] == '-' && b[i + 2] == '_'
}

/// `re.sub(r"(?:_-_)+", "_-_", …)`: runs de 2+ unidades `_-_` colapsan a
/// una sola. (Una unidad suelta, o desalineada como en `K_-_-_T`, queda
/// tal cual — verificado contra el .py.)
fn collapse_sep_runs(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < b.len() {
        if is_sep_run(&b, i) {
            let mut k = 1usize;
            while is_sep_run(&b, i + 3 * k) {
                k += 1;
            }
            out.push_str("_-_");
            i += 3 * k;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// `int(round(x))` de Python: half-to-even en el .5 exacto.
fn round_half_even(x: f64) -> i64 {
    let t = x.trunc();
    if (x - t).abs() == 0.5 {
        let ti = t as i64;
        if ti % 2 == 0 {
            ti
        } else {
            ti + x.signum() as i64
        }
    } else {
        x.round() as i64
    }
}

/// `(stem, extensión_con_punto)`; sin punto o `.hidden` → extensión vacía
/// (semántica de `Path.suffix` en el .py).
fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(0) | None => (name, ""),
        Some(i) => (&name[..i], &name[i..]),
    }
}

/// Campos sanitizados para la plantilla. Título vacío → stem original
/// (mismo fallback que `normalize_analysis` en values.py).
fn fields_for(info: &RenameInfo, stem: &str) -> Vec<(&'static str, String)> {
    let title = if info.title.trim().is_empty() {
        sanitize_token(stem)
    } else {
        sanitize_token(&info.title)
    };
    let artist = sanitize_token(&info.artist);
    // 'D Minor' → key 'D', mode 'Minor' (split de la primera letra).
    let (key, mode) = match camelot_to_musical(&info.key_camelot) {
        Some(m) => match m.split_once(' ') {
            Some((k, rest)) => (sanitize_token(k), sanitize_token(rest)),
            None => (sanitize_token(m), String::new()),
        },
        None => (String::new(), String::new()),
    };
    let bpm = match info.bpm {
        Some(b) if b.is_finite() => round_half_even(b).to_string(),
        _ => String::new(),
    };
    vec![
        ("key", key),
        ("mode", mode),
        ("camelot", sanitize_token(&info.key_camelot)),
        ("bpm", bpm),
        ("title", title),
        ("artist", artist),
    ]
}

/// Plantilla + campos → nombre final SIN extensión (el `render_filename`
/// del .py: placeholders desconocidos → vacío, malformed → Err, colapso
/// de runs `_-_`, bordes `_-` fuera, puntos finales fuera), más el
/// endurecimiento anti-traversal (ilegales y `..` también en el literal).
fn render_filename(template: &str, fields: &[(&str, String)]) -> Result<String> {
    let mut out = String::with_capacity(template.len() + 24);
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    out.push('{');
                    continue;
                }
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => name.push(ch),
                        None => {
                            bail!("plantilla de renombrado inválida ({{ sin cerrar): {template:?}")
                        }
                    }
                }
                let key = name.trim().to_ascii_lowercase();
                if let Some((_, v)) = fields.iter().find(|(k, _)| *k == key) {
                    out.push_str(v);
                }
                // desconocido → vacío (como _MissingAsEmpty del .py)
            }
            '}' => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    out.push('}');
                } else {
                    bail!("plantilla de renombrado inválida (}} suelto): {template:?}");
                }
            }
            _ => out.push(c),
        }
    }
    let mut name = collapse_sep_runs(&strip_illegal(&out));
    name = name
        .trim_matches(|c| c == '_' || c == '-')
        .trim_end_matches('.')
        .to_string();
    // Path traversal imposible: sin separadores (ya filtrados) y sin `..`
    // (colapsados a `.`); `..` entero reduce a vacío → no-op en el plan.
    while name.contains("..") {
        name = name.replace("..", ".");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: &str) -> String {
        sanitize_token(v)
    }

    // ---- sanitize_token: espejo de los casos del .py (verificados en vivo)

    #[test]
    fn sanitize_espejo_py() {
        assert_eq!(f("Canción (Mix)"), "Canción_(Mix)"); // paréntesis legales
        assert_eq!(f("a/b\\c:d*e?f\"g<h>i|j\u{1}"), "abcdefghij");
        assert_eq!(f("  F  Major "), "F_Major"); // runs de \s+ → un _
        assert_eq!(f("..trailing..."), "trailing"); // bordes ._ fuera
        assert_eq!(f("_x_"), "x");
        assert_eq!(f("six million"), "six_million");
        assert_eq!(f("upper class"), "upper_class");
    }

    // ---- render_filename: espejo del DEFAULT_TEMPLATE del .py

    fn keybpm(key: &str, bpm: &str, title: &str, artist: &str) -> String {
        render_filename(
            "{Key}_-_{BPM}_-_{Title}_-_{Artist}",
            &[
                ("key", f(key)),
                ("bpm", f(bpm)),
                ("title", f(title)),
                ("artist", f(artist)),
            ],
        )
        .unwrap()
    }

    #[test]
    fn render_espejo_py() {
        // Ejemplo del docstring de rename.py.
        assert_eq!(
            keybpm("F_Major", "128", "Canción_(Mix)", "Autor"),
            "F_Major_-_128_-_Canción_(Mix)_-_Autor"
        );
        // Caso de aceptación (7A = D Minor en values.rs, verbatim values.py).
        assert_eq!(
            keybpm("D_Minor", "120", "six_million", "upper_class"),
            "D_Minor_-_120_-_six_million_-_upper_class"
        );
        // Campos vacíos: colapso + bordes.
        assert_eq!(keybpm("", "128", "T", "A"), "128_-_T_-_A");
        assert_eq!(keybpm("D_Minor", "120", "T", ""), "D_Minor_-_120_-_T");
        assert_eq!(keybpm("D_Minor", "120", "", ""), "D_Minor_-_120");
    }

    #[test]
    fn render_runs_desalineados_no_colapsan() {
        // El regex (?:_-_)+ solo colapsa unidades completas: K_-_-_T queda
        // igual (verificado contra el .py real).
        assert_eq!(
            render_filename(
                "{Key}_-_-_{Title}",
                &[("key", "K".into()), ("title", "T".into())]
            )
            .unwrap(),
            "K_-_-_T"
        );
        // Dos unidades adyacentes sí colapsan: _-__-_ → _-_.
        assert_eq!(
            render_filename("{A}_-__-_{B}", &[("a", String::new()), ("b", "128".into())]).unwrap(),
            "128"
        );
    }

    #[test]
    fn render_placeholders_desconocidos_vacios() {
        assert_eq!(render_filename("{X}{Y}", &[]).unwrap(), "");
        assert_eq!(render_filename("pre{nope}post", &[]).unwrap(), "prepost");
    }

    #[test]
    fn render_plantilla_malformada_error() {
        assert!(render_filename("{key", &[]).is_err());
        assert!(render_filename("a}b", &[]).is_err());
    }

    #[test]
    fn render_dots_y_traversal() {
        assert_eq!(
            render_filename("{Title}...", &[("title", "T".into())]).unwrap(),
            "T"
        );
        // El literal del patrón no puede inyectar rutas ni ..
        assert_eq!(
            render_filename(
                "{title}/../{artist}",
                &[
                    ("title", "six_million".into()),
                    ("artist", "upper_class".into())
                ]
            )
            .unwrap(),
            "six_million.upper_class"
        );
        assert_eq!(render_filename("..", &[]).unwrap(), "");
    }

    // ---- BPM: half-to-even como int(round()) de Python

    #[test]
    fn bpm_half_to_even() {
        assert_eq!(round_half_even(120.01), 120);
        assert_eq!(round_half_even(120.5), 120);
        assert_eq!(round_half_even(121.5), 122);
        assert_eq!(round_half_even(-121.5), -122);
        assert_eq!(round_half_even(0.4), 0);
    }

    #[test]
    fn campos_bpm_y_titulo_vacio() {
        let info = RenameInfo {
            title: String::new(),
            artist: "A".into(),
            key_camelot: "8B".into(),
            bpm: Some(f64::NAN),
        };
        let fields = fields_for(&info, "old name");
        let get = |k: &str| fields.iter().find(|(n, _)| *n == k).unwrap().1.clone();
        assert_eq!(get("title"), "old_name"); // stem como fallback
        assert_eq!(get("bpm"), ""); // NaN → ausente
        assert_eq!(get("key"), "C");
        assert_eq!(get("mode"), "Major");
        assert_eq!(get("camelot"), "8B");
    }
}
