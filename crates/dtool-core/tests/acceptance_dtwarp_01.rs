use dtool_core::document::Units;
use dtool_core::object::ObjectId;
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_protocol::Command;
use serde_json::{json, Value};

fn new_workspace() -> (Workspace, ContextId) {
    let mut workspace = Workspace::default();
    let (target, result) = workspace.run(
        Scope::active(),
        Command::with_params(
            "doc.new",
            json!({"width": 400.0, "height": 200.0, "units": Units::Px, "dpi": 72.0, "pages": 1}),
        ),
    );
    let reply = result.expect("doc.new");
    let context = target
        .or_else(|| reply["context_id"].as_u64().map(ContextId))
        .expect("context");
    (workspace, context)
}

fn run(workspace: &mut Workspace, context: ContextId, name: &str, params: Value) -> Value {
    let (_, result) = workspace.run(Scope::of(context), Command::with_params(name, params));
    result.unwrap_or_else(|err| panic!("{name} failed: {} {}", err.code, err.message_key))
}

#[test]
fn live_mesh_edit_is_one_undo_and_cancel_drag_skips_undo() {
    let (mut workspace, context) = new_workspace();
    run(
        &mut workspace,
        context,
        "object.create",
        json!({"name": "child", "pos": [0.0, 0.0], "size": [100.0, 50.0]}),
    );
    let child = workspace.get(context).unwrap().session.document.objects.last().unwrap().id.0;
    run(&mut workspace, context, "warp.group.create", json!({ "nodes": [child] }));
    let before = run(&mut workspace, context, "warp.mesh.query", json!({}));
    let a = before["mesh"]["points"][0]["id"].as_u64().unwrap();
    let b = before["mesh"]["points"][1]["id"].as_u64().unwrap();
    let cols = before["mesh"]["cols"].as_u64().unwrap();
    let generation = workspace.get(context).unwrap().session.generation;
    let undo_before = workspace.get(context).unwrap().session.undo.applied_steps();

    let reply = run(
        &mut workspace,
        context,
        "warp.mesh.edit",
        json!({ "ops": [{ "add_junction": { "a": a, "b": b } }] }),
    );
    assert_eq!(reply["applied_ops"], json!(1));
    assert_eq!(workspace.get(context).unwrap().session.generation, generation + 1);
    let after = run(&mut workspace, context, "warp.mesh.query", json!({}));
    assert_eq!(after["mesh"]["cols"].as_u64().unwrap(), cols + 1);
    run(&mut workspace, context, "edit.undo", json!({}));
    let restored = run(&mut workspace, context, "warp.mesh.query", json!({}));
    assert_eq!(restored["mesh"]["cols"], before["mesh"]["cols"]);
    assert_eq!(workspace.get(context).unwrap().session.undo.applied_steps(), undo_before);

    let point = restored["mesh"]["points"][0]["id"].as_u64().unwrap();
    let drag_gen = workspace.get(context).unwrap().session.generation;
    run(
        &mut workspace,
        context,
        "warp.mesh.edit",
        json!({ "ops": [{ "point": point, "pos": [-10.0, -10.0] }], "commit": false }),
    );
    assert_eq!(workspace.get(context).unwrap().session.generation, drag_gen);
    run(
        &mut workspace,
        context,
        "warp.mesh.edit",
        json!({ "cancel_drag": true }),
    );
    assert_eq!(workspace.get(context).unwrap().session.undo.applied_steps(), undo_before);
    let cancelled = run(&mut workspace, context, "warp.mesh.query", json!({}));
    assert_eq!(cancelled["mesh"]["points"][0]["id"].as_u64().unwrap(), point);
    let _ = ObjectId(child);
}
