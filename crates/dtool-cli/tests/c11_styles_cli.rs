use dtool_core::workspace::Workspace;
use serde_json::{json, Value};

fn run(shared: &dtool_core::workspace::SharedWorkspace, name: &str, params: Value) -> Value {
    dtool_cli::run_cli_args(shared, &["run".into(), name.into(), params.to_string()]).unwrap()
}

#[test]
fn cli_define_apply_clear_and_undo() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("c11-styles-cli");
    workspace.activate(id).unwrap();
    let shared = workspace.into_shared();
    let added = run(&shared, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "Hello" }));
    let story = added["story"].as_u64().expect("story");
    run(&shared, "text.style.define", json!({ "id": "Emph", "kind": "character", "props": { "italic": true } }));
    run(&shared, "text.style.apply_named", json!({ "story": story, "id": "Emph", "kind": "character", "range": { "start": 0, "end": 5 }, "keep_overrides": false }));
    let listed = run(&shared, "text.styles.list", json!({ "kind": "character" }));
    assert!(listed["styles"].as_array().unwrap().iter().any(|style| style["name"] == json!("Emph")), "{listed}");
    run(&shared, "edit.undo", json!({}));
    let query = run(&shared, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 5 } }));
    assert_ne!(query["italic"], json!(true), "{query}");
}
