use dtool_core::object::Object;
use dtool_core::paint::{Colour, Paint};
use dtool_core::view_state::ColourTarget;
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_protocol::Command;
use serde_json::{json, Value};

fn run(workspace: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    let (_, result) = workspace.run(Scope { context: Some(id) }, Command::with_params(name, params));
    result.unwrap()
}

fn target(workspace: &Workspace, id: ContextId) -> ColourTarget {
    workspace.get(id).unwrap().session.view.colour_target.target
}

fn two_views() -> (Workspace, ContextId, ContextId) {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let b = workspace.new_document("b");
    for id in [a, b] {
        let session = &mut workspace.get_mut(id).unwrap().session;
        session.document.create_object(Object { name: "red".into(), size: [20.0, 20.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Object::default() });
    }
    (workspace, a, b)
}

#[test]
fn c07_fix22_each_view_holds_its_own_target() {
    let (mut workspace, a, b) = two_views();
    assert_eq!(target(&workspace, a), ColourTarget::Fill);
    assert_eq!(target(&workspace, b), ColourTarget::Fill);
    run(&mut workspace, a, "ui.color.target.set", json!({ "target": "stroke" }));
    assert_eq!(target(&workspace, a), ColourTarget::Stroke);
    assert_eq!(target(&workspace, b), ColourTarget::Fill);
    assert_eq!(run(&mut workspace, a, "ui.color.target.query", json!({}))["target"], "stroke");
    assert_eq!(run(&mut workspace, b, "ui.color.target.query", json!({}))["target"], "fill");
}

#[test]
fn c07_fix22_override_is_per_call_and_apply_false_keeps_document() {
    let (mut workspace, a, b) = two_views();
    run(&mut workspace, a, "ui.color.target.set", json!({ "target": "stroke" }));
    let version = workspace.get(a).unwrap().session.document.version;
    let reply = run(&mut workspace, a, "color.pick", json!({ "x": 5.0, "y": 5.0, "target": "fill", "apply": false }));
    assert_eq!(reply["target"]["target"], "fill");
    assert_eq!(target(&workspace, a), ColourTarget::Stroke);
    assert_eq!(workspace.get(a).unwrap().session.document.version, version);
    let reply = run(&mut workspace, b, "color.pick", json!({ "x": 5.0, "y": 5.0, "apply": false }));
    assert_eq!(reply["target"]["target"], "fill");
    assert_eq!(target(&workspace, b), ColourTarget::Fill);
}