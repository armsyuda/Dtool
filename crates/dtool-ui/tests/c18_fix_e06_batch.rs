#[path = "support/test_home.rs"]
mod test_home;
use dtool_export::ExportFormat;
use dtool_export::batch::{
    self, e06_label_key, is_e06_held, is_e06_offered, output_skip_reason, BatchManifest, BatchOutput,
    BatchStatus, Destination, FileStatus, OverwritePolicy, E06_OFFERED_FORMATS,
};
use dtool_ui::batch_dialog::BatchDialog;
use dtool_ui::dialogs::export::{self, ExportDialog};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 2, 8, 6,
    0, 0, 0, 114, 182, 13, 36, 0, 0, 0, 17, 73, 68, 65, 84, 120, 156, 99, 16, 209, 136, 250,
    15, 194, 12, 48, 6, 0, 51, 32, 6, 85, 123, 239, 145, 110, 0, 0, 0, 0, 73, 69, 78, 68, 174,
    66, 96, 130,
];

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_root(tag: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let seq = SEQ.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "dtool-c18-{tag}-{}-{nanos}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("in")).unwrap();
    fs::create_dir_all(root.join("out")).unwrap();
    root
}

fn input(root: &Path, name: &str) -> PathBuf {
    let path = root.join("in").join(name);
    fs::write(&path, PNG).unwrap();
    path
}

fn output(root: &Path, format: ExportFormat, name: &str, webp_lossy: bool) -> BatchOutput {
    BatchOutput {
        format,
        width: None,
        height: None,
        keep_aspect: true,
        dest: Destination::Folder(root.join("out")),
        name: name.into(),
        overwrite: OverwritePolicy::Fail,
        webp_lossy,
    }
}

fn wait_done(handle: &dtool_export::batch::BatchHandle) -> dtool_export::batch::BatchSnapshot {
    let start = SystemTime::now();
    loop {
        let snapshot = batch::query(handle);
        if snapshot.status != BatchStatus::Running && snapshot.status != BatchStatus::Prepared {
            return snapshot;
        }
        if !batch::is_running(handle) && snapshot.status != BatchStatus::Prepared {
            return snapshot;
        }
        if start.elapsed().unwrap().as_secs() > 8 {
            return snapshot;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn shared_e06_list_hides_held_formats() {
    let _home = test_home::enter("shared_e06_list_hides_held_formats");
    let offered: Vec<_> = E06_OFFERED_FORMATS.to_vec();
    assert_eq!(
        offered,
        vec![
            ExportFormat::Png,
            ExportFormat::Jpeg,
            ExportFormat::Pdf,
            ExportFormat::Svg,
            ExportFormat::Webp,
            ExportFormat::Eps,
        ]
    );
    assert_eq!(BatchDialog::offered_formats(), E06_OFFERED_FORMATS);
    assert_eq!(export::format_choices(), E06_OFFERED_FORMATS);
    for format in [
        ExportFormat::Tiff,
        ExportFormat::Gif,
        ExportFormat::Wmf,
    ] {
        assert!(is_e06_held(format));
        assert!(!is_e06_offered(format));
        assert!(!export::format_choices().contains(&format));
    }
    assert_eq!(e06_label_key(ExportFormat::Eps), "batch.format_eps");
}

#[test]
fn export_dialog_refuses_held_format_on_open() {
    let _home = test_home::enter("export_dialog_refuses_held_format_on_open");
    let doc = dtool_core::document::Document::new([120.0, 80.0], dtool_core::document::Units::Px, 96.0, 1);
    let mut dialog = ExportDialog::new();
    let err = dialog
        .run(&doc, export::UI_EXPORT_OPEN, &json!({"format": "tiff"}))
        .unwrap()
        .unwrap_err();
    assert!(matches!(err, dtool_export::raster::ExportError::Unavailable(_)));
    let ok = dialog
        .run(&doc, export::UI_EXPORT_OPEN, &json!({"format": "pdf"}))
        .unwrap()
        .unwrap();
    assert!(ok.get("open").and_then(|v| v.as_bool()).unwrap_or(true) || dialog.is_open());
    assert!(dialog.is_open());
}

#[test]
fn batch_skips_held_and_missing_writers_without_fake_files() {
    let _home = test_home::enter("batch_skips_held_and_missing_writers_without_fake_files");
    let root = temp_root("skip");
    let mut manifest = BatchManifest::default();
    manifest.add(input(&root, "a.png")).unwrap();
    manifest.outputs.push(output(&root, ExportFormat::Png, "{name}_ok.{format}", false));
    manifest.outputs.push(output(&root, ExportFormat::Tiff, "{name}_held.{format}", false));
    manifest.outputs.push(output(&root, ExportFormat::Eps, "{name}_eps.{format}", false));
    manifest.outputs.push(output(&root, ExportFormat::Webp, "{name}_lossy.{format}", true));
    let handle = batch::prepare(manifest).unwrap();
    let snap = batch::start(&handle).unwrap();
    assert_eq!(snap.status, BatchStatus::Done);
    assert_eq!(snap.files.len(), 1);
    let file = &snap.files[0];
    assert_eq!(file.status, FileStatus::Partial);
    assert_eq!(file.outputs.len(), 3);
    assert!(file.outputs.iter().all(|o| o.complete));
    assert!(root.join("out").join("a_ok.png").exists());
    assert!(!root.join("out").join("a_held.tiff").exists());
    let eps = fs::read(root.join("out").join("a_eps.eps")).unwrap();
    assert!(eps.starts_with(b"%!PS-Adobe-3.0 EPSF-3.0"));
    assert!(String::from_utf8_lossy(&eps).contains("%%BoundingBox: 0 0 2 2"));
    let lossy = fs::read(root.join("out").join("a_lossy.webp")).unwrap();
    assert!(lossy.starts_with(b"RIFF") && &lossy[8..12] == b"WEBP" && lossy.windows(4).any(|w| w == b"VP8 "));
    assert!(file.warnings.contains(&"FORMAT_HELD".to_string()));
    assert_eq!(
        file.warnings.iter().filter(|w| *w == "WRITER_UNAVAILABLE").count(),
        0
    );
    assert!(file.error.is_none());
}

#[test]
fn held_output_does_not_fail_prepare() {
    let _home = test_home::enter("held_output_does_not_fail_prepare");
    let root = temp_root("held-prep");
    let mut manifest = BatchManifest::default();
    manifest.add(input(&root, "a.png")).unwrap();
    manifest.outputs.push(output(&root, ExportFormat::Tiff, "{name}.{format}", false));
    let problems = batch::validate_manifest(&manifest);
    assert!(problems.iter().all(|p| p.code != "FORMAT_HELD"));
    let handle = batch::prepare(manifest).unwrap();
    let snap = batch::start(&handle).unwrap();
    assert_eq!(snap.status, BatchStatus::Done);
    assert_eq!(snap.files[0].status, FileStatus::Partial);
    assert!(snap.files[0].outputs.is_empty());
    assert!(!root.join("out").join("a.tiff").exists());
}

#[test]
fn cancel_sets_export_control() {
    let _home = test_home::enter("cancel_sets_export_control");
    let root = temp_root("ctrl");
    let mut manifest = BatchManifest::default();
    manifest.add(input(&root, "a.png")).unwrap();
    manifest.outputs.push(output(&root, ExportFormat::Png, "{name}.{format}", false));
    let handle = batch::prepare(manifest).unwrap();
    let snapshot = batch::cancel(&handle);
    let ctrl = batch::export_control(&handle).expect("ExportControl bound by cancel");
    assert!(ctrl.is_cancelled());
    assert_ne!(snapshot.status, BatchStatus::Failed);
}

#[test]
fn dialog_binds_start_query_cancel_and_close() {
    let _home = test_home::enter("dialog_binds_start_query_cancel_and_close");
    let root = temp_root("bind");
    let mut dialog = BatchDialog::default();
    dialog.add_paths([input(&root, "a.png")]);
    dialog.add_output(ExportFormat::Png, false);
    dialog.manifest.outputs[0].dest = Destination::Folder(root.join("out"));
    assert!(dialog.can_close());
    let handle = dialog.start_run().unwrap();
    let snap = wait_done(&handle);
    dialog.refresh_snapshot();
    let last = dialog.last_snapshot().cloned().unwrap_or(snap);
    assert!(matches!(last.status, BatchStatus::Done | BatchStatus::Cancelled));
    assert!(dialog.can_close());
    assert!(!dialog.is_busy());
    assert!(root.join("out").join("a.png").exists() || last.status == BatchStatus::Cancelled);
}

#[test]
fn skip_reason_matches_policy() {
    let _home = test_home::enter("skip_reason_matches_policy");
    let png = output(&temp_root("r"), ExportFormat::Png, "n.{format}", false);
    assert_eq!(output_skip_reason(&png), None);
    let tiff = output(&temp_root("r2"), ExportFormat::Tiff, "n.{format}", false);
    assert_eq!(output_skip_reason(&tiff), Some("FORMAT_HELD"));
    let eps = output(&temp_root("r3"), ExportFormat::Eps, "n.{format}", false);
    assert_eq!(output_skip_reason(&eps), None);
    let lossy = output(&temp_root("r4"), ExportFormat::Webp, "n.{format}", true);
    assert_eq!(output_skip_reason(&lossy), None);
    let lossless = output(&temp_root("r5"), ExportFormat::Webp, "n.{format}", false);
    assert_eq!(output_skip_reason(&lossless), None);
}
