#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::document::BreakKind;
use dtool_core::{Command, Scope};
use dtool_ui::shell::AppShell;
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

fn mark_texts(shape: &egui::Shape, colour: egui::Color32, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) if text.fallback_color == colour => out.push(text.galley.text().to_string()),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| mark_texts(shape, colour, out)),
        _ => {}
    }
}

fn frame(shell: &mut AppShell, ctx: &egui::Context) -> Vec<String> {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))), ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| shell.show(ui));
    out.textures_delta.clear();
    let mut found = Vec::new();
    let colour = dtool_design::color("invisibles.mark");
    for clipped in &out.shapes {
        mark_texts(&clipped.shape, colour, &mut found);
    }
    found
}

#[test]
fn b41_break_marks_draw_on_the_live_canvas_with_show_invisibles() {
    let _home = test_home::enter("b41_break_marks_canvas");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("break marks");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.art.add", json!({ "pos": [40.0, 60.0], "text": "aaaa bbbb cccc", "size_pt": 36.0 }));
    let story = added["story"].as_u64().unwrap();
    let id = dtool_core::object::ObjectId(added["id"].as_u64().unwrap());
    run(&mut shell, "text.special.insert", json!({ "story": story, "at": 4, "what": "column_break" }));
    run(&mut shell, "text.special.insert", json!({ "story": story, "at": 10, "what": "page_break" }));
    for _ in 0..2 {
        frame(&mut shell, &ctx);
    }
    let marks = dtool_core::text_lines::text_break_marks(&shell.workspace.session().unwrap().document, id);
    assert_eq!(marks.iter().map(|m| m.kind).collect::<Vec<_>>(), vec![BreakKind::Column, BreakKind::Page], "{marks:?}");
    let off = frame(&mut shell, &ctx);
    assert!(!off.iter().any(|t| t == "\u{2193}" || t == "\u{21A7}"), "no break marks while invisibles are hidden");

    run(&mut shell, "view.flags.set", json!({ "show_invisibles": true }));
    frame(&mut shell, &ctx);
    let on = frame(&mut shell, &ctx);
    assert!(on.iter().any(|t| t == "\u{2193}"), "column break mark drawn: {on:?}");
    assert!(on.iter().any(|t| t == "\u{21A7}"), "page break mark drawn");

    run(&mut shell, "edit.undo", json!({}));
    run(&mut shell, "view.flags.set", json!({ "show_invisibles": true }));
    let undone = frame(&mut shell, &ctx);
    assert!(undone.iter().any(|t| t == "\u{2193}"));
    assert!(!undone.iter().any(|t| t == "\u{21A7}"), "undo removes the page break mark");
}
