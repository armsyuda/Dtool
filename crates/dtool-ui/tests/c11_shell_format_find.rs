#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::find_host::{self, FoundSpan};
use dtool_ui::find_panel::FindRequest;
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session_mut().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

fn request(shell: &mut AppShell, request: FindRequest) -> bool {
    let AppShell { find_host, workspace, .. } = shell;
    find_host.request(workspace, request)
}

fn size_at(shell: &mut AppShell, story: u64, at: usize) -> Option<f64> {
    let session = shell.workspace.session_mut().unwrap();
    let story = session.document.stories.iter().find(|s| s.id.0 == story)?;
    let at = at as u64;
    let run = story.ranges.iter().find(|r| r["start"].as_u64().is_some_and(|s| s <= at) && r["end"].as_u64().is_some_and(|e| at < e))?;
    run.get("size_pt").and_then(Value::as_f64)
}

#[test]
fn c11_format_find_and_replace_format_through_the_live_find_window() {
    let _home = test_home::enter("c11_shell_format_find");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c11 format find");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "aa big aa" }));
    let story = added["story"].as_u64().unwrap();
    run(&mut shell, "text.style", json!({ "story": story, "range": { "start": 3, "end": 6 }, "patch": { "size_pt": 30.0 } }));
    shell.find_host.toggle();
    for _ in 0..3 {
        frame(&mut shell, &ctx);
    }
    assert!(ctx.memory(|m| m.area_rect(find_host::window_id())).is_some());

    assert!(!shell.find_host.panel.can_search());
    shell.find_host.panel.format.size_pt = "30".into();
    assert!(shell.find_host.panel.can_search(), "format alone enables search");
    frame(&mut shell, &ctx);
    let params = shell.find_host.base_params(shell.workspace.session().unwrap()).unwrap();
    assert_eq!(params["format"], json!({ "size_pt": 30.0 }));
    assert_eq!(params["pattern"], "");

    request(&mut shell, FindRequest::Next { backwards: false });
    assert_eq!(shell.find_host.last_match(), Some(FoundSpan { story, start: 3, end: 6 }));

    shell.find_host.panel.pattern = "aa".into();
    request(&mut shell, FindRequest::Next { backwards: false });
    assert_eq!(shell.find_host.last_match(), None, "pattern and format combine");
    shell.find_host.panel.pattern.clear();

    shell.find_host.panel.replace_format.size_pt = "12".into();
    shell.find_host.panel.replace_format.para_style = "ignored".into();
    let mut replace = json!({});
    shell.find_host.replace_params(&mut replace);
    assert_eq!(replace, json!({ "replace_format": { "size_pt": 12.0 } }), "empty replacement omitted, no para keys on replace side");

    let before = undo_steps(&mut shell);
    assert!(request(&mut shell, FindRequest::ReplaceAll));
    frame(&mut shell, &ctx);
    assert_eq!(undo_steps(&mut shell), before + 1, "format replace all = one undo step");
    let text = shell.workspace.session().unwrap().document.stories.iter().find(|s| s.id.0 == story).unwrap().text.clone();
    assert_eq!(text, "aa big aa");
    assert_eq!(size_at(&mut shell, story, 4), Some(12.0));
    assert!(shell.find_host.panel.status.as_deref().is_some_and(|s| s.ends_with(" 1")));

    shell.find_host.panel.format = Default::default();
    shell.find_host.panel.replace_format = Default::default();
    assert!(!request(&mut shell, FindRequest::ReplaceAll));
}
