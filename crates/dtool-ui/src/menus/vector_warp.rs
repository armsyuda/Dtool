use dtool_core::commands::warp::{
    can_execute, default_value, parse_preset, preset_name, GROUP_CREATE, GROUP_QUERY, GROUP_UPDATE, INVALID_VALUE, MESH_QUERY,
    VALUE_LIMIT,
};
use dtool_core::{Session, WarpPreset};
use dtool_protocol::{Command, CommandError, CommandResult};
use dtool_strings::text;
use serde_json::{json, Map, Value};

pub const NEW_WARP_GROUP_ID: &str = "vector.new_warp_group";
pub const NEW_WARP_GROUP_KEY: &str = "menu.vector.new_warp_group";
pub const PRESET_MESH_KEY: &str = "warp.preset.mesh";
pub const PRESET_ARCH_KEY: &str = "warp.preset.arch";

pub const NEW_WARP_GROUP_ITEMS: [&str; 15] = [
    "mesh",
    "quad",
    "perspective",
    "arc_vertical",
    "arc_horizontal",
    "bend_vertical",
    "bend_horizontal",
    "fish_eye",
    "twist",
    "arch",
    "upper_arc",
    "lower_arc",
    "bulge",
    "flag",
    "rise",
];

pub const PRESET_CHOICES: [(WarpPreset, &str); 15] = [
    (WarpPreset::Mesh, PRESET_MESH_KEY),
    (WarpPreset::Quad, "warp.preset.quad"),
    (WarpPreset::Perspective, "warp.preset.perspective"),
    (WarpPreset::ArcVertical, "warp.preset.arc_vertical"),
    (WarpPreset::ArcHorizontal, "warp.preset.arc_horizontal"),
    (WarpPreset::BendVertical, "warp.preset.bend_vertical"),
    (WarpPreset::BendHorizontal, "warp.preset.bend_horizontal"),
    (WarpPreset::FishEye, "warp.preset.fish_eye"),
    (WarpPreset::Twist, "warp.preset.twist"),
    (WarpPreset::Arch, PRESET_ARCH_KEY),
    (WarpPreset::UpperArc, "warp.preset.upper_arc"),
    (WarpPreset::LowerArc, "warp.preset.lower_arc"),
    (WarpPreset::Bulge, "warp.preset.bulge"),
    (WarpPreset::Flag, "warp.preset.flag"),
    (WarpPreset::Rise, "warp.preset.rise"),
];

pub fn item_id(item: &str) -> String {
    format!("{NEW_WARP_GROUP_ID}.{item}")
}

pub fn item_key(item: &str) -> String {
    format!("menu.{}", item_id(item))
}

pub fn item_preset(item: &str) -> Option<WarpPreset> {
    parse_preset(item)
}

pub fn preset_key(preset: &WarpPreset) -> String {
    format!("warp.preset.{}", preset_name(preset))
}

fn checked_value(value: Option<f64>) -> Result<Option<f64>, CommandError> {
    match value {
        None => Ok(None),
        Some(v) if v.is_finite() && v.abs() <= VALUE_LIMIT => Ok(Some(v)),
        Some(_) => Err(CommandError::new(INVALID_VALUE)),
    }
}

fn selected_nodes(session: &Session) -> Vec<u64> {
    session.selection.ids.iter().map(|id| id.0).collect()
}

pub fn create_command(session: &Session, preset: &WarpPreset, value: Option<f64>) -> Result<Command, CommandError> {
    let value = checked_value(value)?.unwrap_or_else(|| default_value(preset));
    Ok(Command::with_params(
        GROUP_CREATE,
        json!({
            "nodes": selected_nodes(session),
            "preset": preset_name(preset),
            "value": value,
        }),
    ))
}

pub fn new_warp_group_command(session: &Session) -> Command {
    Command::with_params(
        GROUP_CREATE,
        json!({
            "nodes": selected_nodes(session),
            "preset": preset_name(&WarpPreset::Mesh),
            "value": default_value(&WarpPreset::Mesh),
        }),
    )
}

pub fn menu_command(session: &Session, id: &str) -> Option<Command> {
    if id == NEW_WARP_GROUP_ID {
        return Some(new_warp_group_command(session));
    }
    let item = id.strip_prefix(NEW_WARP_GROUP_ID)?.strip_prefix('.')?;
    if !NEW_WARP_GROUP_ITEMS.contains(&item) {
        return None;
    }
    let preset = item_preset(item)?;
    create_command(session, &preset, None).ok()
}

pub fn preset_choice_command(preset: &str, value: Option<f64>, group: Option<u64>) -> Result<Command, CommandError> {
    let mut params = Map::new();
    params.insert("preset".to_string(), Value::String(preset.to_string()));
    if let Some(v) = checked_value(value)? {
        params.insert("value".to_string(), json!(v));
    }
    if let Some(id) = group {
        params.insert("group".to_string(), json!(id));
    }
    Ok(Command::with_params(GROUP_UPDATE, Value::Object(params)))
}

pub fn value_command(value: f64, group: Option<u64>) -> Result<Command, CommandError> {
    let v = checked_value(Some(value))?.ok_or_else(|| CommandError::new(INVALID_VALUE))?;
    let mut params = Map::new();
    params.insert("value".to_string(), json!(v));
    if let Some(id) = group {
        params.insert("group".to_string(), json!(id));
    }
    Ok(Command::with_params(GROUP_UPDATE, Value::Object(params)))
}

pub fn reset_command(group: Option<u64>) -> Command {
    let mut params = Map::new();
    params.insert("reset".to_string(), json!(true));
    if let Some(id) = group {
        params.insert("group".to_string(), json!(id));
    }
    Command::with_params(GROUP_UPDATE, Value::Object(params))
}

pub fn group_query_command() -> Command {
    Command::with_params(GROUP_QUERY, json!({}))
}

pub fn mesh_query_command() -> Command {
    Command::with_params(MESH_QUERY, json!({}))
}

pub fn is_enabled(session: &Session, command: &Command) -> bool {
    can_execute(session, command).unwrap_or(false)
}

pub fn current_preset(session: &mut Session) -> Option<(String, f64)> {
    let out = session.run(&mesh_query_command()).ok()?;
    let preset = out.get("preset")?.as_str()?.to_string();
    let value = out.get("value")?.as_f64()?;
    Some((preset, value))
}

pub(crate) fn show_selected_group(
    ui: &mut egui::Ui, session: &Session, editor: &mut crate::canvas_input::warp_mesh::WarpMeshEditor,
) -> Option<Command> {
    let [id] = session.selection.ids.as_slice() else { return None };
    let (group, _) = dtool_core::commands::warp::warp_group(session, *id)?;
    let mut command = None;
    ui.horizontal_centered(|ui| {
        dtool_design::toggle(ui, &mut editor.enabled, text("warp.group.edit_mesh"));
        if editor.visible(session) {
            dtool_design::toggle(ui, &mut editor.multiple, text("warp.mesh.multiple"))
                .on_hover_text(text("warp.mesh.multiple_hint"));
            let junction = editor.junction_command(&group);
            if dtool_design::button_with(ui, text("warp.mesh.insert_line"), &dtool_design::ControlState::enabled(junction.is_some()))
                .on_disabled_hover_text(text("warp.mesh.insert_line_hint"))
                .clicked()
            {
                command = junction;
            }
        }
        ui.separator();
        egui::ComboBox::from_id_salt("warp.group.preset")
            .selected_text(text(&preset_key(&group.preset)))
            .show_ui(ui, |ui| {
                for (preset, key) in PRESET_CHOICES {
                    if ui.selectable_label(preset == group.preset, text(key)).clicked() && preset != group.preset {
                        command = preset_choice_command(preset_name(&preset), None, Some(id.0)).ok();
                    }
                }
            });
        if group.preset != WarpPreset::Mesh {
            let draft_id = ui.id().with(("warp.group.value", id.0, session.generation));
            let mut value = ui.data(|data| data.get_temp::<f64>(draft_id)).unwrap_or(group.value);
            let edited = ui.add(egui::DragValue::new(&mut value).range(-VALUE_LIMIT..=VALUE_LIMIT).suffix("%"));
            if edited.dragged() {
                ui.data_mut(|data| data.insert_temp(draft_id, value));
            } else if edited.changed() || edited.drag_stopped() {
                ui.data_mut(|data| data.remove::<f64>(draft_id));
                if value != group.value {
                    command = value_command(value, Some(id.0)).ok();
                }
            }
        }
        if dtool_design::button_with(ui, text("warp.group.reset"), &dtool_design::ControlState::enabled(true)).clicked() {
            command = Some(Command::with_params(GROUP_UPDATE, json!({"group": id.0, "preset": "mesh", "reset": true})));
        }
    });
    command
}

pub fn menu_keys() -> Vec<String> {
    NEW_WARP_GROUP_ITEMS.iter().map(|item| item_key(item)).collect()
}

pub fn render_new_warp_group_menu(ui: &mut egui::Ui, session: &mut Session) -> Option<CommandResult> {
    let mut result = None;
    ui.menu_button(text(NEW_WARP_GROUP_KEY), |ui| {
        for item in NEW_WARP_GROUP_ITEMS {
            let command = menu_command(session, &item_id(item));
            let enabled = command.as_ref().is_some_and(|c| is_enabled(session, c));
            let key = item_key(item);
            let clicked = ui.add_enabled_ui(enabled, |ui| dtool_design::menu_item(ui, text(&key))).inner.clicked();
            if clicked {
                if let Some(command) = command {
                    result = Some(session.run(&command));
                }
                ui.close();
            }
        }
    });
    result
}

pub fn render_preset_choice(ui: &mut egui::Ui, session: &mut Session) -> Option<CommandResult> {
    let current = current_preset(session);
    let mut result = None;
    ui.horizontal(|ui| {
        for (preset, key) in PRESET_CHOICES {
            let name = preset_name(&preset);
            let Ok(command) = preset_choice_command(name, None, None) else {
                continue;
            };
            let enabled = current.is_some() && is_enabled(session, &command);
            let selected = current.as_ref().is_some_and(|(p, _)| p == name);
            let clicked = ui.add_enabled_ui(enabled, |ui| dtool_design::tab(ui, selected, text(key))).inner.clicked();
            if clicked && !selected {
                result = Some(session.run(&command));
            }
        }
    });
    result
}

pub struct VectorWarpMenu {
    pub items: Vec<WarpMenuItem>,
}

#[derive(Clone, Debug)]
pub enum WarpMenuItem {
    NewGroup(WarpPresetKey),
    PresetChoice(WarpPresetKey),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WarpPresetKey {
    Mesh,
    Quad,
    Perspective,
    ArcVertical,
    ArcHorizontal,
    BendVertical,
    BendHorizontal,
    FishEye,
    Twist,
    Arch,
    UpperArc,
    LowerArc,
    Bulge,
    Flag,
    Rise,
    Wave,
    Pinch,
    Twirl,
    Spiral,
}

pub const WARP_PRESET_KEYS: [WarpPresetKey; 15] = [
    WarpPresetKey::Mesh,
    WarpPresetKey::Quad,
    WarpPresetKey::Perspective,
    WarpPresetKey::ArcVertical,
    WarpPresetKey::ArcHorizontal,
    WarpPresetKey::BendVertical,
    WarpPresetKey::BendHorizontal,
    WarpPresetKey::FishEye,
    WarpPresetKey::Twist,
    WarpPresetKey::Arch,
    WarpPresetKey::UpperArc,
    WarpPresetKey::LowerArc,
    WarpPresetKey::Bulge,
    WarpPresetKey::Flag,
    WarpPresetKey::Rise,
];

impl WarpPresetKey {
    pub fn command_name(self) -> &'static str {
        GROUP_CREATE
    }

    pub fn preset_json(self) -> serde_json::Value {
        json!(preset_name(&self.preset()))
    }

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Mesh => PRESET_MESH_KEY,
            Self::Quad => "warp.preset.quad",
            Self::Perspective => "warp.preset.perspective",
            Self::ArcVertical => "warp.preset.arc_vertical",
            Self::ArcHorizontal => "warp.preset.arc_horizontal",
            Self::BendVertical => "warp.preset.bend_vertical",
            Self::BendHorizontal => "warp.preset.bend_horizontal",
            Self::FishEye => "warp.preset.fish_eye",
            Self::Twist => "warp.preset.twist",
            Self::Arch => PRESET_ARCH_KEY,
            Self::UpperArc => "warp.preset.upper_arc",
            Self::LowerArc => "warp.preset.lower_arc",
            Self::Bulge => "warp.preset.bulge",
            Self::Flag => "warp.preset.flag",
            Self::Rise => "warp.preset.rise",
            Self::Wave => "warp.preset.wave",
            Self::Pinch => "warp.preset.pinch",
            Self::Twirl => "warp.preset.twirl",
            Self::Spiral => "warp.preset.spiral",
        }
    }

    pub fn preset(self) -> WarpPreset {
        match self {
            Self::Mesh => WarpPreset::Mesh,
            Self::Quad => WarpPreset::Quad,
            Self::Perspective => WarpPreset::Perspective,
            Self::ArcVertical => WarpPreset::ArcVertical,
            Self::ArcHorizontal => WarpPreset::ArcHorizontal,
            Self::BendVertical => WarpPreset::BendVertical,
            Self::BendHorizontal => WarpPreset::BendHorizontal,
            Self::FishEye => WarpPreset::FishEye,
            Self::Twist => WarpPreset::Twist,
            Self::Arch => WarpPreset::Arch,
            Self::UpperArc => WarpPreset::UpperArc,
            Self::LowerArc => WarpPreset::LowerArc,
            Self::Bulge => WarpPreset::Bulge,
            Self::Flag => WarpPreset::Flag,
            Self::Rise => WarpPreset::Rise,
            Self::Wave => WarpPreset::Wave,
            Self::Pinch => WarpPreset::Pinch,
            Self::Twirl => WarpPreset::Twirl,
            Self::Spiral => WarpPreset::Spiral,
        }
    }

    pub fn from_preset(preset: &WarpPreset) -> Self {
        match preset {
            WarpPreset::Mesh => Self::Mesh,
            WarpPreset::Quad => Self::Quad,
            WarpPreset::Perspective => Self::Perspective,
            WarpPreset::ArcVertical => Self::ArcVertical,
            WarpPreset::ArcHorizontal => Self::ArcHorizontal,
            WarpPreset::BendVertical => Self::BendVertical,
            WarpPreset::BendHorizontal => Self::BendHorizontal,
            WarpPreset::FishEye => Self::FishEye,
            WarpPreset::Twist => Self::Twist,
            WarpPreset::Arch => Self::Arch,
            WarpPreset::UpperArc => Self::UpperArc,
            WarpPreset::LowerArc => Self::LowerArc,
            WarpPreset::Bulge => Self::Bulge,
            WarpPreset::Flag => Self::Flag,
            WarpPreset::Rise => Self::Rise,
            WarpPreset::Wave => Self::Wave,
            WarpPreset::Pinch => Self::Pinch,
            WarpPreset::Twirl => Self::Twirl,
            WarpPreset::Spiral => Self::Spiral,
        }
    }

    pub fn menu_key(self) -> String {
        item_key(preset_name(&self.preset()))
    }
}

impl Default for VectorWarpMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorWarpMenu {
    pub fn new() -> Self {
        let presets = WARP_PRESET_KEYS;
        let items = presets
            .iter()
            .map(|p| WarpMenuItem::NewGroup(*p))
            .chain(presets.iter().map(|p| WarpMenuItem::PresetChoice(*p)))
            .collect();
        Self { items }
    }

    pub fn render(&self, ui: &mut egui::Ui) -> Option<WarpMenuItem> {
        let mut selected = None;
        for item in &self.items {
            let label = match item {
                WarpMenuItem::NewGroup(preset) => preset.menu_key(),
                WarpMenuItem::PresetChoice(preset) => preset.label_key().to_string(),
            };
            if dtool_design::menu_item(ui, text(&label)).clicked() {
                selected = Some(item.clone());
            }
        }
        selected
    }

    pub fn command_for(&self, item: &WarpMenuItem, nodes: Vec<u64>) -> Command {
        match item {
            WarpMenuItem::NewGroup(preset) => Command::with_params(
                preset.command_name(),
                json!({
                    "nodes": nodes,
                    "preset": preset.preset_json(),
                    "value": default_value(&preset.preset()),
                }),
            ),
            WarpMenuItem::PresetChoice(preset) => Command::with_params(GROUP_UPDATE, json!({ "preset": preset.preset_json() })),
        }
    }

    pub fn run(&self, session: &mut Session, item: &WarpMenuItem) -> CommandResult {
        let command = self.command_for(item, selected_nodes(session));
        if !is_enabled(session, &command) {
            return Err(CommandError::invalid_params());
        }
        session.run(&command)
    }
}
