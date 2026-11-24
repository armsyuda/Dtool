use dtool_core::document::{Document, Story as DocumentStory, StoryId as DocumentStoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind, PathFlow as OwnedPathFlow, PathTextData as OwnedPathTextData};
use dtool_text::fonts::FontLibrary;
use dtool_text::path_text::{as_path_text, kind_name, layout_path_text, line_segment, path_point_at, PathFlow, PathTextData};
use dtool_text::story::{CharStyle, Run, Story, StoryId, TextRange};

type Segment = [[f32; 2]; 4];

fn close(actual: f32, expected: f32, tolerance: f32) {
    assert!((actual - expected).abs() <= tolerance, "actual={actual}, expected={expected}");
}

fn angle_close(actual: f32, expected: f32) {
    let delta = (actual - expected).sin().atan2((actual - expected).cos());
    close(delta, 0.0, 0.1f32.to_radians());
}

fn rect_path(x: f32, y: f32, w: f32, h: f32) -> Vec<Segment> {
    vec![
        line_segment([x, y], [x + w, y]),
        line_segment([x + w, y], [x + w, y + h]),
        line_segment([x + w, y + h], [x, y + h]),
        line_segment([x, y + h], [x, y]),
    ]
}

fn circle_path(cx: f32, cy: f32, r: f32) -> Vec<Segment> {
    let k = 0.552_284_8 * r;
    vec![
        [[cx, cy - r], [cx + k, cy - r], [cx + r, cy - k], [cx + r, cy]],
        [[cx + r, cy], [cx + r, cy + k], [cx + k, cy + r], [cx, cy + r]],
        [[cx, cy + r], [cx - k, cy + r], [cx - r, cy + k], [cx - r, cy]],
        [[cx - r, cy], [cx - r, cy - k], [cx - k, cy - r], [cx, cy - r]],
    ]
}

fn world_point(point: [f32; 2]) -> [f32; 2] {
    [100.0 - 3.0 * point[1], 200.0 + 2.0 * point[0]]
}

#[test]
fn arc_point_and_tangent_on_line_match_local_distance() {
    let path = rect_path(8.0, 36.0, 56.0, 8.0);
    let (point, angle) = path_point_at(&path, 17.0);
    close(point[0], 25.0, 0.01);
    close(point[1], 36.0, 0.01);
    angle_close(angle, 0.0);
}

#[test]
fn arc_point_and_tangent_on_circle_match_quarter_turn() {
    let path = circle_path(20.0, 30.0, 10.0);
    let (point, angle) = path_point_at(&path, 5.0 * std::f32::consts::PI);
    close(point[0], 30.0, 0.05);
    close(point[1], 30.0, 0.05);
    angle_close(angle, std::f32::consts::FRAC_PI_2);
}

#[test]
fn bezier_arc_distance_is_not_linear_parameter_interpolation() {
    let path = vec![[[0.0, 0.0], [0.0, 0.0], [10.0, 0.0], [10.0, 0.0]]];
    let (point, angle) = path_point_at(&path, 2.5);
    close(point[0], 2.5, 0.01);
    close(point[1], 0.0, 0.01);
    angle_close(angle, 0.0);
}

#[test]
fn affine_adapter_transforms_locally_laid_out_glyph_origins_and_tangents_once() {
    let lib = FontLibrary::system();
    assert!(lib.resolve("Arial", "Regular").is_some(), "Arial Regular fixture required");
    let story = Story { id: StoryId(7), text: "AB".into(), chars: vec![Run { range: TextRange::new(0, 2), style: CharStyle { family: "Arial".into(), style: "Regular".into(), size_pt: 12.0, ..CharStyle::default() } }], ..Story::default() };
    let data = PathTextData { path: ObjectId(1), story: story.id, flows: vec![PathFlow { baseline_pct: 0.0, ..PathFlow::default() }], source_paint: None };
    let local_path = rect_path(8.0, 36.0, 100.0, 20.0);
    let local = layout_path_text(&lib, &data, &story, &local_path);
    assert!(local.overset.is_none());
    assert!(!local.glyphs.is_empty());
    let before: Vec<_> = local.glyphs.iter().map(|glyph| (glyph.position, glyph.angle)).collect();
    for glyph in &local.glyphs {
        let tip = [glyph.position[0] + glyph.angle.cos(), glyph.position[1] + glyph.angle.sin()];
        let world = vec![line_segment(world_point(glyph.position), world_point(tip))];
        let (point, angle) = path_point_at(&world, 0.0);
        close(point[0], 100.0 - 3.0 * glyph.position[1], 0.01);
        close(point[1], 200.0 + 2.0 * glyph.position[0], 0.01);
        angle_close(angle, (2.0 * glyph.angle.cos()).atan2(-3.0 * glyph.angle.sin()));
    }
    assert_eq!(local.glyphs.iter().map(|glyph| (glyph.position, glyph.angle)).collect::<Vec<_>>(), before);
}

fn owned_document() -> Document {
    let mut doc = Document::new([200.0, 100.0], Units::Pt, 72.0, 1);
    for (source, text, story) in [(1, 2, 7), (3, 4, 8)] {
        doc.objects.push(Object { id: ObjectId(source), name: format!("path-{source}"), size: [56.0, 16.0], ..Object::default() });
        doc.objects.push(Object { id: ObjectId(text), name: format!("text-{text}"), kind: ObjectKind::PathText(OwnedPathTextData { path: ObjectId(source), story: DocumentStoryId(story), ..OwnedPathTextData::default() }), ..Object::default() });
        doc.stories.push(DocumentStory { id: DocumentStoryId(story), text: format!("story-{story}"), ranges: Vec::new(), ..DocumentStory::default() });
    }
    doc.next_object_id = 5;
    doc
}

fn no_dangling_text(doc: &Document) {
    for object in &doc.objects {
        if let ObjectKind::PathText(data) = &object.kind {
            assert!(doc.objects.iter().any(|source| source.id == data.path));
            assert!(doc.stories.iter().any(|story| story.id == data.story));
        }
    }
}

#[test]
fn owned_path_text_converts_to_text_layer_and_back_without_loss() {
    let doc = owned_document();
    no_dangling_text(&doc);
    for object in &doc.objects {
        if let ObjectKind::PathText(owned) = &object.kind {
            let local = as_path_text(&object.kind).unwrap();
            assert_eq!(local.path, owned.path);
            assert_eq!(local.story.0, owned.story.0);
            assert_eq!(local.flows.len(), owned.flows.len());
            assert_eq!(&OwnedPathTextData::from(&local), owned);
        } else {
            assert!(as_path_text(&object.kind).is_none());
        }
    }
}

#[test]
fn path_text_kind_names_distinguish_sources_from_text_objects() {
    let doc = owned_document();
    let names: Vec<_> = doc.objects.iter().map(|object| kind_name(&object.kind)).collect();
    assert_eq!(names.iter().filter(|name| **name == "PathText").count(), 2);
    assert_eq!(names.iter().filter(|name| **name != "PathText").count(), 2);
}

#[test]
fn path_flow_round_trip_preserves_reversed_and_flipped_flags() {
    let local = PathFlow { start_pct: 0.25, end_pct: 0.75, baseline_pct: 0.1, reversed: true, flipped: true };
    let owned = OwnedPathFlow::from(&local);
    assert_eq!(PathFlow::from(&owned), local);
    assert!(owned.reversed && owned.flipped);
}