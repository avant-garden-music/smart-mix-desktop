//! Espejo de `TestScanner` (tests/test_usb.py).

mod common;

use common::{fake_usb, tmpdir};
use sm_usb::{sha256_hex, scan_drive, ScanError};

#[test]
fn manifest_hashes_and_relative_paths() {
    let tmp = tmpdir("scanner_manifest");
    let usb = fake_usb(&tmp);
    let scan = scan_drive(&usb, None).expect("scan");
    let rels: Vec<&str> = scan.files.iter().map(|e| e.relpath.as_str()).collect();
    // mayúsculas y rutas relativas POSIX conservadas
    assert!(rels.contains(&"Music/Techno/Track A.MP3"));
    assert!(rels.contains(&"Music/House/deep/bEEt.Wav"));
    assert!(rels.contains(&"PIONEER/rekordbox.dbs"));
    assert!(rels.contains(&"EngineLibrary/m.db"));
    let entry = scan
        .files
        .iter()
        .find(|e| e.relpath == "Music/House/deep/bEEt.Wav")
        .expect("entry");
    let data = b"audio-b".repeat(77);
    assert_eq!(entry.sha256, sha256_hex(&data));
    assert_eq!(entry.size, data.len() as u64);
    assert_eq!(
        scan.total_bytes,
        scan.files.iter().map(|e| e.size).sum::<u64>()
    );
}

#[test]
fn exclusions() {
    let tmp = tmpdir("scanner_exclusions");
    let usb = fake_usb(&tmp);
    let scan = scan_drive(&usb, None).expect("scan");
    let rels: Vec<&str> = scan.files.iter().map(|e| e.relpath.as_str()).collect();
    for banned in [
        ".DS_Store",
        "._Track A.MP3",
        ".hidden/secret.txt",
        "System Volume Information/WPSettings.dat",
    ] {
        assert!(!rels.contains(&banned), "{banned} no debía estar");
    }
    assert_eq!(scan.files.len(), 7);
}

#[test]
fn special_folders_detection() {
    let tmp = tmpdir("scanner_special");
    let usb = fake_usb(&tmp);
    let scan = scan_drive(&usb, None).expect("scan");
    assert_eq!(scan.special.pioneer.as_deref(), Some("PIONEER"));
    let mut items = scan.special.pioneer_items.clone();
    items.sort();
    assert_eq!(items, vec!["ARTWORK", "USBANLZ", "rekordbox.dbs"]);
    assert_eq!(scan.special.engine.as_deref(), Some("EngineLibrary"));
    let mut detected = scan.special.detected();
    detected.sort();
    assert_eq!(detected, vec!["EngineLibrary", "PIONEER"]);
}

#[test]
fn track_count_and_loose_root_tracks() {
    let tmp = tmpdir("scanner_loose");
    let usb = fake_usb(&tmp);
    common::make_file(&usb.join("loose.mp3"), b"x".repeat(10).as_slice());
    let scan = scan_drive(&usb, None).expect("scan");
    assert_eq!(scan.track_count(), 3);
    assert_eq!(scan.loose_root_tracks, vec!["loose.mp3".to_string()]);
}

#[test]
fn not_a_dir_raises() {
    let tmp = tmpdir("scanner_notadir");
    let result = scan_drive(&tmp.join("nope"), None);
    assert!(matches!(result, Err(ScanError::NotADir(_))));
}

#[test]
fn progress_callback() {
    let tmp = tmpdir("scanner_progress");
    let usb = fake_usb(&tmp);
    let calls: std::cell::RefCell<Vec<(String, f64)>> = std::cell::RefCell::new(Vec::new());
    scan_drive(
        &usb,
        Some(&mut |msg: &str, frac: f64| {
            calls.borrow_mut().push((msg.to_string(), frac));
        }),
    )
    .expect("scan");
    let calls = calls.into_inner();
    assert!(!calls.is_empty(), "progress nunca fue llamado");
    assert!(calls.iter().all(|(m, _)| !m.is_empty()));
    assert!(calls.iter().all(|(_, f)| (0.0..=1.0).contains(f)));
    assert_eq!(calls.last().unwrap().1, 1.0);
    assert_eq!(calls.first().unwrap().1, 0.0);
}
