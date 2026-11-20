//! Espejo de `TestFat32Validators` (tests/test_usb.py). Los casos de
//! nombre inválido/emoji se inyectan como `FileEntry` sintéticos porque
//! NTFS no permite crear archivos con `<>*?` (el .py los crea en disco,
//! pero el validador es puro sobre el manifiesto y el comportamiento
//! observable es idéntico).

mod common;

use common::{fake_usb, synthetic_entry, tmpdir};
use sm_usb::{validate_fat32, FileEntry, FAT32_MAX_FILESIZE};

#[test]
fn oversize_warning_suggests_flac_alac() {
    let tmp = tmpdir("fat32_oversize");
    let mut scan = sm_usb::scan_drive(&fake_usb(&tmp), None).expect("scan");
    scan.files.push(synthetic_entry("Music/big.wav", FAT32_MAX_FILESIZE + 1));
    let warnings = validate_fat32(&scan);
    let w = warnings
        .iter()
        .find(|w| w.code == "oversize")
        .expect("warning oversize");
    assert_eq!(w.relpath, "Music/big.wav");
    assert!(w.message.contains("FLAC"));
}

#[test]
fn invalid_chars_warning_never_renames() {
    let tmp = tmpdir("fat32_invalid_chars");
    let usb = fake_usb(&tmp);
    let before = common::snapshot(&usb);
    let mut scan = sm_usb::scan_drive(&usb, None).expect("scan");
    scan.files.push(synthetic_entry("Music/Bad/weird<name>*?.mp3", 5));
    let warnings = validate_fat32(&scan);
    let w = warnings
        .iter()
        .find(|w| w.code == "invalid_chars")
        .expect("warning invalid_chars");
    assert!(w.relpath.contains("weird<name>*?.mp3"));
    // un validador solo advierte: el árbol queda intacto
    assert_eq!(common::snapshot(&usb), before);
}

#[test]
fn emoji_warning() {
    let tmp = tmpdir("fat32_emoji");
    let mut scan = sm_usb::scan_drive(&fake_usb(&tmp), None).expect("scan");
    scan.files.push(synthetic_entry("Music/Bad/dj \u{1F3A7} set.mp3", 5));
    let warnings = validate_fat32(&scan);
    let w = warnings
        .iter()
        .find(|w| w.code == "emoji")
        .expect("warning emoji");
    assert!(w.relpath.contains("set.mp3"));
}

#[test]
fn name_too_long_warning() {
    let tmp = tmpdir("fat32_name_too_long");
    let mut scan = sm_usb::scan_drive(&fake_usb(&tmp), None).expect("scan");
    let long = format!("{}.mp3", "x".repeat(300));
    scan.files.push(FileEntry {
        relpath: long,
        size: 10,
        sha256: "0".repeat(64),
    });
    let warnings = validate_fat32(&scan);
    assert!(warnings.iter().any(|w| w.code == "name_too_long"));
}

#[test]
fn loose_track_suggests_subfolders() {
    let tmp = tmpdir("fat32_loose");
    let usb = fake_usb(&tmp);
    common::make_file(&usb.join("loose.MP3"), b"q".repeat(4).as_slice());
    let scan = sm_usb::scan_drive(&usb, None).expect("scan");
    let warnings = validate_fat32(&scan);
    let w = warnings
        .iter()
        .find(|w| w.code == "loose_track")
        .expect("warning loose_track");
    assert_eq!(w.relpath, "loose.MP3");
}

#[test]
fn clean_tree_produces_no_warnings() {
    let tmp = tmpdir("fat32_clean");
    let scan = sm_usb::scan_drive(&fake_usb(&tmp), None).expect("scan");
    assert!(validate_fat32(&scan).is_empty());
}
