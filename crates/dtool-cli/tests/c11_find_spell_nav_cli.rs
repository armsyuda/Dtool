use dtool_core::workspace::Workspace;
use serde_json::{json, Value};

fn run(shared: &dtool_core::workspace::SharedWorkspace, name: &str, params: Value) -> Value {
    dtool_cli::run_cli_args(shared, &["run".into(), name.into(), params.to_string()]).unwrap()
}

#[test]
fn cli_replace_all_cat_to_dog_and_spell_teh_undo() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("c11-find-spell-cli");
    workspace.activate(id).unwrap();
    let shared = workspace.into_shared();
    let added = run(&shared, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "cat cat cat cat and teh" }));
    let story = added["story"].as_u64().expect("story");
    let found = run(&shared, "text.find", json!({ "pattern": "cat", "scope": "document" }));
    assert_eq!(found["match"]["start"], json!(0));
    let replaced = run(&shared, "text.replace_all", json!({ "pattern": "cat", "replacement": "dog", "scope": "document" }));
    assert_eq!(replaced["replaced"], json!(4), "{replaced}");
    run(&shared, "edit.undo", json!({}));
    let restored = run(&shared, "text.find", json!({ "pattern": "cat", "scope": "document" }));
    assert_eq!(restored["match"]["start"], json!(0), "{restored}");
    let check = run(&shared, "text.spell.check", json!({ "story": story }));
    assert!(check["marks"].as_array().unwrap().iter().any(|mark| mark["word"] == json!("teh")), "{check}");
}
