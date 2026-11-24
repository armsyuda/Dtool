#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::prefs::Prefs;
use dtool_ui::home::visible_sections;
use dtool_ui::shell::AppShell;
use serde_json::json;

fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, out)),
        _ => {}
    }
}

#[test]
fn c12_shell_prefs_reach_live_sessions_and_workspace_recent_cap() {
    let home = test_home::enter("c12_shell_prefs");
    let mut shell = AppShell::new();
    let first = shell.workspace.new_document("one");
    shell.workspace.activate(first).unwrap();
    shell.pref_command("pref.performance.set", &json!({ "undo_limit": 3 })).expect("pref.performance.set");
    assert_eq!(shell.workspace.session().unwrap().undo.limit(), 3);

    let ctx = egui::Context::default();
    let second = shell.workspace.new_document("two");
    shell.workspace.activate(second).unwrap();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show(ui));
    out.textures_delta.clear();
    assert_eq!(shell.workspace.session().unwrap().undo.limit(), 3, "a context opened later gets the pref cap");

    shell.pref_command("pref.set", &json!({ "recent_count": 2 })).expect("pref.set");
    for name in ["a.dtool", "b.dtool", "c.dtool"] {
        shell.workspace.push_recent(&home.root.join(name));
    }
    assert_eq!(shell.workspace.recent().len(), 2);
}

#[test]
fn c12_home_show_recent_hides_recent_section() {
    let _home = test_home::enter("c12_home_show_recent");
    let mut prefs = Prefs::default();
    dtool_ui::home::ensure_sections(&mut prefs);
    assert!(visible_sections(&prefs).iter().any(|item| item.id == "recent"));
    prefs.home_show_recent = false;
    assert!(!visible_sections(&prefs).iter().any(|item| item.id == "recent"));
    assert!(visible_sections(&prefs).iter().any(|item| item.id == "templates"));
}

#[test]
fn c12_unwired_performance_prefs_are_marked_planned() {
    let _home = test_home::enter("c12_planned_prefs");
    let mut shell = AppShell::new();
    shell.open_preferences();
    shell.prefs_dialog.tab = dtool_ui::prefs_dialog::PrefsTab::Performance;
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show_preferences(ui));
    for _ in 0..2 {
        out.textures_delta.clear();
        out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show_preferences(ui));
    }
    out.textures_delta.clear();
    assert!(shell.prefs_dialog.tab == dtool_ui::prefs_dialog::PrefsTab::Performance);
    let mut found = Vec::new();
    for clipped in &out.shapes {
        texts(&clipped.shape, &mut found);
    }
    let planned = dtool_strings::text("menu.planned.suffix");
    assert_eq!(found.iter().filter(|text| *text == planned).count(), 4, "{found:?}");
}
