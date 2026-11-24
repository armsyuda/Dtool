use dtool_core::paint::{Colour, Hatch, HatchLine, HatchUnits};
use dtool_design::numeric_area_id;
use dtool_ui::hatch_editor::{show, HatchEditorState};
use egui::{Context, Event, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};

fn hatch() -> Hatch {
    Hatch {
        lines: vec![HatchLine { angle_rad: 0.0, origin: [0.0, 0.0], shift: 0.0, spacing: 6.0, dash: Vec::new() }],
        colour: Colour::new(0, 0, 0, 255),
        background: None,
        weight: 1.0,
        units: HatchUnits::Px,
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    }
}

fn run(ctx: &Context, state: &mut HatchEditorState, events: Vec<Event>) -> bool {
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() };
    let mut changed = false;
    ctx.run_ui(input, |ui| changed = show(ui, state)).textures_delta.clear();
    changed
}

fn press(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn key(key: Key) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

fn field_id(name: &str, index: usize, slot: usize) -> Id {
    Id::NULL.with(("dtool.hatch_editor.number", (name, index, slot)))
}

fn type_into(ctx: &Context, state: &mut HatchEditorState, id: Id, typed: &str) -> Vec<bool> {
    let rect = ctx.read_response(numeric_area_id(id)).expect("numeric field area rendered").rect;
    let at = rect.center();
    let mut changed = Vec::new();
    for events in [vec![Event::PointerMoved(at)], vec![press(at, true)], vec![press(at, false)], vec![], vec![Event::Text(typed.to_string())], vec![key(Key::Enter)], vec![]] {
        changed.push(run(ctx, state, events));
    }
    changed
}

fn setup() -> (Context, HatchEditorState) {
    let ctx = Context::default();
    let mut state = HatchEditorState::new();
    state.set_hatch(Some(hatch()));
    run(&ctx, &mut state, vec![]);
    run(&ctx, &mut state, vec![]);
    (ctx, state)
}

#[test]
fn hatch_weight_numeric_field_commits_typed_value_once() {
    let (ctx, mut state) = setup();
    let changed = type_into(&ctx, &mut state, field_id("weight", 0, 0), "3");
    assert_eq!(changed.iter().filter(|c| **c).count(), 1, "{changed:?}");
    assert_eq!(state.get_hatch().unwrap().weight, 3.0);
}

#[test]
fn hatch_weight_keeps_non_negative_guard() {
    let (ctx, mut state) = setup();
    type_into(&ctx, &mut state, field_id("weight", 0, 0), "-2");
    assert_eq!(state.get_hatch().unwrap().weight, 1.0);
}

#[test]
fn hatch_line_spacing_field_targets_its_own_line() {
    let (ctx, mut state) = setup();
    type_into(&ctx, &mut state, field_id("spacing", 0, 0), "12");
    let hatch = state.get_hatch().unwrap();
    assert_eq!(hatch.lines[0].spacing, 12.0);
    assert_eq!(hatch.weight, 1.0);
}

#[test]
fn hatch_field_escape_leaves_value() {
    let (ctx, mut state) = setup();
    let rect = ctx.read_response(numeric_area_id(field_id("shift", 0, 0))).unwrap().rect;
    let at = rect.center();
    for events in [vec![Event::PointerMoved(at)], vec![press(at, true)], vec![press(at, false)], vec![], vec![Event::Text("9".into())], vec![key(Key::Escape)], vec![]] {
        assert!(!run(&ctx, &mut state, events));
    }
    assert_eq!(state.get_hatch().unwrap().lines[0].shift, 0.0);
}