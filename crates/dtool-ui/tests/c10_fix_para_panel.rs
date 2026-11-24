use dtool_core::workspace::Workspace;
use dtool_tools::dtool_text::{story_from_core, Align};
use dtool_ui::panels::paragraph::{apply_patch, drag_commit, mixed_fields, paragraph_ranges, query_style};
use serde_json::json;

fn workspace_with_text() -> (Workspace, dtool_core::ObjectId) {
    let mut workspace = Workspace::default();
    workspace.new_document("p");
    let session = workspace.session_mut().unwrap();
    session.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "one\ntwo"})).unwrap();
    let id = session.document.objects.last().unwrap().id;
    session.selection.ids = vec![id];
    (workspace, id)
}

fn aligns(session: &dtool_core::Session, id: dtool_core::ObjectId) -> Vec<Align> {
    let story_id = session.document.get_object(id).unwrap().story_id().unwrap();
    let story = session.document.stories.iter().find(|s| s.id == story_id).unwrap();
    let core = story_from_core(story);
    let chars = story.text.chars().count();
    [0usize, 4].iter().map(|at| core.paras.iter().find(|r| r.range.start <= *at && *at < r.range.end.max(r.range.start + 1).min(chars.max(1))).map(|r| r.style.align).unwrap_or(Align::Left)).collect()
}

#[test]
fn leading_change_emits_text_para_set_with_one_undo() {
    let (mut workspace, _) = workspace_with_text();
    let session = workspace.session_mut().unwrap();
    let depth = session.undo.applied_steps();
    let mut drag = None;
    let mut sent = Vec::new();
    for (value, changed, dragging, stopped) in [(14.0, true, true, false), (16.0, true, true, false), (18.0, true, true, false), (18.0, false, false, true)] {
        if let Some(v) = drag_commit(&mut drag, "leading", value, changed, dragging, stopped) {
            sent.push(v);
            apply_patch(session, json!({"leading": v})).unwrap();
        }
    }
    assert_eq!(sent, vec![18.0]);
    assert_eq!(query_style(session).unwrap().leading, 18.0);
    assert_eq!(session.undo.applied_steps(), depth + 1);
    session.run_named("edit.undo", json!({})).unwrap();
    assert_ne!(query_style(session).unwrap().leading, 18.0);
    assert_eq!(drag_commit(&mut drag, "leading", 9.0, true, false, false), Some(9.0));
    assert_eq!(drag_commit(&mut drag, "leading", 9.0, false, false, false), None);
}

#[test]
fn alignment_change_is_one_undo() {
    let (mut workspace, id) = workspace_with_text();
    let session = workspace.session_mut().unwrap();
    let depth = session.undo.applied_steps();
    apply_patch(session, json!({"align": Align::Right})).unwrap();
    assert_eq!(session.undo.applied_steps(), depth + 1);
    assert_eq!(aligns(session, id), vec![Align::Right, Align::Right]);
    session.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(aligns(session, id), vec![Align::Left, Align::Left]);
}

#[test]
fn mixed_selection_keeps_unprovided_fields() {
    let (mut workspace, id) = workspace_with_text();
    let session = workspace.session_mut().unwrap();
    let (story, ranges) = paragraph_ranges(session).unwrap();
    assert_eq!(ranges, vec![[0, 4], [4, 7]]);
    session.run_named("text.para.set", json!({"story": story.0, "range": [0, 4], "patch": {"align": Align::Center, "space_before": 3.0}})).unwrap();
    let mixed = mixed_fields(session);
    assert!(mixed.contains(&"align") && mixed.contains(&"space_before") && !mixed.contains(&"leading"), "{mixed:?}");
    apply_patch(session, json!({"leading": 20.0})).unwrap();
    assert_eq!(aligns(session, id), vec![Align::Center, Align::Left]);
    let first = session.run_named("text.para.query", json!({"story": story.0, "range": [0, 4]})).unwrap();
    let second = session.run_named("text.para.query", json!({"story": story.0, "range": [4, 7]})).unwrap();
    assert_eq!(first["style"]["space_before"], json!(3.0));
    assert_ne!(second["style"]["space_before"], json!(3.0));
    assert_eq!(first["style"]["leading"], json!(20.0));
    assert_eq!(second["style"]["leading"], json!(20.0));
    assert!(!mixed_fields(session).contains(&"leading"));
}
