//! Espejo de `test_strings_md_in_sync_with_code`: toda clave i18n usada
//! en `src/*.rs` debe estar documentada en `STRINGS.md` (y este a la par
//! con las plantillas de `strings.rs`), sin claves huérfanas.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn is_key(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_')
        && s.starts_with("usb.")
}

fn rust_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).expect("read_dir src").flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
                out.push(p);
            }
        }
    }
    out
}

fn code_keys() -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for f in rust_files() {
        let text = fs::read_to_string(&f).expect("leer .rs");
        let mut rest = text.as_str();
        while let Some(i) = rest.find("\"usb.") {
            let after = &rest[i + 1..];
            let end = after.find('"').expect("literal sin cerrar");
            let key = &after[..end];
            // El regex del .py (`usb\.[a-z0-9_.]+`) solo captura claves
            // válidas: aquí igual, literales de test/comentarios con otros
            // caracteres se ignoran.
            if is_key(key) {
                keys.insert(key.to_string());
            }
            rest = &after[end..];
        }
    }
    keys
}

fn md_keys() -> BTreeSet<String> {
    let text =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("STRINGS.md")).expect("leer STRINGS.md");
    let mut keys = BTreeSet::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("`usb.") {
        let after = &rest[i + 1..];
        let end = after.find('`').expect("backtick sin cerrar");
        let key = &after[..end];
        // igual que el regex del .py: `*` u otros caracteres no cuentan
        if is_key(key) {
            keys.insert(key.to_string());
        }
        rest = &after[end..];
    }
    keys
}

#[test]
fn strings_md_in_sync_with_code() {
    let code = code_keys();
    let md = md_keys();
    let map: BTreeSet<String> = sm_usb::strings::all_keys()
        .into_iter()
        .map(|k| k.to_string())
        .collect();

    assert!(!code.is_empty(), "no hay claves en código — extracción rota");
    assert!(code.iter().all(|k| is_key(k)), "clave sospechosa: {code:?}");

    let missing_in_md: Vec<_> = code.difference(&md).collect();
    assert!(
        missing_in_md.is_empty(),
        "faltan en STRINGS.md: {missing_in_md:?}"
    );
    let stale_in_md: Vec<_> = md.difference(&code).collect();
    assert!(
        stale_in_md.is_empty(),
        "huérfanas en STRINGS.md: {stale_in_md:?}"
    );
    let map_missing: Vec<_> = code.difference(&map).collect();
    assert!(
        map_missing.is_empty(),
        "claves sin plantilla en strings.rs: {map_missing:?}"
    );
    let map_stale: Vec<_> = map.difference(&code).collect();
    assert!(
        map_stale.is_empty(),
        "plantillas sin uso en strings.rs: {map_stale:?}"
    );
}
