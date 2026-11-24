use dtool_core::command::Session;
use dtool_core::object::ObjectKind;
use dtool_core::shape::ShapeKind;
use dtool_core::{Command, Document, ObjectId};
use dtool_tools::shape_tool::{shape_kind_key, ShapeTool};

macro_rules! extract_object_id {
    ($payload:expr) => {
        $payload["id"].as_u64().unwrap_or(0)
    };
}

fn make_session() -> Session {
    Session::new(Document::default())
}

fn tool(kind: ShapeKind) -> ShapeTool {
    ShapeTool { kind }
}

fn add(session: &mut Session, kind: ShapeKind, pos: [f32; 2], size: [f32; 2]) -> u64 {
    let out = session.run(&tool(kind).create(pos, size)).expect("shape.add failed");
    extract_object_id!(out)
}

fn id_command(name: &str, id: u64) -> Command {
    let mut command = Command::new(name);
    command.params["id"] = id.into();
    command
}

fn query(session: &mut Session, id: u64) -> Command {
    let _ = session;
    id_command("shape.params.query", id)
}

fn recipe(session: &mut Session, id: u64) -> dtool_core::shape::ShapeRecipe {
    session.document.get_object(ObjectId(id)).and_then(|object| object.shape.clone()).expect("shape recipe missing")
}

fn undo(session: &mut Session) {
    session.run(&Command::new("edit.undo")).expect("undo failed");
}

#[test]
fn test_shape_add_rectangle_params() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::Rectangle, [10.0, 20.0], [100.0, 50.0]);
    let object = session.document.get_object(ObjectId(id)).unwrap();
    assert_eq!(object.pos, [10.0, 20.0]);
    assert_eq!(object.size, [100.0, 50.0]);
    let shape = recipe(&mut session, id);
    assert_eq!(shape.kind, ShapeKind::Rectangle);
    assert_eq!(shape.size, [100.0, 50.0]);
    assert!(shape.corner.is_none() && shape.triangle.is_none());
}

#[test]
fn test_shape_add_triangle_params() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::Triangle, [10.0, 20.0], [100.0, 50.0]);
    let shape = recipe(&mut session, id);
    assert_eq!(shape.kind, ShapeKind::Triangle);
    assert_eq!(shape.triangle.expect("triangle recipe").points, [0.5, 1.0, 0.5]);
}

#[test]
fn test_shape_add_ellipse_params() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::Ellipse, [10.0, 20.0], [80.0, 60.0]);
    let shape = recipe(&mut session, id);
    assert_eq!(shape.kind, ShapeKind::Ellipse);
    assert_eq!(shape.size, [80.0, 60.0]);
}

#[test]
fn test_shape_add_rounded_rectangle_params() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::RoundedRectangle, [10.0, 20.0], [100.0, 50.0]);
    let corner = recipe(&mut session, id).corner.expect("corner recipe");
    assert_eq!(corner.radii, [0.0; 4]);
    assert!(!corner.absolute);
    assert!(corner.lock);
}

#[test]
fn test_shape_params_set_rectangle() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::Rectangle, [10.0, 20.0], [100.0, 50.0]);
    session.run(&tool(ShapeKind::Rectangle).set_size(id, [120.0, 60.0])).expect("shape.params.set failed");
    assert_eq!(recipe(&mut session, id).size, [120.0, 60.0]);
    assert_eq!(session.document.get_object(ObjectId(id)).unwrap().size, [120.0, 60.0]);
    undo(&mut session);
    assert_eq!(recipe(&mut session, id).size, [100.0, 50.0]);
}

#[test]
fn test_shape_params_set_triangle() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::Triangle, [10.0, 20.0], [100.0, 50.0]);
    let mut set = id_command("shape.params.set", id);
    set.params["triangle"]["points"] = vec![0.3_f64, 0.1, 0.7].into();
    session.run(&set).expect("triangle set failed");
    let points = recipe(&mut session, id).triangle.unwrap().points;
    assert!((points[0] - 0.3).abs() < 1e-6 && (points[1] - 0.1).abs() < 1e-6 && (points[2] - 0.7).abs() < 1e-6);
    let mut bad = id_command("shape.params.set", id);
    bad.params["triangle"]["points"] = vec![1.5_f64, 0.1, 0.7].into();
    assert!(session.run(&bad).is_err());
}

#[test]
fn test_shape_params_set_ellipse() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::Ellipse, [10.0, 20.0], [80.0, 60.0]);
    session.run(&tool(ShapeKind::Ellipse).set_size(id, [90.0, 70.0])).expect("ellipse set failed");
    assert_eq!(recipe(&mut session, id).size, [90.0, 70.0]);
    assert!(session.run(&tool(ShapeKind::Rectangle).set_size(id, [90.0, 70.0])).is_err());
}

#[test]
fn test_shape_params_set_rounded_rectangle_radii_lock() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::RoundedRectangle, [10.0, 20.0], [100.0, 50.0]);
    let mut set = id_command("shape.params.set", id);
    set.params["corner"]["radii"] = vec![20.0_f64, 20.0, 20.0, 20.0].into();
    set.params["corner"]["absolute"] = false.into();
    set.params["corner"]["lock"] = false.into();
    session.run(&set).expect("corner set failed");
    let corner = recipe(&mut session, id).corner.unwrap();
    assert_eq!(corner.radii, [20.0; 4]);
    assert!(!corner.lock);
    undo(&mut session);
    let corner = recipe(&mut session, id).corner.unwrap();
    assert_eq!(corner.radii, [0.0; 4]);
    assert!(corner.lock);
}

#[test]
fn test_shape_params_query() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::Rectangle, [10.0, 20.0], [100.0, 50.0]);
    let command = query(&mut session, id);
    let out = session.run(&command).expect("query failed");
    assert_eq!(out["kind"], "rectangle");
    assert_eq!(out["size"][0].as_f64(), Some(100.0));
    assert_eq!(out["size"][1].as_f64(), Some(50.0));
}

fn assert_converted(kind: ShapeKind, size: [f32; 2]) {
    let mut session = make_session();
    let id = add(&mut session, kind, [10.0, 20.0], size);
    session.run(&tool(kind).convert_to_curves(id)).expect("convert failed");
    let object = session.document.get_object(ObjectId(id)).expect("converted object missing");
    assert!(matches!(object.kind, ObjectKind::Curve(_)));
    assert!(object.shape.is_none());
    let command = query(&mut session, id);
    assert!(session.run(&command).is_err());
    undo(&mut session);
    assert_eq!(recipe(&mut session, id).kind, kind);
}

#[test]
fn test_shape_convert_to_curves() {
    assert_converted(ShapeKind::Rectangle, [100.0, 50.0]);
}

#[test]
fn test_shape_convert_to_curves_triangle() {
    assert_converted(ShapeKind::Triangle, [100.0, 50.0]);
}

#[test]
fn test_shape_convert_to_curves_ellipse() {
    assert_converted(ShapeKind::Ellipse, [80.0, 60.0]);
}

#[test]
fn test_shape_convert_to_curves_rounded_rectangle() {
    assert_converted(ShapeKind::RoundedRectangle, [100.0, 50.0]);
}

#[test]
fn test_shape_add_shift_create_square() {
    let command = tool(ShapeKind::Rectangle).create_with_modifiers([10.0, 20.0], [110.0, 70.0], true, false);
    assert_eq!(command.params["size"][0].as_f64(), Some(100.0));
    assert_eq!(command.params["size"][1].as_f64(), Some(100.0));
    let mut session = make_session();
    let id = extract_object_id!(session.run(&command).expect("shift add failed"));
    assert_eq!(recipe(&mut session, id).size, [100.0, 100.0]);
}

#[test]
fn test_shape_add_ctrl_create_from_centre() {
    let command = tool(ShapeKind::Rectangle).create_with_modifiers([50.0, 50.0], [150.0, 100.0], false, true);
    let mut session = make_session();
    let id = extract_object_id!(session.run(&command).expect("ctrl add failed"));
    let object = session.document.get_object(ObjectId(id)).unwrap();
    assert_eq!(object.pos, [0.0, 25.0]);
    assert_eq!(object.size, [100.0, 50.0]);
}

#[test]
fn test_shape_add_shift_ctrl_combined() {
    let command = tool(ShapeKind::Ellipse).create_with_modifiers([50.0, 50.0], [130.0, 90.0], true, true);
    let mut session = make_session();
    let id = extract_object_id!(session.run(&command).expect("shift ctrl add failed"));
    let object = session.document.get_object(ObjectId(id)).unwrap();
    assert_eq!(object.size, [80.0, 80.0]);
    assert_eq!(object.pos, [10.0, 10.0]);
}

#[test]
fn test_shape_add_invalid_kind() {
    let mut session = make_session();
    let mut command = tool(ShapeKind::Rectangle).create([10.0, 20.0], [100.0, 50.0]);
    command.params["kind"] = "invalid-shape".into();
    let before = session.document.objects.len();
    assert!(session.run(&command).is_err());
    assert_eq!(session.document.objects.len(), before);
}

#[test]
fn test_shape_params_set_invalid_id() {
    let mut session = make_session();
    let result = session.run(&tool(ShapeKind::Rectangle).set_size(99999, [10.0, 10.0]));
    assert_eq!(result.unwrap_err().code, "NOT_FOUND");
}

#[test]
fn test_shape_params_query_invalid_id() {
    let mut session = make_session();
    let command = query(&mut session, 99999);
    assert_eq!(session.run(&command).unwrap_err().code, "NOT_FOUND");
}

#[test]
fn test_shape_convert_to_curves_invalid_id() {
    let mut session = make_session();
    assert!(session.run(&tool(ShapeKind::Rectangle).convert_to_curves(99999)).is_err());
}

fn assert_created(kind: ShapeKind, size: [f32; 2]) {
    let mut session = make_session();
    let command = tool(kind).create([10.0, 20.0], size);
    assert_eq!(command.params["kind"], shape_kind_key(kind));
    let id = add(&mut session, kind, [10.0, 20.0], size);
    let command = query(&mut session, id);
    let out = session.run(&command).expect("query after create failed");
    assert_eq!(out["kind"], shape_kind_key(kind));
    undo(&mut session);
    assert!(session.document.get_object(ObjectId(id)).is_none());
}

#[test]
fn test_shape_add_handles_rectangle() {
    assert_created(ShapeKind::Rectangle, [100.0, 50.0]);
}

#[test]
fn test_shape_add_handles_triangle() {
    assert_created(ShapeKind::Triangle, [100.0, 50.0]);
}

#[test]
fn test_shape_add_handles_ellipse() {
    assert_created(ShapeKind::Ellipse, [80.0, 60.0]);
}

#[test]
fn test_shape_add_handles_rounded_rectangle() {
    assert_created(ShapeKind::RoundedRectangle, [100.0, 50.0]);
}

#[test]
fn test_flyout_lists_affinity_set() {
    let mut session = make_session();
    for kind in [ShapeKind::Rectangle, ShapeKind::Triangle, ShapeKind::Ellipse, ShapeKind::RoundedRectangle] {
        add(&mut session, kind, [10.0, 20.0], [100.0, 50.0]);
    }
    assert_eq!(session.document.objects.len(), 4);
    let mut corner_on_rectangle = id_command("shape.params.set", session.document.objects[0].id.0);
    corner_on_rectangle.params["corner"]["radii"] = vec![5.0_f64, 5.0, 5.0, 5.0].into();
    corner_on_rectangle.params["corner"]["absolute"] = false.into();
    corner_on_rectangle.params["corner"]["lock"] = true.into();
    assert!(session.run(&corner_on_rectangle).is_err());
}

#[test]
fn test_shape_add_one_undo_unit_and_redo_restores_recipe() {
    let mut session = make_session();
    let id = add(&mut session, ShapeKind::RoundedRectangle, [10.0, 20.0], [100.0, 50.0]);
    let created = recipe(&mut session, id);
    undo(&mut session);
    assert!(session.document.get_object(ObjectId(id)).is_none());
    session.run(&Command::new("edit.redo")).expect("redo failed");
    assert_eq!(recipe(&mut session, id), created);
    assert_eq!(session.document.objects.len(), 1);
}