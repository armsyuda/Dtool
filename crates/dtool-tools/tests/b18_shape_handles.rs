use dtool_core::command::Session;
use dtool_core::shape::ShapeKind;
use dtool_core::{Command, Document, ObjectId};
use dtool_tools::shape_tool::{self, ShapeParamKind, ShapeTool};

fn add(session: &mut Session, kind: ShapeKind, pos: [f32; 2], size: [f32; 2]) -> ObjectId {
    let id = session.run(&ShapeTool { kind }.create(pos, size)).unwrap()["id"].as_u64().unwrap();
    ObjectId(id)
}

fn session() -> Session {
    Session::new(Document::default())
}

#[test]
fn p0_shapes_expose_on_canvas_param_handles() {
    let mut session = session();
    let rect = add(&mut session, ShapeKind::Rectangle, [10.0, 20.0], [100.0, 50.0]);
    assert_eq!(
        shape_tool::param_handles(session.document.get_object(rect).unwrap()),
        vec![(ShapeParamKind::Size, [110.0, 70.0])]
    );

    let tri = add(&mut session, ShapeKind::Triangle, [10.0, 20.0], [100.0, 50.0]);
    let handles = shape_tool::param_handles(session.document.get_object(tri).unwrap());
    assert_eq!(handles, vec![(ShapeParamKind::TriangleApex, [60.0, 20.0])]);

    let ell = add(&mut session, ShapeKind::Ellipse, [0.0, 0.0], [80.0, 60.0]);
    assert_eq!(
        shape_tool::param_handles(session.document.get_object(ell).unwrap()),
        vec![(ShapeParamKind::Size, [80.0, 60.0])]
    );

    let rnd = add(&mut session, ShapeKind::RoundedRectangle, [10.0, 20.0], [100.0, 40.0]);
    assert_eq!(
        shape_tool::param_handles(session.document.get_object(rnd).unwrap()),
        vec![(ShapeParamKind::Corner(0), [10.0, 20.0])]
    );
}

#[test]
fn dragging_handles_updates_params_and_undoes() {
    let mut session = session();
    let tri = add(&mut session, ShapeKind::Triangle, [0.0, 0.0], [100.0, 50.0]);
    let before = session.document.get_object(tri).unwrap().shape.clone();
    let object = session.document.get_object(tri).unwrap().clone();
    session.run(&shape_tool::apply_param_drag(&object, ShapeParamKind::TriangleApex, [25.0, 0.0]).unwrap()).unwrap();
    let points = session.document.get_object(tri).unwrap().shape.as_ref().unwrap().triangle.as_ref().unwrap().points;
    assert!((points[0] - 0.25).abs() < 1e-4);
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(session.document.get_object(tri).unwrap().shape, before);

    let rnd = add(&mut session, ShapeKind::RoundedRectangle, [0.0, 0.0], [100.0, 40.0]);
    let before = session.document.get_object(rnd).unwrap().shape.clone();
    let object = session.document.get_object(rnd).unwrap().clone();
    session.run(&shape_tool::apply_param_drag(&object, ShapeParamKind::Corner(0), [10.0, 0.0]).unwrap()).unwrap();
    let corner = session.document.get_object(rnd).unwrap().shape.as_ref().unwrap().corner.as_ref().unwrap();
    assert!(corner.lock);
    assert!(corner.radii.iter().all(|r| (*r - corner.radii[0]).abs() < 1e-4));
    assert!(corner.radii[0] > 0.0);
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(session.document.get_object(rnd).unwrap().shape, before);

    let rect = add(&mut session, ShapeKind::Rectangle, [0.0, 0.0], [40.0, 20.0]);
    let object = session.document.get_object(rect).unwrap().clone();
    session.run(&shape_tool::apply_param_drag(&object, ShapeParamKind::Size, [80.0, 50.0]).unwrap()).unwrap();
    assert_eq!(session.document.get_object(rect).unwrap().size, [80.0, 50.0]);
}

#[test]
fn hit_param_handle_uses_handle_radius() {
    let mut session = session();
    let id = add(&mut session, ShapeKind::Rectangle, [10.0, 20.0], [100.0, 50.0]);
    let object = session.document.get_object(id).unwrap();
    assert_eq!(shape_tool::hit_param_handle(object, [110.0, 70.0], 6.0), Some(ShapeParamKind::Size));
    assert_eq!(shape_tool::hit_param_handle(object, [0.0, 0.0], 6.0), None);
}