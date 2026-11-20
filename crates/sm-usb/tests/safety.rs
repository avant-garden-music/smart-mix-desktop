//! Guardián de seguridad ≙ `explode_subprocess` del .py: la librería no
//! puede ejecutar comandos del SO. En Python se monkeypatchea subprocess;
//! en Rust el equivalente verificable es que NINGÚN archivo del crate
//! contenga vías de ejecución (`std::process`/`Command::new`), y que las
//! únicas cadenas de comando vivan en `formatter.rs` como preparadores.

use std::fs;
use std::path::PathBuf;

fn src_files() -> Vec<PathBuf> {
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
    assert!(!out.is_empty(), "no se encontraron fuentes");
    out
}

#[test]
fn library_never_executes_os_commands() {
    for f in src_files() {
        let src = fs::read_to_string(&f).expect("leer fuente");
        assert!(
            !src.contains("std::process"),
            "{} no debe ejecutar procesos",
            f.display()
        );
        assert!(
            !src.contains("Command::new"),
            "{} no debe construir comandos",
            f.display()
        );
    }
}

#[test]
fn format_commands_only_prepared_never_run() {
    // Las cadenas de comando solo aparecen en formatter.rs (preparador).
    for f in src_files() {
        let src = fs::read_to_string(&f).expect("leer fuente");
        let preparer = f.file_name().unwrap() == "formatter.rs";
        let has_cmd = src.contains("mkfs.vfat") || src.contains("Format-Volume")
            || src.contains("diskutil eraseDisk") || src.contains("format {letter}");
        if preparer {
            assert!(has_cmd, "formatter.rs debe preparar comandos");
        } else {
            assert!(!has_cmd, "{} no debe contener comandos de formato", f.display());
        }
    }
}
