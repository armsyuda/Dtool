#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::canvas::frame_text_input;
use dtool_ui::context_menu::{spell_choice, SpellChoice};
use dtool_ui::shell::AppShell;
use egui::{Event, PointerButton, Pos2, Rect};
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn texts(shapes: &[egui::Shape], found: &mut Vec<(String, Rect)>) {
    for shape in shapes {
        match shape {
            egui::Shape::Text(text) if text.pos.x.is_finite() && text.pos.y.is_finite() => found.push((text.galley.text().to_string(), Rect::from_min_size(text.pos, text.galley.size()))),
            egui::Shape::Vec(inner) => texts(inner, found),
            _ => {}
        }
    }
}

fn frame(shell: &mut AppShell, ctx: &egui::Context, events: Vec<Event>) -> Vec<(String, Rect)> {
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1000.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
    let shapes: Vec<egui::Shape> = out.shapes.into_iter().map(|clipped| clipped.shape).collect();
    let mut found = Vec::new();
    texts(&shapes, &mut found);
    found
}

fn press(shell: &mut AppShell, ctx: &egui::Context, at: Pos2, button: PointerButton) -> Vec<(String, Rect)> {
    frame(shell, ctx, vec![Event::PointerMoved(at)]);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button, pressed: true, modifiers: egui::Modifiers::default() }]);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button, pressed: false, modifiers: egui::Modifiers::default() }]);
    frame(shell, ctx, Vec::new())
}

fn story_text(shell: &mut AppShell, story: u64) -> String {
    shell.workspace.session().unwrap().document.stories.iter().find(|s| s.id.0 == story).unwrap().text.clone()
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

#[test]
fn c11_right_click_on_a_misspelled_word_offers_suggestions_ignore_and_add() {
    let _home = test_home::enter("c11_shell_spell_menu");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c11 spell menu");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "I recieved teh book" }));
    let (object, story) = (added["id"].as_u64().unwrap(), added["story"].as_u64().unwrap());
    run(&mut shell, "text.select", json!({ "story": story, "id": object, "range": { "start": 4, "end": 4 } }));
    frame(&mut shell, &ctx, Vec::new());
    let (_, mark, suggestions) = frame_text_input::spell_at_caret(shell.workspace.session().unwrap()).expect("caret is on a misspelled word");
    assert_eq!(mark.word, "recieved");
    assert!(suggestions.iter().any(|word| word == "received"), "{suggestions:?}");

    let before = undo_steps(&mut shell);
    let found = press(&mut shell, &ctx, Pos2::new(760.0, 600.0), PointerButton::Secondary);
    assert!(shell.workspace.session().unwrap().text_edit.is_some(), "right-click keeps the text edit");
    let row = found.iter().rev().find(|(text, _)| text == "received").map(|(_, rect)| *rect).unwrap_or_else(|| panic!("context menu lists the suggestion: {:?}", found.iter().map(|(t, _)| t.clone()).collect::<Vec<_>>()));
    press(&mut shell, &ctx, row.center(), PointerButton::Primary);
    assert_eq!(story_text(&mut shell, story), "I received teh book");
    assert_eq!(undo_steps(&mut shell), before + 1, "suggestion replace is one undo step");

    run(&mut shell, "text.select", json!({ "story": story, "id": object, "range": { "start": 13, "end": 13 } }));
    frame(&mut shell, &ctx, Vec::new());
    assert_eq!(frame_text_input::spell_at_caret(shell.workspace.session().unwrap()).unwrap().1.word, "teh");
    let before = undo_steps(&mut shell);
    assert!(spell_choice(shell.workspace.session_mut().unwrap(), &SpellChoice::Ignore).unwrap().is_ok());
    assert!(frame_text_input::spell_at_caret(shell.workspace.session().unwrap()).is_none(), "ignored word drops its mark");
    assert_eq!(undo_steps(&mut shell), before + 1);
    run(&mut shell, "edit.undo", json!({}));
    frame_text_input::refresh_spelling(shell.workspace.session().unwrap(), None);
    assert!(frame_text_input::spell_at_caret(shell.workspace.session().unwrap()).is_some());

    let before = undo_steps(&mut shell);
    assert!(spell_choice(shell.workspace.session_mut().unwrap(), &SpellChoice::AddWord).unwrap().is_ok());
    assert!(frame_text_input::spell_at_caret(shell.workspace.session().unwrap()).is_none(), "added word drops its mark");
    assert_eq!(undo_steps(&mut shell), before);
    let dictionary = std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap()).join("DTool").join("user_dictionary_en_US.txt");
    assert!(std::fs::read_to_string(&dictionary).unwrap().lines().any(|line| line.trim() == "teh"));
}
