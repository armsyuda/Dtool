#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::guides::GuideId;
use dtool_core::workspace::Scope;
use dtool_core::Command;
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn shell() -> AppShell {
    let mut shell = AppShell::new();
    let id = shell.workspace.new_document("c02_guides");
    shell.workspace.activate(id).unwrap();
    shell
}

fn run(shell: &mut AppShell, name: &str, params: Value) -> Result<Value, dtool_core::CommandError> {
    shell.workspace.run(Scope::active(), Command::with_params(name, params)).1
}

fn painted(shell: &mut AppShell) -> usize {
    let ctx = egui::Context::default();
    let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 300.0));
    let input = egui::RawInput { screen_rect: Some(canvas), ..Default::default() };
    let rulers = &shell.rulers;
    let session = shell.workspace.session_mut().unwrap();
    let mut view = session.view.clone();
    view.zoom = 1.0;
    view.scroll = [0.0, 0.0];
    view.rotation_deg = 0.0;
    let session = &*session;
    let mut output = ctx.run_ui(input, |ui| rulers.paint_visible_guides(ui, session, &view, canvas));
    output.textures_delta.clear();
    output.shapes.len()
}

fn guide_ids(shell: &mut AppShell) -> Vec<u64> {
    shell.workspace.session_mut().unwrap().guides.list().iter().map(|g| g.id.0).collect()
}

#[test]
fn guides_add_show_toggle_lock_and_undo_on_live_document() {
    let _home = test_home::enter("guides_add_show_toggle_lock_and_undo_on_live_document");
    let mut s = shell();
    let base = painted(&mut s);

    run(&mut s, "guides.add", json!({ "kind": "Vertical", "position": "150px", "name": "v" })).unwrap();
    run(&mut s, "guides.add", json!({ "kind": "Horizontal", "position": 80.0 })).unwrap();
    let (units, dpi) = { let d = &s.workspace.session_mut().unwrap().document; (d.units, d.dpi) };
    let expected = dtool_core::units::from_px(150.0, units, dpi).unwrap();
    let positions: Vec<f32> = s.workspace.session_mut().unwrap().guides.list().iter().map(|g| g.position).collect();
    assert!((positions[0] - expected).abs() < 1e-3, "{positions:?}");
    assert_eq!(positions[1], 80.0);
    assert!(run(&mut s, "guides.add", json!({ "kind": "Vertical", "position": "12parsec" })).is_err());
    assert_eq!(guide_ids(&mut s).len(), 2);

    let listed = run(&mut s, "guides.list", json!({})).unwrap();
    assert_eq!(listed["guides"].as_array().unwrap().len(), 2, "{listed}");
    let shown = painted(&mut s);
    assert!(shown >= base + 2, "base {base} shown {shown}");

    let hidden = run(&mut s, "guides.show", json!({ "show": false })).unwrap();
    assert_eq!(hidden["show"], json!(false));
    assert_eq!(painted(&mut s), base);
    run(&mut s, "guides.toggle", json!({})).unwrap();
    assert!(s.workspace.session_mut().unwrap().guides.show);
    assert_eq!(painted(&mut s), shown);

    run(&mut s, "guides.add", json!({ "kind": "Column", "position": 40.0 })).unwrap();
    run(&mut s, "guides.show", json!({ "show_columns": false })).unwrap();
    assert_eq!(painted(&mut s), shown);
    run(&mut s, "guides.show", json!({ "show_columns": true })).unwrap();
    assert!(painted(&mut s) > shown);

    let first = GuideId(guide_ids(&mut s)[0]);
    run(&mut s, "guides.show", json!({ "locked": true })).unwrap();
    assert!(s.workspace.session_mut().unwrap().guides.can_move(first).is_err());
    run(&mut s, "guides.show", json!({ "locked": false })).unwrap();
    assert!(s.workspace.session_mut().unwrap().guides.can_move(first).is_ok());

    run(&mut s, "guides.update", json!({ "id": first.0, "position": "200px" })).unwrap();
    let moved = s.workspace.session_mut().unwrap().guides.list()[0].position;
    assert!((moved - dtool_core::units::from_px(200.0, units, dpi).unwrap()).abs() < 1e-3);
    run(&mut s, "edit.undo", json!({})).unwrap();
    let back = s.workspace.session_mut().unwrap().guides.list()[0].position;
    assert!((back - expected).abs() < 1e-3, "{back}");
    run(&mut s, "edit.undo", json!({})).unwrap();
    assert_eq!(guide_ids(&mut s).len(), 2);
    run(&mut s, "edit.redo", json!({})).unwrap();
    assert_eq!(guide_ids(&mut s).len(), 3);
}