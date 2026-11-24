#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_core::{Command, Session};
use dtool_export::macros;
use serde_json::{json, Value};
use std::path::PathBuf;

fn create(name: &str, x: f64) -> Command {
    Command::with_params("object.create", json!({ "name": name, "pos": [x, 10.0], "size": [10.0, 10.0] }))
}

fn mac(session: &mut Session, name: &str, params: Value) -> Result<Value, dtool_core::CommandError> {
    macros::run(session, &Command::with_params(name, params)).expect("macro command owned by macros::run")
}

fn session(ws: &mut Workspace, id: ContextId) -> &mut Session {
    &mut ws.get_mut(id).unwrap().session
}

fn shapes(session: &Session) -> Vec<(String, [f32; 2])> {
    session.document.objects.iter().map(|o| (o.name.clone(), o.pos)).collect()
}

fn doc(session: &Session) -> Value {
    serde_json::to_value(&session.document).unwrap()
}

fn content_of(mut value: Value) -> Value {
    let map = value.as_object_mut().unwrap();
    map.remove("version");
    map.remove("next_object_id");
    value
}

fn content(session: &Session) -> Value {
    content_of(doc(session))
}

fn temp(tag: &str) -> PathBuf {
    let dir = test_home::base().join(format!("c18bus_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn record_on_one_document_then_play_on_another_is_one_undo() {
    let _home = test_home::enter("record_on_one_document_then_play_on_another_is_one_undo");
    let mut ws = Workspace::default();
    let a = ws.new_document("A");
    let b = ws.new_document("B");
    let started = mac(session(&mut ws, a), macros::MACRO_RECORD_START, json!({})).unwrap();
    assert_eq!(started["recording"], true);
    ws.run(Scope::of(a), create("one", 10.0)).1.unwrap();
    ws.run(Scope::of(a), Command::new("view.flags.query")).1.unwrap();
    ws.run(Scope::of(a), create("two", 40.0)).1.unwrap();
    ws.run(Scope::of(b), create("b-only", 90.0)).1.unwrap();
    let stopped = mac(session(&mut ws, a), macros::MACRO_RECORD_STOP, json!({})).unwrap();
    assert_eq!(stopped["recording"], false);
    let steps = stopped["steps"].clone();
    assert_eq!(steps.as_array().unwrap().iter().map(|s| s["command"].as_str().unwrap()).collect::<Vec<_>>(), vec!["object.create", "object.create"]);
    let target = session(&mut ws, b);
    let before = doc(target);
    let depth = target.undo.applied_steps();
    let reply = mac(target, macros::MACRO_PLAY, json!({ "steps": steps })).unwrap();
    assert_eq!(reply["played"], 2);
    assert_eq!(shapes(target)[1..], [("one".to_string(), [10.0, 10.0]), ("two".to_string(), [40.0, 10.0])]);
    assert_eq!(target.undo.applied_steps(), depth + 1);
    let after = doc(target);
    target.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(content(target), content_of(before));
    target.run(&Command::new("edit.redo")).unwrap();
    assert_eq!(content(target), content_of(after));
    let source = session(&mut ws, a);
    let source_before = source.document.objects.len();
    assert_eq!(mac(source, macros::MACRO_PLAY, json!({})).unwrap()["played"], 2);
    assert_eq!(source.document.objects.len(), source_before + 2);
}

#[test]
fn disabled_steps_are_skipped_and_edited_values_are_used() {
    let _home = test_home::enter("disabled_steps_are_skipped_and_edited_values_are_used");
    let mut ws = Workspace::default();
    let a = ws.new_document("A");
    let s = session(&mut ws, a);
    let steps = json!([
        { "command": "object.create", "params": { "name": "kept", "pos": [5.0, 5.0], "size": [10.0, 10.0] }, "enabled": true, "show_name": false },
        { "command": "object.create", "params": { "name": "off", "pos": [50.0, 5.0], "size": [10.0, 10.0] }, "enabled": false, "show_name": false },
        { "command": "object.create", "params": { "name": "edited", "pos": [77.0, 5.0], "size": [10.0, 10.0] } }
    ]);
    let reply = mac(s, macros::MACRO_PLAY, json!({ "steps": steps })).unwrap();
    assert_eq!((reply["played"].as_u64(), reply["skipped"].as_u64()), (Some(2), Some(1)));
    assert_eq!(shapes(s), vec![("kept".to_string(), [5.0, 5.0]), ("edited".to_string(), [77.0, 5.0])]);
}

#[test]
fn failing_step_rolls_back_and_leaves_no_undo_entry() {
    let _home = test_home::enter("failing_step_rolls_back_and_leaves_no_undo_entry");
    let mut ws = Workspace::default();
    let a = ws.new_document("A");
    let s = session(&mut ws, a);
    s.run(&create("base", 0.0)).unwrap();
    let (before, depth, generation) = (doc(s), s.undo.applied_steps(), s.generation);
    let steps = json!([
        { "command": "object.create", "params": { "name": "x", "pos": [1.0, 1.0], "size": [2.0, 2.0] }, "enabled": true, "show_name": false },
        { "command": "object.update", "params": { "id": 999999, "name": "missing" }, "enabled": true, "show_name": false }
    ]);
    assert!(mac(s, macros::MACRO_PLAY, json!({ "steps": steps })).is_err());
    assert_eq!(doc(s), before);
    assert_eq!((s.undo.applied_steps(), s.generation), (depth, generation));
}

#[test]
fn nested_macro_and_non_recordable_steps_are_refused_before_any_change() {
    let _home = test_home::enter("nested_macro_and_non_recordable_steps_are_refused_before_any_change");
    let mut ws = Workspace::default();
    let a = ws.new_document("A");
    let s = session(&mut ws, a);
    let before = doc(s);
    for bad in ["macro.play", "macro.record.start", "view.flags.set", "edit.undo", "no.such.command", "doc.close"] {
        let steps = json!([
            { "command": "object.create", "params": { "name": "x", "pos": [1.0, 1.0], "size": [2.0, 2.0] }, "enabled": true, "show_name": false },
            { "command": bad, "params": {}, "enabled": true, "show_name": false }
        ]);
        let err = mac(s, macros::MACRO_PLAY, json!({ "steps": steps })).unwrap_err();
        assert_eq!(err.code, "MACRO_STEP_NOT_ALLOWED", "{bad}");
        assert_eq!(doc(s), before, "{bad}");
    }
}

#[test]
fn recorder_state_errors_are_real() {
    let _home = test_home::enter("recorder_state_errors_are_real");
    let mut ws = Workspace::default();
    let a = ws.new_document("A");
    let s = session(&mut ws, a);
    assert_eq!(mac(s, macros::MACRO_RECORD_STOP, json!({})).unwrap_err().code, "MACRO_NOT_RECORDING");
    assert_eq!(mac(s, macros::MACRO_PLAY, json!({})).unwrap_err().code, "MACRO_EMPTY");
    mac(s, macros::MACRO_RECORD_START, json!({})).unwrap();
    assert_eq!(macros::can_execute(s, &Command::new(macros::MACRO_PLAY)), Some(false));
    assert_eq!(mac(s, macros::MACRO_PLAY, json!({ "steps": [] })).unwrap_err().code, "MACRO_RECORDING");
    s.run(&create("r", 0.0)).unwrap();
    assert_eq!(s.macro_recorder.steps().len(), 1);
    let reset = mac(s, macros::MACRO_RESET, json!({})).unwrap();
    assert_eq!((reset["recording"].as_bool(), reset["steps"].as_array().map(Vec::len)), (Some(false), Some(0)));
    assert!(macros::run(s, &Command::new("object.create")).is_none());
    assert!(mac(s, macros::MACRO_PLAY, json!({ "name": 5 })).is_err());
}

#[test]
fn library_add_export_import_and_play_by_name_roundtrip() {
    let _home = test_home::enter("library_add_export_import_and_play_by_name_roundtrip");
    let dir = temp("lib");
    let (lib_a, lib_b, file) = (dir.join("a.json"), dir.join("b.json"), dir.join("pair.afmacro"));
    let mut ws = Workspace::default();
    let a = ws.new_document("A");
    let b = ws.new_document("B");
    mac(session(&mut ws, a), macros::MACRO_RECORD_START, json!({})).unwrap();
    ws.run(Scope::of(a), create("p", 3.0)).1.unwrap();
    ws.run(Scope::of(a), create("q", 33.0)).1.unwrap();
    let s = session(&mut ws, a);
    mac(s, macros::MACRO_RECORD_STOP, json!({})).unwrap();
    let added = mac(s, macros::MACRO_LIBRARY_ADD, json!({ "name": "pair", "category": "c18", "library": lib_a })).unwrap();
    assert_eq!(added["steps"], 2);
    mac(s, macros::MACRO_EXPORT, json!({ "name": "pair", "category": "c18", "path": file, "library": lib_a })).unwrap();
    let imported = mac(s, macros::MACRO_IMPORT, json!({ "path": file, "library": lib_b })).unwrap();
    assert_eq!((imported["name"].as_str(), imported["steps"].as_u64()), (Some("pair"), Some(2)));
    let a_lib = dtool_export::macros::MacroLibrary::load_from_file(&lib_a).unwrap();
    let b_lib = dtool_export::macros::MacroLibrary::load_from_file(&lib_b).unwrap();
    assert_eq!(a_lib.get_in("c18", "pair").unwrap().steps, b_lib.get_in("c18", "pair").unwrap().steps);
    let t = session(&mut ws, b);
    let reply = mac(t, macros::MACRO_PLAY, json!({ "name": "pair", "category": "c18", "library": lib_b })).unwrap();
    assert_eq!(reply["played"], 2);
    assert_eq!(shapes(t), vec![("p".to_string(), [3.0, 10.0]), ("q".to_string(), [33.0, 10.0])]);
    assert_eq!(mac(t, macros::MACRO_PLAY, json!({ "name": "missing", "library": lib_b })).unwrap_err().code, "NOT_FOUND");
}

#[test]
fn check_steps_accepts_recordable_commands_and_batch_actions_only() {
    let step = |command: &str| dtool_export::macros::MacroStep { command: command.into(), params: json!({}), enabled: true, show_name: false };
    assert!(macros::check_steps(&[step("object.create"), step("document.resize"), step("document.adjustments")]).is_ok());
    for bad in ["doc.delete_all", "macro.play", "edit.undo", "view.flags.set"] {
        assert!(macros::check_steps(&[step(bad)]).is_err(), "{bad}");
    }
}