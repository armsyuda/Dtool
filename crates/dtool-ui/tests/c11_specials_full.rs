#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_tools::dtool_text::special::resolve_special_char;
use dtool_ui::menu_tree;
use dtool_ui::menus::text_insert;
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

const ROWS: &[&str] = &[
    "dashes_and_hyphens.em_dash", "dashes_and_hyphens.en_dash", "dashes_and_hyphens.figure_dash", "dashes_and_hyphens.soft_hyphen", "dashes_and_hyphens.non_breaking_hyphen",
    "spaces_and_tabs.em_space", "spaces_and_tabs.en_space", "spaces_and_tabs.non_breaking_space", "spaces_and_tabs.zero_width_space", "spaces_and_tabs.hair_space",
    "spaces_and_tabs.thin_space", "spaces_and_tabs.punctuation_space", "spaces_and_tabs.figure_space", "spaces_and_tabs.zero_width_non_joiner",
    "spaces_and_tabs.right_indent_tab", "spaces_and_tabs.indent_to_here",
    "breaks.line_break", "breaks.column_break", "breaks.frame_break", "breaks.page_break", "breaks.odd_page_break", "breaks.even_page_break",
];

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
}

fn steps(shell: &mut AppShell) -> usize {
    shell.workspace.session_mut().unwrap().undo.applied_steps()
}

fn story(shell: &mut AppShell, id: u64) -> dtool_core::Story {
    shell.workspace.session_mut().unwrap().document.stories.iter().find(|s| s.id.0 == id).cloned().unwrap()
}

#[test]
fn every_text_insert_row_inserts_at_the_caret_as_one_undo_step() {
    let _home = test_home::enter("c11_specials_full");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c11 specials");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "ab" }));
    let (object, id) = (added["id"].as_u64().unwrap(), added["story"].as_u64().unwrap());
    frame(&mut shell, &ctx);
    for row in ROWS {
        let menu = format!("{}{row}", text_insert::MENU_PREFIX);
        let what = text_insert::special_for(&menu).unwrap_or_else(|| panic!("{menu} not routed"));
        assert!(menu_tree::row_is_local(&menu, true), "{menu}");
        run(&mut shell, "text.select", json!({ "story": id, "id": object, "range": { "start": 1, "end": 1 } }));
        frame(&mut shell, &ctx);
        let before = steps(&mut shell);
        assert!(shell.insert_special(&menu), "{menu}");
        dtool_core::text_cmd::commit_text_edit(shell.workspace.session_mut().unwrap());
        let after = story(&mut shell, id);
        match resolve_special_char(what) {
            Some(ch) => assert_eq!(after.text, format!("a{ch}b"), "{menu}"),
            None => assert!(after.specials.iter().any(|(at, _)| *at == 1), "{menu}: {:?}", after.specials),
        }
        assert_eq!(steps(&mut shell), before + 1, "{menu}: one undo step");
        run(&mut shell, "edit.undo", json!({}));
        let undone = story(&mut shell, id);
        assert_eq!(undone.text, "ab", "{menu}");
        assert!(undone.specials.is_empty(), "{menu}");
        frame(&mut shell, &ctx);
    }
}
