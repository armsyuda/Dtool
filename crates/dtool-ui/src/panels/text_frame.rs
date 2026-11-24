use dtool_core::object::ObjectKind;
use dtool_core::Session;
use dtool_design::{dropdown_value, number_field, panel_frame, panel_header, space, toggle_with, ControlState};
use dtool_protocol::{Command, CommandError};
use dtool_strings::text;
use serde_json::{json, Map, Value};

pub const FRAME_SET_COMMAND: &str = "text.frame.set";
pub const BATCH_COMMAND: &str = "edit.batch";
pub const VALIGNS: [(&str, &str); 4] = [
    ("Top", "menu.text.vertical_alignment.top"),
    ("Center", "menu.text.vertical_alignment.centre"),
    ("Bottom", "menu.text.vertical_alignment.bottom"),
    ("Justify", "menu.text.vertical_alignment.justify"),
];
pub const INSET_KEYS: [&str; 4] = ["frame_panel.inset_left", "frame_panel.inset_top", "frame_panel.inset_right", "frame_panel.inset_bottom"];

#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Same(Value),
    Mixed,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FrameSelection {
    pub frames: Vec<(u64, Value)>,
}

pub fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64().map(|v| v as f32) == y.as_f64().map(|v| v as f32),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        _ => a == b,
    }
}

pub fn read_selection(session: &Session) -> Option<FrameSelection> {
    let frames: Vec<(u64, Value)> = session
        .selection
        .ids
        .iter()
        .filter_map(|id| {
            let object = session.document.get_object(*id)?;
            if !matches!(object.kind, ObjectKind::FrameText { .. }) {
                return None;
            }
            Some((id.0, serde_json::to_value(object.frame_props()?).ok()?))
        })
        .collect();
    (!frames.is_empty()).then_some(FrameSelection { frames })
}

fn equal_widths(frame: &Value, columns: u64) -> Value {
    let get = |key: &str| frame.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let inset = |i: usize| frame["insets"].get(i).and_then(Value::as_f64).unwrap_or(0.0);
    let width = frame["size"].get(0).and_then(Value::as_f64).unwrap_or(0.0) - inset(0) - inset(2);
    let n = columns.max(1) as f64;
    let each = ((width - get("gutter") * (n - 1.0)) / n).max(1.0);
    json!(vec![each; columns.max(1) as usize])
}

impl FrameSelection {
    fn all(&self, read: impl Fn(&Value) -> Value) -> Field {
        let mut values = self.frames.iter().map(|(_, frame)| read(frame));
        let Some(first) = values.next() else {
            return Field::Mixed;
        };
        if values.all(|value| same(&value, &first)) {
            Field::Same(first)
        } else {
            Field::Mixed
        }
    }

    pub fn field(&self, key: &str) -> Field {
        self.all(|frame| frame.get(key).cloned().unwrap_or(Value::Null))
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        match self.field(key) {
            Field::Same(value) => value.as_f64(),
            Field::Mixed => None,
        }
    }

    pub fn flag(&self, key: &str) -> Option<bool> {
        match self.field(key) {
            Field::Same(value) => value.as_bool(),
            Field::Mixed => None,
        }
    }

    pub fn inset(&self, index: usize) -> Option<f64> {
        match self.all(|frame| frame["insets"].get(index).cloned().unwrap_or(Value::Null)) {
            Field::Same(value) => value.as_f64(),
            Field::Mixed => None,
        }
    }

    pub fn even_columns(&self) -> Option<bool> {
        match self.all(|frame| json!(frame.get("column_widths").is_none_or(Value::is_null))) {
            Field::Same(value) => value.as_bool(),
            Field::Mixed => None,
        }
    }

    pub fn valign(&self) -> Option<String> {
        match self.field("valign") {
            Field::Same(Value::String(name)) => Some(name),
            _ => None,
        }
    }

    pub fn per_frame(&self, change: impl Fn(&Value) -> Option<Map<String, Value>>) -> Option<Command> {
        let mut commands: Vec<Command> = Vec::new();
        for (id, frame) in &self.frames {
            let Some(patch) = change(frame) else {
                continue;
            };
            let changed: Map<String, Value> = patch.into_iter().filter(|(key, value)| !same(frame.get(key).unwrap_or(&Value::Null), value)).collect();
            if changed.is_empty() {
                continue;
            }
            let mut params = changed;
            params.insert("id".to_string(), json!(id));
            commands.push(Command::with_params(FRAME_SET_COMMAND, Value::Object(params)));
        }
        match commands.len() {
            0 => None,
            1 => commands.pop(),
            _ => Some(Command::with_params(BATCH_COMMAND, json!({ "commands": commands }))),
        }
    }

    pub fn set(&self, key: &str, value: Value) -> Option<Command> {
        self.per_frame(|_| Some(Map::from_iter([(key.to_string(), value.clone())])))
    }

    pub fn set_gutter(&self, gutter: f64) -> Option<Command> {
        (gutter.is_finite() && gutter >= 0.0).then(|| self.set("gutter", json!(gutter))).flatten()
    }

    pub fn set_inset(&self, index: usize, value: f64) -> Option<Command> {
        if index > 3 || !value.is_finite() || value < 0.0 {
            return None;
        }
        self.per_frame(|frame| {
            let mut insets = frame.get("insets").cloned().unwrap_or_else(|| json!([0.0, 0.0, 0.0, 0.0]));
            insets[index] = json!(value);
            Some(Map::from_iter([("insets".to_string(), insets)]))
        })
    }

    pub fn set_columns(&self, columns: f64) -> Option<Command> {
        if !columns.is_finite() || columns.round() < 1.0 {
            return None;
        }
        let n = columns.round() as u64;
        self.per_frame(|frame| {
            let mut patch = Map::from_iter([("columns".to_string(), json!(n))]);
            if frame.get("column_widths").is_some_and(|widths| !widths.is_null()) {
                patch.insert("column_widths".to_string(), equal_widths(frame, n));
            }
            Some(patch)
        })
    }

    pub fn set_even_columns(&self, even: bool) -> Option<Command> {
        self.per_frame(|frame| {
            let is_even = frame.get("column_widths").is_none_or(Value::is_null);
            if is_even == even {
                return None;
            }
            let widths = if even { Value::Null } else { equal_widths(frame, frame.get("columns").and_then(Value::as_u64).unwrap_or(1)) };
            Some(Map::from_iter([("column_widths".to_string(), widths)]))
        })
    }
}

pub fn apply(session: &mut Session, command: &Command) -> Result<Value, CommandError> {
    session.run(command)
}

#[derive(Default)]
pub struct TextFramePanel {
    pub selection: Option<FrameSelection>,
    pub error: Option<String>,
}

fn mixed_label(ui: &mut egui::Ui, mixed: bool) {
    if mixed {
        ui.label(text("frame_panel.mixed"));
    }
}

fn number_row(ui: &mut egui::Ui, key: &str, value: Option<f64>, fallback: f64) -> Option<f64> {
    let mut edited = value.unwrap_or(fallback);
    let changed = number_field(ui, &mut edited, text(key)).changed();
    mixed_label(ui, value.is_none());
    changed.then_some(edited)
}

fn toggle_row(ui: &mut egui::Ui, key: &str, value: Option<bool>) -> Option<bool> {
    let mut on = value.unwrap_or(false);
    let state = ControlState::enabled(true).mixed(value.is_none());
    toggle_with(ui, &mut on, text(key), &state).changed().then_some(on)
}

impl TextFramePanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut dtool_core::Workspace) {
        panel_frame(ui, |ui| {
            panel_header(ui, text("frame_panel.title"));
            let Some(session) = workspace.session_mut() else {
                ui.label(text("frame_panel.no_frame"));
                return;
            };
            self.selection = read_selection(session);
            let Some(selection) = self.selection.clone() else {
                ui.label(text("frame_panel.no_frame"));
                return;
            };
            if let Some(command) = self.edits(ui, &selection) {
                self.error = apply(session, &command).err().map(|error| text(&error.message_key).to_string());
            }
            if let Some(message) = &self.error {
                ui.label(message.as_str());
            }
        });
    }

    pub fn edits(&mut self, ui: &mut egui::Ui, selection: &FrameSelection) -> Option<Command> {
        let mut command = None;
        if let Some(columns) = number_row(ui, "frame_panel.columns", selection.number("columns"), 1.0) {
            command = selection.set_columns(columns);
        }
        if let Some(gutter) = number_row(ui, "frame_panel.gutter", selection.number("gutter"), 0.0) {
            command = selection.set_gutter(gutter);
        }
        if let Some(even) = toggle_row(ui, "frame_panel.even_columns", selection.even_columns()) {
            command = selection.set_even_columns(even);
        }
        ui.add_space(space("sm"));
        panel_header(ui, text("frame_panel.insets"));
        for (index, key) in INSET_KEYS.iter().enumerate() {
            if let Some(value) = number_row(ui, key, selection.inset(index), 0.0) {
                command = selection.set_inset(index, value);
            }
        }
        ui.add_space(space("sm"));
        let current = selection.valign();
        let mut picked: &str = current.as_deref().and_then(|name| VALIGNS.iter().find(|(v, _)| *v == name).map(|(v, _)| *v)).unwrap_or("Top");
        let options: Vec<(&str, &str)> = VALIGNS.iter().map(|(value, key)| (*value, text(key))).collect();
        ui.label(text("menu.text.vertical_alignment"));
        let state = ControlState::enabled(true).mixed(current.is_none());
        if dropdown_value(ui, "frame_panel.valign", &mut picked, &options, &state).changed() {
            command = selection.set("valign", json!(picked));
        }
        if let Some(auto) = toggle_row(ui, "frame_panel.auto_height", selection.flag("auto_height")) {
            command = selection.set("auto_height", json!(auto));
        }
        if let Some(ignore) = toggle_row(ui, "table.frame.ignore_grid", selection.flag("ignore_baseline_grid")) {
            command = selection.set("ignore_baseline_grid", json!(ignore));
        }
        command
    }
}
