use dtool_core::commands::colour_pick::{sample, PickMode};
use dtool_core::document::{Story, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::{Colour, Paint};
use dtool_core::{Document, StoryId};
use dtool_render::pick_sampler::pick;

fn glyph_outlines(doc: &Document, id: ObjectId) -> Option<Vec<Vec<[f32; 2]>>> {
    let object = doc.get_object(id)?;
    let [x, y] = object.pos;
    Some(vec![vec![[x, y], [x + 4.0, y], [x + 4.0, y + 10.0], [x, y + 10.0]], vec![[x + 10.0, y], [x + 14.0, y], [x + 14.0, y + 10.0], [x + 10.0, y + 10.0]]])
}

fn text_doc() -> (Document, ObjectId) {
    assert!(dtool_render::scene::install());
    assert!(dtool_core::text_lines::register_text_outlines(glyph_outlines));
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    let story = StoryId(1);
    let mut ranges = vec![serde_json::json!({ "start": 0, "end": 2 })];
    ranges[0]["fill"] = serde_json::to_value(Paint::Solid { colour: Colour::new(0, 160, 0, 255) }).unwrap();
    doc.stories.push(Story { id: story, text: "II".into(), ranges, specials: Vec::new(), paras: Vec::new() });
    let id = doc.create_object(Object { pos: [50.0, 50.0], size: [20.0, 12.0], kind: ObjectKind::ArtText { story }, ..Object::default() });
    (doc, id)
}

#[test]
fn glyph_is_picked_and_gap_is_not() {
    let (doc, id) = text_doc();
    let scene = dtool_render::pick_sampler::glyph_scene(&doc, &Default::default());
    assert!(scene.prims.iter().any(|p| matches!(p, dtool_core::scene::Prim::Fill { object, .. } if *object == id)));
    assert!(!scene.prims.iter().any(|p| matches!(p, dtool_core::scene::Prim::Text { object, .. } if *object == id)));
    let core_gap = sample(&doc, 57.0, 55.0, PickMode::Point, None, 1.0).unwrap();
    assert_ne!([core_gap.r, core_gap.g, core_gap.b], [0, 160, 0]);
    let core_glyph = sample(&doc, 52.0, 55.0, PickMode::Point, None, 1.0).unwrap();
    assert_eq!([core_glyph.r, core_glyph.g, core_glyph.b], [0, 160, 0]);
    let before = serde_json::to_string(&doc).unwrap();
    let on_glyph = pick(&doc, 52.0, 55.0, PickMode::Point, None).unwrap();
    assert_eq!([on_glyph.r, on_glyph.g, on_glyph.b, on_glyph.a], [0, 160, 0, 255]);
    let averaged = pick(&doc, 52.0, 55.0, PickMode::Average3, None).unwrap();
    assert_eq!([averaged.r, averaged.g, averaged.b], [0, 160, 0]);
    let gap = pick(&doc, 57.0, 55.0, PickMode::Point, None).unwrap();
    assert_ne!([gap.r, gap.g, gap.b], [0, 160, 0]);
    assert_eq!(serde_json::to_string(&doc).unwrap(), before);
}

#[test]
fn plain_fill_matches_core_sampler() {
    assert!(dtool_render::scene::install());
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    doc.create_object(Object { pos: [10.0, 10.0], size: [40.0, 40.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, ..Object::default() });
    for mode in [PickMode::Point, PickMode::Average3, PickMode::Average5] {
        let ours = pick(&doc, 30.0, 30.0, mode, None).unwrap();
        let core = sample(&doc, 30.0, 30.0, mode, None, 1.0).unwrap();
        assert_eq!([ours.r, ours.g, ours.b, ours.a], [core.r, core.g, core.b, core.a]);
        assert_eq!([ours.r, ours.g, ours.b, ours.a], [255, 0, 0, 255]);
    }
    let empty = pick(&doc, 150.0, 150.0, PickMode::Point, None).unwrap();
    let core_empty = sample(&doc, 150.0, 150.0, PickMode::Point, None, 1.0).unwrap();
    assert_eq!([empty.r, empty.g, empty.b, empty.a], [core_empty.r, core_empty.g, core_empty.b, core_empty.a]);
    assert!(pick(&doc, f32::NAN, 1.0, PickMode::Point, None).is_none());
}
