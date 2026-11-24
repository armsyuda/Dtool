#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::object::ObjectId;
use dtool_core::{Command, Scope};
use dtool_ui::context_menu::{dialog_of, query, DIALOG_ROWS};
use dtool_ui::shell::AppShell;
use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use serde_json::{json, Value};

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
    let none = Modifiers::default();
    frame(shell, ctx, vec![Event::PointerMoved(at)]);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button, pressed: true, modifiers: none }]);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button, pressed: false, modifiers: none }]);
    frame(shell, ctx, Vec::new())
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

#[test]
fn c08_right_click_layer_effects_opens_the_layer_effects_dialog() {
    let _home = test_home::enter("c08_ctx_layer_effects");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("ctx layer effects");
    shell.workspace.activate(doc).unwrap();
    shell.workspace.run(Scope::active(), Command { name: "object.create".into(), params: json!({ "name": "fx", "pos": [-2000.0, -2000.0], "size": [6000.0, 6000.0] }) }).1.expect("create");
    let id = shell.workspace.session().unwrap().document.objects.iter().find(|o| o.name == "fx").unwrap().id;
    shell.workspace.session_mut().unwrap().selection.ids = vec![id];
    frame(&mut shell, &ctx, Vec::new());
    let before = undo_steps(&mut shell);
    assert!(!shell.layer_effects.open);

    let label = dtool_strings::text("menu.layer.layer_effects");
    let found = press(&mut shell, &ctx, Pos2::new(760.0, 600.0), PointerButton::Secondary);
    assert_eq!(shell.workspace.session().unwrap().selection.ids, vec![id], "right-click keeps the selection");
    let row = found.iter().rev().find(|(text, _)| *text == label).map(|(_, rect)| *rect).unwrap_or_else(|| panic!("context menu lists {label}: {:?}", found.iter().map(|(t, _)| t.clone()).collect::<Vec<_>>()));
    press(&mut shell, &ctx, row.center(), PointerButton::Primary);
    frame(&mut shell, &ctx, Vec::new());
    assert!(shell.layer_effects.open, "layer effects dialog is open");
    assert_eq!(shell.layer_effects_owner, shell.workspace.active());
    assert_eq!(undo_steps(&mut shell), before, "opening the dialog changes nothing");
    let _ = ObjectId(0);
}

#[test]
fn c08_layer_effects_row_is_a_dialog_row_in_the_object_menu() {
    assert!(DIALOG_ROWS.contains(&"layer.layer_effects"));
    let rows = query("default", "Object");
    assert!(rows.as_array().unwrap().iter().any(|row| row["id"] == json!("layer.layer_effects")));
    assert_eq!(dialog_of(&json!({ "dialog": "layer.layer_effects" })), Some("layer.layer_effects"));
    assert_eq!(dialog_of(&Value::Null), None);
}
