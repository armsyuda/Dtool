use dtool_core::document::{Document, Units};
use dtool_core::grid::Grid;
use dtool_core::guides::{GuideKind, Guides};
use dtool_core::object::{Object, ObjectId};
use dtool_core::snap_candidates::{CandidateKind, SnapDocument, snap_candidates, smart_guides};
use dtool_core::snap_prefs::SnapPrefs;
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_core::ViewState;
use dtool_protocol::Command;
use serde_json::{Value, json};
use std::path::PathBuf;

struct OwnedDirectory(PathBuf);
impl OwnedDirectory {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("dtool_b10_{}_{}", std::process::id(), nonce));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        assert!(self.0.file_name().unwrap().to_string_lossy().starts_with("dtool_b10_"));
        let _ = std::fs::remove_file(self.0.join("legacy.dtool"));
        let _ = std::fs::remove_file(self.0.join("saved.dtool"));
        let _ = std::fs::remove_file(self.0.join("prefs.json"));
        let _ = std::fs::remove_file(self.0.join("DTool").join("snap_prefs.json"));
        let _ = std::fs::remove_dir(self.0.join("DTool"));
        let _ = std::fs::remove_dir(&self.0);
    }
}

#[test]
fn all_snap_preference_fields_round_trip_through_actual_load_and_save() {
    let directory = OwnedDirectory::new();
    let mut value = serde_json::to_value(SnapPrefs::default()).unwrap();
    for value in value.as_object_mut().unwrap().values_mut() {
        if let Some(flag) = value.as_bool() { *value = json!(!flag); }
    }
    value["screen_tolerance"] = json!(17);
    value["max_candidates"] = json!(23);
    let prefs: SnapPrefs = serde_json::from_value(value).unwrap();
    let path = directory.0.join("prefs.json");
    prefs.save(path.clone()).unwrap();
    assert_eq!(SnapPrefs::load(path), prefs);
}

#[test]
fn missing_corrupt_and_legacy_per_document_files_use_app_defaults() {
    let directory = OwnedDirectory::new();
    let path = directory.0.join("prefs.json");
    assert_eq!(SnapPrefs::load(path.clone()), SnapPrefs::default());
    std::fs::write(&path, b"invalid json").unwrap();
    assert_eq!(SnapPrefs::load(path.clone()), SnapPrefs::default());
    std::fs::write(&path, serde_json::to_vec(&json!({"snap": {"enabled": false}})).unwrap()).unwrap();
    assert_eq!(SnapPrefs::load(path), SnapPrefs::default());
}

#[test]
fn actual_default_path_is_appdata_dtool_or_relative_fallback() {
    let expected = match std::env::var_os("APPDATA") {
        Some(path) => PathBuf::from(path).join("DTool").join("snap_prefs.json"),
        None => PathBuf::from("./snap_prefs.json"),
    };
    assert_eq!(SnapPrefs::default_path(), expected);
}

fn run(workspace: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    let (target, result) = workspace.run(Scope::of(id), Command::with_params(name, params));
    assert_eq!(target, Some(id));
    let mut value = result.unwrap();
    if name == "snap.prefs.query" {
        assert_eq!(value["context_id"], json!(id.0));
        value.as_object_mut().unwrap().remove("context_id");
    }
    value
}

#[test]
fn prefs_set_is_global_to_existing_and_new_document_contexts_without_document_undo() {
    if std::env::var_os("DTOOL_B10_ISOLATED").is_none() {
        let directory = OwnedDirectory::new();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "prefs_set_is_global_to_existing_and_new_document_contexts_without_document_undo", "--nocapture"])
            .env("APPDATA", &directory.0).env("DTOOL_B10_ISOLATED", "1").output().unwrap();
        assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return;
    }
    let appdata = PathBuf::from(std::env::var_os("APPDATA").unwrap());
    assert_eq!(appdata.parent(), Some(std::env::temp_dir().as_path()));
    assert!(appdata.file_name().unwrap().to_string_lossy().starts_with("dtool_b10_"));
    let mut workspace = Workspace::default();
    let first = workspace.new_document("first");
    let second = workspace.new_document("second");
    let before_first = serde_json::to_value(&workspace.get(first).unwrap().session.document).unwrap();
    let before_second = serde_json::to_value(&workspace.get(second).unwrap().session.document).unwrap();
    run(&mut workspace, first, "snap.prefs.set", json!({"enabled": false, "screen_tolerance": 17, "snap_to_grid": true}));
    let expected = run(&mut workspace, first, "snap.prefs.query", json!({}));
    assert_eq!(expected["enabled"], false);
    assert_eq!(expected["screen_tolerance"], 17);
    assert_eq!(run(&mut workspace, second, "snap.prefs.query", json!({})), expected);
    let third = workspace.new_document("third");
    assert_eq!(run(&mut workspace, third, "snap.prefs.query", json!({})), expected);
    assert_eq!(serde_json::to_value(&workspace.get(first).unwrap().session.document).unwrap(), before_first);
    assert_eq!(serde_json::to_value(&workspace.get(second).unwrap().session.document).unwrap(), before_second);
    for context in workspace.contexts() { assert!(!context.session.undo.can_undo()); }
    assert_eq!(SnapPrefs::load(SnapPrefs::default_path()).screen_tolerance, 17);
}

fn prefs() -> SnapPrefs {
    let mut value = serde_json::to_value(SnapPrefs::default()).unwrap();
    for field in value.as_object_mut().unwrap().values_mut() {
        if field.is_boolean() { *field = json!(false); }
    }
    value["enabled"] = json!(true);
    value["screen_tolerance"] = json!(2);
    value["max_candidates"] = json!(32);
    serde_json::from_value(value).unwrap()
}

fn document() -> Document {
    let mut doc = Document::new([400.0, 400.0], Units::Px, 96.0, 1);
    doc.objects.push(Object { id: ObjectId(1), pos: [100.0, 100.0], size: [20.0, 20.0], ..Object::default() });
    doc
}

#[test]
fn guide_grid_pixel_and_global_enable_flags_control_actual_candidate_kernel() {
    let doc = document();
    let mut guides = Guides::default();
    guides.add(GuideKind::Vertical, 10.0, None).unwrap();
    let grid = Grid { show: true, spacing: [10.0, 10.0], subdivisions: 1, ..Grid::default() };
    let source = SnapDocument { document: &doc, guides: &guides, grid: &grid };
    let view = ViewState::default();
    let mut options = prefs();
    assert!(snap_candidates(&source, &view, &options, [9.5, 29.5]).is_empty());
    options.snap_to_guides = true;
    assert!(snap_candidates(&source, &view, &options, [9.5, 29.5]).iter().any(|candidate| candidate.kind == CandidateKind::Guide));
    options.snap_to_guides = false;
    options.snap_to_grid = true;
    assert!(snap_candidates(&source, &view, &options, [9.5, 29.5]).iter().all(|candidate| candidate.kind == CandidateKind::Grid));
    assert!(!snap_candidates(&source, &view, &options, [9.5, 29.5]).is_empty());
    options.snap_to_grid = false;
    options.force_pixel_alignment = true;
    assert!(snap_candidates(&source, &view, &options, [9.5, 29.5]).iter().any(|candidate| candidate.kind == CandidateKind::Pixel));
    options.enabled = false;
    assert!(snap_candidates(&source, &view, &options, [9.5, 29.5]).is_empty());
}

#[test]
fn object_enable_bounds_midpoints_geometry_keypoints_and_visibility_flags_control_candidates() {
    let mut doc = document();
    let guides = Guides::default();
    let grid = Grid::default();
    let view = ViewState::default();
    let mut options = prefs();
    options.snap_to_object_bounds = true;
    let source = SnapDocument { document: &doc, guides: &guides, grid: &grid };
    assert!(snap_candidates(&source, &view, &options, [100.0, 110.0]).is_empty());
    options.object_snapping = true;
    assert!(!snap_candidates(&source, &view, &options, [100.0, 110.0]).is_empty());
    assert!(snap_candidates(&source, &view, &options, [110.0, 50.0]).is_empty());
    options.include_bbox_midpoints = true;
    assert!(!snap_candidates(&source, &view, &options, [110.0, 50.0]).is_empty());
    options.snap_to_object_bounds = false;
    options.snap_to_shape_key_points = true;
    assert!(snap_candidates(&source, &view, &options, [100.0, 100.0]).iter().any(|candidate| candidate.kind == CandidateKind::ObjectGeometry));
    options.snap_to_shape_key_points = false;
    options.snap_to_object_geometry = true;
    assert!(!snap_candidates(&source, &view, &options, [110.0, 100.0]).is_empty());
    doc.objects[0].opacity = 0.0;
    let source = SnapDocument { document: &doc, guides: &guides, grid: &grid };
    assert!(!snap_candidates(&source, &view, &options, [110.0, 100.0]).is_empty());
    options.only_visible_objects = true;
    assert!(snap_candidates(&source, &view, &options, [110.0, 100.0]).is_empty());
}

#[test]
fn candidate_tolerance_uses_view_zoom_and_max_candidates_limits_results() {
    let doc = document();
    let mut guides = Guides::default();
    guides.add(GuideKind::Vertical, 10.0, None).unwrap();
    guides.add(GuideKind::Horizontal, 10.0, None).unwrap();
    let grid = Grid::default();
    let source = SnapDocument { document: &doc, guides: &guides, grid: &grid };
    let mut view = ViewState::default();
    let mut options = prefs();
    options.snap_to_guides = true;
    assert!(snap_candidates(&source, &view, &options, [7.0, 7.0]).is_empty());
    options.screen_tolerance = 4;
    assert_eq!(snap_candidates(&source, &view, &options, [7.0, 7.0]).len(), 2);
    view.zoom = 2.0;
    assert!(snap_candidates(&source, &view, &options, [7.0, 7.0]).is_empty());
    view.zoom = 1.0;
    options.max_candidates = 1;
    assert_eq!(snap_candidates(&source, &view, &options, [7.0, 7.0]).len(), 1);
}

#[test]
fn page_snapping_flag_exposes_actual_page_edge_candidates() {
    let doc = document();
    let guides = Guides::default();
    let grid = Grid::default();
    let source = SnapDocument { document: &doc, guides: &guides, grid: &grid };
    let view = ViewState::default();
    let mut options = prefs();
    assert!(snap_candidates(&source, &view, &options, [0.25, 30.0]).is_empty());
    options.page_snapping = true;
    assert!(snap_candidates(&source, &view, &options, [0.25, 30.0]).iter().any(|candidate| candidate.point == [0.0, 30.0]));
}

#[test]
fn dynamic_guides_flag_controls_actual_move_overlay_candidates() {
    let doc = document();
    let guides = Guides::default();
    let grid = Grid::default();
    let source = SnapDocument { document: &doc, guides: &guides, grid: &grid };
    let view = ViewState::default();
    let mut options = prefs();
    options.object_snapping = true;
    options.snap_to_object_bounds = true;
    options.dynamic_guides = false;
    assert!(smart_guides(&source, &view, &options, [100.0, 110.0], None).is_empty());
    options.dynamic_guides = true;
    assert!(!smart_guides(&source, &view, &options, [100.0, 110.0], None).is_empty());
}

fn isolated_legacy_process(test: &str) -> bool {
    if std::env::var("DTOOL_B10_LEGACY_CASE").ok().as_deref() == Some(test) {
        let directory = PathBuf::from(std::env::var_os("APPDATA").unwrap());
        assert_eq!(directory.parent(), Some(std::env::temp_dir().as_path()));
        assert!(directory.file_name().unwrap().to_string_lossy().starts_with("dtool_b10_"));
        return true;
    }
    let directory = OwnedDirectory::new();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env("APPDATA", &directory.0).env("DTOOL_B10_LEGACY_CASE", test).output().unwrap();
    assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    false
}

fn legacy_native_fixture() -> (Workspace, ContextId, SnapPrefs) {
    let appdata = PathBuf::from(std::env::var_os("APPDATA").unwrap());
    let mut global = SnapPrefs::default();
    global.enabled = true;
    global.screen_tolerance = 17;
    global.snap_to_grid = false;
    global.object_snapping = true;
    global.snap_to_object_bounds = true;
    global.dynamic_guides = true;
    global.save(SnapPrefs::default_path()).unwrap();
    let mut native = serde_json::to_value(document()).unwrap();
    native["snap"] = json!({"enabled": false, "screen_tolerance": 1, "snap_to_grid": true});
    native["snap_prefs"] = native["snap"].clone();
    native["snap_enabled"] = json!(false);
    native["snap_to_grid"] = json!(true);
    native["snap_overlay"] = json!([{"axis": "x", "position": 100}]);
    native["snap_feedback"] = json!({"candidate": [100, 110]});
    let path = appdata.join("legacy.dtool");
    std::fs::write(&path, serde_json::to_vec(&native).unwrap()).unwrap();
    let mut workspace = Workspace::default();
    let (id, result) = workspace.run(Scope::active(), Command::with_params("doc.open", json!({"path": path})));
    result.unwrap();
    (workspace, id.unwrap(), global)
}

fn saved_native(workspace: &mut Workspace, id: ContextId) -> Value {
    let path = PathBuf::from(std::env::var_os("APPDATA").unwrap()).join("saved.dtool");
    run(workspace, id, "doc.save_as", json!({"path": path}));
    let mut saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    // Native extras written on purpose (C-HUB-B3 guides, C02-5 view state); the rest must equal the document.
    let map = saved.as_object_mut().unwrap();
    let guides = map.remove(dtool_core::workspace::NATIVE_GUIDES_KEY).expect("guides extras");
    assert_eq!(guides, serde_json::to_value(&workspace.get(id).unwrap().session.guides).unwrap());
    assert!(map.remove(dtool_core::workspace::NATIVE_VIEW_KEY).expect("view extras").is_object());
    saved
}

#[test]
fn legacy_native_document_uses_actual_app_global_prefs_instead_of_embedded_flags() {
    if !isolated_legacy_process("legacy_native_document_uses_actual_app_global_prefs_instead_of_embedded_flags") { return; }
    let (mut workspace, id, global) = legacy_native_fixture();
    assert_eq!(run(&mut workspace, id, "snap.prefs.query", json!({})), serde_json::to_value(&global).unwrap());
    assert_eq!(SnapPrefs::load(SnapPrefs::default_path()), global);
    assert!(!workspace.get(id).unwrap().session.undo.can_undo());
}

#[test]
fn native_save_discards_legacy_per_document_snap_flags_and_keeps_global_file() {
    if !isolated_legacy_process("native_save_discards_legacy_per_document_snap_flags_and_keeps_global_file") { return; }
    let (mut workspace, id, global) = legacy_native_fixture();
    let expected = serde_json::to_value(&workspace.get(id).unwrap().session.document).unwrap();
    let saved = saved_native(&mut workspace, id);
    for key in ["snap", "snap_prefs", "snap_enabled", "snap_to_grid"] { assert!(saved.get(key).is_none(), "persisted legacy key {key}"); }
    assert_eq!(saved, expected);
    assert_eq!(SnapPrefs::load(SnapPrefs::default_path()), global);
}

#[test]
fn actual_smart_guide_feedback_is_ephemeral_and_absent_from_native_save() {
    if !isolated_legacy_process("actual_smart_guide_feedback_is_ephemeral_and_absent_from_native_save") { return; }
    let (mut workspace, id, global) = legacy_native_fixture();
    let session = &workspace.get(id).unwrap().session;
    let before = serde_json::to_value(&session.document).unwrap();
    let source = SnapDocument { document: &session.document, guides: &session.guides, grid: &Grid::default() };
    let feedback = smart_guides(&source, &session.view, &global, [100.0, 110.0], None);
    assert!(!feedback.is_empty());
    assert_eq!(serde_json::to_value(&session.document).unwrap(), before);
    let saved = saved_native(&mut workspace, id);
    for key in ["snap_feedback", "snap_overlay", "overlay"] { assert!(saved.get(key).is_none(), "persisted feedback {key}"); }
    assert_eq!(saved, before);
}
