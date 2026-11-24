use dtool_core::macro_recorder::is_recordable;
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use serde_json::json;

fn create(name: &str) -> Command {
    Command::with_params("object.create", json!({ "name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0] }))
}

#[test]
fn recorder_captures_successful_recordable_commands_in_its_own_session_only() {
    let mut ws = Workspace::default();
    let a = ws.new_document("A");
    let b = ws.new_document("B");
    ws.get_mut(a).unwrap().session.macro_recorder.start();
    let reply = ws.run(Scope::of(a), create("one")).1.unwrap();
    let id = ws.get(a).unwrap().session.document.objects.last().unwrap().id.0;
    let _ = reply;
    ws.run(Scope::of(a), Command::with_params("object.update", json!({ "id": id, "name": "renamed" }))).1.unwrap();
    ws.run(Scope::of(a), Command::new("view.flags.query")).1.unwrap();
    ws.run(Scope::of(a), Command::new("history.list")).1.unwrap();
    ws.run(Scope::of(a), Command::with_params("view.flags.set", json!({ "clip_to_canvas": true }))).1.unwrap();
    assert!(ws.run(Scope::of(a), Command::with_params("object.update", json!({ "id": 999_999, "name": "x" }))).1.is_err());
    ws.run(Scope::of(a), Command::new("edit.undo")).1.unwrap();
    ws.run(Scope::of(a), Command::new("edit.redo")).1.unwrap();
    ws.run(Scope::of(b), create("other-doc")).1.unwrap();
    let session = &mut ws.get_mut(a).unwrap().session;
    let names: Vec<&str> = session.macro_recorder.steps().iter().map(|s| s.command.as_str()).collect();
    assert_eq!(names, vec!["object.create", "object.update"]);
    assert_eq!(session.macro_recorder.steps()[0].params["name"], "one");
    let frozen = session.macro_recorder.stop();
    assert_eq!(frozen.len(), 2);
    assert!(!session.macro_recorder.is_recording());
    assert!(ws.get(b).unwrap().session.macro_recorder.steps().is_empty());
    ws.run(Scope::of(a), create("after-stop")).1.unwrap();
    let session = &mut ws.get_mut(a).unwrap().session;
    assert_eq!(session.macro_recorder.steps().len(), 2);
    let taken = session.macro_recorder.take();
    assert_eq!(taken, frozen);
    assert!(session.macro_recorder.steps().is_empty());
    session.macro_recorder.start();
    assert!(session.macro_recorder.steps().is_empty());
}

#[test]
fn recordable_predicate_excludes_queries_undo_ui_and_unknown() {
    let catalog = dtool_protocol::CommandCatalog::new();
    assert!(is_recordable(&catalog, "object.create"));
    assert!(is_recordable(&catalog, "object.update"));
    for name in ["edit.undo", "edit.redo", "history.list", "view.flags.query", "view.flags.set", "doc.save", "doc.open", "ui.state.query", "macro.record.start", "text.font.preview", "no.such.command", "pref.set"] {
        assert!(!is_recordable(&catalog, name), "{name}");
    }
}