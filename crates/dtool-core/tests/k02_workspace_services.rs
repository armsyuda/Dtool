use dtool_core::snap_prefs::set_thread_path;
use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::Command;
use serde_json::json;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("k02_{}_{}_{}", tag, std::process::id(), nonce));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(ws: &mut Workspace, scope: Scope, name: &str, params: serde_json::Value) -> dtool_protocol::CommandResult {
    ws.run(scope, Command::with_params(name, params)).1
}

#[test]
fn snap_prefs_single_store_seen_by_direct_session_reads() {
    let dir = temp_dir("snap");
    set_thread_path(Some(dir.join("snap_prefs.json")));
    let mut ws = Workspace::default();
    let a = ws.new_document("a");
    let b = ws.new_document("b");
    run(&mut ws, Scope::of(a), "snap.prefs.set", json!({"enabled": false})).unwrap();
    let c = ws.new_document("c");
    for id in [a, b, c] {
        let session = &ws.get(id).unwrap().session;
        assert!(!session.snap_prefs.query().enabled, "context {id:?} must see the shared value");
        assert!(session.snap_prefs.is_shared_with(ws.snap_prefs().unwrap()));
        assert_eq!(session.document.version, 0, "prefs are not document edits");
    }
    set_thread_path(None);
}

#[test]
fn snap_prefs_failed_save_keeps_all_contexts_on_old_value() {
    let dir = temp_dir("snapfail");
    let path = dir.join("blocked");
    std::fs::create_dir_all(path.join("x")).unwrap();
    set_thread_path(Some(path.clone()));
    let mut ws = Workspace::default();
    let a = ws.new_document("a");
    let b = ws.new_document("b");
    let before = ws.get(b).unwrap().session.snap_prefs.query().screen_tolerance;
    let result = run(&mut ws, Scope::of(a), "snap.prefs.set", json!({"screen_tolerance": before + 7}));
    assert!(result.is_err());
    assert_eq!(ws.get(a).unwrap().session.snap_prefs.query().screen_tolerance, before);
    assert_eq!(ws.get(b).unwrap().session.snap_prefs.query().screen_tolerance, before);
    set_thread_path(None);
}

#[test]
fn template_and_recovered_contexts_share_app_clipboard() {
    let dir = temp_dir("tpl");
    let mut ws = Workspace::default();
    let a = ws.new_document("a");
    let tpl = dir.join("t.dtool-template");
    run(&mut ws, Scope::of(a), "doc.template.save", json!({"path": tpl.to_string_lossy()})).unwrap();
    run(&mut ws, Scope::active(), "doc.template.open", json!({"path": tpl.to_string_lossy()})).unwrap();
    for ctx in ws.contexts() {
        assert!(ctx.session.clipboard.is_shared_with(ws.app_clipboard()), "context {:?}", ctx.id);
    }
}

#[test]
fn activate_ends_text_edit_of_previous_context() {
    let mut ws = Workspace::default();
    let a = ws.new_document("a");
    let b = ws.new_document("b");
    ws.activate(a).unwrap();
    {
        let session = &mut ws.get_mut(a).unwrap().session;
        let story = session.document.alloc_story_id();
        session.document.stories.push(dtool_core::document::Story { id: story, text: "hi".into(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
        session.text_edit = Some(dtool_core::command::TextEditState { story, object: None, caret: 2, anchor: 2, before: None });
    }
    let undo_before = ws.get(a).unwrap().session.undo.can_undo();
    ws.activate(a).unwrap();
    assert!(ws.get(a).unwrap().session.text_edit.is_some(), "re-activating the same tab changes nothing");
    ws.activate(b).unwrap();
    let ctx = ws.get(a).unwrap();
    assert!(ctx.session.text_edit.is_none());
    assert_eq!(ctx.session.document.stories[0].text, "hi");
    assert_eq!(ctx.session.undo.can_undo(), undo_before);
}

#[test]
fn routed_command_does_not_overwrite_view_flags_set_on_context() {
    let mut ws = Workspace::default();
    let a = ws.new_document("a");
    let mut flagged = ws.get(a).unwrap().view_state.clone();
    flagged.zoom = 3.0;
    *ws.view_mut(a).unwrap() = flagged.clone();
    run(&mut ws, Scope::of(a), "app.status", json!({})).unwrap();
    assert_eq!(ws.get(a).unwrap().view_state.zoom, 3.0);
    assert_eq!(ws.get(a).unwrap().session.view.zoom, 3.0);
}

#[test]
fn save_with_foreign_path_is_refused_and_keeps_modified() {
    let dir = temp_dir("save");
    let mut ws = Workspace::default();
    let a = ws.new_document("a");
    let owned = dir.join("p.dtool");
    ws.save_as(a, &owned).unwrap();
    run(&mut ws, Scope::of(a), "object.create", json!({"pos": [0.0, 0.0], "size": [1.0, 1.0]})).unwrap();
    assert!(ws.get(a).unwrap().is_modified());
    let other = dir.join("q.dtool");
    let result = run(&mut ws, Scope::of(a), "doc.save", json!({"path": other.to_string_lossy()}));
    assert_eq!(result.unwrap_err().code, "INVALID_PARAMS");
    assert!(ws.get(a).unwrap().is_modified());
    assert!(!other.exists());
    assert_eq!(ws.get(a).unwrap().path.as_deref(), Some(owned.as_path()));
    run(&mut ws, Scope::of(a), "doc.save", json!({"path": owned.to_string_lossy()})).unwrap();
    assert!(!ws.get(a).unwrap().is_modified());
}
