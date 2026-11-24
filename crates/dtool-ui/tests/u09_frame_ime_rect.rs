use dtool_core::longdoc::marks::caret_point;
use dtool_core::view_state::ViewState;
use dtool_core::workspace::Workspace;
use dtool_core::doc_to_view;
use dtool_ui::canvas::frame_text_input;
use egui::{Context, Event, ImeEvent, Key, Pos2, RawInput, Rect};
use serde_json::json;

const HAN: &str = "\u{D55C}";
const GUK: &str = "\u{AD6D}";

fn canvas() -> Rect {
    Rect::from_min_size(Pos2::new(100.0, 50.0), egui::vec2(600.0, 400.0))
}

fn primed() -> (Workspace, u64) {
    assert!(dtool_tools::frame_text::install_flow_ranges());
    let mut workspace = Workspace::default();
    workspace.new_document("u09");
    let session = workspace.session_mut().unwrap();
    session.run_named("text.frame.add", json!({"rect": [20.0, 30.0, 300.0, 200.0], "text": "ab"})).unwrap();
    let story = session.document.stories.last().unwrap().id.0;
    session.run_named("text.select", json!({"story": story, "range": {"start": 2, "end": 2}})).unwrap();
    (workspace, story)
}

fn ime_rect(workspace: &Workspace) -> Rect {
    let ctx = Context::default();
    ctx.begin_pass(RawInput::default());
    frame_text_input::publish_caret_ime(&ctx, workspace.session().unwrap(), &ViewState::default(), canvas());
    ctx.output(|o| o.ime.clone()).expect("ime").cursor_rect
}

fn locator_rect(workspace: &Workspace) -> Rect {
    let session = workspace.session().unwrap();
    let edit = session.text_edit.as_ref().expect("text edit");
    let mut offset = 0usize;
    for story in &session.document.stories {
        if story.id == edit.story {
            break;
        }
        offset += story.text.chars().count() + 1;
    }
    let point = caret_point(&session.document, offset + edit.caret).expect("caret point");
    let view = ViewState::default();
    let top = doc_to_view(&view, point.point);
    let bottom = doc_to_view(&view, [point.point[0], point.point[1] + point.line_height]);
    let min = Pos2::new(canvas().min.x + top[0], canvas().min.y + top[1]);
    Rect::from_min_max(min, Pos2::new(min.x + 1.0, (canvas().min.y + bottom[1]).max(min.y + 1.0)))
}

fn feed(workspace: &mut Workspace, events: Vec<Event>) {
    frame_text_input::feed_text_input(workspace.session_mut().unwrap(), &events);
}

fn text(workspace: &Workspace, story: u64) -> String {
    workspace.session().unwrap().document.stories.iter().find(|s| s.id.0 == story).unwrap().text.clone()
}

fn same(a: Rect, b: Rect) -> bool {
    (a.min.x - b.min.x).abs() < 0.01 && (a.min.y - b.min.y).abs() < 0.01 && (a.max.y - b.max.y).abs() < 0.01
}

#[test]
fn frame_text_ime_rect_follows_caret_after_typing() {
    let (mut ws, story) = primed();
    let first = ime_rect(&ws);
    assert!(same(first, locator_rect(&ws)), "{first:?} {:?}", locator_rect(&ws));
    assert!(first.min.x > canvas().min.x + 20.0, "{first:?}");
    feed(&mut ws, vec![Event::Text("cd".into())]);
    assert_eq!(text(&ws, story), "abcd");
    let second = ime_rect(&ws);
    assert!(same(second, locator_rect(&ws)), "{second:?} {:?}", locator_rect(&ws));
    assert!(second.min.x > first.min.x, "{first:?} {second:?}");
    feed(&mut ws, vec![Event::Ime(ImeEvent::Preedit { text: HAN.into(), active_range_chars: None })]);
    feed(&mut ws, vec![Event::Ime(ImeEvent::Commit(HAN.into()))]);
    feed(&mut ws, vec![Event::Ime(ImeEvent::Preedit { text: GUK.into(), active_range_chars: None })]);
    feed(&mut ws, vec![Event::Ime(ImeEvent::Commit(GUK.into()))]);
    assert_eq!(text(&ws, story), format!("abcd{HAN}{GUK}"));
    let third = ime_rect(&ws);
    assert!(same(third, locator_rect(&ws)), "{third:?} {:?}", locator_rect(&ws));
    assert!(third.min.x > second.min.x, "{second:?} {third:?}");
}

#[test]
fn frame_text_ime_rect_moves_to_next_line_after_enter() {
    let (mut ws, story) = primed();
    feed(&mut ws, vec![Event::Text("cd".into())]);
    let before = ime_rect(&ws);
    feed(&mut ws, vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
    assert_eq!(text(&ws, story), "abcd\n");
    let after = ime_rect(&ws);
    assert!(same(after, locator_rect(&ws)), "{after:?} {:?}", locator_rect(&ws));
    assert!(after.min.y > before.min.y, "{before:?} {after:?}");
    assert!(after.min.x < before.min.x, "{before:?} {after:?}");
    feed(&mut ws, vec![Event::Text("x".into())]);
    let typed = ime_rect(&ws);
    assert!(same(typed, locator_rect(&ws)), "{typed:?} {:?}", locator_rect(&ws));
    assert!(typed.min.x > after.min.x && (typed.min.y - after.min.y).abs() <= 0.5, "{before:?} {after:?} {typed:?}");
}

#[test]
fn first_char_on_empty_line_keeps_baseline_and_caret_height() {
    let (mut ws, story) = primed();
    feed(&mut ws, vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
    let empty = ime_rect(&ws);
    let core = ws.session().unwrap().document.stories.iter().find(|s| s.id.0 == story).unwrap().clone();
    let text_lib = dtool_tools::dtool_text::FontLibrary::shared();
    let empty_lines = dtool_tools::dtool_text::layout_story(&text_lib, &dtool_tools::dtool_text::story_from_core(&core), Some(300.0));
    feed(&mut ws, vec![Event::Text("x".into())]);
    assert_eq!(text(&ws, story), "ab\nx");
    let typed = ime_rect(&ws);
    let core = ws.session().unwrap().document.stories.iter().find(|s| s.id.0 == story).unwrap().clone();
    let typed_lines = dtool_tools::dtool_text::layout_story(&text_lib, &dtool_tools::dtool_text::story_from_core(&core), Some(300.0));
    assert_eq!(empty_lines.len(), 2, "{empty_lines:?}");
    assert_eq!(typed_lines.len(), 2, "{typed_lines:?}");
    assert!((empty_lines[1].baseline - typed_lines[1].baseline).abs() <= 0.5, "{} {}", empty_lines[1].baseline, typed_lines[1].baseline);
    assert!((empty.min.y - typed.min.y).abs() <= 0.5, "{empty:?} {typed:?}");
    assert!((empty.height() - typed.height()).abs() <= 0.5, "{empty:?} {typed:?}");
}
