use dtool_core::curve::{CornerKind, Contour, Curve, PathPoint, PointId};
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::outline::object_outline;
use dtool_core::{Command, Session};
use kurbo::Shape;
use serde_json::json;

fn rect(session: &mut Session) -> ObjectId {
    let points = vec![
        PathPoint::sharp(PointId(1), [0.0, 0.0]),
        PathPoint::sharp(PointId(2), [100.0, 0.0]),
        PathPoint::sharp(PointId(3), [100.0, 60.0]),
        PathPoint::sharp(PointId(4), [0.0, 60.0]),
    ];
    let curve = Curve::from_contour_and_holes(Contour { points, closed: true }, Vec::new());
    session.document.create_object(Object { name: "rect".into(), pos: [10.0, 20.0], size: [100.0, 60.0], kind: ObjectKind::Curve(curve), ..Object::default() })
}

fn outline(session: &Session, id: ObjectId) -> kurbo::BezPath {
    object_outline(session.document.get_object(id).unwrap()).unwrap()
}

fn stored(session: &Session, id: ObjectId) -> Contour {
    session.document.get_object(id).unwrap().curve().unwrap().contour.clone()
}

#[test]
fn rounded_corner_shows_in_outline_and_undo_restores() {
    let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
    let id = rect(&mut session);
    let plain = outline(&session, id);
    let before = stored(&session, id);
    session.run(&Command::with_params("corner.set", json!({"id": id.0, "points": [2], "kind": "rounded", "radius": 10.0}))).unwrap();
    let after = stored(&session, id);
    assert_eq!(after.points.iter().map(|p| p.pos).collect::<Vec<_>>(), before.points.iter().map(|p| p.pos).collect::<Vec<_>>());
    assert_eq!(after.points[1].corner.map(|c| c.kind), Some(CornerKind::Rounded));
    let rounded = outline(&session, id);
    assert_eq!(rounded.elements().len(), plain.elements().len() + 1);
    assert!(rounded.elements().iter().any(|e| matches!(e, kurbo::PathEl::CurveTo(..))));
    let (pb, rb) = (plain.bounding_box(), rounded.bounding_box());
    assert!((pb.x0 - rb.x0).abs() < 1e-6 && (pb.y0 - rb.y0).abs() < 1e-6 && (pb.x1 - rb.x1).abs() < 1e-6 && (pb.y1 - rb.y1).abs() < 1e-6);
    assert!(!rounded.contains(kurbo::Point::new(99.5, 0.5)));
    assert!(plain.contains(kurbo::Point::new(99.5, 0.5)));
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(stored(&session, id), before);
    assert_eq!(outline(&session, id).elements().len(), plain.elements().len());
}

#[test]
fn straight_cutout_and_concave_change_outline_without_changing_stored_path() {
    for (kind, extra) in [("straight", 1usize), ("concave", 1), ("cutout", 2)] {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let id = rect(&mut session);
        let plain = outline(&session, id).elements().len();
        session.run(&Command::with_params("corner.set", json!({"id": id.0, "points": [1, 3], "kind": kind, "radius": 8.0}))).unwrap();
        assert_eq!(outline(&session, id).elements().len(), plain + 2 * extra, "{kind}");
        assert_eq!(stored(&session, id).points.len(), 4);
    }
}

#[test]
fn corner_next_to_a_curved_side_is_left_alone() {
    let build = |corner: bool| {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let id = rect(&mut session);
        if let Some(object) = session.document.objects.iter_mut().find(|o| o.id == id) {
            if let ObjectKind::Curve(curve) = &mut object.kind {
                if corner {
                    curve.contour.points[1].corner = Some(dtool_core::curve::CornerSpec { kind: CornerKind::Rounded, radius: 10.0 });
                }
                curve.contour.points[0].handle_out = Some([30.0, -20.0]);
            }
        }
        outline(&session, id)
    };
    assert_eq!(build(true), build(false));
}
