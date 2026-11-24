use dtool_core::commands::stroke::{arrowhead, arrowheads, ARROW_NAMES};
use dtool_core::document::{Document, Units};
use dtool_core::paint::{Arrow, Colour, Paint, StrokeStyle};
use dtool_core::scene::{rasterize_scene, Prim, Scene};
use dtool_core::{Object, ObjectId, Session};
use serde_json::json;

fn red() -> Paint {
    Paint::Solid { colour: Colour::new(255, 0, 0, 255) }
}

fn coverage(doc: &Document, prim: Prim) -> usize {
    let scene = Scene { prims: vec![prim], ..Scene::default() };
    let pixels = rasterize_scene(doc, &scene, [0.0, 0.0, 40.0, 40.0], 40, 40, None);
    pixels.chunks_exact(4).filter(|px| px[3] > 0 && px[0] > 128).count()
}

#[test]
fn c07_every_listed_arrow_draws_pixels() {
    let doc = Document::new([40.0, 40.0], Units::Px, 96.0, 1);
    for name in ARROW_NAMES {
        let head = arrowhead(name, [30.0, 20.0], [0.0, 20.0], 2.0, 2.0).unwrap_or_else(|| panic!("{name}"));
        let prim = if head.filled {
            Prim::Fill { object: ObjectId(1), contours: vec![head.points.clone()], origin: [0.0, 0.0], paint: red(), opacity: 1.0 }
        } else {
            Prim::Stroke { object: ObjectId(1), points: head.points.clone(), closed: false, width: head.line_width, paint: red(), opacity: 1.0 }
        };
        assert!(coverage(&doc, prim) > 4, "{name} drew nothing");
    }
}

#[test]
fn c07_arrow_list_matches_renderer_names() {
    let mut s = Session::new(Document::new([40.0, 40.0], Units::Px, 96.0, 1));
    let list = s.run_named("stroke.arrows.list", json!({})).expect("list");
    let names: Vec<&str> = list["arrows"].as_array().expect("arrows").iter().map(|item| item["name"].as_str().unwrap()).collect();
    assert_eq!(names, ARROW_NAMES.to_vec());
    assert!(list["arrows"].as_array().unwrap().iter().all(|item| item["label_key"].as_str().unwrap().starts_with("stroke.arrow_head.")));
}

#[test]
fn c07_arrowheads_follow_path_ends_and_skip_closed() {
    let mut stroke = StrokeStyle::default();
    stroke.start_arrow = Some(Arrow { name: "circle".into(), scale: 1.0 });
    stroke.end_arrow = Some(Arrow { name: "triangle".into(), scale: 1.0 });
    let points = [[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]];
    let heads = arrowheads(&stroke, &points, false, 1.0);
    assert_eq!(heads.len(), 2);
    assert_eq!(heads[1].points[0], [20.0, 0.0]);
    assert!(heads[0].points.iter().all(|p| p[0] >= -1e-3));
    assert!(arrowheads(&stroke, &points, true, 1.0).is_empty());
    assert!(arrowhead("wavy", [1.0, 0.0], [0.0, 0.0], 1.0, 1.0).is_none());
    assert_eq!(arrowhead("Arrow", [1.0, 0.0], [0.0, 0.0], 1.0, 1.0), arrowhead("triangle", [1.0, 0.0], [0.0, 0.0], 1.0, 1.0));
    assert!(arrowhead("Circle", [1.0, 0.0], [0.0, 0.0], 1.0, 1.0).is_some());
    assert!(arrowhead("triangle", [1.0, 0.0], [1.0, 0.0], 1.0, 1.0).is_none());
}

#[test]
fn c07_stroke_set_rejects_unknown_arrow_name() {
    let mut s = Session::new(Document::new([40.0, 40.0], Units::Px, 96.0, 1));
    let id = s.document.create_object(Object { name: "a".into(), size: [10.0, 10.0], ..Object::default() });
    assert!(s.run_named("stroke.set", json!({ "id": id.0, "end_arrow": { "name": "wavy", "scale": 1.0 } })).is_err());
    s.run_named("stroke.set", json!({ "id": id.0, "end_arrow": { "name": "diamond", "scale": 1.0 } })).expect("diamond");
    assert_eq!(s.document.get_object(id).unwrap().stroke.end_arrow.as_ref().unwrap().name, "diamond");
}