//! Espejo de `TestFormatter` (tests/test_usb.py): el preparador de
//! comandos NUNCA ejecuta nada.

mod common;

use sm_usb::{prepare_format_command, FormatError};

#[test]
fn macos_command() {
    let plan = prepare_format_command("disk4", Some("macos"), "SMARTMIX", None).expect("plan");
    assert_eq!(
        plan.command,
        "diskutil eraseDisk \"MS-DOS FAT32\" SMARTMIX /dev/disk4"
    );
    assert_eq!(plan.shell, "bash");
    assert!(plan.needs_admin);
    assert!(!plan.confirm_token.is_empty());
}

#[test]
fn macos_accepts_dev_path_and_darwin_alias() {
    let plan = prepare_format_command("/dev/disk2", Some("Darwin"), "HARMONIC", None).expect("plan");
    assert!(plan.command.contains("diskutil eraseDisk \"MS-DOS FAT32\""));
    assert!(plan.command.contains("/dev/disk2"));
}

#[test]
fn windows_small_volume_uses_format() {
    let plan = prepare_format_command("E:", Some("windows"), "SMARTMIX", Some(16.0)).expect("plan");
    assert!(plan.command.starts_with("format E:"));
    assert!(plan.command.contains("/FS:FAT32"));
    assert!(plan.command.contains("/V:SMARTMIX"));
    assert_eq!(plan.shell, "cmd");
    assert_eq!(plan.device, "E:");
}

#[test]
fn windows_large_volume_uses_powershell() {
    // La GUI de Windows rechaza FAT32 por encima de 32 GB: PowerShell.
    let plan = prepare_format_command("E", Some("windows"), "HARMONIC", Some(64.0)).expect("plan");
    assert!(plan.command.contains("Format-Volume"));
    assert!(plan.command.contains("-FileSystem FAT32"));
    assert!(plan.command.contains("-DriveLetter E"));
    assert!(plan.command.contains("-NewFileSystemLabel HARMONIC"));
    assert!(plan.command.contains("-Force"));
    assert_eq!(plan.shell, "powershell");
    assert!(plan.needs_admin);
}

#[test]
fn windows_unknown_size_defaults_to_powershell() {
    let plan = prepare_format_command("E", Some("windows"), "HARMONIC", None).expect("plan");
    assert!(plan.command.contains("Format-Volume"));
}

#[test]
fn linux_command() {
    let plan =
        prepare_format_command("/dev/sdb", Some("linux"), "harmonic", None).expect("plan");
    assert_eq!(plan.command, "mkfs.vfat -F 32 -n HARMONIC /dev/sdb");
}

#[test]
fn label_truncated_to_11_chars() {
    let plan = prepare_format_command("/dev/sdb", Some("linux"), "VERYLONGLABEL", None)
        .expect("plan");
    assert!(plan.command.contains("-n VERYLONGLAB"));
    assert_eq!(plan.label, "VERYLONGLAB");
    assert!(!plan.notes.is_empty()); // una nota explica el truncado
}

#[test]
fn unsupported_os_raises() {
    let err = prepare_format_command("/dev/sdb", Some("haiku"), "HARMONIC", None);
    assert!(matches!(err, Err(FormatError::UnsupportedOs(_))));
}

#[test]
fn bad_devices_raise() {
    for (bad, os) in [
        ("1:", "windows"),
        ("", "windows"),
        ("notadisk", "macos"),
        ("sdb", "linux"),
        ("", "linux"),
    ] {
        let err = prepare_format_command(bad, Some(os), "HARMONIC", None);
        assert!(
            matches!(err, Err(FormatError::BadDevice(_))),
            "{bad} en {os} debía rechazarse"
        );
    }
}

#[test]
fn default_os_is_platform() {
    // dispositivo válido POR PLATAFORMA: letra en windows, ruta /dev/ en unix
    let dev = if cfg!(windows) { "d" } else { "/dev/disk99" };
    let plan = prepare_format_command(dev, None, "HARMONIC", None).expect("plan");
    assert!([ "windows", "macos", "linux" ].contains(&plan.os_name.as_str()));
}
