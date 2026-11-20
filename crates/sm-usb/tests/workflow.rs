//! Espejo de `TestWorkflow` (tests/test_usb.py): dry-run seguro, executor
//! inyectable con doble confirmación, abortos.

mod common;

use std::fs;

use common::{fake_usb, snapshot, tmpdir};
use sm_usb::{run_workflow, Confirmations, WorkflowError, WorkflowOptions};

#[test]
fn dry_run_touches_nothing() {
    let tmp = tmpdir("wf_dry_run");
    let usb = fake_usb(&tmp);
    let dest = tmp.join("backup");
    let before = snapshot(&usb);
    let result = run_workflow(
        &usb,
        &dest,
        WorkflowOptions {
            os_name: Some("linux"),
            format_device: Some("/dev/sdb"),
            ..Default::default() // dry_run = true POR DEFECTO
        },
        None,
    )
    .expect("dry run");
    assert!(result.dry_run);
    assert!(result.backup.is_none());
    assert!(result.restore.is_none());
    assert!(!result.backup_verified);
    assert!(!dest.exists(), "el dry run no debe escribir NADA");
    assert_eq!(snapshot(&usb), before, "la unidad debe quedar intacta");
    assert_eq!(result.scan.files.len(), 7);
    assert!(result.format_plan.is_some());
    assert!(!result.steps.is_empty());
    // el paso de formato aparece como preparado, no ejecutado
    assert!(result.steps.iter().any(|s| s.contains("/dev/sdb")));
}

#[test]
fn full_workflow_with_fake_format_executor() {
    let tmp = tmpdir("wf_full");
    let usb = fake_usb(&tmp);
    let dest = tmp.join("backup");
    let executed: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
    let expected = snapshot(&usb);
    let usb_ref = &usb;

    let mut executor = |plan: &sm_usb::FormatPlan| -> bool {
        executed.borrow_mut().push(plan.command.clone());
        fs::remove_dir_all(usb_ref).ok(); // simula el formato borrando el stick
        true
    };
    let mut confirmations = |plan: &sm_usb::FormatPlan| -> Vec<String> {
        vec![plan.confirm_token.clone(), plan.confirm_token.clone()]
    };

    let result = run_workflow(
        &usb,
        &dest,
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
    .expect("workflow completo");

    assert_eq!(
        executed.into_inner(),
        vec!["mkfs.vfat -F 32 -n HARMONIC /dev/sdb".to_string()]
    );
    assert!(result.backup_verified);
    assert!(result.backup.as_ref().unwrap().ok());
    let restore = result.restore.as_ref().expect("restore");
    assert!(restore.hash_failures.is_empty());
    // el contenido de la unidad volvió byte a byte (basura excluida por diseño)
    let after = snapshot(&usb);
    let manifest_map: std::collections::BTreeMap<String, String> = result
        .scan
        .files
        .iter()
        .map(|e| (e.relpath.clone(), e.sha256.clone()))
        .collect();
    assert_eq!(after, manifest_map);
    for (rel, _) in after {
        assert!(expected.contains_key(&rel), "archivo inesperado: {rel}");
    }
}

#[test]
fn missing_double_confirmation_aborts_before_format() {
    let tmp = tmpdir("wf_noconfirm");
    let usb = fake_usb(&tmp);
    let before = snapshot(&usb);
    let executed: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
    let usb_ref = &usb;

    let mut executor = |plan: &sm_usb::FormatPlan| -> bool {
        executed.borrow_mut().push(plan.command.clone());
        fs::remove_dir_all(usb_ref).ok();
        true
    };
    // solo UN token: la doble confirmación exige DOS
    let mut confirmations =
        |plan: &sm_usb::FormatPlan| -> Vec<String> { vec![plan.confirm_token.clone()] };

    let err = run_workflow(
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
    );
    assert!(matches!(err, Err(WorkflowError(_))));
    assert!(err.unwrap_err().0.contains("token"), "mensaje con token");
    assert!(executed.into_inner().is_empty(), "el formato NUNCA corrió");
    assert_eq!(snapshot(&usb), before, "unidad intacta");
    assert!(usb.exists());
}

#[test]
fn failing_executor_raises() {
    let tmp = tmpdir("wf_execfail");
    let usb = fake_usb(&tmp);
    let mut executor = |_plan: &sm_usb::FormatPlan| -> bool { false };
    let mut confirmations = |plan: &sm_usb::FormatPlan| -> Vec<String> {
        vec![plan.confirm_token.clone(), plan.confirm_token.clone()]
    };
    let err = run_workflow(
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
    );
    assert!(matches!(err, Err(WorkflowError(_))));
    assert!(usb.exists(), "sin restore: nada volvió a borrarse");
}

#[test]
fn no_format_device_backup_only() {
    let tmp = tmpdir("wf_backuponly");
    let usb = fake_usb(&tmp);
    let result = run_workflow(
        &usb,
        &tmp.join("b"),
        WorkflowOptions {
            dry_run: false,
            ..Default::default()
        },
        None,
    )
    .expect("backup only");
    assert!(result.backup.as_ref().unwrap().ok());
    assert!(result.backup_verified);
    assert!(result.format_plan.is_none());
    assert!(result.restore.is_none());
}

#[test]
fn empty_drive_aborts() {
    let tmp = tmpdir("wf_empty");
    let empty = tmp.join("empty");
    fs::create_dir_all(&empty).unwrap();
    let err = run_workflow(
        &empty,
        &tmp.join("b"),
        WorkflowOptions {
            dry_run: false,
            ..Default::default()
        },
        None,
    );
    assert!(matches!(err, Err(WorkflowError(_))));
}
