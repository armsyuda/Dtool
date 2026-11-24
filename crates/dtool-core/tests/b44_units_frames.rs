use dtool_core::document::{Document, Units};
use dtool_core::frame::FrameProps;
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::Arrow;
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn near(value: f32, expected: f32) {
    assert!((value - expected).abs() < 1e-3, "{value} != {expected}");
}

fn framed() -> Object {
    let mut object = Object { id: ObjectId(4), pos: [72.0, 72.0], size: [288.0, 144.0], ..Object::default() };
    object.frame = Some(FrameProps {
        size: [288.0, 144.0],
        columns: 2,
        gutter: 18.0,
        column_widths: Some(vec![144.0, 126.0]),
        insets: [7.2, 14.4, 21.6, 28.8],
        max_para_space: 36.0,
        ..FrameProps::default()
    });
    object.stroke.width = 3.6;
    object.stroke.start_arrow = Some(Arrow { name: "arrow".to_string(), scale: 2.0 });
    object.stroke.end_arrow = Some(Arrow { name: "circle".to_string(), scale: 1.5 });
    object
}

#[test]
fn unit_change_converts_frame_insets_and_keeps_relative_arrow_scale() {
    let mut document = Document::new([720.0, 360.0], Units::Pt, 72.0, 1);
    document.objects.push(framed());
    let mut session = Session::new(document);
    let before = serde_json::to_value(&session.document.objects[0]).unwrap();
    run(&mut session, "doc.setup.apply", json!({ "units": "inches" }));
    let object = &session.document.objects[0];
    let frame = object.frame.as_ref().unwrap();
    near(frame.insets[0], 0.1);
    near(frame.insets[3], 0.4);
    near(frame.gutter, 0.25);
    near(frame.max_para_space, 0.5);
    near(frame.column_widths.as_ref().unwrap()[0], 2.0);
    near(frame.size[0], 4.0);
    near(frame.size[1], 2.0);
    near(object.stroke.width, 0.05);
    assert_eq!(object.stroke.start_arrow.as_ref().unwrap().scale, 2.0);
    assert_eq!(object.stroke.end_arrow.as_ref().unwrap().scale, 1.5);
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(serde_json::to_value(&session.document.objects[0]).unwrap(), before);
    run(&mut session, "edit.redo", json!({}));
    near(session.document.objects[0].frame.as_ref().unwrap().insets[1], 0.2);
}

#[test]
fn unit_round_trip_keeps_frame_point_values() {
    let mut document = Document::new([720.0, 360.0], Units::Pt, 72.0, 1);
    document.objects.push(framed());
    let mut session = Session::new(document);
    run(&mut session, "doc.setup.apply", json!({ "units": "mm" }));
    run(&mut session, "doc.setup.apply", json!({ "units": "pt" }));
    let frame = session.document.objects[0].frame.as_ref().unwrap();
    near(frame.insets[2], 21.6);
    near(frame.gutter, 18.0);
    near(frame.column_widths.as_ref().unwrap()[1], 126.0);
}