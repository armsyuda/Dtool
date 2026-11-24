use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::workspace::{CloseAnswer, ContextId, Scope, Workspace};
use dtool_protocol::Command;
use serde_json::{Value, json};
use std::path::PathBuf;

fn fixture() -> (Workspace, ContextId) {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("path-text-owned");
    let doc = &mut workspace.get_mut(id).unwrap().session.document;
    *doc = Document::new([72.0, 72.0], Units::Pt, 72.0, 1);
    doc.stories.push(Story { id: StoryId(7), text: "ABCD".into(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
    let shape = serde_json::from_value(json!({"kind": "rectangle", "size": [56.0, 16.0]})).unwrap();
    doc.objects.push(Object { id: ObjectId(11), name: "source-path".into(), pos: [8.0, 36.0], size: [56.0, 16.0], shape: Some(shape), ..Object::default() });
    doc.objects.push(Object { id: ObjectId(12), name: "owned-text".into(), pos: [8.0, 36.0], size: [24.0, 12.0], kind: ObjectKind::ArtText { story: StoryId(7) }, ..Object::default() });
    doc.next_object_id = 13;
    (workspace, id)
}

fn run(workspace: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
    let (target, result) = workspace.run(Scope::of(id), Command::with_params(name, params));
    assert_eq!(target, Some(id));
    result.unwrap()
}

fn owned_state(workspace: &Workspace, id: ContextId) -> Value {
    let doc = &workspace.get(id).unwrap().session.document;
    json!({"objects": doc.objects, "stories": doc.stories})
}

fn assert_owned(workspace: &Workspace, id: ContextId) {
    let doc = &workspace.get(id).unwrap().session.document;
    assert!(doc.objects.iter().any(|object| object.id == ObjectId(11)));
    let ObjectKind::PathText(data) = &doc.get_object(ObjectId(12)).unwrap().kind else { panic!("attach must publish canonical PathText"); };
    assert_eq!(data.path, ObjectId(11));
    assert_eq!(data.story, StoryId(7));
    assert_eq!(doc.stories.iter().filter(|story| story.id == StoryId(7)).count(), 1);
    assert_eq!(doc.stories.iter().find(|story| story.id == StoryId(7)).unwrap().text, "ABCD");
}

fn attach(workspace: &mut Workspace, id: ContextId) {
    run(workspace, id, "text.path.attach", json!({"text": 12, "path": 11}));
}

struct NativeFile { directory: PathBuf, path: PathBuf }
impl NativeFile {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("dtool_path_ownership_{}_{}", std::process::id(), nonce));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("roundtrip.dtool");
        Self { directory, path }
    }
}
impl Drop for NativeFile {
    fn drop(&mut self) {
        assert_eq!(self.directory.parent(), Some(std::env::temp_dir().as_path()));
        assert!(self.directory.file_name().unwrap().to_string_lossy().starts_with("dtool_path_ownership_"));
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_file_name("roundtrip.dtool.tmp"));
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[test]
fn attach_keeps_source_path_id_and_story_id_and_publishes_owned_path_text() {
    let (mut workspace, id) = fixture();
    let source = serde_json::to_value(workspace.get(id).unwrap().session.document.get_object(ObjectId(11)).unwrap()).unwrap();
    attach(&mut workspace, id);
    assert_owned(&workspace, id);
    assert_eq!(serde_json::to_value(workspace.get(id).unwrap().session.document.get_object(ObjectId(11)).unwrap()).unwrap(), source);
}

#[test]
fn native_save_open_round_trip_preserves_path_story_flow_and_source_ownership() {
    let (mut workspace, id) = fixture();
    attach(&mut workspace, id);
    run(&mut workspace, id, "text.path.set", json!({"id": 12, "flows": [{"start_pct": 0.125, "end_pct": 0.875, "baseline_pct": 0.25, "reversed": true, "flipped": true}]}));
    let expected = owned_state(&workspace, id);
    let file = NativeFile::new();
    run(&mut workspace, id, "doc.save_as", json!({"path": file.path}));
    assert!(workspace.close_with(id, CloseAnswer::No).unwrap());
    let (opened, result) = workspace.run(Scope::active(), Command::with_params("doc.open", json!({"path": file.path})));
    result.unwrap();
    let reopened = opened.unwrap();
    assert_ne!(reopened, id);
    assert_owned(&workspace, reopened);
    assert_eq!(owned_state(&workspace, reopened), expected);
    assert!(workspace.close_with(reopened, CloseAnswer::No).unwrap());
}

#[test]
fn attach_and_flow_edit_are_separate_atomic_undo_redo_steps() {
    let (mut workspace, id) = fixture();
    let original = owned_state(&workspace, id);
    let depth = workspace.get(id).unwrap().session.undo.applied_steps();
    attach(&mut workspace, id);
    let attached = owned_state(&workspace, id);
    assert_eq!(workspace.get(id).unwrap().session.undo.applied_steps(), depth + 1);
    run(&mut workspace, id, "text.path.set", json!({"id": 12, "flows": [{"start_pct": 0.2, "end_pct": 0.9, "baseline_pct": 0.25, "reversed": false, "flipped": true}]}));
    let edited = owned_state(&workspace, id);
    assert_ne!(edited, attached);
    assert_eq!(workspace.get(id).unwrap().session.undo.applied_steps(), depth + 2);
    for expected in [&attached, &original] {
        run(&mut workspace, id, "edit.undo", json!({}));
        assert_eq!(&owned_state(&workspace, id), expected);
    }
    for expected in [&attached, &edited] {
        run(&mut workspace, id, "edit.redo", json!({}));
        assert_eq!(&owned_state(&workspace, id), expected);
        assert_owned(&workspace, id);
    }
    assert_eq!(workspace.get(id).unwrap().session.undo.applied_steps(), depth + 2);
}
