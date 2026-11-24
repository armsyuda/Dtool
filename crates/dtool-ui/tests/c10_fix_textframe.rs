#[allow(dead_code)]
#[path = "../src/panels/text_frame.rs"]
mod text_frame;

use dtool_core::command::Session;
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::{FrameProps, ObjectId, VAlign};
use dtool_protocol::Command;
use serde_json::json;
use text_frame::{read_selection, Field, BATCH_COMMAND, FRAME_SET_COMMAND};

fn frame(session: &mut Session, story: u64, props: FrameProps) -> ObjectId {
    session.document.stories.push(Story { id: StoryId(story), text: "abc".to_string(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
    session.document.create_object(Object { kind: ObjectKind::FrameText { story: StoryId(story) }, size: props.size, frame: Some(props), ..Object::default() })
}

fn session_with(props: Vec<FrameProps>) -> (Session, Vec<ObjectId>) {
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let ids: Vec<ObjectId> = props.into_iter().enumerate().map(|(i, p)| frame(&mut session, 80 + i as u64, p)).collect();
    session.selection.ids = ids.clone();
    (session, ids)
}

fn props_of(session: &Session, id: ObjectId) -> FrameProps {
    session.document.get_object(id).unwrap().frame_props().unwrap()
}

fn base() -> FrameProps {
    let mut props = FrameProps::from_rect([200.0, 100.0]);
    props.columns = 2;
    props.gutter = 6.0;
    props.insets = [1.0, 2.0, 3.0, 4.0];
    props
}

fn undo(session: &mut Session) {
    session.run(&Command::new("edit.undo")).unwrap();
}

#[test]
fn valign_change_emits_frame_set_and_is_one_undo() {
    let (mut s, ids) = session_with(vec![base()]);
    let before = props_of(&s, ids[0]);
    let selection = read_selection(&s).unwrap();
    assert_eq!(selection.valign().as_deref(), Some("Top"));
    let command = selection.set("valign", json!("Bottom")).unwrap();
    assert_eq!(command.name, FRAME_SET_COMMAND);
    assert_eq!(command.params, json!({"id": ids[0].0, "valign": "Bottom"}));
    s.run(&command).unwrap();
    let after = props_of(&s, ids[0]);
    assert_eq!(after.valign, VAlign::Bottom);
    assert_eq!(FrameProps { valign: VAlign::Top, ..after }, before);
    undo(&mut s);
    assert_eq!(props_of(&s, ids[0]), before);
}

#[test]
fn inset_change_keeps_other_edges_and_is_one_undo() {
    let (mut s, ids) = session_with(vec![base()]);
    let before = props_of(&s, ids[0]);
    let command = read_selection(&s).unwrap().set_inset(1, 9.0).unwrap();
    assert_eq!(command.name, FRAME_SET_COMMAND);
    s.run(&command).unwrap();
    assert_eq!(props_of(&s, ids[0]).insets, [1.0, 9.0, 3.0, 4.0]);
    assert_eq!(props_of(&s, ids[0]).columns, 2);
    undo(&mut s);
    assert_eq!(props_of(&s, ids[0]), before);
    assert!(read_selection(&s).unwrap().set_inset(0, -1.0).is_none());
}

#[test]
fn no_op_and_cancel_leave_document_version_unchanged() {
    let (s, _) = session_with(vec![base()]);
    let version = s.document.version;
    let generation = s.generation;
    let selection = read_selection(&s).unwrap();
    assert!(selection.set("valign", json!("Top")).is_none());
    assert!(selection.set_inset(1, 2.0).is_none());
    assert!(selection.set_columns(2.0).is_none());
    assert!(selection.set_gutter(6.0).is_none());
    assert!(selection.set_even_columns(true).is_none());
    assert!(selection.set("auto_height", json!(false)).is_none());
    assert_eq!(s.document.version, version);
    assert_eq!(s.generation, generation);
}

#[test]
fn mixed_frames_keep_unedited_values_and_batch_is_one_undo() {
    let mut other = base();
    other.valign = VAlign::Center;
    other.insets = [5.0, 2.0, 7.0, 8.0];
    let (mut s, ids) = session_with(vec![base(), other]);
    let before: Vec<FrameProps> = ids.iter().map(|id| props_of(&s, *id)).collect();
    let selection = read_selection(&s).unwrap();
    assert_eq!(selection.field("valign"), Field::Mixed);
    assert_eq!(selection.inset(0), None);
    assert_eq!(selection.inset(1), Some(2.0));
    let command = selection.set_inset(0, 10.0).unwrap();
    assert_eq!(command.name, BATCH_COMMAND);
    s.run(&command).unwrap();
    assert_eq!(props_of(&s, ids[0]).insets, [10.0, 2.0, 3.0, 4.0]);
    assert_eq!(props_of(&s, ids[1]).insets, [10.0, 2.0, 7.0, 8.0]);
    assert_eq!(props_of(&s, ids[1]).valign, VAlign::Center);
    undo(&mut s);
    let restored: Vec<FrameProps> = ids.iter().map(|id| props_of(&s, *id)).collect();
    assert_eq!(restored, before);
}

#[test]
fn even_columns_and_column_count_stay_valid() {
    let (mut s, ids) = session_with(vec![base()]);
    let uneven = read_selection(&s).unwrap().set_even_columns(false).unwrap();
    s.run(&uneven).unwrap();
    let widths = props_of(&s, ids[0]).column_widths.unwrap();
    assert_eq!(widths.len(), 2);
    assert!((widths[0] - 95.0).abs() < 0.01);
    s.run(&read_selection(&s).unwrap().set_columns(3.0).unwrap()).unwrap();
    let props = props_of(&s, ids[0]);
    assert_eq!(props.columns, 3);
    assert_eq!(props.column_widths.as_ref().map(Vec::len), Some(3));
    assert!(read_selection(&s).unwrap().set_columns(0.0).is_none());
    s.run(&read_selection(&s).unwrap().set_even_columns(true).unwrap()).unwrap();
    assert_eq!(props_of(&s, ids[0]).column_widths, None);
    s.run(&read_selection(&s).unwrap().set("auto_height", json!(true)).unwrap()).unwrap();
    assert!(props_of(&s, ids[0]).auto_height);
}