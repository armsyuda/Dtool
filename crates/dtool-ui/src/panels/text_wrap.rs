use dtool_core::text_wrap::{TextWrap, WrapMode, WrapSides, CLEAR, IGNORE, QUERY, SET};
use dtool_core::workspace::{Scope, Workspace};
use dtool_design::{ControlState, NumericEvent, NumericSpec, NumericValue};
use dtool_protocol::Command;
use serde_json::{json, Value};

pub const TEXT_WRAP_PANEL_KEY: &str = "text_wrap";
pub const KEY_TITLE: &str = "panel.text_wrap";
pub const KEY_MODE: &str = "text_wrap.mode";
pub const KEY_SIDES: &str = "text_wrap.sides";
pub const KEY_DISTANCE: &str = "text_wrap.distance";
pub const KEY_IGNORE: &str = "menu.text.text_wrap.ignore_text_wraps";
pub const KEY_CLEAR: &str = "text_wrap.clear";
pub const KEY_NO_SELECTION: &str = "text_wrap.no_selection";
pub const BATCH: &str = "edit.batch";
pub const MODES: [WrapMode; 5] = [WrapMode::None, WrapMode::Square, WrapMode::Tight, WrapMode::JumpObject, WrapMode::Through];
pub const SIDES: [WrapSides; 5] = [WrapSides::Both, WrapSides::Left, WrapSides::Right, WrapSides::Largest, WrapSides::Smallest];
pub const DISTANCE_SIDES: [&str; 4] = ["top", "right", "bottom", "left"];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WrapState {
    pub id: u64,
    pub wrap: Option<TextWrap>,
    pub ignore: bool,
}

impl WrapState {
    pub fn effective(&self) -> TextWrap {
        self.wrap.unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WrapView {
    pub ids: Vec<u64>,
    pub mode: Option<WrapMode>,
    pub sides: Option<WrapSides>,
    pub distance: [Option<f32>; 4],
    pub ignore: Option<bool>,
    pub any_wrap: bool,
}

fn common<T: PartialEq + Copy>(values: impl Iterator<Item = T>) -> Option<T> {
    let mut values = values;
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

impl WrapView {
    pub fn from_states(states: &[WrapState]) -> Option<WrapView> {
        if states.is_empty() {
            return None;
        }
        let wraps: Vec<TextWrap> = states.iter().map(WrapState::effective).collect();
        let mut distance = [None; 4];
        for (index, slot) in distance.iter_mut().enumerate() {
            *slot = common(wraps.iter().map(|wrap| wrap.distance[index]));
        }
        Some(WrapView {
            ids: states.iter().map(|state| state.id).collect(),
            mode: common(wraps.iter().map(|wrap| wrap.mode)),
            sides: common(wraps.iter().map(|wrap| wrap.sides)),
            distance,
            ignore: common(states.iter().map(|state| state.ignore)),
            any_wrap: states.iter().any(|state| state.wrap.is_some()),
        })
    }
}

#[derive(Default)]
pub struct TextWrapPanel {
    pub last_error: Option<String>,
}

pub fn label(key: &str) -> Option<String> {
    dtool_strings::has(key).then(|| dtool_strings::text(key).to_string())
}

fn enum_name<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

pub fn option_label<T: serde::Serialize>(prefix: &str, value: &T) -> String {
    let name = enum_name(value);
    label(&format!("{prefix}.{name}")).unwrap_or(name)
}

pub fn parse_state(value: &Value) -> Option<WrapState> {
    Some(WrapState {
        id: value.get("id")?.as_u64()?,
        wrap: value.get("wrap").filter(|wrap| !wrap.is_null()).and_then(|wrap| serde_json::from_value(wrap.clone()).ok()),
        ignore: value.get("ignore").and_then(Value::as_bool).unwrap_or(false),
    })
}

pub fn targets(workspace: &mut Workspace) -> Vec<u64> {
    workspace.session_mut().map(|session| session.selection.ids.iter().map(|id| id.0).collect()).unwrap_or_default()
}

pub fn grouped(name: &str, ids: &[u64], params: &Value) -> Option<Command> {
    let per_id = |id: &u64| {
        let mut next = params.clone();
        next["id"] = json!(id);
        next
    };
    match ids {
        [] => None,
        [id] => Some(Command::with_params(name, per_id(id))),
        _ => Some(Command::with_params(BATCH, json!({ "commands": ids.iter().map(|id| json!({ "name": name, "params": per_id(id) })).collect::<Vec<_>>() }))),
    }
}

impl TextWrapPanel {
    fn run(&mut self, workspace: &mut Workspace, command: Command) -> bool {
        let (_, result) = workspace.run(Scope::active(), command);
        match result {
            Ok(_) => {
                self.last_error = None;
                true
            }
            Err(error) => {
                self.last_error = Some(error.message_key);
                false
            }
        }
    }

    pub fn states(&mut self, workspace: &mut Workspace) -> Vec<WrapState> {
        targets(workspace)
            .into_iter()
            .filter_map(|id| workspace.run(Scope::active(), Command::with_params(QUERY, json!({ "id": id }))).1.ok().as_ref().and_then(parse_state))
            .collect()
    }

    pub fn state(&mut self, workspace: &mut Workspace) -> Option<WrapState> {
        self.states(workspace).into_iter().next()
    }

    pub fn view(&mut self, workspace: &mut Workspace) -> Option<WrapView> {
        WrapView::from_states(&self.states(workspace))
    }

    fn edit_all(&mut self, workspace: &mut Workspace, name: &str, params: Value) -> bool {
        let ids = targets(workspace);
        match grouped(name, &ids, &params) {
            Some(command) => self.run(workspace, command),
            None => false,
        }
    }

    pub fn set_mode(&mut self, workspace: &mut Workspace, mode: WrapMode) -> bool {
        self.edit_all(workspace, SET, json!({ "mode": mode }))
    }

    pub fn set_sides(&mut self, workspace: &mut Workspace, sides: WrapSides) -> bool {
        self.edit_all(workspace, SET, json!({ "sides": sides }))
    }

    pub fn set_distance(&mut self, workspace: &mut Workspace, side: usize, value: f32) -> bool {
        let Some(key) = DISTANCE_SIDES.get(side) else { return false };
        let mut distance = serde_json::Map::new();
        distance.insert(key.to_string(), json!(value));
        self.edit_all(workspace, SET, json!({ "distance": distance }))
    }

    pub fn set_ignore(&mut self, workspace: &mut Workspace, on: bool) -> bool {
        self.edit_all(workspace, IGNORE, json!({ "on": on }))
    }

    pub fn clear(&mut self, workspace: &mut Workspace) -> bool {
        self.edit_all(workspace, CLEAR, json!({}))
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        let view = self.view(workspace);
        let mut mode_change = None;
        let mut sides_change = None;
        let mut distance_change = None;
        let mut ignore_change = None;
        let mut clear = false;
        dtool_design::panel_frame(ui, |ui| {
            if let Some(title) = label(KEY_TITLE) {
                dtool_design::panel_header(ui, title);
            }
            let Some(view) = view else {
                if let Some(text) = label(KEY_NO_SELECTION) {
                    ui.label(text);
                }
                return;
            };
            let mode_labels: Vec<(WrapMode, String)> = MODES.iter().map(|mode| (*mode, option_label(KEY_MODE, mode))).collect();
            let mode_options: Vec<(WrapMode, &str)> = mode_labels.iter().map(|(mode, text)| (*mode, text.as_str())).collect();
            let shown_mode = view.mode.unwrap_or(WrapMode::None);
            let mut mode = shown_mode;
            let mut mode_picked = false;
            ui.horizontal(|ui| {
                if let Some(text) = label(KEY_MODE) {
                    ui.label(text);
                }
                mode_picked = dtool_design::dropdown_value(ui, "text_wrap.mode", &mut mode, &mode_options, &ControlState::enabled(true).mixed(view.mode.is_none())).changed();
            });
            if mode_picked {
                mode_change = Some(mode);
            }
            let sides_labels: Vec<(WrapSides, String)> = SIDES.iter().map(|sides| (*sides, option_label(KEY_SIDES, sides))).collect();
            let sides_options: Vec<(WrapSides, &str)> = sides_labels.iter().map(|(sides, text)| (*sides, text.as_str())).collect();
            let shown_sides = view.sides.unwrap_or(WrapSides::Both);
            let mut sides = shown_sides;
            let mut sides_picked = false;
            ui.horizontal(|ui| {
                if let Some(text) = label(KEY_SIDES) {
                    ui.label(text);
                }
                sides_picked = dtool_design::dropdown_value(ui, "text_wrap.sides", &mut sides, &sides_options, &ControlState::enabled(true).mixed(view.sides.is_none())).changed();
            });
            if sides_picked {
                sides_change = Some(sides);
            }
            if let Some(text) = label(KEY_DISTANCE) {
                ui.label(text);
            }
            ui.horizontal(|ui| {
                for (index, side) in DISTANCE_SIDES.iter().enumerate() {
                    let side_label = label(&format!("{KEY_DISTANCE}.{side}")).unwrap_or_else(|| side.to_string());
                    let value = match view.distance[index] {
                        Some(value) => NumericValue::Value(f64::from(value)),
                        None => NumericValue::Mixed,
                    };
                    let spec = NumericSpec::new(&side_label).range(0.0..=f64::from(dtool_core::text_wrap::MAX_DISTANCE));
                    let id = ui.make_persistent_id(("text_wrap.distance", index));
                    let (_, event) = dtool_design::numeric_field(ui, id, value, &spec);
                    if let NumericEvent::Commit(next) = event {
                        distance_change = Some((index, next as f32));
                    }
                }
            });
            let mut ignore = view.ignore.unwrap_or(false);
            let ignore_label = label(KEY_IGNORE).unwrap_or_default();
            if dtool_design::toggle_with(ui, &mut ignore, ignore_label, &ControlState::enabled(true).mixed(view.ignore.is_none())).clicked() {
                ignore_change = Some(ignore);
            }
            if view.any_wrap {
                if let Some(text) = label(KEY_CLEAR) {
                    clear = dtool_design::button(ui, text).clicked();
                }
            }
            if let Some(key) = self.last_error.as_deref().and_then(label) {
                ui.colored_label(dtool_design::color("error.red"), key);
            }
        });
        if let Some(mode) = mode_change {
            self.set_mode(workspace, mode);
        }
        if let Some(sides) = sides_change {
            self.set_sides(workspace, sides);
        }
        if let Some((side, value)) = distance_change {
            self.set_distance(workspace, side, value);
        }
        if let Some(on) = ignore_change {
            self.set_ignore(workspace, on);
        }
        if clear {
            self.clear(workspace);
        }
    }
}