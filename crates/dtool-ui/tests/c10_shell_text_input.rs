#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::canvas::frame_text_input;
use dtool_ui::menu_tree;
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

const EM_DASH: &str = "text.insert.dashes_and_hyphens.em_dash";
const COLUMN_BREAK: &str = "text.insert.breaks.column_break";

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

fn story(shell: &mut AppShell, id: u64) -> dtool_core::Story {
    let session = shell.workspace.session_mut().unwrap();
    session.document.stories.iter().find(|s| s.id.0 == id).cloned().unwrap()
}

fn caret(shell: &mut AppShell) -> usize {
    shell.workspace.session_mut().unwrap().text_edit.as_ref().map(|edit| edit.caret).unwrap()
}

fn feed(shell: &mut AppShell, events: Vec<egui::Event>) -> bool {
    frame_text_input::feed_text_input(shell.workspace.session_mut().unwrap(), &events)
}

#[test]
fn c10_insert_menu_autocorrect_and_paste_run_on_the_live_text_edit() {
    let _home = test_home::enter("c10_shell_text_input");
    let mut shell = AppShell::new();
    if !dtool_core::commands::prefs_cmd::installed() {
        shell.install_core_prefs();
    }
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c10 text input");
    shell.workspace.activate(doc).unwrap();
    assert!(menu_tree::row_is_local(EM_DASH, true));
    assert!(menu_tree::row_is_local(COLUMN_BREAK, true));
    assert!(menu_tree::row_is_local("text.insert.quotation_marks.left_guillemet", true));
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "ab" }));
    let object = added["id"].as_u64().unwrap();
    let id = added["story"].as_u64().unwrap();
    frame(&mut shell, &ctx);
    assert!(!shell.insert_special(EM_DASH), "no text edit, no insert");
    run(&mut shell, "text.select", json!({ "story": id, "id": object, "range": { "start": 1, "end": 1 } }));
    frame(&mut shell, &ctx);

    let base = undo_steps(&mut shell);
    assert!(shell.insert_special(EM_DASH));
    assert_eq!(story(&mut shell, id).text, "a\u{2014}b");
    assert_eq!(caret(&mut shell), 2);
    let specials = story(&mut shell, id).specials.len();
    assert!(shell.insert_special(COLUMN_BREAK));
    assert!(story(&mut shell, id).specials.len() > specials);
    assert_eq!(undo_steps(&mut shell), base + 2, "em dash edit group + column break");
    frame(&mut shell, &ctx);

    shell.prefs.autocorrect_enabled = true;
    shell.prefs.ac_replace_while_typing = true;
    shell.push_core_prefs();
    let typed: Vec<egui::Event> = ["(", "c", ")", " "].into_iter().map(|s| egui::Event::Text(s.into())).collect();
    for event in typed {
        assert!(feed(&mut shell, vec![event]));
    }
    assert_eq!(story(&mut shell, id).text, "a\u{2014}\u{a9} b");
    assert_eq!(caret(&mut shell), 4);
    frame(&mut shell, &ctx);
    let before_paste = undo_steps(&mut shell);

    assert!(feed(&mut shell, vec![egui::Event::Paste("https://x.example".into())]));
    let pasted = story(&mut shell, id);
    assert_eq!(pasted.text, "a\u{2014}\u{a9} https://x.exampleb");
    assert!(serde_json::to_string(&pasted.ranges).unwrap().contains("https://x.example"), "URL paste links its range");
    assert_eq!(caret(&mut shell), 4 + "https://x.example".chars().count());
    assert_eq!(undo_steps(&mut shell), before_paste + 2, "typing group (with autocorrect) + one URL paste step");
    frame(&mut shell, &ctx);

    run(&mut shell, "edit.undo", json!({}));
    assert_eq!(story(&mut shell, id).text, "a\u{2014}\u{a9} b", "one undo removes the whole URL paste, link included");
    assert!(!serde_json::to_string(&story(&mut shell, id).ranges).unwrap().contains("https://x.example"));
    run(&mut shell, "edit.redo", json!({}));
    assert_eq!(story(&mut shell, id).text, "a\u{2014}\u{a9} https://x.exampleb");
    assert!(serde_json::to_string(&story(&mut shell, id).ranges).unwrap().contains("https://x.example"), "redo brings text and link back together");
    run(&mut shell, "edit.undo", json!({}));
    run(&mut shell, "edit.undo", json!({}));
    assert_eq!(story(&mut shell, id).text, "a\u{2014}b", "one undo restores the pre-typing text, autocorrect included");
}
