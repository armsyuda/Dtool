#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_strings::text;
use dtool_ui::find_host::{self, FoundSpan, FIND_MENU_ID};
use dtool_ui::find_panel::{FindPanelScope, FindRequest};
use dtool_ui::menu_tree;
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

fn story_text(shell: &mut AppShell, story: u64) -> String {
    let session = shell.workspace.session_mut().unwrap();
    session.document.stories.iter().find(|s| s.id.0 == story).map(|s| s.text.clone()).unwrap_or_default()
}

fn selected(shell: &mut AppShell) -> Option<(u64, usize, usize)> {
    let edit = shell.workspace.session_mut().unwrap().text_edit.clone()?;
    Some((edit.story.0, edit.anchor.min(edit.caret), edit.anchor.max(edit.caret)))
}

fn request(shell: &mut AppShell, request: FindRequest) -> bool {
    let AppShell { find_host, workspace, .. } = shell;
    find_host.request(workspace, request)
}

#[test]
fn c11_find_window_drives_text_find_and_replace_on_the_live_session() {
    let _home = test_home::enter("c11_shell_find");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    assert!(!menu_tree::row_is_local(FIND_MENU_ID, false));
    let doc = shell.workspace.new_document("c11 find");
    shell.workspace.activate(doc).unwrap();
    assert!(menu_tree::row_is_local(FIND_MENU_ID, true));
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "cat and cat" }));
    let story = added["story"].as_u64().unwrap();
    frame(&mut shell, &ctx);
    frame(&mut shell, &ctx);

    shell.find_host.toggle();
    for _ in 0..2 {
        frame(&mut shell, &ctx);
    }
    assert!(shell.find_host.open);
    assert!(ctx.memory(|m| m.area_rect(find_host::window_id())).is_some(), "find window drawn in a real shell frame");

    shell.find_host.panel.pattern = "cat".into();
    shell.find_host.panel.replacement = "dog".into();
    assert!(!request(&mut shell, FindRequest::Next { backwards: false }));
    assert_eq!(shell.find_host.last_match(), Some(FoundSpan { story, start: 0, end: 3 }));
    assert_eq!(selected(&mut shell), Some((story, 0, 3)));
    request(&mut shell, FindRequest::Next { backwards: false });
    assert_eq!(selected(&mut shell), Some((story, 8, 11)));
    request(&mut shell, FindRequest::Next { backwards: true });
    assert_eq!(selected(&mut shell), Some((story, 0, 3)));
    frame(&mut shell, &ctx);

    let before = undo_steps(&mut shell);
    assert!(request(&mut shell, FindRequest::Replace));
    assert_eq!(story_text(&mut shell, story), "dog and cat");
    assert_eq!(undo_steps(&mut shell), before + 1, "one replace = one undo step");
    assert_eq!(selected(&mut shell), Some((story, 8, 11)), "next match selected after replace");

    shell.find_host.panel.scope = FindPanelScope::Story;
    shell.find_host.panel.regex = true;
    shell.find_host.panel.pattern = "(".into();
    assert!(!request(&mut shell, FindRequest::ReplaceAll));
    assert_eq!(shell.find_host.panel.status.as_deref(), Some(text("find.status.invalid")));
    assert_eq!(story_text(&mut shell, story), "dog and cat");

    shell.find_host.panel.pattern = "c.t".into();
    let before = undo_steps(&mut shell);
    assert!(request(&mut shell, FindRequest::ReplaceAll));
    assert_eq!(story_text(&mut shell, story), "dog and dog");
    assert_eq!(undo_steps(&mut shell), before + 1, "replace all = one undo step");
    assert!(shell.find_host.panel.status.as_deref().is_some_and(|s| s.ends_with(" 1")));

    request(&mut shell, FindRequest::Next { backwards: false });
    assert_eq!(shell.find_host.last_match(), None);
    assert_eq!(shell.find_host.panel.status.as_deref(), Some(text("find.status.none")));
    frame(&mut shell, &ctx);
    run(&mut shell, "edit.undo", json!({}));
    assert_eq!(story_text(&mut shell, story), "dog and cat");
    shell.find_host.toggle();
    frame(&mut shell, &ctx);
    assert!(!shell.find_host.open);
}

#[test]
fn c11_whole_word_checkbox_reaches_text_find_on_the_live_session() {
    let _home = test_home::enter("c11_shell_find_whole_word");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c11 whole word");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "concat cat" }));
    let story = added["story"].as_u64().unwrap();
    shell.find_host.toggle();
    frame(&mut shell, &ctx);
    frame(&mut shell, &ctx);
    assert!(!shell.find_host.panel.whole_word);
    shell.find_host.panel.pattern = "cat".into();
    let params = shell.find_host.base_params(shell.workspace.session().unwrap()).unwrap();
    assert_eq!(params["whole_word"], false);
    shell.find_host.panel.whole_word = true;
    frame(&mut shell, &ctx);
    let params = shell.find_host.base_params(shell.workspace.session().unwrap()).unwrap();
    assert_eq!(params["whole_word"], true);
    let mut direct = params.clone();
    direct["whole_word"] = json!(true);
    let found = run(&mut shell, "text.find", direct);
    assert!(found["match"].is_object(), "{found}");
    request(&mut shell, FindRequest::Next { backwards: false });
    let span = shell.find_host.last_match().expect("match");
    assert_eq!(span.story, story);
    assert!(span == FoundSpan { story, start: 7, end: 10 } || span == FoundSpan { story, start: 3, end: 6 }, "{span:?}");
}

#[test]
fn c11_ctrl_f_opens_the_find_window_through_the_shortcut_store() {
    use dtool_ui::shortcut_commands::{ShellAction, ShortcutTarget};
    use dtool_ui::shortcuts::{Focus, Modifiers, ShortcutMap, FIND_OPEN_COMMAND};
    let _home = test_home::enter("c11_shell_find_ctrl_f");
    let ctrl = Modifiers { ctrl: true, shift: false, alt: false };
    let defaults = ShortcutMap::affinity_default();
    let bound = defaults.bindings_for(FIND_OPEN_COMMAND);
    assert_eq!(bound.len(), 1);
    assert_eq!((bound[0].key.as_str(), bound[0].modifiers), ("F", ctrl));
    let implemented: Vec<String> = defaults
        .dispatch_all(Focus::Canvas, "F", ctrl)
        .into_iter()
        .filter(|command| command != FIND_OPEN_COMMAND && dtool_ui::shortcut_commands::target_for(command).is_some())
        .collect();
    assert!(implemented.is_empty(), "no other working command on Ctrl+F: {implemented:?}");
    assert_eq!(defaults.clone().with_find_default().bindings_for(FIND_OPEN_COMMAND).len(), 1);

    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c11 ctrl f");
    shell.workspace.activate(doc).unwrap();
    frame(&mut shell, &ctx);
    assert!(!shell.find_host.open);
    assert_eq!(shell.handle_shortcut_key(Focus::Canvas, "F", ctrl), Some(ShortcutTarget::Shell(ShellAction::OpenFind)));
    assert!(shell.find_host.open);
    frame(&mut shell, &ctx);
    assert!(ctx.memory(|m| m.area_rect(find_host::window_id())).is_some());
}