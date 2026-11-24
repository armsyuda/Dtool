use dtool_core::object::ObjectId;
use dtool_core::workspace::{Scope, SharedServices, Workspace};
use dtool_protocol::Command;
use serde_json::{json, Value};
use std::sync::Arc;

fn run(ws: &mut Workspace, id: dtool_core::workspace::ContextId, name: &str, params: Value) -> Result<Value, String> {
    ws.run(Scope::of(id), Command::with_params(name, params)).1.map_err(|error| error.code)
}

#[test]
fn unnamed_objects_store_an_empty_name_for_the_string_keyed_default() {
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let doc = ws.new_document("names");
    let unnamed = run(&mut ws, doc, "object.create", json!({"pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    let unnamed = unnamed["id"].as_u64().or_else(|| unnamed["object_id"].as_u64()).unwrap_or_else(|| ws.get(doc).unwrap().session.document.objects[0].id.0);
    let named = run(&mut ws, doc, "object.create", json!({"name": "Box", "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    let named = named["id"].as_u64().or_else(|| named["object_id"].as_u64()).unwrap_or_else(|| ws.get(doc).unwrap().session.document.objects[1].id.0);
    let document = &ws.get(doc).unwrap().session.document;
    assert_eq!(document.get_object(ObjectId(unnamed)).unwrap().name, "");
    assert!(dtool_core::commands::layer_meta::has_default_name(&document.get_object(ObjectId(unnamed)).unwrap().name));
    assert_eq!(document.get_object(ObjectId(named)).unwrap().name, "Box");
    assert_eq!(dtool_core::commands::layer_meta::DEFAULT_NAME_KEY, "object.default_name");
}

#[test]
fn layer_tag_colour_and_collapse_live_on_the_document_and_save() {
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let doc = ws.new_document("meta");
    let other = ws.new_document("other");
    run(&mut ws, doc, "object.create", json!({"name": "A", "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    let id = ws.get(doc).unwrap().session.document.objects[0].id.0;
    let undo_before = ws.get(doc).unwrap().session.undo.can_undo();
    let reply = run(&mut ws, doc, "layer.meta.set", json!({"object": id, "tag": "hero", "colour": "#ff0000"})).unwrap();
    assert_eq!(reply["tag"], json!("hero"));
    assert_eq!(reply["colour"], json!("#ff0000"));
    let entry = dtool_core::commands::layer_meta::meta(&ws.get(doc).unwrap().session.document, ObjectId(id)).unwrap().clone();
    assert_eq!((entry.tag.as_deref(), entry.colour.as_deref(), entry.collapsed), (Some("hero"), Some("#ff0000"), false));
    assert!(ws.get(other).unwrap().session.document.layer_meta.is_empty());

    run(&mut ws, doc, "edit.undo", json!({})).unwrap();
    assert!(ws.get(doc).unwrap().session.document.layer_meta.is_empty());
    assert_eq!(ws.get(doc).unwrap().session.undo.can_undo(), undo_before);
    run(&mut ws, doc, "edit.redo", json!({})).unwrap();
    assert_eq!(ws.get(doc).unwrap().session.document.layer_meta.len(), 1);

    let before = ws.get(doc).unwrap().session.undo.applied_steps();
    let collapsed = run(&mut ws, doc, "layer.meta.set", json!({"object": id, "collapsed": true})).unwrap();
    assert_eq!(collapsed["changes"], json!([]));
    assert_eq!(ws.get(doc).unwrap().session.undo.applied_steps(), before);
    assert!(ws.get(doc).unwrap().session.document.layer_meta[0].collapsed);

    assert_eq!(run(&mut ws, doc, "layer.meta.set", json!({"object": id, "colour": "red"})), Err("INVALID_PARAMS".into()));
    assert_eq!(run(&mut ws, doc, "layer.meta.set", json!({"object": id})), Err("INVALID_PARAMS".into()));
    assert_eq!(run(&mut ws, doc, "layer.meta.set", json!({"object": 999, "tag": "x"})), Err("NOT_FOUND".into()));

    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c06c_meta_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("meta.dtool");
    run(&mut ws, doc, "doc.save_as", json!({"path": path})).unwrap();
    let opened = ws.open_path(&path).unwrap();
    let reopened = ws.get(opened).unwrap().session.document.layer_meta.clone();
    assert_eq!(reopened, ws.get(doc).unwrap().session.document.layer_meta);
    assert!(reopened[0].collapsed);

    let mut legacy: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("layer_meta");
    let legacy_path = dir.join("legacy.dtool");
    std::fs::write(&legacy_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let legacy_id = ws.open_path(&legacy_path).unwrap();
    assert!(ws.get(legacy_id).unwrap().session.document.layer_meta.is_empty());

    run(&mut ws, doc, "layer.meta.set", json!({"object": id, "tag": null, "colour": null, "collapsed": false})).unwrap();
    assert!(ws.get(doc).unwrap().session.document.layer_meta.is_empty());
}