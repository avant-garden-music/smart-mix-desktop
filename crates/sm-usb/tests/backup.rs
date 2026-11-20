//! Espejo de `TestBackupRestore` (tests/test_usb.py): roundtrips carpeta y
//! zip con verificación SHA-256, detección de corrupción, manifiesto
//! autodescriptivo y detección de hash mismatch en destino.

mod common;

use std::fs;

use common::{assert_trees_identical, fake_usb, rel_to_path, tmpdir, Rng};
use sm_usb::{
    backup_drive, backup_folder, backup_zip, restore_backup, scan_drive, verify_backup,
    BackupMode, FileEntry,
};

#[test]
fn roundtrip_folder_mode() {
    let tmp = tmpdir("backup_folder");
    let usb = fake_usb(&tmp);
    let scan = scan_drive(&usb, None).expect("scan");
    let dest = tmp.join("backup");
    let backup = backup_folder(&scan, &dest, None);
    assert_eq!(backup.mode, BackupMode::Folder);
    assert!(backup.ok(), "warnings: {:?}", backup.warnings);
    assert!(verify_backup(&backup, None).is_empty());

    let formatted = tmp.join("formatted_usb");
    let result = restore_backup(&backup, &formatted, None);
    assert_eq!(result.files_restored, scan.files.len());
    assert!(result.hash_failures.is_empty());
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.ok());
    assert_trees_identical(&usb, &formatted, &scan.relpaths());
}

#[test]
fn roundtrip_zip_mode() {
    let tmp = tmpdir("backup_zip");
    let usb = fake_usb(&tmp);
    let scan = scan_drive(&usb, None).expect("scan");
    let dest = tmp.join("backup.zip");
    // modo por defecto por extensión, como el .py
    let backup = backup_drive(&scan, &dest, None, None);
    assert_eq!(backup.mode, BackupMode::Zip);
    assert!(dest.is_file());
    let zf = fs::File::open(&dest).unwrap();
    assert!(zip::ZipArchive::new(zf).is_ok(), "no es un zip válido");
    assert!(backup.ok(), "warnings: {:?}", backup.warnings);
    assert!(verify_backup(&backup, None).is_empty());

    let formatted = tmp.join("formatted_usb");
    let result = restore_backup(&backup, &formatted, None);
    assert!(result.hash_failures.is_empty());
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_trees_identical(&usb, &formatted, &scan.relpaths());
}

/// Caso pedido por el port: árbol temporal con archivos de bytes
/// aleatorios (incluido un archivo vacío) → backup verificado → restore
/// verificado.
#[test]
fn roundtrip_folder_con_bytes_aleatorios() {
    let tmp = tmpdir("backup_random");
    let usb = tmp.join("USB");
    let mut rng = Rng::new(0x5EED_1234_ABCD_0001);
    let mut expected = std::collections::BTreeMap::new();
    for (dir, i) in [("a", 0u8), ("b", 1), ("b/c", 2), ("PIONEER", 3), ("", 4)] {
        for j in 0..3 {
            let size = (usize::from(i * 7 + j) * 977) % 65_536; // incluye tamaños variados
            let data = rng.bytes(size);
            let name = if dir.is_empty() {
                format!("root{j}.bin")
            } else {
                format!("{dir}/file{j}.bin")
            };
            let path = rel_to_path(&usb, &name);
            common::make_file(&path, &data);
            expected.insert(name, data);
        }
    }
    // archivo vacío: hash del contenido vacío, casos límite de chunking
    common::make_file(&rel_to_path(&usb, "empty.mp3"), b"");

    let scan = scan_drive(&usb, None).expect("scan");
    assert_eq!(scan.files.len(), expected.len() + 1);
    let backup = backup_folder(&scan, &tmp.join("backup"), None);
    assert!(backup.ok(), "warnings: {:?}", backup.warnings);
    assert!(verify_backup(&backup, None).is_empty());

    let out = tmp.join("out");
    let result = restore_backup(&backup, &out, None);
    assert_eq!(result.files_restored, scan.files.len());
    assert!(result.ok(), "errors: {:?}", result.errors);
    for (rel, data) in &expected {
        let restored = fs::read(rel_to_path(&out, rel)).expect("archivo restaurado");
        assert_eq!(&restored, data, "contenido difiere: {rel}");
    }
    assert_eq!(fs::read(rel_to_path(&out, "empty.mp3")).unwrap(), b"");
}

#[test]
fn backup_detects_corrupted_copy() {
    let tmp = tmpdir("backup_corrupt");
    let usb = fake_usb(&tmp);
    let scan = scan_drive(&usb, None).expect("scan");
    let backup = backup_folder(&scan, &tmp.join("backup"), None);
    assert!(backup.ok());
    fs::write(
        rel_to_path(&tmp.join("backup"), "Music/Techno/Track A.MP3"),
        b"corrupted",
    )
    .unwrap();
    let mismatches = verify_backup(&backup, None);
    assert_eq!(mismatches.len(), 1);
    assert!(mismatches[0].contains("Music/Techno/Track A.MP3"));
}

#[test]
fn backup_falla_cuando_el_hash_en_destino_no_coincide() {
    // Re-hash en destino: manifiesto con sha256 equivocado → la copia se
    // hace pero la verificación en destino marca failed → ok=false.
    let tmp = tmpdir("backup_mismatch");
    let usb = fake_usb(&tmp);
    let mut scan = scan_drive(&usb, None).expect("scan");
    scan.files[0].sha256 = "f".repeat(64); // hash imposible
    let relpath = scan.files[0].relpath.clone();

    let backup = backup_folder(&scan, &tmp.join("backup"), None);
    assert!(!backup.failed.is_empty(), "failed debía ser no vacío");
    assert!(backup.failed.contains(&relpath));
    assert!(!backup.ok());

    // ídem en modo zip
    let backup_zip = backup_zip(&scan, &tmp.join("b2.zip"), None);
    assert!(!backup_zip.failed.is_empty());
    assert!(!backup_zip.ok());
}

#[test]
fn restore_reports_hash_failure() {
    let tmp = tmpdir("restore_hashfail");
    let usb = fake_usb(&tmp);
    let scan = scan_drive(&usb, None).expect("scan");
    let backup = backup_folder(&scan, &tmp.join("backup"), None);
    // alterar el backup y saltarse la verificación para forzar un restore malo
    fs::write(
        rel_to_path(&tmp.join("backup"), "Music/Techno/Track A.MP3"),
        b"tampered",
    )
    .unwrap();
    let result = restore_backup(&backup, &tmp.join("out"), None);
    assert!(result
        .hash_failures
        .contains(&"Music/Techno/Track A.MP3".to_string()));
    assert!(!result.ok());
}

#[test]
fn manifest_roundtrip_autodescriptivo() {
    let tmp = tmpdir("manifest_roundtrip");
    let usb = fake_usb(&tmp);
    let scan = scan_drive(&usb, None).expect("scan");

    // carpeta: _manifest.json en la raíz del backup
    let dest = tmp.join("backup");
    backup_folder(&scan, &dest, None);
    let raw = fs::read_to_string(dest.join("_manifest.json")).unwrap();
    let parsed: Vec<FileEntry> = serde_json::from_str(&raw).expect("manifiesto parseable");
    assert_eq!(parsed, scan.files);
    // autodescriptivo: objetos con exactamente relpath/size/sha256 (el
    // orden en el TEXTO sigue la declaración, como el json.dumps del .py;
    // serde_json::Value itera ordenado, así que el set se compara aparte)
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    for obj in v.as_array().unwrap() {
        let mut keys: Vec<&str> = obj
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["relpath", "sha256", "size"]);
    }
    let (i_rel, i_size, i_sha) = (
        raw.find("\"relpath\"").unwrap(),
        raw.find("\"size\"").unwrap(),
        raw.find("\"sha256\"").unwrap(),
    );
    assert!(i_rel < i_size && i_size < i_sha, "orden de campos del manifiesto");
    assert!(raw.contains("\"relpath\""));

    // zip: mismo manifiesto como miembro del archivo
    let zdest = tmp.join("backup.zip");
    backup_zip(&scan, &zdest, None);
    let f = fs::File::open(&zdest).unwrap();
    let mut zf = zip::ZipArchive::new(f).unwrap();
    let mut member = zf.by_name("_manifest.json").expect("manifiesto en zip");
    let mut zraw = String::new();
    std::io::Read::read_to_string(&mut member, &mut zraw).unwrap();
    let zparsed: Vec<FileEntry> = serde_json::from_str(&zraw).expect("manifiesto zip parseable");
    assert_eq!(zparsed, scan.files);
}

#[test]
fn backup_progress_callback() {
    let tmp = tmpdir("backup_progress");
    let usb = fake_usb(&tmp);
    let scan = scan_drive(&usb, None).expect("scan");
    let calls: std::cell::RefCell<Vec<f64>> = std::cell::RefCell::new(Vec::new());
    backup_folder(
        &scan,
        &tmp.join("backup"),
        Some(&mut |_msg: &str, frac: f64| {
            calls.borrow_mut().push(frac);
        }),
    );
    let calls = calls.into_inner();
    assert!(!calls.is_empty());
    assert_eq!(*calls.last().unwrap(), 1.0);
    assert!(calls.iter().all(|f| (0.0..=1.0).contains(f)));
    assert_eq!(calls[0], 0.0); // start
}
