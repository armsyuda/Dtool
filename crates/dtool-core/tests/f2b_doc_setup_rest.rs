use dtool_core::commands::doc_setup::apply_new_setup;
use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn points(units: Units, dpi: f32) -> f32 {
    match units {
        Units::Pt => 1.0,
        Units::Mm => 72.0 / 25.4,
        Units::Cm => 72.0 / 2.54,
        Units::In => 72.0,
        Units::Px => 72.0 / dpi,
    }
}

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.01, "{a} != {b}");
}

#[test]
fn every_unit_change_keeps_point_geometry() {
    for start in [Units::Pt, Units::Mm, Units::Cm, Units::In, Units::Px] {
        for target in ["points", "millimeters", "inches", "pixels"] {
            let factor = points(start, 72.0);
            let mut document = Document::new([720.0 / factor, 360.0 / factor], start, 72.0, 1);
            document.objects.push(Object { id: ObjectId(3), pos: [72.0 / factor, 36.0 / factor], size: [144.0 / factor, 72.0 / factor], ..Object::default() });
            let mut session = Session::new(document);
            run(&mut session, "doc.setup.apply", json!({ "units": target }));
            let scale = points(session.document.units, session.document.dpi);
            let object = &session.document.objects[0];
            assert_eq!(object.id, ObjectId(3));
            near(object.pos[0] * scale, 72.0);
            near(object.pos[1] * scale, 36.0);
            near(object.size[0] * scale, 144.0);
            near(session.document.pages[0].size[0] * scale, 720.0);
            near(session.document.pages[0].size[1] * scale, 360.0);
        }
    }
}

#[test]
fn page_origin_is_stored_and_old_files_default() {
    let document = Document::new([720.0, 360.0], Units::Pt, 72.0, 1);
    let mut value = serde_json::to_value(&document).unwrap();
    value["setup"].as_object_mut().unwrap().remove("page_origin");
    let loaded: Document = serde_json::from_value(value).unwrap();
    assert_eq!(loaded.setup.page_origin, [0.0, 0.0]);
    let mut session = Session::new(document);
    run(&mut session, "doc.setup.apply", json!({ "page_origin": [10.0, 20.0] }));
    assert_eq!(session.document.setup.page_origin, [10.0, 20.0]);
    let stored: Document = serde_json::from_str(&serde_json::to_string(&session.document).unwrap()).unwrap();
    assert_eq!(stored.setup.page_origin, [10.0, 20.0]);
    let query = run(&mut session, "doc.setup.query", json!({}));
    assert_eq!(query["page_origin"], json!([10.0, 20.0]));
}

#[test]
fn new_document_takes_setup_params() {
    let mut document = Document::new([595.0, 842.0], Units::Pt, 72.0, 2);
    apply_new_setup(&mut document, &json!({
        "width": 595.0, "height": 842.0, "units": "Pt", "dpi": 72.0, "pages": 2,
        "facing_pages": false, "start_on_right": false, "reflow_pages": true, "page_origin": { "x": 5.0 },
        "setup": { "arrange_vertical": true, "transparent_background": true, "bleed": { "left": 3.0, "right": 3.0, "top": 3.0, "bottom": 3.0 } }
    })).unwrap();
    assert!(document.setup.arrange_vertical);
    assert!(document.setup.transparent_background);
    assert_eq!(document.setup.bleed.left, 3.0);
    assert!(document.setup.facing_pages);
    let mut flat = Document::new([595.0, 842.0], Units::Pt, 72.0, 2);
    apply_new_setup(&mut flat, &json!({ "facing_pages": false, "page_origin": { "x": 5.0 } })).unwrap();
    assert!(!flat.setup.facing_pages);
    assert_eq!(flat.setup.page_origin, [5.0, 0.0]);
    assert_eq!(flat.pages.len(), 2);
    assert!(apply_new_setup(&mut flat, &json!({ "margins": { "left": 400.0, "right": 400.0 } })).is_err());
    assert_eq!(flat.setup.margins.left, 0.0);
}

#[test]
fn turn_canvas_undo_restores_full_setup() {
    let mut document = Document::new([100.0, 50.0], Units::Px, 72.0, 1);
    document.setup.facing_pages = false;
    document.setup.page_origin = [4.0, 2.0];
    document.setup.margins.left = 5.0;
    let before = document.setup.clone();
    let mut session = Session::new(document);
    run(&mut session, "doc.turn_canvas", json!({ "turn": "rotate_left" }));
    assert_eq!(session.document.pages[0].size, [50.0, 100.0]);
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.setup, before);
    assert_eq!(session.document.pages[0].size, [100.0, 50.0]);
    run(&mut session, "edit.redo", json!({}));
    assert_eq!(session.document.pages[0].size, [50.0, 100.0]);
}

#[test]
fn unit_change_converts_stroke_guides_and_undoes() {
    let mut document = Document::new([720.0, 360.0], Units::Pt, 72.0, 1);
    let mut object = Object { id: ObjectId(9), pos: [72.0, 36.0], size: [144.0, 72.0], ..Object::default() };
    object.stroke.width = 7.2;
    object.stroke.dash = vec![3.6, 7.2];
    document.objects.push(object);
    let mut session = Session::new(document);
    session.guides.add(dtool_core::guides::GuideKind::Vertical, 72.0, None).unwrap();
    let guides = session.guides.clone();
    run(&mut session, "doc.setup.apply", json!({ "units": "inches" }));
    let object = &session.document.objects[0];
    near(object.stroke.width, 0.1);
    near(object.stroke.dash[1], 0.1);
    near(session.guides.items[0].position, 1.0);
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.guides, guides);
    near(session.document.objects[0].stroke.width, 7.2);
    run(&mut session, "edit.redo", json!({}));
    near(session.guides.items[0].position, 1.0);
}