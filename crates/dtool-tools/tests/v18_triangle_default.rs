use dtool_core::command::Session;
use dtool_core::commands::shape::DEFAULT_TRIANGLE_POINTS;
use dtool_core::shape::ShapeKind;
use dtool_core::{Command, Document, ObjectId};
use dtool_tools::shape_tool::{param_handles, shape_kind_key, ShapeParamKind, ShapeTool};

fn add(pos: [f32; 2], size: [f32; 2]) -> Command {
    let mut command = Command::new("shape.add");
    command.params["kind"] = shape_kind_key(ShapeKind::Triangle).into();
    command.params["pos"] = pos.to_vec().into();
    command.params["size"] = size.to_vec().into();
    command
}

fn points(session: &Session, id: u64) -> [f32; 3] {
    session.document.objects.iter().find(|o| o.id == ObjectId(id)).and_then(|o| o.shape.as_ref()).and_then(|r| r.triangle.as_ref()).unwrap().points
}

#[test]
fn shape_add_and_tool_seed_the_same_triangle() {
    let mut session = Session::new(Document::default());
    let by_command = session.run(&add([0.0, 0.0], [40.0, 20.0])).unwrap()["id"].as_u64().unwrap();
    let tool = ShapeTool { kind: ShapeKind::Triangle };
    let by_tool = session.run(&tool.create([50.0, 0.0], [40.0, 20.0])).unwrap()["id"].as_u64().unwrap();
    let by_drag = session.run(&tool.create_with_modifiers([0.0, 50.0], [40.0, 70.0], false, false)).unwrap()["id"].as_u64().unwrap();
    assert_eq!(points(&session, by_command), DEFAULT_TRIANGLE_POINTS);
    assert_eq!(points(&session, by_tool), DEFAULT_TRIANGLE_POINTS);
    assert_eq!(points(&session, by_drag), DEFAULT_TRIANGLE_POINTS);
}

#[test]
fn default_apex_is_top_centre() {
    let mut session = Session::new(Document::default());
    let id = session.run(&add([10.0, 10.0], [40.0, 20.0])).unwrap()["id"].as_u64().unwrap();
    let object = session.document.objects.iter().find(|o| o.id == ObjectId(id)).unwrap();
    let handles = param_handles(object);
    let apex = handles.iter().find(|(kind, _)| *kind == ShapeParamKind::TriangleApex).unwrap().1;
    assert_eq!(apex, [30.0, 10.0]);
}
