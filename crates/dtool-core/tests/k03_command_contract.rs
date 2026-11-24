use dtool_core::document::{Document, Units};
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Session;
use dtool_protocol::{Command, CommandCatalog};
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1))
}

fn run(s: &mut Session, name: &str, params: serde_json::Value) -> dtool_protocol::CommandResult {
    s.run(&Command::with_params(name, params))
}

fn create(s: &mut Session) -> u64 {
    run(s, "object.create", json!({"pos": [1.0, 1.0], "size": [5.0, 5.0]})).unwrap();
    s.document.objects.last().unwrap().id.0
}

#[test]
fn doc_query_command_returns_real_snapshot_and_version() {
    let mut s = session();
    create(&mut s);
    let value = run(&mut s, "doc.query", json!({})).unwrap();
    assert_eq!(value["doc_version"], json!(s.generation));
    assert_eq!(value["snapshot"]["objects"].as_array().unwrap().len(), 1);
    let direct = s.query(None).unwrap();
    assert_eq!(direct["snapshot"]["objects"].as_array().unwrap().len(), 1, "Session::query is not an empty stub");
}

#[test]
fn doc_query_since_last_version_is_empty_and_since_older_lists_changes() {
    let mut s = session();
    create(&mut s);
    let mid = s.generation;
    create(&mut s);
    let last = s.generation;
    let value = run(&mut s, "doc.query", json!({"since": last})).unwrap();
    assert_eq!(value["doc_version"], json!(last));
    assert!(value["changes"].as_array().unwrap().is_empty());
    let value = run(&mut s, "doc.query", json!({"since": mid})).unwrap();
    assert_eq!(value["changes"], json!([{"name": "object.create"}]));
    assert_eq!(s.generation, last, "query does not mutate");
}

#[test]
fn doc_query_stale_and_future_since_are_invalid_without_mutation() {
    let mut s = session();
    s.generation = 5;
    create(&mut s);
    let generation = s.generation;
    let objects = s.document.objects.len();
    for since in [3, generation + 1] {
        let err = run(&mut s, "doc.query", json!({"since": since})).unwrap_err();
        assert_eq!(err.code, "INVALID_PARAMS", "since {since}");
        assert_eq!(s.generation, generation);
        assert_eq!(s.document.objects.len(), objects);
    }
}

#[test]
fn doc_query_has_one_meaning_and_object_inspect_moved() {
    let mut s = session();
    let id = create(&mut s);
    assert_eq!(run(&mut s, "doc.query", json!({"id": id})).unwrap_err().code, "INVALID_PARAMS");
    let value = run(&mut s, "doc.object.query", json!({"id": id})).unwrap();
    assert!(value.is_object());
}

#[test]
fn batch_with_invalid_last_target_changes_nothing() {
    let mut s = session();
    let existing = create(&mut s);
    run(&mut s, "selection.set", json!({"ids": [existing]})).unwrap();
    let generation = s.generation;
    let selection = s.selection.clone();
    let objects = s.document.objects.len();
    let err = run(&mut s, "edit.batch", json!({"commands": [
        {"name": "object.create", "params": {"pos": [0.0, 0.0], "size": [1.0, 1.0]}},
        {"name": "transform.set", "params": {"ids": [9999], "pos": [3.0, 3.0]}}
    ]})).unwrap_err();
    assert_eq!(err.code, "BATCH_PREFLIGHT_FAILED");
    assert_eq!(s.document.objects.len(), objects);
    assert_eq!(s.generation, generation);
    assert_eq!(s.selection, selection);
}

#[test]
fn batch_mid_apply_failure_restores_selection_and_guides() {
    let mut s = session();
    let a = create(&mut s);
    let generation = s.generation;
    let selection = s.selection.clone();
    let guides = s.guides.clone();
    let result = run(&mut s, "edit.batch", json!({"commands": [
        {"name": "selection.set", "params": {"ids": [a]}},
        {"name": "guides.remove", "params": {"id": 424242}}
    ]}));
    assert!(result.is_err());
    assert_ne!(result.unwrap_err().code, "BATCH_PREFLIGHT_FAILED", "apply-time errors keep their own code");
    assert_eq!(s.selection, selection);
    assert_eq!(s.guides, guides);
    assert_eq!(s.generation, generation);
}

#[test]
fn successful_batch_is_one_undo_step_and_one_generation_bump() {
    let mut s = session();
    let generation = s.generation;
    run(&mut s, "edit.batch", json!({"commands": [
        {"name": "object.create", "params": {"pos": [0.0, 0.0], "size": [1.0, 1.0]}},
        {"name": "object.create", "params": {"pos": [2.0, 2.0], "size": [1.0, 1.0]}}
    ]})).unwrap();
    assert_eq!(s.generation, generation + 1);
    assert_eq!(s.document.objects.len(), 2);
    run(&mut s, "edit.undo", json!({})).unwrap();
    assert_eq!(s.document.objects.len(), 0);
}

#[test]
fn catalogued_without_session_handler_is_not_implemented_and_unknown_is_unknown() {
    let mut s = session();
    assert_eq!(run(&mut s, "doc.save", json!({})).unwrap_err().code, "NOT_IMPLEMENTED");
    assert_eq!(run(&mut s, "no.such.command", json!({})).unwrap_err().code, "UNKNOWN_COMMAND");
}

#[test]
fn can_execute_agrees_with_run_for_listing_and_noop() {
    let mut s = session();
    for name in ["commands.list", "noop", "command.noop"] {
        assert!(s.can_execute(&Command::with_params(name, json!({}))), "{name}");
        assert!(run(&mut s, name, json!({})).is_ok(), "{name}");
    }
}

#[test]
fn catalog_is_truthful_and_resolves_aliases() {
    let catalog = CommandCatalog::new();
    assert!(catalog.get("doc.query").unwrap().implemented);
    assert!(catalog.get("doc.query").unwrap().input_schema.is_some());
    assert_eq!(catalog.resolve("document.setup.transparent_background").unwrap().name, "doc.setup.transparent_background");
    assert!(catalog.resolve("no.such.command").is_none());
    for name in ["object.delete", "view.zoom.set", "view.zoom.fit", "selection.save", "selection.load", "doc.new", "doc.open", "doc.save", "doc.save_as", "doc.object.query"] {
        assert!(catalog.contains(name), "{name}");
    }
    let mut ws = Workspace::default();
    ws.new_document("planned");
    for spec in catalog.all().iter().filter(|spec| !spec.implemented) {
        let (_, result) = ws.run(Scope::active(), Command::with_params(spec.name, json!({})));
        assert_eq!(result.unwrap_err().code, "NOT_IMPLEMENTED", "{}", spec.name);
    }
}

#[test]
fn workspace_results_carry_outcome_fields() {
    let mut ws = Workspace::default();
    let id = ws.new_document("a");
    let (ctx, result) = ws.run(Scope::active(), Command::with_params("selection.all", json!({})));
    let value = result.unwrap();
    assert_eq!(ctx, Some(id));
    assert!(value["doc_version"].is_u64(), "{value}");
    assert!(value["changes"].is_array(), "{value}");
    assert_eq!(value["context_id"], json!(id.0));
    let (_, result) = ws.run(Scope::active(), Command::with_params("commands.list", json!({})));
    let value = result.unwrap();
    assert!(value["commands"].is_array());
    assert_eq!(value["context_id"], json!(id.0));
}
