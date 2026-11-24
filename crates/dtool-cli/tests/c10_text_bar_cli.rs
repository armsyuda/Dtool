use dtool_core::workspace::Workspace;
use serde_json::{json, Value};

fn run(shared: &dtool_core::workspace::SharedWorkspace, name: &str, params: Value) -> Value {
    dtool_cli::run_cli_args(shared, &["run".into(), name.into(), params.to_string()]).unwrap()
}

#[test]
fn cli_text_bar_style_query_set_and_undo() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("c10-textbar-cli");
    workspace.activate(id).unwrap();
    let shared = workspace.into_shared();
    let added = run(&shared, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "Hello" }));
    let story = added["story"].as_u64().expect("story");
    run(&shared, "text.style", json!({ "story": story, "range": { "start": 0, "end": 5 }, "patch": { "family": "SourceSans", "size_pt": 30.0, "weight": 700, "italic": true, "tracking": 12.0 } }));
    let reply = run(&shared, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 5 } }));
    assert_eq!(reply["family"], json!("SourceSans"));
    assert_eq!(reply["size_pt"], json!(30.0));
    assert_eq!(reply["weight"], json!(700));
    assert_eq!(reply["italic"], json!(true));
    assert_eq!(reply["tracking"], json!(12.0));
    run(&shared, "edit.undo", json!({}));
    let undone = run(&shared, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 5 } }));
    assert_ne!(undone["family"], json!("SourceSans"));
    assert_ne!(undone["size_pt"], json!(30.0));
}
