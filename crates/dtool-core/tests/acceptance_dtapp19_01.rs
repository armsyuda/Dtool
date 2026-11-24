use dtool_core::curve::{Contour, Curve, PathPoint, PointId};
use dtool_core::document::Units;
use dtool_core::object::{Object, ObjectKind};
use dtool_core::perspective::{self, has_pixel_layer, PerspectiveSpec};
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_protocol::{Command, CommandError};
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

fn curve_object() -> Object {
    let contour = Contour {
        points: [[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]]
            .iter()
            .enumerate()
            .map(|(i, pos)| PathPoint::sharp(PointId(i as u64 + 1), *pos))
            .collect(),
        closed: true,
    };
    let mut object = Object::default();
    object.kind = ObjectKind::Curve(Curve {
        contour,
        holes: Vec::new(),
        source_object: None,
    });
    object.pos = [0.0, 0.0];
    object.size = [40.0, 20.0];
    object
}

fn run_perspective(workspace: &mut Workspace, context: ContextId, name: &str, params: Value) -> Value {
    let command = Command::with_params(name, params);
    let (_, result) = workspace.run(Scope::of(context), command.clone());
    match result {
        Ok(value) => value,
        Err(err) if err.code == "UNKNOWN_COMMAND" => perspective::run(
            &mut workspace.get_mut(context).expect("context").session,
            &command,
        )
        .expect("perspective helper")
        .expect("perspective ok"),
        Err(err) => panic!("{name} failed: {} {}", err.code, err.message_key),
    }
}

fn corner(workspace: &Workspace, context: ContextId, id: dtool_core::object::ObjectId, index: usize) -> [f64; 2] {
    workspace
        .get(context)
        .unwrap()
        .session
        .document
        .get_object(id)
        .unwrap()
        .curve()
        .unwrap()
        .contour
        .points[index]
        .pos
}

fn close(a: [f64; 2], b: [f64; 2]) {
    assert!((a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4, "{a:?} != {b:?}");
}

#[test]
fn perspective_preview_apply_cancel() {
    let (mut workspace, context) = new_workspace();
    let id = workspace
        .get_mut(context)
        .unwrap()
        .session
        .document
        .create_object(curve_object());
    let params = json!({
        "planes": 1,
        "source": [[[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]]],
        "target": [[[0.0, 0.0], [40.0, 0.0], [36.0, 20.0], [4.0, 20.0]]]
    });
    let before = corner(&workspace, context, id, 2);
    close(before, [40.0, 20.0]);
    assert!(!has_pixel_layer(&workspace.get(context).unwrap().session.document));
    let undo_before = workspace.get(context).unwrap().session.undo.applied_steps();

    run_perspective(&mut workspace, context, "perspective.preview", params.clone());
    assert!(!has_pixel_layer(&workspace.get(context).unwrap().session.document));
    let previewed = corner(&workspace, context, id, 2);
    close(previewed, [36.0, 20.0]);
    assert_eq!(workspace.get(context).unwrap().session.undo.applied_steps(), undo_before);

    let tighter = json!({
        "planes": 1,
        "source": [[[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]]],
        "target": [[[0.0, 0.0], [40.0, 0.0], [30.0, 20.0], [8.0, 20.0]]]
    });
    run_perspective(&mut workspace, context, "perspective.preview", tighter);
    close(corner(&workspace, context, id, 2), [30.0, 20.0]);

    let cancelled = run_perspective(&mut workspace, context, "perspective.cancel", json!({}));
    assert_eq!(cancelled["cancelled"], true);
    close(corner(&workspace, context, id, 2), [40.0, 20.0]);
    assert_eq!(workspace.get(context).unwrap().session.undo.applied_steps(), undo_before);

    let applied = run_perspective(&mut workspace, context, "perspective.apply", params);
    assert_eq!(applied["applied"], true);
    close(corner(&workspace, context, id, 2), [36.0, 20.0]);
    assert_eq!(workspace.get(context).unwrap().session.undo.applied_steps(), undo_before + 1);
    let (_, undo) = workspace.run(Scope::of(context), Command::new("edit.undo"));
    undo.expect("undo apply");
    close(corner(&workspace, context, id, 2), [40.0, 20.0]);
    assert!(!has_pixel_layer(&workspace.get(context).unwrap().session.document));
}

#[test]
fn perspective_workspace_route_reaches_the_handler() {
    let (mut workspace, context) = new_workspace();
    let (_, result) = workspace.run(Scope::of(context), Command::new("perspective.preview"));
    match result {
        Ok(_) => {}
        Err(CommandError { code, .. }) => assert_eq!(code, "INVALID_PARAMS"),
    }
}
