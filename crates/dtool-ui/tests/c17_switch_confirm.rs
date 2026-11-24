use dtool_core::document::{Document, Units};
use dtool_core::workspace::{host_channel, ContextId, Scope, Workspace};
use dtool_core::{Command, CommandError, CommandResult};
use dtool_ui::persona::{install, PersonaStore};
use dtool_ui::persona_bar::{pending_edit_dirty, SwitchChoice, SwitchGuard};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static SERIAL: Mutex<()> = Mutex::new(());

fn persona_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c17_switch_confirm_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        install(PersonaStore { dir: dir.clone() }).unwrap();
        dir
    })
    .clone()
}

fn live(workspace: &mut Workspace, command: &Command) -> CommandResult {
    let (handle, inbox) = host_channel(std::sync::Arc::new(|| {}));
    inbox.publish(workspace);
    let command = command.clone();
    let worker = std::thread::spawn(move || handle.request(Scope::active(), command, None, Duration::from_secs(10)));
    while !worker.is_finished() {
        inbox.drain(workspace, 32);
        std::thread::sleep(Duration::from_millis(1));
    }
    worker.join().unwrap().result.map_err(|error| CommandError::new(&error.code))
}

fn named(workspace: &mut Workspace, name: &str, params: Value) -> CommandResult {
    live(workspace, &Command::with_params(name, params))
}

fn texts(output: &egui::FullOutput) -> Vec<String> {
    let mut out = Vec::new();
    for clipped in &output.shapes {
        collect(&clipped.shape, &mut out);
    }
    out
}

fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, out)),
        _ => {}
    }
}

fn frame(ctx: &egui::Context, mut paint: impl FnMut(&egui::Context)) -> egui::FullOutput {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))), ..Default::default() };
    let mut first = ctx.run_ui(input.clone(), |ui| paint(&ui.ctx().clone()));
    first.textures_delta.clear();
    let mut second = ctx.run_ui(input, |ui| paint(&ui.ctx().clone()));
    second.textures_delta.clear();
    second
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

#[test]
fn c17_switch_guard_asks_only_for_a_changed_text_edit() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    persona_dir();
    let mut workspace = Workspace::default();
    workspace.new_document("c17_guard_base");
    named(&mut workspace, "ui.persona.set", json!({ "id": "vector" })).unwrap();
    let (id, story) = editing(&mut workspace, "c17_guard");
    let mut guard = SwitchGuard::new();
    let to_pixel = Command::with_params("ui.persona.set", json!({ "id": "pixel" }));
    assert!(!pending_edit_dirty(&workspace.get(id).unwrap().session));
    assert_eq!(guard.request(workspace.session(), to_pixel.clone()).map(|command| command.params), Some(to_pixel.params.clone()));
    assert!(!guard.is_asking());
    workspace.get_mut(id).unwrap().session.run_named("text.edit.compose", json!({ "text": "한" })).unwrap();
    assert!(pending_edit_dirty(&workspace.get(id).unwrap().session));
    assert!(guard.request(workspace.session(), Command::new("ui.query")).is_some(), "non-switch commands pass");
    assert!(guard.request(workspace.session(), to_pixel.clone()).is_none(), "dirty edit asks first");
    assert!(guard.is_asking());
    let ctx = egui::Context::default();
    let output = frame(&ctx, |ctx| assert!(guard.show(ctx).is_none()));
    assert!(guard.is_asking());
    assert!(texts(&output).iter().any(|text| text == dtool_strings::text("persona.switch.discard")));
    assert!(guard.resolve(SwitchChoice::Stay).is_none());
    assert!(!guard.is_asking());
    assert_eq!(named(&mut workspace, "ui.persona.query", json!({})).unwrap()["id"], "vector");
    assert!(workspace.get(id).unwrap().session.text_edit.is_some(), "stay keeps editing");
    guard.request(workspace.session(), to_pixel.clone());
    let discard = guard.resolve(SwitchChoice::Discard).unwrap();
    assert_eq!(discard.params["pending_edit"], "cancel");
    live(&mut workspace, &discard).unwrap();
    assert_eq!(named(&mut workspace, "ui.persona.query", json!({})).unwrap()["id"], "pixel");
    assert_eq!(story_text(&workspace, id, story), "ab");
    assert!(workspace.get(id).unwrap().session.text_edit.is_none());
    let (id, story) = editing(&mut workspace, "c17_guard_keep");
    workspace.get_mut(id).unwrap().session.run_named("text.edit.compose", json!({ "text": "글" })).unwrap();
    let to_vector = Command::with_params("ui.persona.set", json!({ "id": "vector" }));
    assert!(guard.request(workspace.session(), to_vector).is_none());
    let keep = guard.resolve(SwitchChoice::Keep).unwrap();
    assert_eq!(keep.params["pending_edit"], "commit");
    live(&mut workspace, &keep).unwrap();
    assert_eq!(named(&mut workspace, "ui.persona.query", json!({})).unwrap()["id"], "vector");
    assert_eq!(story_text(&workspace, id, story), "ab글");
    let explicit = Command::with_params("ui.persona.set", json!({ "id": "pixel", "pending_edit": "abort" }));
    assert!(guard.request(workspace.session(), explicit).is_some(), "explicit pending_edit is not asked again");
}
