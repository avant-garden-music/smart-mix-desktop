//! Tests F-30: renombrado de archivos estilo herramienta de referencia.
//!
//! Espejo de los casos de `core/tagger/rename.py` (sanitización, colisiones
//! con sufijo `' (N)'`, preservación de extensión, no-ops fuera del plan) +
//! seguridad: preview/plan nunca escriben, apply nunca sobrescribe, path
//! traversal imposible. Todo en temp dirs únicos bajo `std::env::temp_dir()`.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sm_export::{
    apply_renames, plan_renames, preview_renames, RenameInfo, RenamePattern, RenamePlan,
};

// ------------------------------------------------------------- utilidades

/// Temp dir único por test (pid + nanos) — nunca colisiona entre tests.
fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let d = std::env::temp_dir().join(format!(
        "sm-export-rename-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&d).unwrap();
    d
}

fn info(title: &str, artist: &str, key: &str, bpm: Option<f64>) -> RenameInfo {
    RenameInfo {
        title: title.to_string(),
        artist: artist.to_string(),
        key_camelot: key.to_string(),
        bpm,
    }
}

fn touch(p: &Path, bytes: &[u8]) {
    fs::write(p, bytes).unwrap();
}

/// Snapshot del directorio: (nombre, bytes, mtime) de cada archivo.
fn snapshot(d: &Path) -> Vec<(String, Vec<u8>, SystemTime)> {
    let mut v: Vec<_> = fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.path().is_file())
        .map(|e| {
            let p = e.path();
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                fs::read(&p).unwrap(),
                fs::metadata(&p).unwrap().modified().unwrap(),
            )
        })
        .collect();
    v.sort();
    v
}

// ----------------------------------------------------------------- plan

/// Caso de aceptación: 'six million.mp3' de 'upper class', 7A/120.01.
/// Nota: el brief decía 'G_Major', pero camelot_to_musical (values.rs,
/// verbatim de values.py) mapea 7A → 'D Minor' ('G Major' es 9B) —
/// values.rs es la ley (punto 3 del brief).
#[test]
fn ref_style_caso_aceptacion() {
    let d = temp_dir("keybpmstyle");
    let from = d.join("six million.mp3");
    touch(&from, b"x");

    let plan = plan_renames(
        &[(
            &from,
            &info("six million", "upper class", "7A", Some(120.01)),
        )],
        &RenamePattern::KeyBpmStyle,
        None,
    )
    .unwrap();

    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].from, from);
    assert_eq!(
        plan[0].to.file_name().unwrap(),
        "D_Minor_-_120_-_six_million_-_upper_class.mp3"
    );
    // Mismo directorio y extensión preservada.
    assert_eq!(plan[0].to.parent().unwrap(), d.as_path());
    assert!(plan[0].to.to_string_lossy().ends_with(".mp3"));
}

/// 8B → 'C Major' (punto 3 del brief) y BPM redondeado a entero.
#[test]
fn ref_style_8b_c_major() {
    let d = temp_dir("camelot8b");
    let from = d.join("track.aiff");
    touch(&from, b"x");

    let plan = plan_renames(
        &[(&from, &info("Song", "Art", "8B", Some(120.4)))],
        &RenamePattern::KeyBpmStyle,
        None,
    )
    .unwrap();

    assert_eq!(plan.len(), 1);
    assert_eq!(
        plan[0].to.file_name().unwrap(),
        "C_Major_-_120_-_Song_-_Art.aiff"
    );
}

/// Título vacío → file_stem original como título.
#[test]
fn titulo_vacio_usa_stem() {
    let d = temp_dir("stem");
    let from = d.join("old name.mp3");
    touch(&from, b"x");

    let plan = plan_renames(
        &[(&from, &info("", "someone", "8B", Some(128.0)))],
        &RenamePattern::KeyBpmStyle,
        None,
    )
    .unwrap();

    assert_eq!(plan.len(), 1);
    assert_eq!(
        plan[0].to.file_name().unwrap(),
        "C_Major_-_128_-_old_name_-_someone.mp3"
    );
}

/// CamelotSimple: código Camelot en vez de tonalidad musical.
#[test]
fn ref_simple_camelot() {
    let d = temp_dir("simple");
    let from = d.join("six million.mp3");
    touch(&from, b"x");

    let plan = plan_renames(
        &[(
            &from,
            &info("six million", "upper class", "7A", Some(120.01)),
        )],
        &RenamePattern::CamelotSimple,
        None,
    )
    .unwrap();

    assert_eq!(plan.len(), 1);
    assert_eq!(
        plan[0].to.file_name().unwrap(),
        "7A_-_120_-_six_million_-_upper_class.mp3"
    );
}

/// Custom con los placeholders {key},{mode},{bpm},{title},{artist}
/// (mayúsculas da igual, como {Key} del .py).
#[test]
fn custom_placeholders() {
    let d = temp_dir("custom");
    let from = d.join("t.mp3");
    touch(&from, b"x");

    let plan = plan_renames(
        &[(&from, &info("T", "A", "11A", Some(90.0)))],
        &RenamePattern::Custom("{mode}_{key}__{BPM}__[{Title}]__{artist}".into()),
        None,
    )
    .unwrap();

    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].to.file_name().unwrap(), "Minor_F#__90__[T]__A.mp3");
}

/// BPM half-to-even como int(round()) de Python (paridad con el .py).
#[test]
fn bpm_half_to_even_en_nombre() {
    let d = temp_dir("bpm");
    let a = d.join("a.mp3");
    let b = d.join("b.mp3");
    touch(&a, b"x");
    touch(&b, b"x");

    let plan = plan_renames(
        &[
            (&a, &info("A1", "X", "8B", Some(120.5))),
            (&b, &info("B2", "X", "8B", Some(121.5))),
        ],
        &RenamePattern::CamelotSimple,
        None,
    )
    .unwrap();

    assert!(plan[0].to.file_name().unwrap().to_string_lossy().starts_with("8B_-_120_-"));
    assert!(plan[1].to.file_name().unwrap().to_string_lossy().starts_with("8B_-_122_-"));
}

/// Extensión preservada tal cual, incluidas mayúsculas.
#[test]
fn extension_preservada() {
    let d = temp_dir("ext");
    let from = d.join("SONG.MP3");
    touch(&from, b"x");

    let plan = plan_renames(
        &[(&from, &info("T", "A", "8B", Some(120.0)))],
        &RenamePattern::CamelotSimple,
        None,
    )
    .unwrap();

    assert_eq!(plan.len(), 1);
    assert!(plan[0].to.file_name().unwrap().to_string_lossy().ends_with(".MP3"));
}

// ------------------------------------------------------------- colisiones

/// Dos (tres) archivos que producirían el mismo nombre: el 2º y 3º
/// reciben sufijo ' (2)' / ' (3)' — formato espejo de resolve_collision
/// del .py (espacio + paréntesis antes de la extensión).
#[test]
fn colision_duplicados_sufijo() {
    let d = temp_dir("dup");
    let f1 = d.join("one.mp3");
    let f2 = d.join("two.mp3");
    let f3 = d.join("three.mp3");
    for f in [&f1, &f2, &f3] {
        touch(f, b"x");
    }
    let i = info("Same", "Person", "8B", Some(120.0));

    let plan = plan_renames(
        &[(&f1, &i), (&f2, &i), (&f3, &i)],
        &RenamePattern::CamelotSimple,
        None,
    )
    .unwrap();

    assert_eq!(plan.len(), 3);
    assert_eq!(
        plan[0].to.file_name().unwrap(),
        "8B_-_120_-_Same_-_Person.mp3"
    );
    assert_eq!(
        plan[1].to.file_name().unwrap(),
        "8B_-_120_-_Same_-_Person (2).mp3"
    );
    assert_eq!(
        plan[2].to.file_name().unwrap(),
        "8B_-_120_-_Same_-_Person (3).mp3"
    );
}

/// El destino ya existe en disco (archivo ajeno al lote) → sufijo.
#[test]
fn colision_con_archivo_existente() {
    let d = temp_dir("disk");
    let from = d.join("src.mp3");
    touch(&from, b"src");
    touch(&d.join("8B_-_120_-_T_-_A.mp3"), b"ajeno");

    let plan = plan_renames(
        &[(&from, &info("T", "A", "8B", Some(120.0)))],
        &RenamePattern::CamelotSimple,
        None,
    )
    .unwrap();

    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].to.file_name().unwrap(), "8B_-_120_-_T_-_A (2).mp3");
}

/// El nombre final igual al actual (exacto o solo mayúsculas) queda fuera
/// del plan — rename.py tampoco ejecuta esos casos.
#[test]
fn no_ops_fuera_del_plan() {
    let d = temp_dir("noop");
    let a = d.join("8B_-_120_-_T_-_A.mp3"); // ya renombrado (exacto)
    let b = d.join("8b_-_120_-_t_-_a.mp3"); // solo difiere en mayúsculas
    touch(&a, b"x");
    touch(&b, b"x");
    let i = info("T", "A", "8B", Some(120.0));

    let plan_a = plan_renames(&[(&a, &i)], &RenamePattern::CamelotSimple, None).unwrap();
    assert!(plan_a.is_empty(), "no-op exacto: {plan_a:?}");

    let plan_b = plan_renames(&[(&b, &i)], &RenamePattern::CamelotSimple, None).unwrap();
    assert!(plan_b.is_empty(), "no-op solo mayúsculas: {plan_b:?}");
}

/// Dos archivos que compiten por el mismo nombre canónico cuando UNO ya
/// lo tiene: el ya-nombrado es no-op y el otro recibe sufijo (los orígenes
/// del lote cuentan como nombres ocupados).
#[test]
fn duplicado_con_ya_renombrado_sufijo() {
    let d = temp_dir("dupmix");
    let bien = d.join("8B_-_120_-_Same_-_Person.mp3");
    let crudo = d.join("raw.mp3");
    touch(&bien, b"x");
    touch(&crudo, b"x");
    let i = info("Same", "Person", "8B", Some(120.0));

    let plan = plan_renames(&[(&bien, &i), (&crudo, &i)], &RenamePattern::CamelotSimple, None).unwrap();
    assert_eq!(plan.len(), 1, "el ya-nombrado queda fuera: {plan:?}");
    assert_eq!(plan[0].from, crudo);
    assert_eq!(
        plan[0].to.file_name().unwrap(),
        "8B_-_120_-_Same_-_Person (2).mp3"
    );
}

// -------------------------------------------------------------- seguridad

/// plan + preview NO tocan el disco: mismo snapshot antes y después.
#[test]
fn preview_y_plan_no_tocan_disco() {
    let d = temp_dir("preview");
    let a = d.join("one.mp3");
    let b = d.join("two.mp3");
    touch(&a, b"payload-a");
    touch(&b, b"payload-b");
    let i = info("Same", "Person", "8B", Some(120.0));

    let before = snapshot(&d);
    let lines = preview_renames(&[(&a, &i), (&b, &i)], &RenamePattern::CamelotSimple, None);
    let plan = plan_renames(&[(&a, &i), (&b, &i)], &RenamePattern::CamelotSimple, None).unwrap();
    let after = snapshot(&d);

    assert_eq!(before, after);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains(" -> "));
    assert!(lines[0].ends_with("8B_-_120_-_Same_-_Person.mp3"));
    assert!(lines[1].contains("8B_-_120_-_Same_-_Person (2).mp3"));
    assert_eq!(plan.len(), 2);
}

/// El patrón no puede inyectar '..' ni separadores: path traversal
/// imposible (destinos siempre en el mismo directorio del origen).
#[test]
fn path_traversal_imposible() {
    let d = temp_dir("traversal");
    let from = d.join("six million.mp3");
    touch(&from, b"x");

    for pat in [
        RenamePattern::Custom("{title}/../{artist}".into()),
        RenamePattern::Custom("..".into()),
        RenamePattern::Custom("..\\..\\{artist}".into()),
        RenamePattern::Custom("../../{title}".into()),
    ] {
        let plan = plan_renames(
            &[(
                &from,
                &info("six million", "upper class", "8B", Some(120.0)),
            )],
            &pat,
            None,
        )
        .unwrap_or_default();
        for r in &plan {
            assert_eq!(r.to.parent().unwrap(), d.as_path(), "con {pat:?}");
            assert!(
                !r.to.components().any(|c| c.as_os_str() == ".."),
                "componente .. con {pat:?}: {}",
                r.to.display()
            );
            assert!(!r.to.to_string_lossy().contains(".."), "con {pat:?}");
        }
        // '..' entero no planifica nada.
        if pat == RenamePattern::Custom("..".into()) {
            assert!(plan.is_empty());
        }
    }

    // El caso con slash queda sanitizado: separadores e incluso '..'
    // colapsados (ver test de render en el módulo).
    let plan = plan_renames(
        &[(
            &from,
            &info("six million", "upper class", "8B", Some(120.0)),
        )],
        &RenamePattern::Custom("{title}/../{artist}".into()),
        None,
    )
    .unwrap();
    assert_eq!(
        plan[0].to.file_name().unwrap(),
        "six_million.upper_class.mp3"
    );
}

/// Plantilla malformada → Err (equivalente a BadTemplateError).
#[test]
fn plantilla_malformada_error() {
    let d = temp_dir("badtpl");
    let from = d.join("t.mp3");
    touch(&from, b"x");

    assert!(plan_renames(
        &[(&from, &info("T", "A", "8B", None))],
        &RenamePattern::Custom("{key".into()),
        None
    )
    .is_err());
    assert!(plan_renames(
        &[(&from, &info("T", "A", "8B", None))],
        &RenamePattern::Custom("a}b".into()),
        None
    )
    .is_err());
    // Y preview lo reporta sin entrar en pánico.
    let lines = preview_renames(
        &[(&from, &info("T", "A", "8B", None))],
        &RenamePattern::Custom("{key".into()),
        None,
    );
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("error: "));
}

/// dest_dir mueve el destino (mismo volumen, fs::rename); inválido → Err.
#[test]
fn dest_dir_opcional() {
    let d = temp_dir("srcdir");
    let out = temp_dir("outdir");
    let from = d.join("t.mp3");
    touch(&from, b"x");

    let plan = plan_renames(
        &[(&from, &info("T", "A", "8B", Some(120.0)))],
        &RenamePattern::CamelotSimple,
        Some(&out),
    )
    .unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].to.parent().unwrap(), out.as_path());

    assert!(plan_renames(
        &[(&from, &info("T", "A", "8B", Some(120.0)))],
        &RenamePattern::CamelotSimple,
        Some(&d.join("no_existe"))
    )
    .is_err());
}

// ----------------------------------------------------------------- apply

/// Apply real en un árbol temporal: renombra, preserva bytes, reporta.
#[test]
fn apply_real_preserva_bytes() {
    let d = temp_dir("apply");
    let a = d.join("one.mp3");
    let b = d.join("two.mp3");
    touch(&a, b"payload-a");
    touch(&b, b"payload-b");
    let i = info("Same", "Person", "8B", Some(120.0));

    let plan = plan_renames(&[(&a, &i), (&b, &i)], &RenamePattern::CamelotSimple, None).unwrap();
    assert_eq!(plan.len(), 2);

    let calls = RefCell::new(0usize);
    let report = apply_renames(&plan, |_| *calls.borrow_mut() += 1).unwrap();

    assert_eq!(report.applied, 2);
    assert_eq!(report.skipped_conflict, 0);
    assert!(report.errors.is_empty());
    // progress: una por elemento + cierre.
    assert_eq!(*calls.borrow(), plan.len() + 1);

    assert!(!a.exists() && !b.exists(), "los orígenes desaparecen");
    assert_eq!(
        fs::read(d.join("8B_-_120_-_Same_-_Person.mp3")).unwrap(),
        b"payload-a"
    );
    assert_eq!(
        fs::read(d.join("8B_-_120_-_Same_-_Person (2).mp3")).unwrap(),
        b"payload-b"
    );
}

/// Apply NUNCA sobrescribe: destino existente → skipped_conflict y el
/// archivo ajeno queda intacto.
#[test]
fn apply_nunca_sobrescribe() {
    let d = temp_dir("nooverwrite");
    let from = d.join("src.mp3");
    touch(&from, b"src");
    let victim = d.join("8B_-_120_-_T_-_A.mp3");
    touch(&victim, b"contenido-ajeno");

    let plan = plan_renames(
        &[(&from, &info("T", "A", "8B", Some(120.0)))],
        &RenamePattern::CamelotSimple,
        None,
    )
    .unwrap();
    // La planificación detectó el archivo ajeno y sufijó…
    assert_eq!(plan[0].to.file_name().unwrap(), "8B_-_120_-_T_-_A (2).mp3");

    // …pero si un plan manual (o una carrera) apunta al existente, apply
    // se niega:
    let hostile = [RenamePlan {
        from: from.clone(),
        to: victim.clone(),
    }];
    let report = apply_renames(&hostile, |_| {}).unwrap();
    assert_eq!(report.applied, 0);
    assert_eq!(report.skipped_conflict, 1);
    assert!(report.errors.is_empty());
    assert_eq!(
        fs::read(&victim).unwrap(),
        b"contenido-ajeno",
        "el destino queda intacto"
    );
    assert!(from.exists(), "el origen no se pierde");
}
/// Origen desaparecido → error individual, el lote continúa.
#[test]
fn apply_origen_inexistente_error() {
    let d = temp_dir("missing");
    let from = d.join("one.mp3");
    touch(&from, b"x");
    let ghost = d.join("ghost.mp3");

    let i = info("Same", "P", "8B", Some(120.0));
    let plan = plan_renames(
        &[(&from, &i), (&ghost, &i)],
        &RenamePattern::CamelotSimple,
        None,
    )
    .unwrap();
    assert_eq!(plan.len(), 2); // el plan no exige existencia

    let report = apply_renames(&plan, |_| {}).unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].from, ghost);
}
