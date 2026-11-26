pub mod align;
mod node;
pub(crate) use node::{show as show_node_controls, show_paint_wells};

use dtool_core::workspace::ContextId;
use dtool_core::Session;
use dtool_protocol::{Command, CommandError};
use dtool_strings::text;
use serde_json::{json, Value};

use crate::icon_cache::IconCache;

pub const QUERY: &str = "ui.context_toolbar.query";
pub const OPTION_SET: &str = "ui.context_option.set";

#[derive(Debug, Clone, PartialEq)]
pub enum ContextControl {
    Toggle(bool),
    Number { value: f64, min: f64, max: f64, step: f64 },
    Choice { value: String, values: Vec<String> },
    Text(String),
    Action { command: String },
    Unsupported { kind: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlState {
    #[default]
    Enabled,
    Disabled,
    Unsupported,
    Mixed,
}

impl ControlState {
    pub fn parse(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_str) {
            None => Self::Enabled,
            Some("enabled") => Self::Enabled,
            Some("disabled") => Self::Disabled,
            Some("mixed") => Self::Mixed,
            Some(_) => Self::Unsupported,
        }
    }

    pub fn editable(self) -> bool {
        matches!(self, Self::Enabled | Self::Mixed)
    }
}

pub type Stamp = (ContextId, u64);

#[derive(Debug, Clone, PartialEq)]
pub struct ContextItem {
    pub id: String,
    pub key: String,
    pub control: ContextControl,
    pub state: ControlState,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ContextBarModel {
    pub tool: Option<String>,
    pub items: Vec<ContextItem>,
    pub stamp: Option<Stamp>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextEdit {
    pub id: String,
    pub value: Value,
}

fn parse_item(option: &Value) -> Option<ContextItem> {
    let id = option.get("id")?.as_str()?.to_string();
    let key = option.get("key").and_then(Value::as_str).unwrap_or(id.as_str()).to_string();
    let value = option.get("value").cloned().unwrap_or(Value::Null);
    let number = |name: &str, fallback: f64| option.get(name).and_then(Value::as_f64).unwrap_or(fallback);
    let kind = option.get("kind").and_then(Value::as_str).unwrap_or_default();
    let mut state = ControlState::parse(option.get("state"));
    let control = match kind {
        "toggle" => ContextControl::Toggle(value.as_bool().unwrap_or(false)),
        "number" => ContextControl::Number {
            value: value.as_f64().unwrap_or(0.0),
            min: number("min", f64::MIN),
            max: number("max", f64::MAX),
            step: number("step", 1.0),
        },
        "choice" => ContextControl::Choice {
            value: value.as_str().unwrap_or_default().to_string(),
            values: option
                .get("values")
                .and_then(Value::as_array)
                .map(|values| values.iter().filter_map(Value::as_str).map(String::from).collect())
                .unwrap_or_default(),
        },
        "text" => ContextControl::Text(value.as_str().unwrap_or_default().to_string()),
        "action" => ContextControl::Action {
            command: option.get("command").and_then(Value::as_str).unwrap_or_default().to_string(),
        },
        other => {
            state = ControlState::Unsupported;
            ContextControl::Unsupported { kind: other.to_string() }
        }
    };
    if matches!(&control, ContextControl::Action { command } if command.is_empty()) {
        state = ControlState::Unsupported;
    }
    Some(ContextItem { id, key, control, state })
}

impl ContextBarModel {
    pub fn from_query(result: &Value) -> Self {
        let tool = result.get("tool").and_then(Value::as_str).map(String::from);
        let items = result
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options.iter().filter_map(parse_item).filter(|item| {
                    !matches!(item.id.as_str(), "fill.context_fill" | "fill.fill_type" | "fill.pigment_blend")
                }).collect()
            })
            .unwrap_or_default();
        Self { tool, items, stamp: None }
    }

    pub fn from_stamped_query(result: &Value, context: ContextId, generation: u64) -> Self {
        Self { stamp: Some((context, generation)), ..Self::from_query(result) }
    }

    pub fn stamped(session: &Session, context: ContextId) -> Self {
        Self::from_stamped_query(&dtool_core::commands::context_toolbar::query_with_state(session, &session.active_tool), context, session.generation)
    }

    pub fn is_current(&self, current: Option<Stamp>) -> bool {
        self.stamp.is_some() && self.stamp == current
    }

    pub fn section<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a ContextItem> + 'a {
        self.items.iter().filter(move |item| item.id.starts_with(prefix))
    }

    pub fn item(&self, id: &str) -> Option<&ContextItem> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn edit_command(&self, edit: &ContextEdit) -> Option<Command> {
        let tool = self.tool.as_deref()?;
        let item = self.item(&edit.id)?;
        if !item.state.editable() {
            return None;
        }
        match &item.control {
            ContextControl::Action { command } => Some(Command::new(command)),
            ContextControl::Unsupported { .. } => None,
            _ => Some(Command::with_params(OPTION_SET, json!({ "tool": tool, "id": edit.id, "value": edit.value }))),
        }
    }

    pub fn bind(&self, edit: &ContextEdit, current: Option<Stamp>) -> Option<Command> {
        if !self.is_current(current) {
            return None;
        }
        self.edit_command(edit)
    }

    pub fn load(session: &mut Session) -> Result<Self, CommandError> {
        session.run_named(QUERY, json!({})).map(|result| Self::from_query(&result))
    }

    pub fn current(session: &Session) -> Self {
        Self::from_query(&dtool_core::commands::context_toolbar::query_with_state(session, &session.active_tool))
    }

    pub fn ids(&self) -> Vec<&str> {
        self.items.iter().map(|item| item.id.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

pub const VIEW_ZOOM_FIT: &str = "view.zoom.fit";

pub fn action_params(command: &str, view_size: [f32; 2]) -> Value {
    if command == VIEW_ZOOM_FIT {
        json!({ "view_size": view_size })
    } else {
        json!({})
    }
}

impl ContextBarModel {
    pub fn with_view_size(mut self, session: &Session, view_size: [f32; 2]) -> Self {
        for item in &mut self.items {
            if item.id == dtool_core::context_toolbar::PEN_CONSTRUCTION_SNAP {
                use crate::canvas_curves::PenMode;
                let tool = &session.active_tool;
                let mode = session.tool_options.choice(tool, "pen.mode");
                if PenMode::for_tool(tool, mode.as_deref()) != PenMode::Pen {
                    item.state = ControlState::Disabled;
                }
            }
            if let ContextControl::Action { command } = &item.control {
                if command == VIEW_ZOOM_FIT {
                    let ok = session.can_execute(&Command::with_params(command, action_params(command, view_size)));
                    item.state = if ok { ControlState::Enabled } else { ControlState::Disabled };
                }
                if command == align::ALIGN_POPUP_COMMAND {
                    item.state = if session.has_document { ControlState::Enabled } else { ControlState::Disabled };
                }
            }
        }
        self
    }

    pub(crate) fn set_pen_close_enabled(&mut self, enabled: bool) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == PEN_CLOSE_CURVE) {
            item.state = if enabled { ControlState::Enabled } else { ControlState::Disabled };
        }
    }

    pub fn bind_with_view(&self, edit: &ContextEdit, current: Option<Stamp>, view_size: [f32; 2]) -> Option<Command> {
        let command = self.bind(edit, current)?;
        if command.name == VIEW_ZOOM_FIT {
            return Some(Command::with_params(VIEW_ZOOM_FIT, action_params(VIEW_ZOOM_FIT, view_size)));
        }
        Some(command)
    }
}

fn action_target(tool: &str, id: &str) -> Option<&'static str> {
    let tool = dtool_core::context_toolbar::canonical_tool(tool)?;
    match dtool_core::context_toolbar::find_option(tool, id)?.kind {
        dtool_core::context_toolbar::OptionKind::Action { command } => Some(command),
        _ => None,
    }
}

fn pick_from(ui: &mut egui::Ui, model: &ContextBarModel) -> Option<ContextEdit> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        for item in &model.items {
            if let Some(edit) = show_item(ui, item) {
                picked = Some(edit);
            }
        }
    });
    picked
}

pub fn pick_with_view(ui: &mut egui::Ui, session: &Session, view_size: [f32; 2]) -> Option<(String, ContextEdit)> {
    let model = ContextBarModel::current(session).with_view_size(session, view_size);
    let tool = model.tool.clone()?;
    pick_from(ui, &model).map(|edit| (tool, edit))
}

pub fn pick_bound_with_view(ui: &mut egui::Ui, session: &Session, context: ContextId, view_size: [f32; 2]) -> Option<(ContextEdit, Option<Command>)> {
    let model = ContextBarModel::stamped(session, context).with_view_size(session, view_size);
    model.tool.as_ref()?;
    let edit = pick_from(ui, &model)?;
    let command = model.bind_with_view(&edit, Some((context, session.generation)), view_size);
    Some((edit, command))
}

pub fn apply_edit_with_view(session: &mut Session, tool: &str, edit: &ContextEdit, view_size: [f32; 2]) -> Result<Value, CommandError> {
    match action_target(tool, &edit.id) {
        Some(command) => session.run_named(OPTION_SET, json!({ "tool": tool, "id": edit.id, "params": action_params(command, view_size) })),
        None => apply_edit(session, tool, edit),
    }
}

pub fn apply_edit(session: &mut Session, tool: &str, edit: &ContextEdit) -> Result<Value, CommandError> {
    session.run_named(OPTION_SET, json!({ "tool": tool, "id": edit.id, "value": edit.value }))
}

pub fn choice_label(key: &str, value: &str) -> String {
    let value_key = format!("{key}.{value}");
    if dtool_strings::has(&value_key) {
        text(&value_key).to_string()
    } else {
        value.to_string()
    }
}

const UNSUPPORTED_TOOLTIP_KEY: &str = "context_bar.unsupported.tooltip";

fn show_item(ui: &mut egui::Ui, item: &ContextItem) -> Option<ContextEdit> {
    ui.add_enabled_ui(item.state.editable(), |ui| show_control(ui, item)).inner.filter(|_| item.state.editable())
}

fn show_item_with_icons(ui: &mut egui::Ui, item: &ContextItem, icons: &mut IconCache) -> Option<ContextEdit> {
    use dtool_core::context_toolbar::{MOVE_SHOW_ORIGIN, MOVE_SHOW_SELECTION_BOX, MOVE_TRANSFORM_SEPARATELY};
    if item.id == dtool_core::context_toolbar::PEN_CONSTRUCTION_SNAP
        && let ContextControl::Toggle(active) = item.control
        && let Some(texture) = icons.themed_texture(ui.ctx(), "ctx-node-handle-construction", dtool_design::icon_texture_px(ui))
    {
        let label = text(&item.key);
        let state = dtool_design::ControlState::enabled(item.state.editable()).tooltip(label);
        let button = dtool_design::icon_image_button_selected(ui, &texture, active, &state);
        button.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, button.enabled(), active, label));
        return button.clicked().then(|| ContextEdit { id: item.id.clone(), value: json!(!active) });
    }
    let move_icon = match item.id.as_str() {
        MOVE_TRANSFORM_SEPARATELY => Some("ctx-transform-separately"),
        MOVE_SHOW_SELECTION_BOX => Some("cmd-transform-box"),
        MOVE_SHOW_ORIGIN => Some("ctx-transform-origin"),
        "move.reset_origin" => Some("cmd-undo"),
        _ => None,
    };
    if let Some(icon) = move_icon
        && let Some(texture) = icons.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui))
    {
        let label = text(&item.key);
        let enabled = item.state.editable();
        let toggle = match item.control { ContextControl::Toggle(value) => Some(value), _ => None };
        let image = egui::Image::new(&texture).fit_to_exact_size(egui::Vec2::splat(17.0));
        let mut button = egui::Button::image(image).min_size(egui::Vec2::splat(28.0));
        if toggle == Some(true) {
            let visuals = ui.visuals();
            button = button.fill(visuals.selection.bg_fill.gamma_multiply(0.48))
                .stroke(egui::Stroke::new(1.0, visuals.selection.bg_fill));
        }
        let response = ui.add_enabled(enabled, button).on_hover_text(label);
        response.widget_info(|| match toggle {
            Some(value) => egui::WidgetInfo::selected(egui::Role::Button, response.enabled(), value, label),
            None => egui::WidgetInfo::labeled(egui::Role::Button, response.enabled(), label),
        });
        return response.clicked().then(|| ContextEdit { id: item.id.clone(), value: toggle.map_or(Value::Null, |value| json!(!value)) });
    }
    if item.id == dtool_core::context_toolbar::CURVE_ENERGY {
        let label = text("context.curve_energy.settings");
        let button = align::icon_action(ui, icons, "ctx-settings", label, item.state.editable());
        let mut picked = None;
        egui::Popup::menu(&button).show(|ui| {
            picked = show_item(ui, item);
            if picked.is_some() {
                ui.close();
            }
        });
        return picked;
    }
    if let ("shape_builder.action", ContextControl::Choice { value, .. }) = (item.id.as_str(), &item.control) {
        ui.label(text(&item.key));
        let enabled = item.state.editable();
        for (action, icon) in [("add", "cmd-bool-add"), ("delete", "cmd-bool-subtract"), ("create", "edit-duplicate")] {
            let label = choice_label(&item.key, action);
            let state = dtool_design::ControlState::enabled(enabled).tooltip(&label);
            let response = match icons.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui)) {
                Some(texture) => dtool_design::icon_image_button_selected(ui, &texture, value == action, &state),
                None => dtool_design::button_with(ui, &label, &state),
            };
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled && ui.is_enabled(), value == action, &label));
            if enabled && response.clicked() {
                return Some(ContextEdit { id: item.id.clone(), value: json!(action) });
            }
        }
        return None;
    }
    if let ContextControl::Action { command } = &item.control {
        let icon = match command.as_str() {
            "vector.convert_to_curves" => Some("tool-shape-curve"),
            "view.zoom.in" => Some("ctx-zoom-in"),
            "view.zoom.out" => Some("ctx-zoom-out"),
            "view.zoom.fit" => Some("ctx-zoom-fit"),
            "view.zoom.actual" => Some("ctx-zoom-actual"),
            "path.close" => Some("ctx-close-curve"),
            _ => align::action_icon(command),
        };
        if let Some(icon) = icon {
            let enabled = item.state.editable();
            let response = align::icon_action(ui, icons, icon, text(&item.key), enabled);
            if !enabled || !response.clicked() {
                return None;
            }
            remember_alignment_origin(ui, &response, command);
            return Some(ContextEdit { id: item.id.clone(), value: Value::Null });
        }
    }
    show_item(ui, item)
}

pub(crate) fn paint_transform_origin(painter: &egui::Painter, centre: egui::Pos2, colour: egui::Color32, radius: f32) {
    let stroke = egui::Stroke::new(1.25, colour);
    painter.circle_stroke(centre, radius, stroke);
    painter.circle_filled(centre, 1.1, colour);
    for direction in [egui::Vec2::X, -egui::Vec2::X, egui::Vec2::Y, -egui::Vec2::Y] {
        painter.line_segment([centre + direction * (radius - 0.5), centre + direction * (radius + 2.0)], stroke);
    }
}

pub(crate) fn remember_alignment_origin(ui: &egui::Ui, response: &egui::Response, command: &str) {
    if command == align::ALIGN_POPUP_COMMAND {
        let position = response.rect.left_bottom() + egui::vec2(0.0, 4.0);
        ui.data_mut(|data| data.insert_temp(egui::Id::new(align::ALIGN_LAUNCHER_POSITION), position));
    }
}

fn show_zoom_level(ui: &mut egui::Ui, item: &ContextItem, current: f64, range: std::ops::RangeInclusive<f64>, step: f64) -> Option<ContextEdit> {
    let label = text(&item.key);
    let caption = ui.label(label);
    let mut value = current;
    let field = egui::DragValue::new(&mut value).range(range.clone()).speed(step)
        .suffix("%").max_decimals(2).clamp_existing_to_range(false).update_while_editing(false);
    let entry = ui.add_sized([70.0, ui.spacing().interact_size.y], field).labelled_by(caption.id);
    let mut picked = entry.changed().then_some(value);
    let presets_label = text("context.zoom.presets");
    let presets = ui.menu_button("⌄", |ui| {
        for preset in [12.5, 25.0, 50.0, 100.0, 200.0, 400.0, 800.0, 1600.0] {
            if range.contains(&preset)
                && ui.selectable_label((current - preset).abs() < 0.01, format!("{preset}%")).clicked()
            {
                picked = Some(preset);
                ui.close();
            }
        }
    }).response.on_hover_text(presets_label);
    presets.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, presets.enabled(), presets_label));

    // The rail covers working magnifications; the field can still enter the full range.
    let min = range.start().max(5.0) as f32;
    let max = range.end().min(3200.0) as f32;
    if min < max {
        let mut rail = (current as f32).clamp(min, max);
        if dtool_design::logarithmic_slider_rail(ui, &mut rail, min..=max, 112.0, label, ui.is_enabled(), f64::from(min)).changed() {
            picked = Some(f64::from(rail));
        }
    }
    picked.map(|value| ContextEdit { id: item.id.clone(), value: json!(value) })
}

fn show_control(ui: &mut egui::Ui, item: &ContextItem) -> Option<ContextEdit> {
    let label = text(&item.key);
    let edit = |value: Value| Some(ContextEdit { id: item.id.clone(), value });
    match &item.control {
        ContextControl::Toggle(current) => {
            let mut value = *current;
            if dtool_design::toggle(ui, &mut value, label).changed() {
                return edit(json!(value));
            }
        }
        ContextControl::Number { value, min, max, step } => {
            if item.id == "zoom.level" {
                return show_zoom_level(ui, item, *value, *min..=*max, *step);
            }
            let mut value = *value;
            let caption = ui.label(label).id;
            if item.id == dtool_core::context_toolbar::SHAPE_CORNER_RADIUS {
                use dtool_design::{NumericEvent, NumericSpec, NumericValue};
                let id = ui.make_persistent_id(&item.id);
                let spec = NumericSpec::new("").range(*min..=*max).step(*step)
                    .suffix("%").width(48.0).decimals(1);
                let shown = if item.state == ControlState::Mixed { NumericValue::Mixed } else { NumericValue::Value(value) };
                let (field, event) = dtool_design::numeric_field(ui, id, shown, &spec);
                field.labelled_by(caption).on_hover_text(label);
                return match event {
                    NumericEvent::Commit(value) => edit(json!(value)),
                    _ => None,
                };
            }
            let field = egui::DragValue::new(&mut value).range(*min..=*max).speed(*step);
            let field = if item.id == "wand.tolerance" {
                field.suffix("%")
            } else if matches!(item.id.as_str(), "shape.stroke_width" | "pen.stroke_width")
                || (item.id == "brush.width" && item.key == "context.vector_brush.width")
            {
                field.suffix(" pt").max_decimals(2).clamp_existing_to_range(false).update_while_editing(false)
            } else if item.id.starts_with("shape.recipe.") {
                field.clamp_existing_to_range(false).update_while_editing(false)
            } else {
                field
            };
            let field = if item.state == ControlState::Mixed {
                field.custom_formatter(|_, _| "—".to_owned())
            } else {
                field
            };
            let response = ui.add(field).labelled_by(caption);
            if response.changed() {
                return edit(json!(value));
            }
        }
        ContextControl::Choice { value, values } => {
            ui.label(label);
            let labels: Vec<String> = values.iter().map(|choice| choice_label(&item.key, choice)).collect();
            let options: Vec<(String, &str)> = values.iter().zip(labels.iter()).map(|(choice, label)| (choice.clone(), label.as_str())).collect();
            let mut current = value.clone();
            let response = dtool_design::dropdown_value(ui, &format!("context_bar.{}", item.id), &mut current, &options, &dtool_design::ControlState::default());
            if response.changed() && current != *value {
                return edit(json!(current));
            }
        }
        ContextControl::Text(value) => {
            if matches!(item.id.as_str(), "brush.replacement_colour" | "brush.mixer_colour" | "brush.tone_colour") {
                let Some(dtool_core::paint::Paint::Solid { colour }) = dtool_core::paint::paint_from_legacy_str(value) else {
                    return None;
                };
                let mut rgb = [colour.r, colour.g, colour.b];
                let caption = ui.label(label).id;
                if ui.color_edit_button_srgb(&mut rgb).labelled_by(caption).changed() {
                    let [r, g, b] = rgb;
                    let value = if item.id != "brush.replacement_colour" { format!("#{r:02X}{g:02X}{b:02X}FF") }
                        else { format!("#{r:02X}{g:02X}{b:02X}") };
                    return edit(json!(value));
                }
                return None;
            }
            let mut value = value.clone();
            ui.label(label);
            if ui.text_edit_singleline(&mut value).lost_focus() {
                return edit(json!(value));
            }
        }
        ContextControl::Action { command } => {
            let response = dtool_design::tooltip(dtool_design::button(ui, label), label);
            if response.clicked() {
                remember_alignment_origin(ui, &response, command);
                return edit(Value::Null);
            }
        }
        ContextControl::Unsupported { .. } => {
            let tip = if dtool_strings::has(UNSUPPORTED_TOOLTIP_KEY) { text(UNSUPPORTED_TOOLTIP_KEY) } else { label };
            dtool_design::tooltip(ui.label(label), tip);
        }
    }
    None
}

pub fn show_section(ui: &mut egui::Ui, model: &ContextBarModel, prefix: &str, current: Option<Stamp>) -> Option<Command> {
    show_section_with_icons(ui, model, prefix, current, &mut IconCache::new())
}

fn show_section_with_icons(ui: &mut egui::Ui, model: &ContextBarModel, prefix: &str, current: Option<Stamp>, icons: &mut IconCache) -> Option<Command> {
    let mut picked = None;
    for (index, item) in model.section(prefix).enumerate() {
        if index > 0 && starts_option_group(item) {
            ui.separator();
        }
        if let Some(edit) = show_item_with_icons(ui, item, icons) {
            picked = Some(edit);
        }
    }
    picked.and_then(|edit| model.bind(&edit, current))
}

pub fn show_bound(ui: &mut egui::Ui, model: &ContextBarModel, current: Option<Stamp>) -> Option<Command> {
    show_bound_with_icons(ui, model, current, &mut IconCache::new())
}

pub(crate) fn show_bound_with_icons(ui: &mut egui::Ui, model: &ContextBarModel, current: Option<Stamp>, icons: &mut IconCache) -> Option<Command> {
    model.tool.as_ref()?;
    ui.horizontal(|ui| show_section_with_icons(ui, model, "", current, icons)).inner
}

fn starts_option_group(item: &ContextItem) -> bool {
    matches!(item.id.as_str(), "move.align" | "move.convert_to_curves" | "shape.convert_to_curves" | "zoom.level" | "zoom.in")
}

pub(crate) fn show_vector_brush_tip(ui: &mut egui::Ui, session: &Session) -> Option<Command> {
    use dtool_core::path_brush::{Texture, TextureMode};
    let current = dtool_core::commands::brush::current_texture(session);
    let mut tip = None;
    ui.menu_button(text("context.brush.tip"), |ui| {
        if ui.selectable_label(current.is_none(), text("context.brush.tip.solid")).clicked() {
            tip = Some(json!({"texture": null}));
            ui.close();
        }
        for (mode, key) in [
            (TextureMode::Intensity, "context.brush.tip.intensity"),
            (TextureMode::Image, "context.brush.tip.image"),
        ] {
            ui.menu_button(text(key), |ui| {
                let resources = &session.document.resources;
                if resources.is_empty() {
                    ui.label(text("context.brush.tip.no_images"));
                }
                for resource in resources {
                    let texture = Texture { mode, resource_id: resource.id.0.to_string() };
                    let label = if resource.name.is_empty() { texture.resource_id.as_str() } else { &resource.name };
                    let available = resource.status != dtool_core::resources::ResourceStatus::Missing;
                    let selected = current.as_ref() == Some(&texture);
                    if ui.add_enabled(available, egui::Button::selectable(selected, label)).clicked() {
                        tip = Some(json!({"texture": texture}));
                        ui.close();
                    }
                }
            });
        }
    });
    tip.map(|tip| Command::with_params("brush.vector.tip.apply", json!({"tip": tip})))
}

pub(crate) const MORE_WIDTH: f32 = 28.0;

pub(crate) fn more_button(ui: &mut egui::Ui) -> egui::Response {
    let gap = ui.available_width() - MORE_WIDTH - ui.spacing().item_spacing.x;
    if gap > 0.0 {
        ui.add_space(gap);
    }
    let label = text("context.toolbar.more");
    let more = ui.add(egui::Button::new("⋯").min_size(egui::vec2(MORE_WIDTH, ui.spacing().interact_size.y)))
        .on_hover_text(label);
    more.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    more
}

pub(crate) fn show_menu_items(ui: &mut egui::Ui, items: &[ContextItem]) -> Option<ContextEdit> {
    let mut picked = None;
    for (index, item) in items.iter().enumerate() {
        if index > 0 && starts_option_group(item) {
            ui.separator();
        }
        if let Some(edit) = ui.push_id(&item.id, |ui| ui.horizontal(|ui| show_item(ui, item)).inner).inner {
            if matches!(item.control, ContextControl::Action { .. }) {
                ui.close();
            }
            picked = Some(edit);
        }
    }
    picked
}

pub(crate) fn show_toolbar_options(
    ui: &mut egui::Ui,
    model: &ContextBarModel,
    current: Option<Stamp>,
    icons: &mut IconCache,
    follows_tools: bool,
) -> Option<Command> {
    let available = ui.available_width();
    let spacing = ui.spacing().item_spacing.x;
    // Measure the same widgets without advancing the live toolbar or accepting input.
    let mut measure = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("context.measure")
            .max_rect(egui::Rect::from_min_size(ui.cursor().min, egui::vec2(10_000.0, ui.available_height())))
            .layout(egui::Layout::left_to_right(egui::Align::Center))
            .invisible()
            .sizing_pass(),
    );
    let origin = measure.min_rect().left();
    let ends: Vec<_> = model.items.iter().enumerate().map(|(index, item)| {
        if (index == 0 && follows_tools) || (index > 0 && starts_option_group(item)) {
            measure.separator();
        }
        let _ = show_item_with_icons(&mut measure, item, icons);
        measure.min_rect().right() - origin
    }).collect();
    let overflow = ends.last().is_some_and(|width| *width > available);
    let visible = if overflow {
        ends.iter().take_while(|width| **width + spacing + MORE_WIDTH <= available).count()
    } else {
        model.items.len()
    };
    let mut picked = None;
    for (index, item) in model.items[..visible].iter().enumerate() {
        if (index == 0 && follows_tools) || (index > 0 && starts_option_group(item)) {
            ui.separator();
        }
        if let Some(edit) = ui.push_id(&item.id, |ui| show_item_with_icons(ui, item, icons)).inner {
            picked = Some(edit);
        }
    }
    if overflow {
        let more = more_button(ui);
        egui::Popup::menu(&more)
            .align(egui::RectAlign::BOTTOM_END)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                egui::ScrollArea::vertical()
                    .max_height((ui.ctx().content_rect().height() * 0.55).min(360.0))
                    .show(ui, |ui| {
                        ui.set_min_width(148.0);
                        if let Some(edit) = show_menu_items(ui, &model.items[visible..]) {
                            picked = Some(edit);
                        }
                    });
            });
    }
    picked.and_then(|edit| model.bind(&edit, current))
}

pub fn bound_command(session: &Session, context: ContextId, edit: &ContextEdit) -> Option<Command> {
    ContextBarModel::stamped(session, context).bind(edit, Some((context, session.generation)))
}

pub fn is_pen_close(edit: &ContextEdit) -> bool {
    edit.id == PEN_CLOSE_CURVE
}

pub fn pick_bound(ui: &mut egui::Ui, session: &Session, context: ContextId) -> Option<(ContextEdit, Option<Command>)> {
    let model = ContextBarModel::stamped(session, context);
    model.tool.as_ref()?;
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        for item in &model.items {
            if let Some(edit) = show_item(ui, item) {
                picked = Some(edit);
            }
        }
    });
    let edit = picked?;
    let command = model.bind(&edit, Some((context, session.generation)));
    Some((edit, command))
}

pub fn show(ui: &mut egui::Ui, session: &mut Session) -> Option<Result<Value, CommandError>> {
    let (tool, edit) = pick(ui, session)?;
    Some(apply_edit(session, &tool, &edit))
}

pub const PEN_CLOSE_CURVE: &str = "pen.close_curve";

pub fn close_pen_draft(edit: &ContextEdit, input: &mut crate::canvas_input::CanvasInput, session: &mut Session) -> bool {
    edit.id == PEN_CLOSE_CURVE && input.close_pen(session)
}

pub fn pick(ui: &mut egui::Ui, session: &Session) -> Option<(String, ContextEdit)> {
    let model = ContextBarModel::current(session);
    let tool = model.tool.clone()?;
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        for item in &model.items {
            if let Some(edit) = show_item(ui, item) {
                picked = Some(edit);
            }
        }
    });
    picked.map(|edit| (tool, edit))
}
