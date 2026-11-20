//! F-25 Export USB — destino canónico y avisos FAT32 (funciones puras).
use std::path::{Path, PathBuf};

#[test]
fn smartmix_dir_cuelga_de_la_raiz() {
    // contrato agnóstico de plataforma: el dir canónico es la raíz
    // recibida + "SmartMix" (la unidad puede ser E:\ en Windows,
    // /Volumes/X en macOS o /mnt/usb en Linux)
    for root in [Path::new("E:\\"), Path::new("/Volumes/USBDISK"), Path::new("/mnt/usb")] {
        assert_eq!(sm_usb::smartmix_dir(root), root.join("SmartMix"));
    }
}

#[test]
fn sin_fat32_no_ha_avisos_aunque_el_nombre_sea_largo() {
    let largos = vec![format!("{}.mp3", "x".repeat(40))];
    assert!(sm_usb::usb_export_warnings(false, &largos).is_empty());
}

#[test]
fn fat32_avisa_solo_nombres_de_mas_de_31() {
    // 27 aes + ".mp3" = 31 exacto (ok) · 28 bes + ".mp3" = 32 (aviso)
    let names = vec![
        format!("{}.mp3", "a".repeat(27)),
        format!("{}.mp3", "b".repeat(28)),
    ];
    let w = sm_usb::usb_export_warnings(true, &names);
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].code, "name_gt_31");
    assert!(w[0].relpath.starts_with("bbbb"));
    assert!(w[0].message.contains("31"), "mensaje: {}", w[0].message);
}

#[test]
fn fat32_acepta_rutas_y_toma_solo_el_nombre() {
    let paths = vec![format!("C:/music/{}.flac", "c".repeat(32))];
    let w = sm_usb::usb_export_warnings(true, &paths);
    assert_eq!(w.len(), 1);
    assert!(!w[0].relpath.contains('/'));
    assert!(!w[0].relpath.contains('\\'));
    // límite expuesto como constante del contrato
    assert_eq!(sm_usb::FAT32_DJ_NAME_LIMIT, 31);
}
