use dtool_core::colour::swatch::{self, SwatchTarget};
use dtool_core::command::Session;
use dtool_core::commands::hatch;
use dtool_core::document::{Document, Story, Units};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{set_range_fill, range_fill, Colour, Hatch, HatchLine, HatchUnits, Paint};
use dtool_core::table::model::StoryId;
use dtool_protocol::Command;
use serde_json::json;

fn hatch_paint(colour: Colour) -> Hatch {
    Hatch {
        lines: vec![HatchLine {
            angle_rad: 0.7853981633974483,
            origin: [0.0, 0.0],
            shift: 0.0,
            spacing: 8.0,
            dash: Vec::new(),
        }],
        colour,
        background: None,
        weight: 1.0,
        units: HatchUnits::Document,
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    }
}

fn session() -> Session {
    let mut document = Document::new([400.0, 300.0], Units::Px, 96.0, 1);
    let mut object = Object::default();
    object.id = ObjectId(1);
    object.size = [120.0, 80.0];
    object.fill = Paint::Solid { colour: Colour::new(20, 20, 20, 255) };
    object.stroke.paint = Paint::Solid { colour: Colour::new(0, 0, 0, 255) };
    document.objects.push(object);
    document.next_object_id = 2;
    let mut story = Story {
        id: StoryId(1),
        text: "Role".to_string(),
        ranges: vec![json!({"start": 0, "end": 4})],
        ..Story::default()
    };
    set_range_fill(
        &mut story.ranges[0],
        Some(&Paint::Solid { colour: Colour::new(10, 200, 30, 255) }),
    );
    document.stories.push(story);
    document.next_story_id = 2;
    Session::new(document)
}

fn run(session: &mut Session, name: &str, params: serde_json::Value) -> serde_json::Value {
    hatch::run(session, &Command::with_params(name, params))
        .expect("hatch command")
        .expect("hatch ok")
}

#[test]
fn c07_role_isolation() {
    let mut session = session();
    let fill_hatch = hatch_paint(Colour::new(255, 0, 0, 0));
    let stroke_hatch = hatch_paint(Colour::new(0, 0, 255, 255));
    let text_paint = Paint::Solid { colour: Colour::new(10, 200, 30, 255) };

    let added = run(
        &mut session,
        "swatch.hatch.add",
        json!({ "name": "Stroke Hatch", "hatch": stroke_hatch, "palette": "C07" }),
    );
    let swatch_id = added["id"].as_u64().expect("swatch id");
    let swatch = session
        .document
        .swatches
        .iter()
        .find(|item| item.id == swatch_id)
        .cloned()
        .expect("stored swatch");
    assert!(swatch.global);
    assert_eq!(swatch.name, "Stroke Hatch");
    assert_eq!(swatch.palette, "C07");
    assert_eq!(swatch.paint, Some(Paint::Hatch(stroke_hatch.clone())));

    let stroke_object = session.document.get_object(ObjectId(1)).unwrap();
    let mut stroke = stroke_object.stroke.clone();
    stroke.paint = Paint::Hatch(stroke_hatch.clone());
    session.document.find_object_mut(1).unwrap().stroke = stroke;
    swatch::bind(&mut session.document, ObjectId(1), SwatchTarget::Stroke, Some(swatch_id));

    run(
        &mut session,
        "fill.hatch.set",
        json!({ "ids": [1], "role": "fill", "hatch": fill_hatch }),
    );

    let object = session.document.get_object(ObjectId(1)).unwrap();
    match &object.fill {
        Paint::Hatch(hatch) => {
            assert_eq!(hatch.colour, Colour::new(255, 0, 0, 0));
            assert_eq!(hatch.colour.a, 0);
            assert_eq!(hatch, &fill_hatch);
        }
        other => panic!("fill should be hatch, got {other:?}"),
    }
    assert_eq!(object.stroke.paint, Paint::Hatch(stroke_hatch.clone()));
    let stroke_binding = session
        .document
        .swatch_bindings
        .iter()
        .find(|binding| binding.object == ObjectId(1) && binding.target == SwatchTarget::Stroke)
        .expect("stroke binding");
    assert_eq!(stroke_binding.swatch, swatch_id);
    assert!(session
        .document
        .swatch_bindings
        .iter()
        .all(|binding| binding.target != SwatchTarget::Fill));
    let range = &session.document.stories[0].ranges[0];
    assert_eq!(range_fill(range), Some(text_paint.clone()));

    let encoded = serde_json::to_value(&session.document).expect("save");
    let loaded: Document = serde_json::from_value(encoded).expect("load");
    match &loaded.get_object(ObjectId(1)).unwrap().fill {
        Paint::Hatch(hatch) => {
            assert_eq!(hatch.colour.a, 0);
            assert_eq!(hatch, &fill_hatch);
        }
        other => panic!("loaded fill should be hatch, got {other:?}"),
    }
    assert_eq!(loaded.get_object(ObjectId(1)).unwrap().stroke.paint, Paint::Hatch(stroke_hatch.clone()));
    assert_eq!(
        loaded
            .swatches
            .iter()
            .find(|item| item.id == swatch_id)
            .and_then(|item| item.paint.clone()),
        Some(Paint::Hatch(stroke_hatch.clone()))
    );
    assert!(loaded.swatches.iter().find(|item| item.id == swatch_id).unwrap().global);
    assert_eq!(
        loaded
            .swatch_bindings
            .iter()
            .find(|binding| binding.target == SwatchTarget::Stroke)
            .map(|binding| binding.swatch),
        Some(swatch_id)
    );
    assert_eq!(range_fill(&loaded.stories[0].ranges[0]), Some(text_paint.clone()));

    session.run_named("edit.undo", json!({})).expect("undo fill hatch");
    let object = session.document.get_object(ObjectId(1)).unwrap();
    assert_eq!(object.fill, Paint::Solid { colour: Colour::new(20, 20, 20, 255) });
    assert_eq!(object.stroke.paint, Paint::Hatch(stroke_hatch));
    assert_eq!(
        session
            .document
            .swatch_bindings
            .iter()
            .find(|binding| binding.target == SwatchTarget::Stroke)
            .map(|binding| binding.swatch),
        Some(swatch_id)
    );
    assert_eq!(range_fill(&session.document.stories[0].ranges[0]), Some(text_paint));
}
