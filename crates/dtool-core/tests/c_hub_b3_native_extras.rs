#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::workspace::{CloseAnswer, ContextId, Scope, Workspace};
use dtool_core::Command;
use serde_json::{json, Value};

fn run(ws: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    ws.run(Scope::of(id), Command::with_params(name, params)).1.unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn guide_ids(ws: &Workspace, id: ContextId) -> Vec<u64> {
    ws.get(id).unwrap().session.guides.items.iter().map(|g| g.id.0).collect()
}

#[test]
fn guides_survive_native_save_and_open() {
    let home = test_home::enter("c_hub_b3_guides");
    let path = home.root.join("guides.dtool");
    let mut ws = Workspace::default();
    let id = ws.new_document("Guides");
    run(&mut ws, id, "guides.add", json!({ "kind": "Horizontal", "position": 40.0 }));
    run(&mut ws, id, "guides.add", json!({ "kind": "Vertical", "position": 75.5, "name": "edge" }));
    let saved = ws.get(id).unwrap().session.guides.clone();
    run(&mut ws, id, "doc.save_as", json!({ "path": path }));
    ws.close_with(id, CloseAnswer::No).unwrap();
    let opened = ws.open_path(&path).unwrap();
    let restored = ws.get(opened).unwrap().session.guides.clone();
    assert_eq!(restored.items, saved.items);
    assert!(ws.native_extras(opened).map_or(true, |extras| extras.get("guides").is_none()));
    let max = guide_ids(&ws, opened).into_iter().max().unwrap();
    run(&mut ws, opened, "guides.add", json!({ "kind": "Horizontal", "position": 1.0 }));
    assert!(guide_ids(&ws, opened).into_iter().max().unwrap() > max);
    assert!(!ws.get(opened).unwrap().is_modified() || ws.get(opened).unwrap().session.guides.items.len() == 3);
}

#[test]
fn file_without_guides_opens_with_default_guides() {
    let home = test_home::enter("c_hub_b3_noguides");
    let path = home.root.join("plain.dtool");
    std::fs::write(&path, serde_json::to_string(&dtool_core::document::Document::default()).unwrap()).unwrap();
    let mut ws = Workspace::default();
    let id = ws.open_path(&path).unwrap();
    assert!(ws.get(id).unwrap().session.guides.items.is_empty());
}

#[test]
fn save_writes_history_sidecar_only_when_flag_is_on() {
    let home = test_home::enter("c_hub_b3_history");
    let on = home.root.join("on.dtool");
    let off = home.root.join("off.dtool");
    let mut ws = Workspace::default();
    let a = ws.new_document("On");
    run(&mut ws, a, "history.save_with_doc.set", json!({ "enabled": true }));
    for name in ["a", "b"] { run(&mut ws, a, "object.create", json!({ "name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0] })); }
    run(&mut ws, a, "doc.save_as", json!({ "path": on }));
    assert!(dtool_core::history_store::sidecar_path(&on).is_file());
    ws.close_with(a, CloseAnswer::No).unwrap();
    let reopened = ws.run(Scope::active(), Command::with_params("doc.open", json!({ "path": on }))).0.unwrap();
    let listed = run(&mut ws, reopened, "history.list", json!({}));
    assert!(listed.to_string().matches("history.object.create").count() >= 2, "{listed}");
    run(&mut ws, reopened, "edit.undo", json!({}));
    assert_eq!(ws.get(reopened).unwrap().session.document.objects.len(), 1);

    let b = ws.new_document("Off");
    run(&mut ws, b, "object.create", json!({ "name": "c", "pos": [0.0, 0.0], "size": [10.0, 10.0] }));
    run(&mut ws, b, "doc.save_as", json!({ "path": off }));
    assert!(!dtool_core::history_store::sidecar_path(&off).exists());
}