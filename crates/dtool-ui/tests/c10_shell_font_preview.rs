#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::{Command, Scope};
use dtool_tools::dtool_text::font_preview::font_preview;
use dtool_tools::dtool_text::FontLibrary;
use dtool_ui::shell::AppShell;
use egui::{Event, PointerButton, Pos2, Rect};
use serde_json::{json, Value};

fn run(shell: &mut AppShell, name: &str, params: Value) -> Value {
    shell.workspace.run(Scope::active(), Command { name: name.into(), params }).1.expect(name)
}

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

fn find(found: &[(String, Rect)], label: &str) -> Option<Rect> {
    found.iter().rev().find(|(text, _)| text == label).map(|(_, rect)| *rect)
}

fn click(shell: &mut AppShell, ctx: &egui::Context, at: Pos2) -> Vec<(String, Rect)> {
    frame(shell, ctx, vec![Event::PointerMoved(at)]);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::default() }]);
    frame(shell, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::default() }]);
    frame(shell, ctx, Vec::new())
}

fn undo_steps(shell: &mut AppShell) -> usize {
    let session = shell.workspace.session_mut().unwrap();
    dtool_core::history_store::HistoryStore::from_undo(&session.undo).list().len()
}

#[test]
fn c10_font_combo_hover_previews_and_pick_applies_on_the_live_shell() {
    let _home = test_home::enter("c10_shell_font_preview");
    let mut shell = AppShell::new();
    let ctx = egui::Context::default();
    let doc = shell.workspace.new_document("c10 font preview");
    shell.workspace.activate(doc).unwrap();
    let added = run(&mut shell, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "hello" }));
    let (object, story) = (added["id"].as_u64().unwrap(), added["story"].as_u64().unwrap());
    run(&mut shell, "selection.set", json!({ "ids": [object], "op": "Replace" }));
    let found = frame(&mut shell, &ctx, Vec::new());
    let current = shell.text_bar.family.clone().expect("text bar shows the family");
    let families: Vec<String> = FontLibrary::shared().families().into_iter().map(str::to_owned).collect();
    let combo = found.iter().find(|(label, rect)| label == &current && rect.min.y < 180.0)
        .map(|(_, rect)| *rect).expect("family selector in the text bar");
    let version = shell.workspace.session().unwrap().document.version;
    let steps = undo_steps(&mut shell);

    let found = click(&mut shell, &ctx, combo.center());
    let (other, row) = found.iter().find(|(text, _)| *text != current && families.contains(text)).map(|(text, rect)| (text.clone(), *rect)).unwrap_or_else(|| panic!("popup lists another family; combo {combo:?} current {current} texts {:?}", found.iter().map(|(t, r)| format!("{t}@{:.0},{:.0}", r.min.x, r.min.y)).collect::<Vec<_>>()));
    frame(&mut shell, &ctx, vec![Event::PointerMoved(row.center())]);
    frame(&mut shell, &ctx, Vec::new());
    assert_eq!(shell.font_preview.as_deref(), Some(other.as_str()));
    let preview = font_preview().expect("hover runs text.font.preview");
    assert_eq!((preview.story, preview.start, preview.end), (story, 0, 5));
    assert!(preview.family.eq_ignore_ascii_case(&other));
    assert_eq!(shell.workspace.session().unwrap().document.version, version);
    assert_eq!(undo_steps(&mut shell), steps);

    let found = frame(&mut shell, &ctx, vec![Event::PointerMoved(Pos2::new(1500.0, 900.0))]);
    frame(&mut shell, &ctx, Vec::new());
    assert_eq!(shell.font_preview, None);
    assert!(font_preview().is_none());

    let row = find(&found, &other).unwrap_or(row);
    frame(&mut shell, &ctx, vec![Event::PointerMoved(row.center())]);
    frame(&mut shell, &ctx, Vec::new());
    assert!(font_preview().is_some());
    frame(&mut shell, &ctx, vec![Event::PointerButton { pos: row.center(), button: PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::default() }]);
    frame(&mut shell, &ctx, vec![Event::PointerButton { pos: row.center(), button: PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::default() }]);
    frame(&mut shell, &ctx, Vec::new());
    assert!(font_preview().is_none(), "pick cancels the preview");
    assert_eq!(shell.font_preview, None);
    assert_eq!(undo_steps(&mut shell), steps + 1, "the pick is one text.style step");
    let style = run(&mut shell, "text.style.query", json!({ "story": story, "range": { "start": 0, "end": 5 } }));
    assert!(style["family"].as_str().unwrap().eq_ignore_ascii_case(&other));
}
