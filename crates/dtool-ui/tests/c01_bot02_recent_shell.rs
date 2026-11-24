#[path = "support/test_home.rs"]
mod test_home;
use dtool_ui::menu_tree::recent::{self, RecentOutcome};
use dtool_ui::shell::AppShell;
use std::path::{Path, PathBuf};

fn frame(shell: &mut AppShell, ctx: &egui::Context) {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| shell.show(ui));
    out.textures_delta.clear();
}

#[test]
fn shell_restores_saves_and_prompts_for_missing_recent_files() {
    let home = test_home::enter("c01_bot02_recent_shell");
    let legacy = recent::default_path();
    let store = dtool_core::Workspace::recent_default_path();
    assert!(legacy.starts_with(&home.root));
    assert!(store.starts_with(&home.root));
    let first = home.root.join("gone_a.dtool");
    let second = home.root.join("gone_b.dtool");
    recent::save(&legacy, &[first.clone(), second.clone()]).unwrap();
    let mut shell = AppShell::new();
    assert_eq!(shell.recent_files.path, None, "the core store owns the list");
    assert_eq!(shell.workspace.recent_store(), Some(store.as_path()));
    assert_eq!(shell.workspace.recent(), &[first.clone(), second.clone()], "legacy list migrated");
    let ctx = egui::Context::default();
    frame(&mut shell, &ctx);
    let third = PathBuf::from("third.dtool");
    shell.workspace.push_recent(Path::new(&third));
    frame(&mut shell, &ctx);
    assert_eq!(recent::load(&store), vec![third.clone(), first.clone(), second.clone()]);
    let outcome = shell.recent_files.handle(&mut shell.workspace, &recent::item_id(1)).unwrap();
    assert_eq!(outcome, RecentOutcome::Missing(first.clone()));
    frame(&mut shell, &ctx);
    assert!(shell.recent_files.missing.is_some());
    shell.recent_files.resolve_missing(&mut shell.workspace, true);
    frame(&mut shell, &ctx);
    assert_eq!(recent::load(&store), vec![third, second]);
}
