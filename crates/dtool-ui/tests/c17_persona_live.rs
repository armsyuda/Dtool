use dtool_core::document::{Document, Units};
use dtool_core::workspace::{host_channel, ContextId, Scope, Workspace};
use dtool_core::{Command, ViewMode};
use dtool_ui::persona::{install, PersonaStore};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static SERIAL: Mutex<()> = Mutex::new(());

fn persona_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c17_persona_live_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        install(PersonaStore { dir: dir.clone() }).unwrap();
        dir
    })
    .clone()
}

fn live(workspace: &mut Workspace, name: &str, params: Value) -> Result<Value, String> {
    let (handle, inbox) = host_channel(std::sync::Arc::new(|| {}));
    inbox.publish(workspace);
    let command = Command { name: name.into(), params };
    let worker = std::thread::spawn(move || handle.request(Scope::active(), command, None, Duration::from_secs(10)));
    while !worker.is_finished() {
        inbox.drain(workspace, 32);
        std::thread::sleep(Duration::from_millis(1));
    }
    worker.join().unwrap().result.map_err(|error| error.code)
}

fn editing(workspace: &mut Workspace, name: &str) -> (ContextId, u64) {
    let id = workspace.insert_context(name, None, Document::new([200.0, 100.0], Units::Px, 96.0, 1), 0);
    workspace.activate(id).unwrap();
    let session = &mut workspace.get_mut(id).unwrap().session;
    session.run_named("text.art.add", json!({ "pos": [0.0, 0.0], "text": "ab" })).unwrap();
    let object = session.document.objects.last().unwrap().id;
    let story = session.document.get_object(object).unwrap().story_id().unwrap().0;
    session.run_named("text.select", json!({ "story": story, "id": object.0, "range": { "start": 2, "end": 2 } })).unwrap();
    (id, story)
}

fn story_text(workspace: &Workspace, id: ContextId, story: u64) -> String {
    workspace.get(id).unwrap().session.document.stories.iter().find(|item| item.id.0 == story).unwrap().text.clone()
}

fn fingerprint(workspace: &Workspace, id: ContextId, expected_mode: ViewMode) -> String {
    let context = workspace.get(id).unwrap();
    let mut session_view = context.session.view.clone();
    let mut context_view = context.view_state.clone();
    assert_eq!(session_view.canvas_flags.view_mode, expected_mode);
    assert_eq!(context_view.canvas_flags.view_mode, expected_mode);
    session_view.canvas_flags.view_mode = ViewMode::Normal;
    context_view.canvas_flags.view_mode = ViewMode::Normal;
    format!(
        "{}|{}|{}|{:?}|{session_view:?}|{context_view:?}",
        context.id.0, context.session.document.objects.len(),
        context.session.document.version, context.session.selection,
    )
}

#[test]
fn c17_repeated_switch_keeps_document_and_restores_layout() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    persona_dir();
    let mut workspace = Workspace::default();
    let (id, _) = editing(&mut workspace, "c17_switch");
    live(&mut workspace, "text.edit.end", json!({})).unwrap();
    live(&mut workspace, "ui.persona.set", json!({ "id": "vector" })).unwrap();
    let vector = live(&mut workspace, "ui.persona.query", json!({})).unwrap();
    live(&mut workspace, "ui.persona.set", json!({ "id": "pixel" })).unwrap();
    let pixel = live(&mut workspace, "ui.persona.query", json!({})).unwrap();
    let before = fingerprint(&workspace, id, ViewMode::Pixel);
    for _ in 0..2 {
        live(&mut workspace, "ui.persona.set", json!({ "id": "vector" })).unwrap();
        let now = live(&mut workspace, "ui.persona.query", json!({})).unwrap();
        assert_eq!(now["id"], "vector");
        assert_eq!(now["layout"], vector["layout"]);
        assert_eq!(fingerprint(&workspace, id, ViewMode::Normal), before);
        live(&mut workspace, "ui.persona.set", json!({ "id": "pixel" })).unwrap();
        let now = live(&mut workspace, "ui.persona.query", json!({})).unwrap();
        assert_eq!(now["id"], "pixel");
        assert_eq!(now["layout"], pixel["layout"]);
        assert_eq!(fingerprint(&workspace, id, ViewMode::Pixel), before);
    }
    assert_eq!(workspace.active(), Some(id));
}

#[test]
fn c17_switch_finishes_pending_text_edit_once() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    persona_dir();
    let mut workspace = Workspace::default();
    workspace.new_document("c17_pending_base");
    live(&mut workspace, "ui.persona.set", json!({ "id": "pixel" })).unwrap();
    let (id, story) = editing(&mut workspace, "c17_pending");
    workspace.get_mut(id).unwrap().session.run_named("text.edit.compose", json!({ "text": "한" })).unwrap();
    let steps = workspace.get(id).unwrap().session.undo.applied_steps();
    live(&mut workspace, "ui.persona.set", json!({ "id": "vector" })).unwrap();
    let session = &workspace.get(id).unwrap().session;
    assert_eq!(story_text(&workspace, id, story), "ab한");
    assert!(session.text_edit.is_none());
    assert_eq!(session.text_composing, None);
    assert_eq!(session.undo.applied_steps(), steps + 1);
}

#[test]
fn c17_switch_cancel_and_abort_pending_edit() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    persona_dir();
    let mut workspace = Workspace::default();
    workspace.new_document("c17_abort_base");
    live(&mut workspace, "ui.persona.set", json!({ "id": "vector" })).unwrap();
    let (id, story) = editing(&mut workspace, "c17_abort");
    workspace.get_mut(id).unwrap().session.run_named("text.edit.compose", json!({ "text": "글" })).unwrap();
    assert_eq!(live(&mut workspace, "ui.persona.set", json!({ "id": "pixel", "pending_edit": "abort" })).unwrap_err(), "SWITCH_CANCELLED");
    assert_eq!(live(&mut workspace, "ui.persona.query", json!({})).unwrap()["id"], "vector");
    assert!(workspace.get(id).unwrap().session.text_edit.is_some());
    assert_eq!(live(&mut workspace, "ui.persona.set", json!({ "id": "pixel", "pending_edit": "later" })).unwrap_err(), "INVALID_PARAMS");
    live(&mut workspace, "ui.persona.set", json!({ "id": "pixel", "pending_edit": "cancel" })).unwrap();
    assert_eq!(live(&mut workspace, "ui.persona.query", json!({})).unwrap()["id"], "pixel");
    assert_eq!(story_text(&workspace, id, story), "ab");
    assert!(workspace.get(id).unwrap().session.text_edit.is_none());
}

#[test]
fn c17_bad_tool_in_state_keeps_other_layouts() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c17_bad_state_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = PersonaStore { dir: dir.clone() };
    let mut state = store.load();
    dtool_ui::persona::switch_persona(&mut state, dtool_ui::persona::PersonaEntry::parse("pixel").unwrap()).unwrap();
    dtool_ui::persona::switch_persona(&mut state, dtool_ui::persona::PersonaEntry::parse("vector").unwrap()).unwrap();
    store.save(&state).unwrap();
    let mut raw: Value = serde_json::from_slice(&std::fs::read(dir.join("state.json")).unwrap()).unwrap();
    let pixel = raw["layouts"].as_object_mut().unwrap().get_mut("pixel").unwrap();
    pixel["tool"] = json!("tool.no_such_tool");
    pixel["tools"].as_array_mut().unwrap().push(json!("tool.no_such_tool"));
    let known = pixel["tools"].as_array().unwrap().len() - 1;
    std::fs::write(dir.join("state.json"), serde_json::to_vec(&raw).unwrap()).unwrap();
    let loaded = store.load();
    assert_eq!(loaded.active.key(), "vector");
    let pixel = loaded.layouts.get("pixel").expect("pixel layout kept");
    assert!(pixel.tool.is_none());
    assert_eq!(pixel.tools.len(), known);
}

#[test]
fn c17_custom_studio_live_path() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    persona_dir();
    let mut workspace = Workspace::default();
    workspace.new_document("c17_studio");
    let created = live(&mut workspace, "ui.studio.create", json!({ "name": "내 벡터", "base": "vector", "tools": ["tool.move", "tool.pen", "tool.no_such_tool"] })).unwrap();
    assert_eq!(created["skipped"], json!(["tool.no_such_tool"]));
    let list = live(&mut workspace, "ui.persona.list", json!({})).unwrap();
    assert!(list.to_string().contains("내 벡터"));
    live(&mut workspace, "ui.persona.set", json!({ "id": created["id"] })).unwrap();
    let now = live(&mut workspace, "ui.persona.query", json!({})).unwrap();
    assert_eq!(now["layout"]["tools"], json!(["tool.move", "tool.pen"]));
    live(&mut workspace, "ui.studio.rename", json!({ "name": "내 벡터", "new_name": "벡터2" })).unwrap();
    live(&mut workspace, "ui.studio.delete", json!({ "name": "벡터2" })).unwrap();
}

#[test]
fn c17_studio_commands_route_to_host_when_catalog_allows() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    persona_dir();
    let session = dtool_core::Session::new(Document::new([10.0, 10.0], Units::Px, 96.0, 1));
    for name in ["ui.studio.create", "ui.studio.update", "ui.studio.rename", "ui.studio.delete", "ui.studio.list", "ui.studio.export", "ui.studio.import"] {
        assert_eq!(dtool_core::commands::persona::can_execute(&session, &Command::new(name)), Some(true), "{name}");
    }
}
#[test]
fn c17_leave_sub_cancel_reverts_document_apply_keeps() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    persona_dir();
    let mut workspace = Workspace::default();
    workspace.new_document("c17_sub_revert");
    live(&mut workspace, "ui.persona.set", json!({ "id": "pixel" })).unwrap();
    let id = workspace.active().unwrap();
    let count = |workspace: &Workspace| workspace.get(id).unwrap().session.document.objects.len();
    let base = count(&workspace);
    live(&mut workspace, "ui.persona.enter_sub", json!({ "id": "panorama" })).unwrap();
    live(&mut workspace, "text.art.add", json!({ "pos": [0.0, 0.0], "text": "a" })).unwrap();
    live(&mut workspace, "text.art.add", json!({ "pos": [10.0, 0.0], "text": "b" })).unwrap();
    assert_eq!(count(&workspace), base + 2);
    let reply = live(&mut workspace, "ui.persona.leave_sub", json!({ "apply": false })).unwrap();
    assert_eq!(reply["reverted"], 2);
    assert_eq!(count(&workspace), base);
    assert_eq!(live(&mut workspace, "ui.persona.query", json!({})).unwrap()["id"], "pixel");
    live(&mut workspace, "ui.persona.enter_sub", json!({ "id": "panorama" })).unwrap();
    live(&mut workspace, "text.art.add", json!({ "pos": [0.0, 0.0], "text": "c" })).unwrap();
    let reply = live(&mut workspace, "ui.persona.leave_sub", json!({ "apply": true })).unwrap();
    assert!(reply.get("reverted").is_none());
    assert_eq!(count(&workspace), base + 1);
}

#[test]
fn c17_studio_preset_commands_removed_from_route_and_host() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    persona_dir();
    let session = dtool_core::Session::new(Document::new([10.0, 10.0], Units::Px, 96.0, 1));
    let mut workspace = Workspace::default();
    workspace.new_document("c17_no_studio_preset");
    for name in ["ui.studio_preset.add", "ui.studio_preset.apply"] {
        let command = Command::with_params(name, json!({ "name": "p" }));
        assert_eq!(dtool_core::commands::persona::can_execute(&session, &command), None, "{name}");
        assert_eq!(dtool_ui::persona::host_action(&command).unwrap_err().code, "NOT_IMPLEMENTED", "{name}");
        assert!(live(&mut workspace, name, json!({ "name": "p" })).is_err(), "{name}");
    }
}

#[test]
fn c17_studio_preset_not_listed_and_unknown_on_session_path() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let catalog = dtool_protocol::CommandCatalog::new();
    let mut workspace = Workspace::default();
    workspace.new_document("c17_no_studio_preset_catalog");
    for name in ["ui.studio_preset.add", "ui.studio_preset.apply"] {
        assert!(catalog.resolve(name).is_none(), "{name}");
        assert_eq!(live(&mut workspace, name, json!({ "name": "p" })).unwrap_err(), "UNKNOWN_COMMAND", "{name}");
    }
}


#[test]
fn slice_persona_locks_artwork_but_allows_selection_and_queue_changes() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    persona_dir();
    let mut workspace = Workspace::default();
    workspace.new_document("slice_read_only");
    live(&mut workspace, "ui.persona.set", json!({"id": "vector"})).unwrap();
    let object = {
        let session = workspace.session_mut().unwrap();
        session.run_named("text.art.add", json!({"pos": [7.0, 8.0], "text": "Export"})).unwrap();
        session.run_named("text.edit.end", json!({})).unwrap();
        session.document.objects.last().unwrap().id
    };
    let artwork = serde_json::to_value(&workspace.session().unwrap().document.objects).unwrap();
    live(&mut workspace, "ui.persona.set", json!({"id": "slice"})).unwrap();
    let movement = Command::with_params("object.update", json!({"id": object, "pos": [30, 40]}));
    let session = workspace.session_mut().unwrap();
    assert!(!session.can_execute(&movement));
    assert_eq!(session.run(&movement).unwrap_err().code, "READ_ONLY");
    for (name, params) in [
        ("object.delete", json!({"id": object})),
        ("node.update", json!({"id": object, "size": [50, 50]})),
        ("edit.undo", json!({})),
        ("edit.redo", json!({})),
    ] {
        assert_eq!(live(&mut workspace, name, params).unwrap_err(), "READ_ONLY");
    }
    live(&mut workspace, "selection.set", json!({"ids": [object]})).unwrap();
    assert_eq!(workspace.session().unwrap().selection.ids, [object]);
    let created = live(&mut workspace, "slice.create", json!({"source": object, "layer": true})).unwrap();
    let duplicate = live(&mut workspace, "slice.create", json!({"source": object, "layer": true})).unwrap();
    assert_eq!(created["id"], duplicate["id"]);
    assert_eq!(workspace.session().unwrap().document.slices.len(), 1);
    live(&mut workspace, "slice.delete", json!({"id": created["id"]})).unwrap();
    let session = workspace.session().unwrap();
    assert!(session.document.slices.is_empty());
    assert_eq!(serde_json::to_value(&session.document.objects).unwrap(), artwork);
    live(&mut workspace, "ui.persona.set", json!({"id": "vector"})).unwrap();
    assert!(workspace.session().unwrap().can_execute(&movement));
}
