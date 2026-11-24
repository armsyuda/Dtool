#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Session, Workspace};
use dtool_export::macros::{Macro, MacroLibrary, MacroStep};
use dtool_ui::macro_library_panel::{LibraryAction, MacroLibraryPanel};
use dtool_ui::macro_panel::{Alignment, MacroPanel, MacroStepUI, ScaleMode};
use serde_json::json;

fn step(command: &str, params: serde_json::Value, enabled: bool) -> MacroStep {
    MacroStep { command: command.into(), params, enabled, show_name: false }
}

fn session(workspace: &mut Workspace) -> &mut Session {
    let id = workspace.new_document("c18");
    workspace.activate(id).unwrap();
    workspace.session_mut().unwrap()
}

#[test]
fn library_play_runs_enabled_steps_on_the_session() {
    let _home = test_home::enter("library_play_runs_enabled_steps_on_the_session");
    let mut workspace = Workspace::default();
    let session = session(&mut workspace);
    let before = session.document.objects.len();
    let mut library = MacroLibrary::new();
    library.add_macro("two", "c18", Macro {
        name: "two".into(),
        category: "c18".into(),
        steps: vec![
            step("object.create", json!({"pos": [0.0, 0.0], "size": [10.0, 10.0]}), true),
            step("object.create", json!({"pos": [20.0, 0.0], "size": [10.0, 10.0]}), false),
            step("object.create", json!({"pos": [40.0, 0.0], "size": [10.0, 10.0]}), true),
        ],
    });
    let mut panel = MacroLibraryPanel::with_library(library, None);
    panel.selected_category = Some("c18".into());
    panel.selected_macro = Some("two".into());
    assert_eq!(panel.play_selected(session), Some(LibraryAction::Played("two".into())));
    assert_eq!(session.document.objects.len(), before + 2);
    assert!(panel.problem.is_none());
}

#[test]
fn failing_step_stops_play_and_reports_the_error() {
    let _home = test_home::enter("failing_step_stops_play_and_reports_the_error");
    let mut workspace = Workspace::default();
    let session = session(&mut workspace);
    let before = session.document.objects.len();
    let steps = vec![
        step("object.create", json!({"pos": [0.0, 0.0], "size": [10.0, 10.0]}), true),
        step("no.such.command", json!({}), true),
        step("object.create", json!({"pos": [40.0, 0.0], "size": [10.0, 10.0]}), true),
    ];
    assert!(dtool_export::macros::play_steps(session, &steps).is_err());
    assert_eq!(session.document.objects.len(), before);
    let nested = vec![step("macro.play", json!({"name": "x"}), true)];
    assert!(dtool_export::macros::play_steps(session, &nested).is_err());
}

#[test]
fn macro_panel_play_runs_its_steps() {
    let _home = test_home::enter("macro_panel_play_runs_its_steps");
    let mut workspace = Workspace::default();
    let session = session(&mut workspace);
    let before = session.document.objects.len();
    let mut panel = MacroPanel::default();
    panel.steps.push(MacroStepUI {
        command: "object.create".into(),
        params: json!({"pos": [0.0, 0.0], "size": [10.0, 10.0]}),
        enabled: true,
        show_name: false,
        scale_mode: ScaleMode::NoScale,
        alignment: Alignment::Centre,
    });
    assert_eq!(panel.play(session).unwrap(), 1);
    assert_eq!(session.document.objects.len(), before + 1);
}
