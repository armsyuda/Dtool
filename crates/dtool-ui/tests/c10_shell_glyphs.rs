#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_ui::dock::PanelId;
use dtool_ui::panels::{self, glyph_browser::GLYPH_BROWSER_PANEL_KEY, WINDOW_GLYPH_BROWSER};
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

#[test]
fn c10_glyph_browser_docks_and_inserts_into_the_live_story() {
    let _home = test_home::enter("c10_shell_glyphs");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c10 glyphs");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "ab" }));
    let object = added["id"].as_u64().unwrap();
    let story = added["story"].as_u64().unwrap();
    run(&mut shell, "selection.set", json!({ "ids": [object], "op": "Replace" }));
    let lib = dtool_tools::dtool_text::FontLibrary::shared();
    let family = ["Malgun Gothic", "Arial", "Segoe UI"]
        .into_iter()
        .find(|family| !lib.faces(family).is_empty())
        .map(str::to_string)
        .or_else(|| lib.families().into_iter().find(|family| !lib.faces(family).is_empty()).map(str::to_string))
        .expect("an installed font");
    let style = lib.faces(&family)[0].style.clone();
    run(&mut shell, "text.style", json!({ "story": story, "range": { "start": 0, "end": 2 }, "patch": { "family": family, "style": style } }));

    let id = panels::window_panel(WINDOW_GLYPH_BROWSER).expect("menu row maps to the panel");
    assert_eq!(id, PanelId::Other(GLYPH_BROWSER_PANEL_KEY.to_string()));
    assert!(panels::WINDOW_PANELS.contains(&WINDOW_GLYPH_BROWSER));
    assert!(shell.panel_registry.has_body(&id));
    frame(&mut shell, &ctx);
    frame(&mut shell, &ctx);
    if !shell.layout.is_panel_shown(&id) {
        panels::toggle_panel(&mut shell.layout, &id);
    }
    assert!(shell.layout.is_panel_shown(&id));
    for _ in 0..3 {
        frame(&mut shell, &ctx);
    }
    assert!(shell.layout.is_panel_shown(&id));
    assert!(panels::window_panel_checked(WINDOW_GLYPH_BROWSER), "menu row shows the checkmark while docked");

    let browser = &mut shell.panel_registry.glyph_browser;
    assert!(browser.font_id.is_some(), "docked browser drew a frame and followed the story face");
    assert_eq!(lib.face(browser.font_id.unwrap()).map(|face| face.family.clone()), Some(family.clone()));
    if browser.page.is_none() {
        browser.query(&lib);
    }
    let index = browser.page.as_ref().and_then(|page| page.items.iter().position(|g| g.unicode.is_some_and(|c| c.is_alphanumeric()))).expect("a printable glyph");
    let before = undo_steps(&mut shell);
    let text_before = story_text(&mut shell, story);
    let session = shell.workspace.session_mut().unwrap();
    assert!(shell.panel_registry.glyph_browser.insert_at(session, index).unwrap());
    assert_eq!(undo_steps(&mut shell), before + 1, "one glyph insert = one undo step");
    let text_after = story_text(&mut shell, story);
    assert_eq!(text_after.chars().count(), text_before.chars().count() + 1, "{text_before:?} -> {text_after:?}");
    assert!(!shell.panel_registry.glyph_browser.recent.items.is_empty());
    frame(&mut shell, &ctx);
}
