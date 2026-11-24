use std::sync::Arc;
use dtool_core::document::Units;
use dtool_core::frame::{flow_frames, frame_story};
use dtool_core::serde_json::{json, Value};
use dtool_core::{Command, Document, Object, ObjectId, ObjectKind, Scope, Session, SharedServices, VAlign, Workspace};

fn session() -> Session {
    Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1))
}

fn run(session: &mut Session, name: &str, params: Value) -> Value {
    session.run(&Command::with_params(name, params)).unwrap_or_else(|error| panic!("{name} failed: {error:?}"))
}

fn frame(session: &mut Session, rect: [f32; 4], text: &str) -> ObjectId {
    ObjectId(run(session, "text.frame.add", json!({ "rect": rect, "text": text }))["id"].as_u64().unwrap())
}

fn story_text(session: &Session, id: ObjectId) -> String {
    let story = frame_story(&session.document, id).expect("frame has a story");
    session.document.stories.iter().find(|item| item.id == story).map(|item| item.text.clone()).unwrap_or_default()
}

#[test]
fn drag_frame_and_shape_text_creation() {
    let mut session = session();
    let before = session.document.objects.len();
    let id = frame(&mut session, [20.0, 30.0, 160.0, 80.0], "");
    let object = session.document.get_object(id).cloned().unwrap();
    assert!(matches!(object.kind, ObjectKind::FrameText { .. }));
    assert_eq!(object.pos, [20.0, 30.0]);
    assert_eq!(object.size, [160.0, 80.0]);
    assert_eq!(object.frame_props().unwrap().size, [160.0, 80.0]);
    let edit = session.text_edit.as_ref().expect("new frame starts editing");
    assert_eq!(edit.object, Some(id));
    assert_eq!(Some(edit.story), frame_story(&session.document, id));
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(session.document.objects.len(), before);
    assert!(session.document.get_object(id).is_none());
    run(&mut session, "edit.redo", json!({}));
    assert!(session.document.get_object(id).is_some());

    let shape = session.document.create_object(Object { pos: [200.0, 40.0], size: [120.0, 90.0], kind: ObjectKind::Shape, ..Object::default() });
    session.text_edit = None;
    let result = run(&mut session, "text.shape_text.add", json!({ "shape": shape.0 }));
    assert_eq!(result["id"].as_u64(), Some(shape.0));
    let converted = session.document.get_object(shape).cloned().unwrap();
    assert!(matches!(converted.kind, ObjectKind::FrameText { .. }));
    let props = converted.frame_props().unwrap();
    assert_eq!(props.shape, Some(shape));
    assert!(props.shape_path.as_ref().is_some_and(|path| path.len() >= 3));
    assert_eq!(converted.pos, [200.0, 40.0]);
    assert_eq!(session.text_edit.as_ref().and_then(|edit| edit.object), Some(shape));
    run(&mut session, "edit.undo", json!({}));
    assert!(matches!(session.document.get_object(shape).unwrap().kind, ObjectKind::Shape));
    assert!(session.document.get_object(shape).unwrap().frame.is_none());
}

#[test]
fn overflow_then_link_and_unlink() {
    let mut session = session();
    let first = frame(&mut session, [0.0, 0.0, 60.0, 20.0], "Long text that cannot fit in one small frame");
    let second = frame(&mut session, [0.0, 40.0, 60.0, 20.0], "");
    let third = frame(&mut session, [0.0, 80.0, 60.0, 20.0], "tail");
    session.text_edit = None;
    let story = frame_story(&session.document, first).unwrap();
    let stories = session.document.stories.len();
    let linked = run(&mut session, "text.link", json!({ "from": first.0, "to": second.0 }));
    assert_eq!(linked["frames"], json!([first.0, second.0]));
    run(&mut session, "text.link", json!({ "from": second.0, "to": third.0 }));
    assert_eq!(flow_frames(&session.document, story), vec![first, second, third]);
    for id in [first, second, third] {
        assert_eq!(frame_story(&session.document, id), Some(story));
    }
    assert_eq!(story_text(&session, first), "Long text that cannot fit in one small frametail");
    assert_eq!(session.document.stories.len(), stories - 2);
    let error = session.run(&Command::with_params("text.link", json!({ "from": first.0, "to": second.0 }))).expect_err("frame already in chain");
    assert!(!error.code.is_empty());
    let unlinked = run(&mut session, "text.unlink", json!({ "id": second.0 }));
    assert_eq!(unlinked["detached"], json!([second.0, third.0]));
    assert!(session.document.frame_chains.iter().all(|chain| chain.story != story || chain.frames == vec![first]), "{:?}", session.document.frame_chains);
    assert_eq!(flow_frames(&session.document, story), vec![first]);
    assert_ne!(frame_story(&session.document, second), Some(story));
    assert_ne!(frame_story(&session.document, third), Some(story));
    assert_ne!(frame_story(&session.document, second), frame_story(&session.document, third));
    assert_eq!(story_text(&session, first), "Long text that cannot fit in one small frametail");
    assert_eq!(story_text(&session, second), "");
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(flow_frames(&session.document, story), vec![first, second, third]);
    run(&mut session, "edit.undo", json!({}));
    run(&mut session, "edit.undo", json!({}));
    assert_eq!(flow_frames(&session.document, story), vec![first]);
    assert_eq!(story_text(&session, first), "Long text that cannot fit in one small frame");
    assert_eq!(story_text(&session, third), "tail");
    assert_eq!(session.document.stories.len(), stories);
}

#[test]
fn columns_insets_valign_auto_height() {
    let mut session = session();
    let id = frame(&mut session, [10.0, 10.0, 200.0, 100.0], "Body");
    session.text_edit = None;
    let generation = session.generation;
    run(&mut session, "text.frame.set", json!({ "id": id.0, "columns": 2, "gutter": 12.0, "insets": [4.0, 5.0, 6.0, 7.0], "valign": "Center", "auto_height": true }));
    assert_eq!(session.generation, generation + 1);
    let props = session.document.get_object(id).unwrap().frame_props().unwrap();
    assert_eq!(props.columns, 2);
    assert_eq!(props.gutter, 12.0);
    assert_eq!(props.insets, [4.0, 5.0, 6.0, 7.0]);
    assert_eq!(props.valign, VAlign::Center);
    assert!(props.auto_height);
    run(&mut session, "text.frame.set", json!({ "id": id.0, "valign": "Bottom", "size": [240.0, 120.0] }));
    let props = session.document.get_object(id).unwrap().frame_props().unwrap();
    assert_eq!(props.valign, VAlign::Bottom);
    assert_eq!(props.columns, 2);
    assert_eq!(session.document.get_object(id).unwrap().size, [240.0, 120.0]);
    let snapshot = serde_json::to_value(&session.document.objects).unwrap();
    for bad in [json!({ "id": id.0, "columns": 0 }), json!({ "id": id.0, "gutter": -1.0 }), json!({ "id": id.0, "insets": [-1.0, 0.0, 0.0, 0.0] }), json!({ "id": id.0, "column_widths": [10.0] })] {
        assert!(session.run(&Command::with_params("text.frame.set", bad)).is_err());
        assert_eq!(serde_json::to_value(&session.document.objects).unwrap(), snapshot);
    }
    run(&mut session, "edit.undo", json!({}));
    let props = session.document.get_object(id).unwrap().frame_props().unwrap();
    assert_eq!(props.valign, VAlign::Center);
    assert_eq!(session.document.get_object(id).unwrap().size, [200.0, 100.0]);
    run(&mut session, "edit.undo", json!({}));
    let props = session.document.get_object(id).unwrap().frame_props().unwrap();
    assert_eq!(props.columns, 1);
    assert_eq!(props.insets, [0.0; 4]);
    assert_eq!(props.valign, VAlign::Top);
    assert!(!props.auto_height);
}

#[test]
fn editing_and_context_bar_on_frames() {
    let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
    let context = workspace.new_document("Frames");
    let (_, added) = workspace.run(Scope::active(), Command::with_params("text.frame.add", json!({ "rect": [0.0, 0.0, 120.0, 40.0] })));
    let added = added.expect("frame add on the live workspace");
    let id = ObjectId(added["id"].as_u64().unwrap());
    let story = added["story"].as_u64().unwrap();
    let (_, inserted) = workspace.run(Scope::active(), Command::with_params("text.insert", json!({ "story": story, "at": 0, "text": "Hello" })));
    inserted.expect("typing into the frame");
    let session = &workspace.get(context).unwrap().session;
    assert_eq!(story_text(session, id), "Hello");
    let edit = session.text_edit.as_ref().expect("still editing the frame");
    assert_eq!(edit.object, Some(id));
    assert_eq!(edit.caret, 5);
    assert!(session.can_execute_named("text.frame.set"));
    let (_, set) = workspace.run(Scope::active(), Command::with_params("text.frame.set", json!({ "id": id.0, "columns": 3 })));
    set.expect("frame options from the context bar");
    assert_eq!(workspace.get(context).unwrap().session.document.get_object(id).unwrap().frame_props().unwrap().columns, 3);
    workspace.run(Scope::active(), Command::new("edit.undo")).1.expect("undo frame options");
    workspace.run(Scope::active(), Command::new("edit.undo")).1.expect("undo typing");
    let session = &workspace.get(context).unwrap().session;
    assert_eq!(session.document.get_object(id).unwrap().frame_props().unwrap().columns, 1);
    assert_eq!(story_text(session, id), "");
}