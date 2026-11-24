#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::command::TextEditState;
use dtool_core::StoryId;
use dtool_ui::shell::AppShell;
use dtool_ui::shortcuts::{Focus, Modifiers, ShortcutMap, TabAction};

fn no_mods() -> Modifiers {
    Modifiers { ctrl: false, shift: false, alt: false }
}

fn begin_text_edit(shell: &mut AppShell) {
    let session = shell.workspace.session_mut().expect("document");
    session.text_edit = Some(TextEditState {
        story: StoryId(1),
        object: None,
        caret: 0,
        anchor: 0,
        before: None,
    });
}

fn document_snapshot(shell: &AppShell) -> serde_json::Value {
    serde_json::to_value(&shell.workspace.session().expect("document").document).unwrap()
}

#[test]
fn tab_action_ignores_canvas_tab_while_text_edit_is_active() {
    let _home = test_home::enter("tab_action_ignores_canvas_tab_while_text_edit_is_active");
    let map = ShortcutMap::affinity_default();
    let mods = no_mods();
    assert_eq!(map.tab_action(Focus::Canvas, mods), TabAction::ToggleUi);
    assert_eq!(map.tab_action_for_edit(Focus::Canvas, mods, false), TabAction::ToggleUi);
    assert_eq!(map.tab_action_for_edit(Focus::Canvas, mods, true), TabAction::Ignore);
    assert_eq!(map.tab_action_for_edit(Focus::TextField, mods, true), TabAction::NextField);
}

#[test]
fn canvas_tab_without_text_edit_still_hides_and_shows_columns() {
    let _home = test_home::enter("canvas_tab_without_text_edit_still_hides_and_shows_columns");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("c16-tab");
    shell.workspace.activate(id).unwrap();
    assert!(!shell.text_editing());
    assert!(shell.tools_column_visible());
    let before = document_snapshot(&shell);
    assert_eq!(shell.handle_tab_command(Focus::Canvas, no_mods()), TabAction::ToggleUi);
    assert!(shell.ui_hidden());
    assert!(!shell.tools_column_visible());
    assert_eq!(document_snapshot(&shell), before);
    assert_eq!(shell.handle_tab_command(Focus::Canvas, no_mods()), TabAction::ToggleUi);
    assert!(!shell.ui_hidden());
    assert!(shell.tools_column_visible());
    assert_eq!(document_snapshot(&shell), before);
}

#[test]
fn canvas_tab_with_text_edit_does_not_toggle_columns_or_change_document() {
    let _home = test_home::enter("canvas_tab_with_text_edit_does_not_toggle_columns_or_change_document");
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("c16-tab-edit");
    shell.workspace.activate(id).unwrap();
    begin_text_edit(&mut shell);
    assert!(shell.text_editing());
    assert!(shell.tools_column_visible());
    let before = document_snapshot(&shell);
    let hidden = shell.ui_hidden();
    let tools = shell.tools_column_visible();
    assert_eq!(shell.handle_tab_command(Focus::Canvas, no_mods()), TabAction::Ignore);
    assert_eq!(shell.ui_hidden(), hidden);
    assert_eq!(shell.tools_column_visible(), tools);
    assert!(shell.tools_column_visible());
    assert_eq!(document_snapshot(&shell), before);
    assert!(shell.text_editing());
}
