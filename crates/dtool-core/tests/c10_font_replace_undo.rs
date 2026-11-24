use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_core::{Command, CommandResult};
use serde_json::{json, Value};

fn run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> CommandResult {
    ws.run(Scope::of(id), Command::with_params(name, params)).1
}

fn families(ws: &Workspace, id: ContextId) -> Vec<String> {
    ws.get(id).unwrap().session.document.stories.iter().flat_map(|s| s.ranges.iter()).filter_map(|r| r.get("family").and_then(Value::as_str).map(str::to_owned)).collect()
}

#[test]
fn c10_font_replace_undo_and_redo_on_live_document() {
    let mut ws = Workspace::default();
    let id = ws.new_document("fonts");
    let added = run(&mut ws, id, "text.art.add", json!({ "pos": [10.0, 20.0], "text": "Hello", "size_pt": 24.0 })).unwrap();
    assert!(added["id"].is_u64(), "{added}");
    {
        let story = &mut ws.get_mut(id).unwrap().session.document.stories[0];
        if story.ranges.is_empty() {
            story.ranges.push(json!({ "start": 0, "end": 5, "size_pt": 24.0 }));
        }
        for range in story.ranges.iter_mut() {
            range["family"] = "Inter".into();
        }
    }
    let before = ws.get(id).unwrap().session.document.stories.clone();
    let replaced = run(&mut ws, id, "font.replace", json!({ "from_family": "Inter", "from_style": "", "to_family": "Pretendard", "to_style": "" })).unwrap();
    assert!(replaced["doc_version"].is_u64(), "{replaced}");
    assert!(families(&ws, id).iter().all(|f| f == "Pretendard"), "{:?}", families(&ws, id));

    run(&mut ws, id, "edit.undo", json!({})).unwrap();
    assert_eq!(ws.get(id).unwrap().session.document.stories, before);
    assert!(families(&ws, id).iter().all(|f| f == "Inter"));

    run(&mut ws, id, "edit.redo", json!({})).unwrap();
    assert!(families(&ws, id).iter().all(|f| f == "Pretendard"));

    let mut bad = Command::new("font.replace.undo");
    bad.params["snapshot"] = json!("nope");
    assert_eq!(dtool_core::text_cmd::apply(&bad, &mut ws.get_mut(id).unwrap().session.document).unwrap_err().code, "INVALID_PARAMS");
}