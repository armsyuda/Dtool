use dtool_core::document::{Document, Story, Units};
use dtool_core::object::{Object, ObjectKind, PathTextData};
use dtool_core::scene::{estimate_scene, Prim};
use dtool_core::text_lines::is_text;
use dtool_core::{CanvasFlags, StoryId};

#[test]
fn path_text_is_text_and_emits_text_prim() {
    let mut doc = Document::new([200.0, 200.0], Units::Px, 72.0, 1);
    let path = doc.create_object(Object {
        pos: [10.0, 40.0],
        size: [120.0, 20.0],
        ..Object::default()
    });
    let story = StoryId(1);
    doc.stories.push(Story { id: story, text: "Path".into(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
    let id = doc.create_object(Object {
        kind: ObjectKind::PathText(PathTextData { path, story, flows: Vec::new(), source_paint: None }),
        ..Object::default()
    });
    assert!(is_text(&ObjectKind::PathText(PathTextData { path, story, flows: Vec::new(), source_paint: None })));
    let scene = estimate_scene(&doc, &CanvasFlags::default());
    let texts = scene.prims.iter().filter(|prim| matches!(prim, Prim::Text { object, .. } if *object == id)).count();
    assert!(texts >= 1, "PathText should emit Prim::Text, got {:?}", scene.counts());
}
