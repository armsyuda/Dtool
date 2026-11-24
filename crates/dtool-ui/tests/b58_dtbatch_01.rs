#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope, Workspace};
use dtool_export::ExportFormat;
use dtool_export::batch::{BatchManifest, BatchOutput, BatchSnapshot, BatchStatus, Destination, FileStatus, OverwritePolicy};
use serde_json::{Value, json};
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
    let root = std::env::temp_dir().join(format!("dtool-b58-01-{tag}-{}-{nanos}-{seq}", std::process::id()));
    fs::create_dir_all(root.join("in")).unwrap();
    fs::create_dir_all(root.join("out")).unwrap();
    root
}

fn input(root: &Path, name: &str) -> PathBuf {
    let path = root.join("in").join(name);
    fs::write(&path, PNG).unwrap();
    path
}

fn output(root: &Path, format: ExportFormat, name: &str, overwrite: OverwritePolicy) -> BatchOutput {
    BatchOutput { format, width: None, height: None, keep_aspect: true, dest: Destination::Folder(root.join("out")), name: name.into(), overwrite, webp_lossy: false }
}

fn run(workspace: &mut Workspace, name: &str, params: Value) -> Result<Value, String> {
    workspace.run(Scope::active(), Command::with_params(name, params)).1.map_err(|error| error.code)
}

fn prepare(workspace: &mut Workspace, manifest: &BatchManifest) -> Result<String, String> {
    run(workspace, "export.batch.prepare", json!({"manifest": manifest})).map(|reply| reply["handle"].as_str().unwrap().to_owned())
}

fn problems(workspace: &mut Workspace, handle: &str) -> Vec<String> {
    let reply = run(workspace, "export.batch.validate", json!({"handle": handle})).unwrap();
    reply["problems"].as_array().unwrap().iter().map(|p| p["code"].as_str().unwrap().to_owned()).collect()
}

fn start(workspace: &mut Workspace, handle: &str) -> BatchSnapshot {
    serde_json::from_value(run(workspace, "export.batch.start", json!({"handle": handle})).unwrap()).unwrap()
}

fn manifest(root: &Path, names: &[&str]) -> BatchManifest {
    let mut manifest = BatchManifest::default();
    for name in names {
        manifest.add(input(root, name)).unwrap();
    }
    manifest
}

#[test]
fn stable_input_occurrences() {
    let _home = test_home::enter("stable_input_occurrences");
    let root = temp_root("stable");
    let mut m = manifest(&root, &["a.png", "b.png"]);
    m.outputs.push(output(&root, ExportFormat::Png, "{name}.{format}", OverwritePolicy::Fail));
    let mut ws = Workspace::default();
    let handle = prepare(&mut ws, &m).unwrap();
    assert!(problems(&mut ws, &handle).is_empty());
    let snap = start(&mut ws, &handle);
    assert_eq!(snap.status, BatchStatus::Done);
    assert_eq!(snap.files.iter().map(|f| f.input).collect::<Vec<_>>(), m.inputs.iter().map(|i| i.id).collect::<Vec<_>>());
    assert!(snap.files.iter().all(|f| f.status == FileStatus::Done));
    assert!(root.join("out").join("a.png").exists());
    assert!(root.join("out").join("b.png").exists());
}

#[test]
fn bad_input_validation() {
    let _home = test_home::enter("bad_input_validation");
    let root = temp_root("bad");
    let mut m = manifest(&root, &["a.png"]);
    m.outputs.push(output(&root, ExportFormat::Png, "{name}.{format}", OverwritePolicy::Fail));
    let mut missing = m.clone();
    missing.inputs[0].path = root.join("in").join("missing.png");
    let mut ws = Workspace::default();
    assert_eq!(prepare(&mut ws, &missing), Err("INPUT_NOT_FOUND".to_string()));
    let handle = prepare(&mut ws, &m).unwrap();
    fs::remove_file(&m.inputs[0].path).unwrap();
    assert!(problems(&mut ws, &handle).contains(&"INPUT_NOT_FOUND".to_string()));
    assert!(problems(&mut ws, "batch-unknown").contains(&"HANDLE_NOT_FOUND".to_string()));
}

#[test]
fn actual_action_effect() {
    let _home = test_home::enter("actual_action_effect");
    let root = temp_root("action");
    let mut m = manifest(&root, &["a.png"]);
    m.actions.push(dtool_export::batch::BatchAction { command: "document.resize".into(), params: json!({"width": 4, "height": 4}) });
    m.outputs.push(output(&root, ExportFormat::Png, "{name}.{format}", OverwritePolicy::Fail));
    let mut ws = Workspace::default();
    let handle = prepare(&mut ws, &m).unwrap();
    start(&mut ws, &handle);
    assert_ne!(fs::read(root.join("out").join("a.png")).unwrap(), PNG);
}

#[test]
fn invalid_action_before_output() {
    let _home = test_home::enter("invalid_action_before_output");
    let root = temp_root("invalid-action");
    let mut m = manifest(&root, &["a.png"]);
    m.outputs.push(output(&root, ExportFormat::Png, "{name}.{format}", OverwritePolicy::Fail));
    m.actions.push(dtool_export::batch::BatchAction { command: "invalid.command.name".into(), params: json!({}) });
    let mut ws = Workspace::default();
    assert_eq!(prepare(&mut ws, &m), Err("ACTION_NOT_ALLOWED".to_string()));
    m.actions[0] = dtool_export::batch::BatchAction { command: "document.resize".into(), params: json!({"width": 4}) };
    assert_eq!(prepare(&mut ws, &m), Err("ACTION_PARAMS_INVALID".to_string()));
    assert!(!root.join("out").join("a.png").exists());
}

#[test]
fn same_stem_case_collision() {
    let _home = test_home::enter("same_stem_case_collision");
    let root = temp_root("collision");
    fs::create_dir_all(root.join("in2")).unwrap();
    let mut m = manifest(&root, &["input.png"]);
    let other = root.join("in2").join("INPUT.png");
    fs::write(&other, PNG).unwrap();
    m.add(other).unwrap();
    m.outputs.push(output(&root, ExportFormat::Png, "{name}.{format}", OverwritePolicy::Fail));
    let mut ws = Workspace::default();
    assert_eq!(prepare(&mut ws, &m), Err("NAME_COLLISION".to_string()));
}

#[test]
fn explicit_multi_format_names() {
    let _home = test_home::enter("explicit_multi_format_names");
    let root = temp_root("multi");
    let mut m = manifest(&root, &["a.png"]);
    m.outputs.push(output(&root, ExportFormat::Png, "{name}_png.{format}", OverwritePolicy::Skip));
    m.outputs.push(output(&root, ExportFormat::Jpeg, "{name}_jpg.{format}", OverwritePolicy::Skip));
    let mut ws = Workspace::default();
    let handle = prepare(&mut ws, &m).unwrap();
    assert!(problems(&mut ws, &handle).is_empty());
    let snap = start(&mut ws, &handle);
    assert_eq!(snap.files[0].outputs.len(), 2);
    assert!(root.join("out").join("a_png.png").exists());
    assert!(root.join("out").join("a_jpg.jpg").exists());
    let mut same = manifest(&root, &["b.png"]);
    same.outputs.push(output(&root, ExportFormat::Png, "{name}.png", OverwritePolicy::Skip));
    same.outputs.push(output(&root, ExportFormat::Jpeg, "{name}.png", OverwritePolicy::Skip));
    assert_eq!(prepare(&mut ws, &same), Err("NAME_COLLISION".to_string()));
}

#[test]
fn no_output_validation() {
    let _home = test_home::enter("no_output_validation");
    let root = temp_root("no-output");
    let m = manifest(&root, &["a.png"]);
    let mut ws = Workspace::default();
    assert_eq!(prepare(&mut ws, &m), Err("NO_OUTPUT".to_string()));
}

#[test]
fn all_errors_preflight() {
    let _home = test_home::enter("all_errors_preflight");
    let root = temp_root("preflight");
    let mut m = manifest(&root, &["a.png"]);
    m.inputs[0].path = root.join("in").join("missing.png");
    m.actions.push(dtool_export::batch::BatchAction { command: "invalid.action".into(), params: json!({}) });
    m.outputs.push(output(&root, ExportFormat::Png, "CON", OverwritePolicy::Fail));
    let codes: Vec<String> = dtool_export::batch::validate_manifest(&m).into_iter().map(|p| p.code).collect();
    for code in ["INPUT_NOT_FOUND", "ACTION_NOT_ALLOWED", "RESERVED_NAME"] {
        assert!(codes.contains(&code.to_string()), "{code} missing from {codes:?}");
    }
    let mut ws = Workspace::default();
    assert!(prepare(&mut ws, &m).is_err());
}

#[test]
fn start_duplicate_same_handle() {
    let _home = test_home::enter("start_duplicate_same_handle");
    let root = temp_root("duplicate");
    let mut m = manifest(&root, &["a.png"]);
    m.outputs.push(output(&root, ExportFormat::Png, "{name}.{format}", OverwritePolicy::Fail));
    let mut ws = Workspace::default();
    let handle = prepare(&mut ws, &m).unwrap();
    let first = start(&mut ws, &handle);
    let second = start(&mut ws, &handle);
    assert_eq!(serde_json::to_value(&first).unwrap(), serde_json::to_value(&second).unwrap());
    assert_eq!(second.status, BatchStatus::Done);
}

#[test]
fn query_before_start_is_prepared() {
    let _home = test_home::enter("query_before_start_is_prepared");
    let root = temp_root("query");
    let mut m = manifest(&root, &["a.png"]);
    m.outputs.push(output(&root, ExportFormat::Png, "{name}.{format}", OverwritePolicy::Fail));
    let mut ws = Workspace::default();
    let handle = prepare(&mut ws, &m).unwrap();
    let reply = run(&mut ws, "export.batch.query", json!({"handle": handle})).unwrap();
    assert_eq!(reply["status"], json!("Prepared"));
    assert_eq!(run(&mut ws, "export.batch.query", json!({"handle": ""})), Err("INVALID_PARAMS".to_string()));
}

#[test]
fn measured_progress() {
    let _home = test_home::enter("measured_progress");
    let root = temp_root("progress");
    let mut m = manifest(&root, &["a.png", "b.png"]);
    m.outputs.push(output(&root, ExportFormat::Png, "{name}_1.{format}", OverwritePolicy::Fail));
    m.outputs.push(output(&root, ExportFormat::Png, "{name}_2.{format}", OverwritePolicy::Fail));
    let mut ws = Workspace::default();
    let handle = prepare(&mut ws, &m).unwrap();
    let snap = start(&mut ws, &handle);
    assert_eq!(snap.total_units, 4);
    assert_eq!(snap.done_units, snap.total_units);
}

#[test]
fn ordered_parallel_results() {
    let _home = test_home::enter("ordered_parallel_results");
    let root = temp_root("parallel");
    let mut m = manifest(&root, &["a.png", "b.png", "c.png"]);
    m.parallel = true;
    m.outputs.push(output(&root, ExportFormat::Png, "{name}.{format}", OverwritePolicy::Fail));
    let mut ws = Workspace::default();
    let handle = prepare(&mut ws, &m).unwrap();
    let snap = start(&mut ws, &handle);
    assert_eq!(snap.files.iter().map(|f| f.input).collect::<Vec<_>>(), m.inputs.iter().map(|i| i.id).collect::<Vec<_>>());
}
