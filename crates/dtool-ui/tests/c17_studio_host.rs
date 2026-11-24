use dtool_core::workspace::{host_channel, Scope, Workspace};
use dtool_core::{Command, CommandError, CommandResult};
use dtool_ui::persona::{install, PersonaId, PersonaStore};
use dtool_ui::persona_bar::{close_export_preview, request_studio, take_studio_request};
use dtool_ui::studio_editor::{StudioRequest, StudioWindows};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static SERIAL: Mutex<()> = Mutex::new(());

fn persona_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c17_studio_host_{}", std::process::id()));
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

#[test]
fn c17_studio_request_round_trips_through_the_persona_bar() {
    let ctx = egui::Context::default();
    assert_eq!(take_studio_request(&ctx), None);
    request_studio(&ctx, StudioRequest::Manager);
    assert_eq!(take_studio_request(&ctx), Some(StudioRequest::Manager));
    assert_eq!(take_studio_request(&ctx), None);
    request_studio(&ctx, StudioRequest::Create);
    assert_eq!(take_studio_request(&ctx), Some(StudioRequest::Create));
}

#[test]
fn c17_studio_windows_create_manage_and_switch_on_live_path() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    persona_dir();
    let mut workspace = Workspace::default();
    let doc = workspace.new_document("c17_studio_host");
    named(&mut workspace, "ui.persona.set", json!({ "id": "vector" })).unwrap();
    let version = workspace.get(doc).unwrap().session.document.version;
    let mut windows = StudioWindows::new();
    {
        let mut run = |command: &Command| live(&mut workspace, command);
        windows.open(StudioRequest::Create, &mut run);
    }
    assert!(windows.is_open());
    assert!(!windows.manager_open);
    let editor = windows.state.editing.as_mut().expect("create opens the editor");
    assert_eq!(editor.base, PersonaId::Vector);
    editor.name = "host studio".into();
    let ctx = egui::Context::default();
    let output = {
        let mut run = |command: &Command| live(&mut workspace, command);
        frame(&ctx, |ctx| assert!(windows.show(ctx, &mut run).is_none()))
    };
    let shown = texts(&output);
    assert!(shown.iter().any(|text| text == dtool_strings::text("studio.editor.save")), "editor window drawn: {shown:?}");
    {
        let mut run = |command: &Command| live(&mut workspace, command);
        let saved = windows.state.close_editor(&mut run).expect("save studio").expect("studio listed");
        assert_eq!(saved.name, "host studio");
        windows.open(StudioRequest::Manager, &mut run);
    }
    assert!(windows.manager_open);
    assert!(windows.state.studios.iter().any(|studio| studio.name == "host studio"));
    let output = {
        let mut run = |command: &Command| live(&mut workspace, command);
        frame(&ctx, |ctx| assert!(windows.show(ctx, &mut run).is_none()))
    };
    let shown = texts(&output);
    assert!(shown.iter().any(|text| text == "host studio"), "manager lists the studio: {shown:?}");
    assert!(shown.iter().any(|text| text == dtool_strings::text("studio.manager.use")));
    assert_eq!(named(&mut workspace, "ui.persona.query", json!({})).unwrap()["id"], "vector", "use is not run by the window");
    windows.state.request_switch("host studio");
    let switch = {
        let mut run = |command: &Command| live(&mut workspace, command);
        let mut switch = None;
        frame(&ctx, |ctx| if let Some(command) = windows.show(ctx, &mut run) { switch = Some(command) });
        switch.expect("use returns the switch for the shell")
    };
    assert!(!windows.is_open(), "windows close after use");
    assert_eq!(switch.name, "ui.persona.set");
    live(&mut workspace, &switch).expect("switch through the shell route");
    assert_eq!(named(&mut workspace, "ui.persona.query", json!({})).unwrap()["id"], "custom:host studio");
    assert_eq!(workspace.get(doc).unwrap().session.document.version, version, "studios never touch the document");
    named(&mut workspace, "ui.persona.set", json!({ "id": "vector" })).unwrap();
    named(&mut workspace, "ui.studio.delete", json!({ "name": "host studio" })).unwrap();
}

#[test]
fn c17_close_export_preview_is_safe_when_closed() {
    assert!(!close_export_preview());
}