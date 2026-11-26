use std::ops::RangeInclusive;

use egui::{Event, Id, Key, Response, Sense, Ui, Vec2};

use crate::tokens::{color, size, stroke_width};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumericValue {
    Value(f64),
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumericEvent {
    None,
    Preview(f64),
    Commit(f64),
    Cancel,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NumericState {
    pub buffer: Option<String>,
    pub edited: bool,
    pub invalid: bool,
    pub scrubbing: bool,
    pub start: Option<f64>,
    pub scrub: f64,
    pub previewed: bool,
    pub select_pending: bool,
    pub edit_request: bool,
    pub scrub_press: Option<egui::Pos2>,
}

pub struct NumericSpec<'a> {
    pub label: &'a str,
    pub range: RangeInclusive<f64>,
    pub step: f64,
    pub decimals: usize,
    pub suffix: Option<&'a str>,
    pub mixed_hint: &'a str,
    mixed_origin: Option<f64>,
    pub width: Option<f32>,
    pub label_width: Option<f32>,
    pub enabled: bool,
    pub parse: Option<&'a dyn Fn(&str) -> Option<f64>>,
    pub to_display: Option<&'a dyn Fn(f64) -> f64>,
}

impl<'a> NumericSpec<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            range: f64::NEG_INFINITY..=f64::INFINITY,
            step: 1.0,
            decimals: 2,
            suffix: None,
            mixed_hint: super::MIXED_TEXT,
            mixed_origin: None,
            width: None,
            label_width: None,
            enabled: true,
            parse: None,
            to_display: None,
        }
    }

    pub fn range(mut self, range: RangeInclusive<f64>) -> Self {
        self.range = range;
        self
    }

    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }

    pub fn suffix(mut self, suffix: &'a str) -> Self {
        self.suffix = Some(suffix);
        self
    }

    pub fn mixed_hint(mut self, hint: &'a str) -> Self {
        self.mixed_hint = hint;
        self
    }

    pub fn mixed_origin(mut self, value: f64) -> Self {
        self.mixed_origin = Some(value);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn label_width(mut self, width: f32) -> Self {
        self.label_width = Some(width);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn parser(mut self, parse: &'a dyn Fn(&str) -> Option<f64>) -> Self {
        self.parse = Some(parse);
        self
    }

    pub fn display(mut self, to_display: &'a dyn Fn(f64) -> f64) -> Self {
        self.to_display = Some(to_display);
        self
    }

    pub fn clamp(&self, value: f64) -> f64 {
        let (low, high) = (*self.range.start(), *self.range.end());
        if low <= high { value.max(low).min(high) } else { value }
    }

    pub fn format(&self, value: f64) -> String {
        let shown = self.to_display.map_or(value, |convert| convert(value));
        let text = format_number(shown, self.decimals);
        match self.suffix {
            Some(suffix) if !suffix.is_empty() => format!("{text} {suffix}"),
            _ => text,
        }
    }

    pub fn parse_text(&self, text: &str) -> Option<f64> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        let value = match self.parse {
            Some(parse) => parse(trimmed),
            None => default_parse(trimmed, self.suffix),
        }?;
        if value.is_finite() { Some(self.clamp(value)) } else { None }
    }
}

pub fn format_number(value: f64, decimals: usize) -> String {
    let mut text = format!("{:.*}", decimals, value);
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        text = "0".to_string();
    }
    text
}

fn default_parse(text: &str, suffix: Option<&str>) -> Option<f64> {
    let mut body = text.trim();
    if let Some(suffix) = suffix.filter(|suffix| !suffix.is_empty()) {
        body = body.strip_suffix(suffix).unwrap_or(body).trim_end();
    }
    for tail in ["%", "\u{b0}", "deg"] {
        if let Some(rest) = body.strip_suffix(tail) {
            body = rest.trim_end();
            break;
        }
    }
    body.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn default_width() -> f32 {
    let width = size("field.number.width");
    if width > 0.0 { width } else { (size("input") * 3.0).max(48.0) }
}

fn wheel_notches(events: &[Event]) -> f64 {
    events
        .iter()
        .map(|event| match event {
            Event::MouseWheel { delta, .. } if delta.y > 0.0 => 1.0,
            Event::MouseWheel { delta, .. } if delta.y < 0.0 => -1.0,
            _ => 0.0,
        })
        .sum()
}

pub fn numeric_area_id(id: Id) -> Id {
    id.with("area")
}

pub fn numeric_request_edit(ctx: &egui::Context, id: Id) {
    ctx.data_mut(|data| {
        let mut state = data.get_temp::<NumericState>(id).unwrap_or_default();
        state.edit_request = true;
        data.insert_temp(id, state);
    });
}

pub fn numeric_cancel_edit(ctx: &egui::Context, id: Id) {
    ctx.data_mut(|data| data.remove::<NumericState>(id));
    ctx.memory_mut(|memory| {
        memory.surrender_focus(id.with("text"));
        memory.surrender_focus(numeric_area_id(id));
    });
}

pub fn numeric_state(ctx: &egui::Context, id: Id) -> NumericState {
    ctx.data_mut(|data| data.get_temp::<NumericState>(id)).unwrap_or_default()
}

fn start_scrub(state: &mut NumericState, start: f64) {
    state.scrubbing = true;
    state.start = Some(start);
    state.scrub = start;
    state.previewed = false;
}

fn drive_scrub(state: &mut NumericState, handle: &Response, spec: &NumericSpec, factor: f64, escape: bool, event: &mut NumericEvent) {
    if !state.scrubbing {
        return;
    }
    if escape {
        state.scrubbing = false;
        state.start = None;
        state.previewed = false;
        *event = NumericEvent::Cancel;
        return;
    }
    if handle.dragged() {
        let dx = f64::from(handle.drag_delta().x);
        if dx != 0.0 {
            state.scrub = spec.clamp(state.scrub + dx * spec.step * factor);
            state.previewed = true;
            *event = NumericEvent::Preview(state.scrub);
        }
    }
    if handle.drag_stopped() {
        if state.previewed {
            *event = NumericEvent::Commit(state.scrub);
        }
        state.scrubbing = false;
        state.start = None;
        state.previewed = false;
    }
}

fn released_scrub_delta(ui: &Ui, area: &Response, label: Option<&Response>, scrub_press: &mut Option<egui::Pos2>) -> Option<f64> {
    // egui can clear a pending drag when the first movement arrives with the release.
    if ui.ctx().current_pass_index() != 0 {
        return None;
    }
    let (press, release, primary_down) = ui.input(|input| {
        let mut press = None;
        let mut release = None;
        for event in &input.events {
            if let Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } = event {
                if *pressed {
                    press = Some(*pos);
                    release = None;
                } else {
                    release = Some(*pos);
                }
            }
        }
        (press, release, input.pointer.primary_down())
    });
    let transform = ui.ctx().layer_transform_from_global(area.layer_id)
        .unwrap_or(egui::emath::TSTransform::IDENTITY);
    if let Some(pos) = press {
        let local_pos = transform * pos;
        let hit = |response: &Response| response.enabled() && response.interact_rect.contains(local_pos);
        *scrub_press = (ui.ctx().layer_id_at(pos) == Some(area.layer_id) && (hit(area) || label.is_some_and(hit)))
            .then_some(pos);
    }
    let end = match release {
        Some(end) => end,
        None => {
            if !primary_down {
                *scrub_press = None;
            }
            return None;
        }
    };
    let start = scrub_press.take()?;
    let dx = end.x - start.x;
    let click_distance = ui.ctx().options(|options| options.input_options.max_click_dist);
    (dx.abs() > click_distance && ui.ctx().layer_id_at(start) == Some(area.layer_id))
        .then_some(f64::from(dx * transform.scaling))
}

fn select_all(ctx: &egui::Context, text_id: Id, text: &str) {
    let mut edit_state = egui::text_edit::TextEditState::load(ctx, text_id).unwrap_or_default();
    let end = text.chars().count();
    edit_state.cursor.set_char_range(Some(egui::text_selection::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(end))));
    edit_state.store(ctx, text_id);
}

fn paint_value(ui: &Ui, rect: egui::Rect, response: &Response, text: &str, placeholder: bool) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let visuals = ui.style().interact(response);
    let fill = if response.enabled() { color("input") } else { color("input.disabled") };
    let stroke = if response.has_focus() { egui::Stroke::new(stroke_width("thin").max(1.0), ui.visuals().selection.bg_fill) } else { visuals.bg_stroke };
    ui.painter().rect(rect, visuals.corner_radius, fill, stroke, egui::StrokeKind::Inside);
    let text_color = if placeholder { ui.visuals().weak_text_color() } else { ui.visuals().text_color() };
    let at = rect.left_center() + Vec2::new(ui.spacing().button_padding.x, 0.0);
    ui.painter().text(at, egui::Align2::LEFT_CENTER, text, egui::TextStyle::Body.resolve(ui.style()), text_color);
}

pub fn numeric_field(ui: &mut Ui, id: Id, value: NumericValue, spec: &NumericSpec) -> (Response, NumericEvent) {
    let mut state = numeric_state(ui.ctx(), id);
    let mut event = NumericEvent::None;
    let current = match value {
        NumericValue::Value(v) if v.is_finite() => Some(spec.clamp(v)),
        _ => None,
    };
    let scrub_origin = current.or(spec.mixed_origin).map_or_else(|| spec.clamp(0.0), |value| spec.clamp(value));
    let (shift, ctrl, escape, wheel) = ui.input(|input| {
        (input.modifiers.shift, input.modifiers.ctrl || input.modifiers.command, input.key_pressed(Key::Escape), wheel_notches(&input.events))
    });
    let factor = if shift { 0.1 } else if ctrl { 10.0 } else { 1.0 };
    let hint = if current.is_none() { spec.mixed_hint } else { "" };
    let text_id = id.with("text");
    let was_editing = state.buffer.is_some() || ui.memory(|memory| memory.has_focus(text_id));
    let select_now = was_editing && state.select_pending;
    let requested = std::mem::take(&mut state.edit_request);

    let (label, area, field, buffer, started) = ui
        .horizontal(|ui| {
            let label = if spec.label.is_empty() {
                None
            } else {
                let label = egui::Label::new(spec.label).sense(Sense::click_and_drag());
                let response = if let Some(width) = spec.label_width {
                    ui.add_enabled_ui(spec.enabled, |ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ui.spacing().interact_size.y), Sense::hover());
                        ui.put(rect, label.truncate()).on_hover_text(spec.label)
                    }).inner
                } else {
                    ui.add_enabled(spec.enabled, label)
                };
                Some(response)
            };
            let size = Vec2::new(spec.width.unwrap_or_else(default_width), ui.spacing().interact_size.y);
            let sense = if spec.enabled && !was_editing { Sense::click_and_drag() } else { Sense::hover() };
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            let area = ui.interact(rect, numeric_area_id(id), sense);
            let area = area.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            let started = !was_editing && spec.enabled && (area.clicked() || area.gained_focus() || requested);
            if was_editing || started {
                let mut buffer = state.buffer.clone()
                    .unwrap_or_else(|| current.map(|value| spec.format(value)).unwrap_or_default());
                if select_now {
                    select_all(ui.ctx(), text_id, &buffer);
                }
                let edit = egui::TextEdit::singleline(&mut buffer).id(text_id).desired_width(size.x).hint_text(hint);
                let field = ui.put(rect, edit);
                if started {
                    field.request_focus();
                }
                (label, area, Some(field), buffer, started)
            } else {
                (label, area, None, String::new(), false)
            }
        })
        .inner;

    if started {
        state.buffer = Some(buffer.clone());
        state.edited = false;
        state.invalid = false;
        state.select_pending = true;
    } else if select_now {
        state.select_pending = false;
    }

    if let Some(field) = &field {
        if field.changed() {
            state.buffer = Some(buffer.clone());
            state.edited = true;
            state.invalid = false;
        }
        if escape && (field.lost_focus() || field.has_focus() || state.buffer.is_some()) && !started {
            if field.has_focus() {
                field.surrender_focus();
            }
            state = NumericState::default();
            event = NumericEvent::Cancel;
        } else if field.lost_focus() {
            if state.edited {
                match spec.parse_text(&buffer) {
                    Some(v) => {
                        event = NumericEvent::Commit(v);
                        state.buffer = None;
                        state.edited = false;
                        state.invalid = false;
                    }
                    None if buffer.trim().is_empty() => {
                        state.buffer = None;
                        state.edited = false;
                        state.invalid = false;
                    }
                    None => {
                        state.invalid = true;
                        field.request_focus();
                    }
                }
            } else {
                state.buffer = None;
                state.invalid = false;
            }
        } else if !started && !field.has_focus() && !state.invalid && !state.edited {
            state.buffer = None;
        }

        if wheel != 0.0 && spec.enabled && field.has_focus() && field.hovered() {
            let base = state.buffer.as_deref().and_then(|text| spec.parse_text(text)).unwrap_or(scrub_origin);
            let next = spec.clamp(base + wheel * spec.step * factor);
            event = NumericEvent::Commit(next);
            state.buffer = Some(spec.format(next));
            select_all(ui.ctx(), text_id, &spec.format(next));
            state.edited = false;
            state.invalid = false;
        }

        if state.invalid {
            ui.painter().rect_stroke(
                field.rect,
                0.0,
                egui::Stroke::new(stroke_width("md").max(1.0), color("error")),
                egui::StrokeKind::Outside,
            );
        }
    } else if spec.enabled {
        if area.drag_started() {
            start_scrub(&mut state, scrub_origin);
        }
        drive_scrub(&mut state, &area, spec, factor, escape, &mut event);
    }

    if let Some(label) = &label {
        if spec.enabled && label.drag_started() {
            start_scrub(&mut state, scrub_origin);
        }
        drive_scrub(&mut state, label, spec, factor, escape, &mut event);
    }

    if spec.enabled && field.is_none() && !escape {
        let released = released_scrub_delta(ui, &area, label.as_ref(), &mut state.scrub_press);
        if let Some(dx) = released.filter(|_| !state.scrubbing && event == NumericEvent::None) {
            event = NumericEvent::Commit(spec.clamp(scrub_origin + dx * spec.step * factor));
        }
    } else {
        state.scrub_press = None;
    }

    if field.is_none() {
        let display = match event {
            NumericEvent::Preview(value) | NumericEvent::Commit(value) => Some(value),
            _ if state.scrubbing => Some(state.scrub),
            _ => current,
        };
        let text = display.map_or_else(|| hint.to_owned(), |value| spec.format(value));
        paint_value(ui, area.rect, &area, &text, display.is_none());
    }

    ui.data_mut(|data| {
        if state == NumericState::default() {
            data.remove::<NumericState>(id);
        } else {
            data.insert_temp(id, state);
        }
    });

    let main = match field {
        Some(field) => field.union(area),
        None => area,
    };
    let mut response = match label {
        Some(label) => main.union(label),
        None => main,
    };
    let differs = |v: f64| current.is_none_or(|c| c != v);
    if let NumericEvent::Preview(v) | NumericEvent::Commit(v) = event {
        if differs(v) {
            response.mark_changed();
        }
    }
    (response, event)
}

pub fn numeric_value(ui: &mut Ui, id: Id, value: &mut f64, spec: &NumericSpec) -> Response {
    let (response, event) = numeric_field(ui, id, NumericValue::Value(*value), spec);
    if let NumericEvent::Preview(v) | NumericEvent::Commit(v) = event {
        if v.is_finite() {
            *value = v;
        }
    }
    response
}

pub fn number_drag(ui: &mut Ui, id: Id, value: &mut f64, range: RangeInclusive<f64>, speed: f64) -> Response {
    number_drag_with(ui, id, value, range, speed, &super::ControlState::default())
}

pub fn number_drag_with(ui: &mut Ui, id: Id, value: &mut f64, range: RangeInclusive<f64>, speed: f64, state: &super::ControlState) -> Response {
    let speed = if speed.is_finite() && speed > 0.0 { speed } else { 1.0 };
    let decimals = (-speed.log10().floor()).clamp(0.0, 6.0) as usize;
    let spec = NumericSpec::new("").range(range).step(speed).decimals(decimals).enabled(state.enabled);
    let response = numeric_value(ui, id, value, &spec);
    state.decorate(response)
}
