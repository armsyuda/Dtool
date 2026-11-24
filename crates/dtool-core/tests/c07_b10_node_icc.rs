use dtool_core::colour::RenderingIntent;
use dtool_core::document::Document;
use dtool_core::object::{Object, ObjectId};
use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::{Command, CommandResult};
use serde_json::{json, Value};

fn fresh() -> (Workspace, ObjectId) {
    let mut workspace = Workspace::default();
    workspace.new_document("c07_b10_icc");
    let id = workspace.session_mut().unwrap().document.create_object(Object { name: "box".into(), size: [10.0, 10.0], ..Object::default() });
    (workspace, id)
}

fn call(workspace: &mut Workspace, name: &str, params: Value) -> CommandResult {
    workspace.run(Scope::active(), Command::with_params(name, params)).1
}

fn object(workspace: &Workspace, id: ObjectId) -> Object {
    workspace.session().unwrap().document.get_object(id).unwrap().clone()
}

#[test]
fn c07_b10_node_icc_set_is_dispatched_and_undoable() {
    let (mut workspace, id) = fresh();
    let reply = call(&mut workspace, "node.icc.set", json!({ "id": id.0, "profile": "srgb", "intent": "Saturation" })).expect("node.icc.set on the live path");
    assert_eq!(reply["id"], id.0);
    let icc = object(&workspace, id).icc.expect("icc stored");
    assert_eq!(icc.intent, Some(RenderingIntent::Saturation));
    assert!(icc.profile.as_deref().is_some_and(|name| name.to_lowercase().contains("srgb")), "{icc:?}");
    assert_eq!(icc.effective_intent(RenderingIntent::RelativeColorimetric), RenderingIntent::Saturation);
    call(&mut workspace, "edit.undo", json!({})).unwrap();
    let undone = object(&workspace, id).icc;
    assert!(undone.as_ref().map_or(true, |icc| icc.profile.is_none() && icc.intent.is_none()), "{undone:?}");
    assert_eq!(undone.map(|icc| icc.effective_intent(RenderingIntent::Perceptual)).unwrap_or(RenderingIntent::Perceptual), RenderingIntent::Perceptual);
    call(&mut workspace, "edit.redo", json!({})).unwrap();
    assert_eq!(object(&workspace, id).icc.and_then(|icc| icc.intent), Some(RenderingIntent::Saturation));
}

#[test]
fn c07_b10_node_icc_intent_only_keeps_profile_and_null_clears() {
    let (mut workspace, id) = fresh();
    call(&mut workspace, "node.icc.set", json!({ "id": format!("node:{}", id.0), "profile": "srgb" })).unwrap();
    call(&mut workspace, "node.icc.set", json!({ "id": id.0, "intent": "Perceptual" })).unwrap();
    let icc = object(&workspace, id).icc.unwrap();
    assert!(icc.profile.is_some());
    assert_eq!(icc.intent, Some(RenderingIntent::Perceptual));
    call(&mut workspace, "node.icc.set", json!({ "id": id.0, "intent": null })).unwrap();
    let icc = object(&workspace, id).icc.unwrap();
    assert_eq!(icc.intent, None);
    assert_eq!(icc.effective_intent(RenderingIntent::AbsoluteColorimetric), RenderingIntent::AbsoluteColorimetric);
    call(&mut workspace, "edit.undo", json!({})).unwrap();
    assert_eq!(object(&workspace, id).icc.unwrap().intent, Some(RenderingIntent::Perceptual));
}

#[test]
fn c07_b10_node_icc_rejects_bad_input_without_side_effects() {
    let (mut workspace, id) = fresh();
    let version = workspace.session().unwrap().document.version;
    assert_eq!(call(&mut workspace, "node.icc.set", json!({ "id": id.0, "profile": "nope" })).unwrap_err().code, "PROFILE_NOT_FOUND");
    assert_eq!(call(&mut workspace, "node.icc.set", json!({ "id": id.0, "intent": "Weird" })).unwrap_err().code, "UNSUPPORTED_INTENT");
    assert!(call(&mut workspace, "node.icc.set", json!({ "id": id.0 })).is_err());
    assert!(call(&mut workspace, "node.icc.set", json!({ "id": 9999, "intent": "Saturation" })).is_err());
    assert!(object(&workspace, id).icc.is_none());
    assert_eq!(workspace.session().unwrap().document.version, version);
}

#[test]
fn c07_b10_node_icc_survives_document_round_trip() {
    let (mut workspace, id) = fresh();
    call(&mut workspace, "node.icc.set", json!({ "id": id.0, "intent": "AbsoluteColorimetric" })).unwrap();
    let text = serde_json::to_string(&workspace.session().unwrap().document).unwrap();
    let loaded: Document = serde_json::from_str(&text).unwrap();
    assert_eq!(loaded.get_object(id).unwrap().icc.as_ref().and_then(|icc| icc.intent), Some(RenderingIntent::AbsoluteColorimetric));
}