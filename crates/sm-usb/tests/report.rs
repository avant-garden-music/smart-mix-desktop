//! Espejo de `TestReport` (tests/test_usb.py).

mod common;

use std::fs;

use common::{fake_usb, tmpdir};
use sm_usb::{build_report_from_workflow, format_report, run_workflow, Confirmations, WorkflowOptions};

#[test]
fn report_counts() {
    let tmp = tmpdir("report_counts");
    let usb = fake_usb(&tmp);
    let usb_ref = &usb;

    let mut executor = |_plan: &sm_usb::FormatPlan| -> bool {
        fs::remove_dir_all(usb_ref).ok(); // simula el formato
        true
    };
    let mut confirmations = |plan: &sm_usb::FormatPlan| -> Vec<String> {
        vec![plan.confirm_token.clone(), plan.confirm_token.clone()]
    };

    let result = run_workflow(
        &usb,
        &tmp.join("b"),
        WorkflowOptions {
            dry_run: false,
            os_name: Some("linux"),
            format_device: Some("/dev/sdb"),
            format_executor: Some(&mut executor),
            confirmations: Confirmations::Provider(&mut confirmations),
            ..Default::default()
        },
        None,
    )
    .expect("workflow");

    let rep = build_report_from_workflow(&result);
    assert_eq!(rep.files_total, 7);
    assert_eq!(rep.files_restored, 7);
    assert_eq!(rep.tracks_total, 2);
    assert_eq!(rep.tracks_restored, 2);
    // PIONEER (USBANLZ + ARTWORK + rekordbox.dbs) + EngineLibrary
    assert_eq!(rep.playlists_cues_restored, 4);
    assert_eq!(rep.bytes_restored, result.scan.total_bytes);
    assert!(rep.errors.is_empty());
    assert_eq!(rep.special.len(), 2);

    let text = format_report(&rep);
    // un hecho por línea; el contenido está localizado, se afirma estructura
    assert!(text.lines().count() >= 8, "{text}");
    assert!(text.contains("USB Safe Format"));
    assert!(text.lines().any(|l| l.contains("No errors")));
}
