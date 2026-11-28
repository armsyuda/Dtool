use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::paint::{BitmapPaint, Paint};
use dtool_core::{Command, Scope, Workspace};
use serde_json::{json, Value};

fn run(ws: &mut Workspace, name: &str, params: Value) -> Value {
    ws.run(Scope::active(), Command::with_params(name, params)).1.unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn live() -> Workspace {
    let mut ws = Workspace::default();
    ws.new_document("c11 captions");
    ws
}

fn add(ws: &mut Workspace, object: Object) -> u64 {
    let id = ws.active().unwrap();
    ws.get_mut(id).unwrap().session.document.create_object(object).0
}

fn node(ws: &Workspace, value: &Value) -> Object {
    let id = ws.active().unwrap();
    ws.get(id).unwrap().session.document.get_object(ObjectId(value["text_node"].as_u64().expect("text_node"))).cloned().expect("caption node")
}

fn text(object: &Object) -> String {
    match &object.kind {
        ObjectKind::Text(text) => text.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn shape_captions_number_figures_below_and_undo_redo() {
    let mut ws = live();
    let a = add(&mut ws, Object { pos: [10.0, 10.0], size: [40.0, 30.0], ..Object::default() });
    let b = add(&mut ws, Object { pos: [100.0, 10.0], size: [40.0, 30.0], ..Object::default() });
    let first = run(&mut ws, "caption.add", json!({ "owner": a }));
    let cap = node(&ws, &first);
    assert_eq!(text(&cap), "Figure 1");
    assert_eq!(cap.pos, [10.0, 44.0]);
    assert_eq!(cap.parent_id, Some(ObjectId(a)));
    let second = run(&mut ws, "caption.add", json!({ "owner": b, "settings": { "side": "right" } }));
    let cap2 = node(&ws, &second);
    assert_eq!(text(&cap2), "Figure 2");
    assert_eq!(cap2.pos, [144.0, 10.0]);
    assert!(ws.run(Scope::active(), Command::with_params("caption.add", json!({ "owner": a }))).1.is_err());
    run(&mut ws, "edit.undo", json!({}));
    let id = ws.active().unwrap();
    assert!(ws.get(id).unwrap().session.document.get_object(cap2.id).is_none());
    run(&mut ws, "edit.redo", json!({}));
    let doc = &ws.get(id).unwrap().session.document;
    let redone = doc.objects.iter().find(|o| o.parent_id == Some(ObjectId(b))).expect("redo caption");
    assert_eq!((text(redone), redone.pos), ("Figure 2".to_string(), [144.0, 10.0]));
}

#[test]
fn image_caption_uses_the_file_name_and_rejects_non_images() {
    let mut ws = live();
    let image = add(&mut ws, Object { pos: [0.0, 0.0], size: [64.0, 48.0], fill: Paint::Bitmap(BitmapPaint { resource_id: "photos/cat.png".into(), matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], seed: None, single_image: false }), ..Object::default() });
    let shape = add(&mut ws, Object { pos: [0.0, 100.0], size: [10.0, 10.0], ..Object::default() });
    let value = run(&mut ws, "caption.add", json!({ "owner": image, "settings": { "from_name": true } }));
    assert_eq!(text(&node(&ws, &value)), "cat.png");
    let err = ws.run(Scope::active(), Command::with_params("caption.add", json!({ "owner": shape, "settings": { "from_name": true } }))).1.unwrap_err();
    assert_eq!(err.code, "INVALID_PARAMS");
}

#[test]
fn frame_caption_and_table_caption_above_with_its_own_counter() {
    let mut ws = live();
    let frame = run(&mut ws, "text.frame.add", json!({ "rect": [20, 200, 220, 300], "text": "Body" }))["id"].as_u64().unwrap();
    let table = run(&mut ws, "table.create", json!({ "rows": 2, "cols": 2, "bounds": [120.0, 60.0], "pos": [10.0, 100.0] }))["id"].as_u64().unwrap();
    let f = run(&mut ws, "caption.add", json!({ "owner": frame }));
    assert_eq!(text(&node(&ws, &f)), "Figure 1");
    let t = run(&mut ws, "caption.add", json!({ "owner": table }));
    let cap = node(&ws, &t);
    assert_eq!(text(&cap), "Table 1");
    assert_eq!(cap.pos[1], 100.0 - 20.0 - 4.0);
    run(&mut ws, "caption.remove", json!({ "owner": table }));
    run(&mut ws, "edit.undo", json!({}));
    let id = ws.active().unwrap();
    let doc = &ws.get(id).unwrap().session.document;
    let back = doc.objects.iter().find(|o| o.parent_id == Some(ObjectId(table)) && matches!(o.kind, ObjectKind::Text(_))).expect("undo remove");
    assert_eq!((text(back), back.pos), ("Table 1".to_string(), cap.pos));
}

#[test]
fn filler_fills_the_live_frame_story_and_undoes() {
    let mut ws = live();
    let frame = run(&mut ws, "text.frame.add", json!({ "rect": [20, 20, 220, 120], "text": "x" }));
    let (id, story) = (frame["id"].as_u64().unwrap(), frame["story"].as_u64());
    let value = run(&mut ws, "text.filler", json!({ "id": id, "text": "Lorem ipsum dolor sit amet. " }));
    assert!(value["length"].as_u64().unwrap() > 1);
    let ctx = ws.active().unwrap();
    let story = story.or_else(|| value["story"].as_u64()).unwrap();
    let filled = ws.get(ctx).unwrap().session.document.stories.iter().find(|s| s.id.0 == story).unwrap().text.clone();
    assert!(filled.starts_with("Lorem ipsum"), "{filled}");
    run(&mut ws, "edit.undo", json!({}));
    assert_eq!(ws.get(ctx).unwrap().session.document.stories.iter().find(|s| s.id.0 == story).unwrap().text, "x");
    assert!(ws.run(Scope::active(), Command::with_params("text.filler", json!({}))).1.is_err());
}
