use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Paint, StrokeStyle};
use dtool_core::workspace::Workspace;

fn gradient(kind: GradientKind) -> Paint {
    Paint::Gradient {
        kind,
        stops: vec![
            GradientStop { offset: 0.0, colour: Colour::new(255, 0, 0, 255) },
            GradientStop { offset: 0.35, colour: Colour::new(0, 128, 255, 64) },
            GradientStop { offset: 1.0, colour: Colour::new(10, 200, 30, 0) },
        ],
        geom: GradientGeom { start: [12.5, 7.25], end: [80.0, 41.5], ratio: 0.4, ..GradientGeom::default() },
    }
}

fn document(kind: GradientKind) -> (Document, ObjectId) {
    let mut doc = Document::new([200.0, 100.0], Units::Px, 96.0, 1);
    let other = if kind == GradientKind::Elliptical { GradientKind::Conical } else { GradientKind::Elliptical };
    let id = doc.create_object(Object { name: "g".into(), size: [100.0, 50.0], fill: gradient(kind), stroke: StrokeStyle { paint: gradient(other), width: 2.0, ..StrokeStyle::default() }, ..Object::default() });
    (doc, id)
}

fn assert_same(loaded: &Document, id: ObjectId, kind: GradientKind) {
    let object = loaded.get_object(id).expect("object");
    assert_eq!(object.fill, gradient(kind));
    let other = if kind == GradientKind::Elliptical { GradientKind::Conical } else { GradientKind::Elliptical };
    assert_eq!(object.stroke.paint, gradient(other));
    match &object.fill {
        Paint::Gradient { kind: loaded_kind, stops, geom } => {
            assert_eq!(*loaded_kind, kind);
            assert_eq!(stops.len(), 3);
            assert_eq!(stops[1].colour.a, 64);
            assert_eq!(stops[2].colour.a, 0);
            assert_eq!(geom.ratio, 0.4);
            assert_eq!((geom.start, geom.end), ([12.5, 7.25], [80.0, 41.5]));
        }
        other => panic!("expected gradient, got {other:?}"),
    }
}

#[test]
fn c07_elliptical_and_conical_survive_json_round_trip() {
    for kind in [GradientKind::Elliptical, GradientKind::Conical] {
        let (doc, id) = document(kind);
        let saved = serde_json::to_value(&doc).expect("save");
        let loaded: Document = serde_json::from_value(saved.clone()).expect("load");
        assert_same(&loaded, id, kind);
        assert_eq!(serde_json::to_value(&loaded).expect("save again"), saved);
    }
}

#[test]
fn c07_elliptical_and_conical_survive_file_save_and_open() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("c07_gradient_roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    for (kind, name) in [(GradientKind::Elliptical, "elliptical.dtool"), (GradientKind::Conical, "conical.dtool")] {
        let (doc, id) = document(kind);
        let path = dir.join(name);
        Workspace::write_document(&doc, &path).expect("write");
        let loaded = dtool_core::commands::document_files::read_document(&path).expect("read");
        assert_same(&loaded, id, kind);
    }
}