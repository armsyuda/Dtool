use dtool_design::{format_number, numeric_field, numeric_state, numeric_value, NumericEvent, NumericSpec, NumericValue};
use egui::{Context, Event, Id, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, Response, TouchPhase, Ui};

fn frame<T>(ctx: &Context, events: Vec<Event>, mut add: impl FnMut(&mut Ui) -> T) -> T {
    let mut out = None;
    let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))), events, ..Default::default() };
    ctx.run_ui(input, |ui| out = Some(add(ui))).textures_delta.clear();
    out.unwrap()
}

fn press(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn key(key: Key) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

fn id() -> Id {
    Id::NULL.with("numeric")
}

struct Rig<'a> {
    ctx: Context,
    value: NumericValue,
    spec: NumericSpec<'a>,
    events: Vec<NumericEvent>,
    rect: Rect,
}

impl<'a> Rig<'a> {
    fn new(value: NumericValue, spec: NumericSpec<'a>) -> Self {
        let mut rig = Self { ctx: Context::default(), value, spec, events: Vec::new(), rect: Rect::NOTHING };
        rig.step(vec![]);
        rig
    }

    fn step(&mut self, input: Vec<Event>) -> Response {
        let (value, spec) = (self.value, &self.spec);
        let (response, event) = frame(&self.ctx, input, |ui| numeric_field(ui, id(), value, spec));
        self.rect = response.rect;
        if event != NumericEvent::None {
            self.events.push(event);
            if let NumericEvent::Commit(v) = event {
                self.value = NumericValue::Value(v);
            }
        }
        response
    }

    fn focus(&mut self) {
        let at = self.rect.right_center() - egui::vec2(4.0, 0.0);
        self.step(vec![Event::PointerMoved(at)]);
        self.step(vec![press(at, true)]);
        self.step(vec![press(at, false)]);
        self.step(vec![]);
    }

    fn typed(&mut self, text: &str) {
        self.step(vec![Event::Text(text.to_string())]);
    }

    fn commits(&self) -> Vec<f64> {
        self.events.iter().filter_map(|e| if let NumericEvent::Commit(v) = e { Some(*v) } else { None }).collect()
    }
}

#[test]
fn numeric_field_mixed_shows_placeholder_and_empty_does_not_commit() {
    let mut rig = Rig::new(NumericValue::Mixed, NumericSpec::new(""));
    rig.focus();
    assert_eq!(numeric_state(&rig.ctx, id()).buffer.as_deref(), Some(""));
    rig.step(vec![key(Key::Enter)]);
    rig.step(vec![]);
    assert!(rig.commits().is_empty());
    assert!(!numeric_state(&rig.ctx, id()).invalid);
}

#[test]
fn numeric_field_invalid_or_nonfinite_formula_refused() {
    let parse = |text: &str| -> Option<f64> {
        match text {
            "1/0" => Some(f64::INFINITY),
            "10mm+" => None,
            other => other.parse().ok(),
        }
    };
    for bad in ["10mm+", "1/0", "abc"] {
        let mut rig = Rig::new(NumericValue::Mixed, NumericSpec::new("").parser(&parse));
        rig.focus();
        rig.typed(bad);
        rig.step(vec![key(Key::Enter)]);
        let response = rig.step(vec![]);
        assert!(rig.commits().is_empty(), "{bad}");
        let state = numeric_state(&rig.ctx, id());
        assert!(state.invalid, "{bad}");
        assert_eq!(state.buffer.as_deref(), Some(bad));
        assert!(response.has_focus(), "{bad}");
    }
}

#[test]
fn numeric_field_formula_and_units_convert_to_doc_px() {
    let dpi = 300.0;
    let parse = move |text: &str| -> Option<f64> {
        let mm = text.strip_suffix("mm")?;
        let (a, b) = mm.split_once('*')?;
        Some(a.trim().parse::<f64>().ok()? * b.trim().parse::<f64>().ok()? * dpi / 25.4)
    };
    let to_mm = move |px: f64| px * 25.4 / dpi;
    let spec = NumericSpec::new("").parser(&parse).display(&to_mm).suffix("mm");
    let mut rig = Rig::new(NumericValue::Mixed, spec);
    rig.focus();
    rig.typed("10*2mm");
    rig.step(vec![key(Key::Enter)]);
    let commits = rig.commits();
    assert_eq!(commits.len(), 1);
    assert!((commits[0] - 236.22).abs() < 0.01, "{}", commits[0]);
    assert_eq!(rig.spec.format(commits[0]), "20 mm");
}

#[test]
fn numeric_field_enter_tab_focus_loss_commit_escape_cancels() {
    let mut rig = Rig::new(NumericValue::Mixed, NumericSpec::new(""));
    rig.focus();
    rig.typed("5");
    rig.step(vec![key(Key::Enter)]);
    assert_eq!(rig.commits(), vec![5.0]);

    let mut rig = Rig::new(NumericValue::Mixed, NumericSpec::new(""));
    rig.focus();
    rig.typed("6");
    rig.step(vec![key(Key::Tab)]);
    rig.step(vec![]);
    assert_eq!(rig.commits(), vec![6.0]);

    let mut rig = Rig::new(NumericValue::Mixed, NumericSpec::new(""));
    rig.focus();
    rig.typed("7");
    let away = Pos2::new(390.0, 290.0);
    rig.step(vec![Event::PointerMoved(away)]);
    rig.step(vec![press(away, true)]);
    rig.step(vec![press(away, false)]);
    rig.step(vec![]);
    assert_eq!(rig.commits(), vec![7.0]);

    let mut rig = Rig::new(NumericValue::Value(3.0), NumericSpec::new(""));
    rig.focus();
    rig.typed("8");
    rig.step(vec![key(Key::Escape)]);
    rig.step(vec![]);
    assert!(rig.commits().is_empty());
    assert_eq!(rig.events, vec![NumericEvent::Cancel]);
    assert_eq!(numeric_state(&rig.ctx, id()).buffer, None);
}

#[test]
fn numeric_field_unchanged_focus_loss_commits_nothing() {
    let mut rig = Rig::new(NumericValue::Value(12.0), NumericSpec::new(""));
    rig.focus();
    rig.step(vec![key(Key::Enter)]);
    rig.step(vec![]);
    assert!(rig.events.is_empty());
}

#[test]
fn numeric_field_scrub_previews_then_single_commit() {
    let mut rig = Rig::new(NumericValue::Value(10.0), NumericSpec::new("W").step(0.5));
    let label_at = rig.rect.left_center() + egui::vec2(3.0, 0.0);
    rig.step(vec![Event::PointerMoved(label_at)]);
    rig.step(vec![press(label_at, true)]);
    let mut at = label_at;
    for _ in 0..6 {
        at += egui::vec2(10.0, 0.0);
        rig.step(vec![Event::PointerMoved(at)]);
    }
    rig.step(vec![press(at, false)]);
    rig.step(vec![]);
    let previews: Vec<f64> = rig.events.iter().filter_map(|e| if let NumericEvent::Preview(v) = e { Some(*v) } else { None }).collect();
    assert!(!previews.is_empty());
    let commits = rig.commits();
    assert_eq!(commits.len(), 1);
    assert_eq!(Some(&commits[0]), previews.last());
    assert!(commits[0] > 10.0);
}

#[test]
fn numeric_field_clamps_to_range() {
    for (typed, expect) in [("500", 100.0), ("-3", 0.0), ("42", 42.0)] {
        let mut rig = Rig::new(NumericValue::Mixed, NumericSpec::new("").range(0.0..=100.0));
        rig.focus();
        rig.typed(typed);
        rig.step(vec![key(Key::Enter)]);
        assert_eq!(rig.commits(), vec![expect], "{typed}");
    }
    let spec = NumericSpec::new("").range(0.0..=100.0);
    assert_eq!(spec.clamp(250.0), 100.0);
    assert_eq!(spec.clamp(-1.0), 0.0);
}

#[test]
fn numeric_field_wheel_steps_focused_field() {
    let mut rig = Rig::new(NumericValue::Value(10.0), NumericSpec::new("").step(2.0));
    rig.focus();
    let wheel = Event::MouseWheel { unit: MouseWheelUnit::Line, delta: egui::vec2(0.0, 1.0), phase: TouchPhase::Move, source: MouseWheelSource::Wheel, modifiers: Modifiers::NONE };
    rig.step(vec![wheel]);
    assert_eq!(rig.commits(), vec![12.0]);
}

#[test]
fn format_number_trims_and_keeps_decimals() {
    assert_eq!(format_number(236.2204, 2), "236.22");
    assert_eq!(format_number(20.0, 2), "20");
    assert_eq!(format_number(-0.0001, 2), "0");
    assert_eq!(NumericSpec::new("").suffix("px").format(1.5), "1.5 px");
    assert_eq!(NumericSpec::new("").suffix("%").parse_text("50 %"), Some(50.0));
    assert_eq!(NumericSpec::new("").parse_text("45deg"), Some(45.0));
    assert_eq!(NumericSpec::new("").parse_text("inf"), None);
}
#[test]
fn numeric_field_unlabeled_drag_scrubs_like_drag_value() {
    let mut rig = Rig::new(NumericValue::Value(4.0), NumericSpec::new(""));
    let start = rig.rect.center();
    rig.step(vec![Event::PointerMoved(start)]);
    rig.step(vec![press(start, true)]);
    let mut at = start;
    for _ in 0..5 {
        at += egui::vec2(8.0, 0.0);
        rig.step(vec![Event::PointerMoved(at)]);
    }
    rig.step(vec![press(at, false)]);
    rig.step(vec![]);
    let commits = rig.commits();
    assert_eq!(commits.len(), 1);
    assert!(commits[0] > 4.0);
    assert!(rig.events.iter().any(|e| matches!(e, NumericEvent::Preview(_))));
    assert_eq!(numeric_state(&rig.ctx, id()).buffer, None);
}

#[test]
fn numeric_field_click_selects_all_so_typing_replaces() {
    let mut rig = Rig::new(NumericValue::Value(3.0), NumericSpec::new(""));
    rig.focus();
    rig.typed("8");
    rig.step(vec![key(Key::Enter)]);
    assert_eq!(rig.commits(), vec![8.0]);
}

#[test]
fn numeric_value_writes_back_and_marks_changed_only_on_real_change() {
    let ctx = Context::default();
    let spec = NumericSpec::new("");
    let mut value = 2.0;
    let response = frame(&ctx, vec![], |ui| numeric_value(ui, id(), &mut value, &spec));
    assert!(!response.changed());
    let at = response.rect.center();
    frame(&ctx, vec![Event::PointerMoved(at)], |ui| numeric_value(ui, id(), &mut value, &spec));
    frame(&ctx, vec![press(at, true)], |ui| numeric_value(ui, id(), &mut value, &spec));
    frame(&ctx, vec![press(at, false)], |ui| numeric_value(ui, id(), &mut value, &spec));
    frame(&ctx, vec![Event::Text("9".into())], |ui| numeric_value(ui, id(), &mut value, &spec));
    let response = frame(&ctx, vec![key(Key::Enter)], |ui| numeric_value(ui, id(), &mut value, &spec));
    assert!(response.changed());
    assert!(response.lost_focus());
    assert_eq!(value, 9.0);
}