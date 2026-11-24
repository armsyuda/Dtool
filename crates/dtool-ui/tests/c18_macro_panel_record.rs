#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::Command;
use dtool_ui::macro_panel::MacroPanel;
use serde_json::json;

#[test]
fn panel_record_stop_edit_disable_play_and_reset_use_the_session_recorder() {
    let _home = test_home::enter("panel_record_stop_edit_disable_play_and_reset_use_the_session_recorder");
    let mut ws = Workspace::default();
    let a = ws.new_document("A");
    let b = ws.new_document("B");
    let mut panel = MacroPanel::default();
    panel.start_recording(&mut ws.get_mut(a).unwrap().session).unwrap();
    assert!(panel.recording && ws.get(a).unwrap().session.macro_recorder.is_recording());
    for (name, x) in [("one", 1.0), ("two", 2.0), ("three", 3.0)] {
        ws.run(Scope::of(a), Command::with_params("object.create", json!({ "name": name, "pos": [x, 0.0], "size": [4.0, 4.0] }))).1.unwrap();
    }
    assert_eq!(panel.stop_recording(&mut ws.get_mut(a).unwrap().session).unwrap(), 3);
    assert!(!panel.recording);
    panel.steps[1].enabled = false;
    panel.steps[2].params["name"] = json!("edited");
    let target = &mut ws.get_mut(b).unwrap().session;
    assert_eq!(panel.play(target).unwrap(), 2);
    let names: Vec<String> = target.document.objects.iter().map(|o| o.name.clone()).collect();
    assert_eq!(names, vec!["one".to_string(), "edited".to_string()]);
    let source = &mut ws.get_mut(a).unwrap().session;
    panel.reset(source).unwrap();
    assert!(panel.steps.is_empty() && source.macro_recorder.steps().is_empty() && !source.macro_recorder.is_recording());
}