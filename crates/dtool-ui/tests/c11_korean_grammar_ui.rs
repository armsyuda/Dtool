#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use dtool_ui::spell_host::SpellHost;
use dtool_ui::spelling_panel::SpellPanelAction;
use serde_json::{json, Value};
const BAD: &str = "아버지가방에들어가신다";
fn call(ws: &mut Workspace, name: &str, params: Value) -> Value { ws.run(Scope::active(), Command::with_params(name, params)).1.unwrap() }
fn setup() -> (Workspace, u64, u64) {
    dtool_tools::dtool_text::text_host::install();
    assert!(dtool_tools::dtool_text::spelling::ko_grammar::kiwi_dir().is_some(), "real prepared Kiwi bundle is required");
    let mut ws = Workspace::default(); ws.new_document("grammar");
    let reply = call(&mut ws, "text.frame.add", json!({"rect": [20, 20, 400, 300], "text": BAD}));
    let object = reply["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&ws.session().unwrap().document, dtool_core::ObjectId(object)).unwrap().0;
    (ws, object, story)
}
#[test]
fn spacing_issue_appears_in_panel_and_replacement_is_one_undo() {
    let _home = test_home::enter("c11_grammar_panel"); let (mut ws, _, story) = setup();
    let mut host = SpellHost::default(); host.check(&mut ws);
    let index = host.marks.iter().position(|m| m.word == BAD && host.grammar_suggestions.contains_key(&(m.story, m.start, m.end))).expect("grammar row");
    assert!(host.rows()[index].contains("띄어쓰기"));
    host.apply(&mut ws, SpellPanelAction::Select(index));
    assert!(host.suggestions.iter().any(|s| s == "아버지가 방에 들어가신다"));
    assert!(host.apply(&mut ws, SpellPanelAction::Replace("아버지가 방에 들어가신다".into())));
    assert_eq!(ws.session().unwrap().document.stories.iter().find(|s| s.id.0 == story).unwrap().text, "아버지가 방에 들어가신다");
    call(&mut ws, "edit.undo", json!({}));
    assert_eq!(ws.session().unwrap().document.stories.iter().find(|s| s.id.0 == story).unwrap().text, BAD);
    host.grammar_disabled = true; host.check(&mut ws); assert!(host.grammar_suggestions.is_empty());
}
#[test]
fn grammar_has_separate_canvas_underlines_and_toggle_keeps_document_unchanged() {
    let _home = test_home::enter("c11_grammar_canvas"); let (mut ws, object, story) = setup();
    assert!(!dtool_tools::dtool_text::spelling::ko_grammar::check(BAD).is_empty());
    call(&mut ws, "text.select", json!({"story": story, "id": object, "range": {"start": 0, "end": 0}}));
    let session = ws.session().unwrap(); let version = session.document.version;
    dtool_ui::canvas::frame_text_input::set_grammar_enabled(true);
    dtool_ui::canvas::frame_text_input::refresh_spelling(session, None);
    let _ = dtool_ui::canvas::frame_text_input::spell_underlines(session);
    assert!(!dtool_ui::canvas::frame_text_input::grammar_underlines(session).is_empty());
    dtool_ui::canvas::frame_text_input::note_spell_edit(session);
    assert!(dtool_ui::canvas::frame_text_input::spell_pending(session));
    dtool_ui::canvas::frame_text_input::set_grammar_enabled(false);
    let _ = dtool_ui::canvas::frame_text_input::spell_underlines(session);
    assert!(!dtool_ui::canvas::frame_text_input::spell_pending(session));
    assert!(dtool_ui::canvas::frame_text_input::grammar_underlines(session).is_empty());
    assert_eq!(session.document.version, version);
    dtool_ui::canvas::frame_text_input::set_grammar_enabled(true);
    assert!(dtool_ui::about::LICENCES.iter().any(|s| s.contains("Kiwi")));
}
