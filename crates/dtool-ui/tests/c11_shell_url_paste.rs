#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::shell::AppShell;
use dtool_ui::text_edit::paste::is_url;
use egui::{Event, Pos2, Rect};
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn frame(shell: &mut AppShell, ctx: &egui::Context, events: Vec<Event>) {
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1000.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn story(shell: &mut AppShell, id: u64) -> dtool_core::Story {
    shell.workspace.session().unwrap().document.stories.iter().find(|s| s.id.0 == id).cloned().unwrap()
}

fn linked(shell: &mut AppShell, id: u64, url: &str) -> bool {
    serde_json::to_string(&story(shell, id).ranges).unwrap().contains(url)
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

#[test]
fn b41_t021_pasting_a_url_in_a_live_frame_links_it_in_one_undo_step() {
    let _home = test_home::enter("c11_shell_url_paste");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("url paste");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "see " }));
    let (object, id) = (added["id"].as_u64().unwrap(), added["story"].as_u64().unwrap());
    for _ in 0..3 { frame(&mut shell, &ctx, Vec::new()); }
    shell.pick_tool(dtool_tools::frame_text::TOOL_IDS[0]).expect("pick text tool");
    run(&mut shell, "text.select", json!({ "story": id, "id": object, "range": { "start": 4, "end": 4 } }));
    frame(&mut shell, &ctx, Vec::new());
    frame(&mut shell, &ctx, Vec::new());

    let url = "https://dtool.example/docs";
    assert!(is_url(url) && !is_url("plain words") && !is_url("https://a b"));
    let before = undo_steps(&mut shell);
    frame(&mut shell, &ctx, vec![Event::Paste(url.into())]);
    frame(&mut shell, &ctx, Vec::new());
    assert_eq!(story(&mut shell, id).text, format!("see {url}"));
    assert!(linked(&mut shell, id, url), "the pasted URL carries a hyperlink");
    assert_eq!(undo_steps(&mut shell), before + 1, "URL paste is one undo step");

    frame(&mut shell, &ctx, vec![Event::Paste(" and more".into())]);
    frame(&mut shell, &ctx, Vec::new());
    assert_eq!(story(&mut shell, id).text, format!("see {url} and more"));

    dtool_core::text_cmd::commit_text_edit(shell.workspace.session_mut().unwrap());
    assert_eq!(undo_steps(&mut shell), before + 2, "plain paste is its own typing step");
    run(&mut shell, "edit.undo", json!({}));
    assert_eq!(story(&mut shell, id).text, format!("see {url}"), "plain paste is undone separately");
    assert!(linked(&mut shell, id, url));
    run(&mut shell, "edit.undo", json!({}));
    assert_eq!(story(&mut shell, id).text, "see ");
    assert!(!linked(&mut shell, id, url), "one undo removes text and link together");
}
