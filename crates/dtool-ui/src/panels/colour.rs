use dtool_core::colour::{convert, hsl_to_rgb, rgb_to_hsl, ColourModel, ConvertCtx, ProfileStore, RenderingIntent};
use dtool_core::commands::appearance::{Focus, Kind, PaintSlot};
use dtool_core::object::ObjectId;
use dtool_core::paint::{paint_from_legacy_str, Colour, Paint};
use dtool_core::workspace::{ContextId, Scope, Workspace};
use dtool_design::{dropdown, panel_frame, panel_header, toggle, NumericEvent, NumericSpec, NumericValue};
use dtool_protocol::{Command, CommandError};
use dtool_strings::text;
use serde_json::{json, Value};

mod picker;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColourTarget {
    #[default]
    Fill,
    Stroke,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PanelModel {
    #[default]
    Rgb,
    Hsl,
    Cmyk,
    Lab,
    Grey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisplayMode {
    Wheel,
    Sliders,
    Boxes,
    Tint,
}

const DISPLAY_MODES: [(DisplayMode, &str); 4] = [
    (DisplayMode::Wheel, "colour.panel.wheel"),
    (DisplayMode::Sliders, "colour.panel.channels"),
    (DisplayMode::Boxes, "colour.panel.boxes"),
    (DisplayMode::Tint, "colour.panel.tint"),
];

#[derive(Clone, Debug)]
struct Tint {
    base: Colour,
    result: Colour,
    amount: f32,
}

const TOOL_SWATCH_HEIGHT: f32 = 34.0;

pub(crate) fn tool_swatches_height(ui: &egui::Ui) -> f32 {
    // Well pair, its gap to Swap, and the divider with its surrounding spacing.
    TOOL_SWATCH_HEIGHT + 1.0 + 23.0 + 2.0 * ui.spacing().item_spacing.y + 6.0
}

pub(crate) enum ToolColourAction {
    Open(ColourTarget),
    Edit(ColourTarget),
    Clear(ColourTarget),
    Defaults,
    Swap,
}

pub const PANEL_MODELS: [PanelModel; 5] = [PanelModel::Rgb, PanelModel::Hsl, PanelModel::Cmyk, PanelModel::Lab, PanelModel::Grey];
pub const PROOF_INTENTS: [(RenderingIntent, &str, &str); 4] = [
    (RenderingIntent::Perceptual, "perceptual", "colour.intent.perceptual"),
    (RenderingIntent::RelativeColorimetric, "relative_colorimetric", "colour.intent.relative"),
    (RenderingIntent::Saturation, "saturation", "colour.intent.saturation"),
    (RenderingIntent::AbsoluteColorimetric, "absolute_colorimetric", "colour.intent.absolute"),
];

impl PanelModel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rgb => "RGB",
            Self::Hsl => "HSL",
            Self::Cmyk => "CMYK",
            Self::Lab => "LAB",
            Self::Grey => text("menu.view.mode.greyscale"),
        }
    }

    pub fn ranges(self) -> &'static [(f32, f32)] {
        match self {
            Self::Rgb => &[(0.0, 255.0), (0.0, 255.0), (0.0, 255.0)],
            Self::Hsl => &[(0.0, 360.0), (0.0, 100.0), (0.0, 100.0)],
            Self::Cmyk => &[(0.0, 100.0), (0.0, 100.0), (0.0, 100.0), (0.0, 100.0)],
            Self::Lab => &[(0.0, 100.0), (-128.0, 127.0), (-128.0, 127.0)],
            Self::Grey => &[(0.0, 100.0)],
        }
    }

    fn channel_labels(self) -> &'static [&'static str] {
        match self {
            Self::Rgb => &["R", "G", "B"],
            Self::Hsl => &["H", "S", "L"],
            Self::Cmyk => &["C", "M", "Y", "K"],
            Self::Lab => &["L*", "a*", "b*"],
            Self::Grey => &["K"],
        }
    }

    fn managed(self) -> Option<(ColourModel, &'static str)> {
        match self {
            Self::Cmyk => Some((ColourModel::Cmyk, ProfileStore::FOGRA39L)),
            Self::Lab => Some((ColourModel::Lab, ProfileStore::LAB_D50)),
            Self::Grey => Some((ColourModel::Grey, ProfileStore::GREY_D50)),
            Self::Rgb | Self::Hsl => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProofState {
    pub enabled: bool,
    pub profile: String,
    pub intent: RenderingIntent,
    pub bpc: bool,
    pub gamut_check: bool,
}

impl Default for ProofState {
    fn default() -> Self {
        Self { enabled: false, profile: ProfileStore::FOGRA39L.to_string(), intent: RenderingIntent::RelativeColorimetric, bpc: true, gamut_check: false }
    }
}

#[derive(Clone, Debug)]
pub struct ColourPanel {
    pub target: ColourTarget,
    pub model: PanelModel,
    pub colour: Colour,
    pub hex: String,
    pub error: Option<String>,
    pub proof: ProofState,
    pub contrast: Option<f64>,
    seen: Option<(Option<ContextId>, u64, Vec<ObjectId>, ColourTarget)>,
    seen_slot: Option<(ContextId, ObjectId, PaintSlot)>,
    seen_colour: Option<Colour>,
    hex_input: String,
    display: DisplayMode,
    tint: Option<Tint>,
    channel_hue: f32,
    channel_edit: Option<ChannelEdit>,
    drag: Option<ColourDrag>,
    picker: picker::Picker,
    icons: crate::icon_cache::IconCache,
}

#[derive(Clone, Debug)]
struct ChannelEdit {
    model: PanelModel,
    rgb: [u8; 3],
    values: Vec<f32>,
}

#[derive(Clone, Debug)]
struct ColourDrag {
    colour: Colour,
    channel_edit: Option<ChannelEdit>,
    hue: f32,
    tint: Option<Tint>,
    pass: u64,
    cancelled: bool,
}

impl Default for ColourPanel {
    fn default() -> Self {
        let colour = Colour::new(0, 0, 0, 255);
        let hex = hex_of(&colour);
        Self { target: ColourTarget::Fill, model: PanelModel::Hsl, colour, hex_input: hex.clone(), hex,
            error: None, proof: ProofState::default(), contrast: None, seen: None, seen_slot: None, seen_colour: None,
            display: DisplayMode::Sliders, tint: None, channel_hue: 0.0, channel_edit: None, drag: None,
            picker: picker::Picker::default(), icons: crate::icon_cache::IconCache::default() }
    }
}

pub fn hex_of(colour: &Colour) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", colour.r, colour.g, colour.b, colour.a)
}

fn rgb_channels(colour: Colour) -> [u8; 3] {
    [colour.r, colour.g, colour.b]
}

pub(crate) fn picker_hsva(colour: Colour) -> egui::ecolor::Hsva {
    let mut hsva = egui::ecolor::Hsva::from_srgb([colour.r, colour.g, colour.b]);
    hsva.a = f32::from(colour.a) / 255.0;
    hsva
}

pub(crate) fn picker_colour(hsva: egui::ecolor::Hsva) -> Colour {
    let [r, g, b, a] = hsva.to_srgba_unmultiplied();
    Colour::new(r, g, b, a)
}

pub(crate) fn picker_button(ui: &mut egui::Ui, colour: &mut Colour) -> egui::Response {
    use egui::{color_picker::{color_edit_button_hsva, Alpha}, ecolor::Hsva};

    let id = ui.next_auto_id().with("picker_hsva");
    let channels = [colour.r, colour.g, colour.b, colour.a];
    let mut hsva = ui.data(|data| data.get_temp::<Hsva>(id))
        .filter(|draft| draft.to_srgba_unmultiplied() == channels)
        .unwrap_or_else(|| picker_hsva(*colour));
    let response = color_edit_button_hsva(ui, &mut hsva, Alpha::OnlyBlend);
    if response.changed() {
        if hsva.to_srgba_unmultiplied() != channels {
            *colour = picker_colour(hsva);
        }
        ui.data_mut(|data| data.insert_temp(id, hsva));
    }
    response
}

impl ColourTarget {
    fn for_slot(slot: PaintSlot) -> Self {
        match slot.kind {
            Kind::Fill => Self::Fill,
            Kind::Stroke => Self::Stroke,
        }
    }

    fn primary(self, object: &dtool_core::Object) -> &Paint {
        match self {
            Self::Fill => &object.fill,
            Self::Stroke => &object.stroke.paint,
        }
    }
}

fn focused_slot(workspace: &Workspace, target: ColourTarget) -> Option<Focus> {
    Focus::current(workspace.session()?).filter(|focus| ColourTarget::for_slot(focus.slot) == target)
}

fn solid(paint: &Paint) -> Option<Colour> {
    match paint {
        Paint::Solid { colour } => Some(*colour),
        _ => None,
    }
}

fn draw_swatch(
    ui: &egui::Ui,
    centre: egui::Pos2,
    value: dtool_design::WellValue,
    ring: bool,
    active: bool,
    backing: egui::Color32,
) {
    use dtool_design::WellValue;
    use egui::{Color32, Stroke};
    let painter = ui.painter();
    let rim = if active { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.inactive.fg_stroke.color };
    painter.circle_filled(centre, 9.5, backing);
    let colour = match value {
        WellValue::Colour(colour) => colour,
        WellValue::None => Color32::WHITE,
        WellValue::Mixed => ui.visuals().widgets.inactive.bg_fill,
    };
    if ring {
        painter.circle_stroke(centre, 6.4, Stroke::new(3.5, colour));
    } else {
        painter.circle_filled(centre, 8.0, colour);
    }
    painter.circle_stroke(centre, 9.5, Stroke::new(1.0, rim));
    match value {
        WellValue::None => { painter.line_segment([centre + egui::vec2(-5.0, 5.0), centre + egui::vec2(5.0, -5.0)], Stroke::new(1.5, Color32::from_rgb(233, 65, 65))); }
        WellValue::Mixed => { painter.line_segment([centre + egui::vec2(-3.0, 0.0), centre + egui::vec2(3.0, 0.0)], Stroke::new(1.0, rim)); }
        WellValue::Colour(_) => {}
    }
}

fn ctx(from: &str, to: &str) -> ConvertCtx {
    ConvertCtx { from: from.to_string(), to: to.to_string(), intent: RenderingIntent::RelativeColorimetric, bpc: true }
}

fn to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

impl ColourPanel {
    pub fn selection(workspace: &Workspace) -> Vec<ObjectId> {
        workspace.session().map(|session| session.selection.ids.clone()).unwrap_or_default()
    }

    pub fn sync(&mut self, workspace: &Workspace) -> bool {
        let slot = workspace.active().zip(workspace.session().and_then(Focus::current))
            .map(|(context, focus)| (context, focus.node, focus.slot));
        if self.seen_slot != slot {
            self.seen_slot = slot;
            self.seen = None;
            self.tint = None;
            self.channel_edit = None;
            if let Some((_, _, slot)) = slot {
                self.target = ColourTarget::for_slot(slot);
            }
        }
        let ids = Self::selection(workspace);
        let version = workspace.session().map(|session| session.document.version).unwrap_or(0);
        let key = (workspace.active(), version, ids.clone(), self.target);
        let paint = Self::well_state(workspace, self.target, true);
        let colour = match &paint {
            crate::fill_stroke::WellState::Single(paint) => solid(paint),
            _ => None,
        };
        if self.seen.as_ref() == Some(&key) && colour == self.seen_colour {
            return false;
        }
        self.drag = None;
        if self.seen.as_ref().is_some_and(|old| old.0 != key.0 || old.2 != key.2 || old.3 != key.3) {
            self.tint = None;
            self.channel_edit = None;
        }
        self.seen = Some(key);
        self.seen_colour = colour;
        self.picker.cancel();
        if let Some(colour) = colour {
            self.set_colour(colour);
        } else {
            self.channel_edit = None;
            self.set_colour(Colour::new(0, 0, 0, 255));
            self.hex_input.clear();
        }
        colour.is_some()
    }

    fn brush_colour(session: &dtool_core::Session) -> Option<(dtool_core::brush::BrushOp, &'static str)> {
        use dtool_core::brush::BrushOp;
        match dtool_core::commands::pixel_brush::operation(&session.active_tool)? {
            BrushOp::ColourReplace => Some((BrushOp::ColourReplace, "brush.replacement_colour")),
            BrushOp::Mixer => Some((BrushOp::Mixer, "brush.mixer_colour")),
            BrushOp::Tone => Some((BrushOp::Tone, "brush.tone_colour")),
            _ => None,
        }
    }

    fn well_state(workspace: &Workspace, target: ColourTarget, follow_slot: bool) -> crate::fill_stroke::WellState {
        use crate::fill_stroke::WellState;
        if target == ColourTarget::Fill
            && let Some(session) = workspace.session()
            && let Some((op, _)) = Self::brush_colour(session)
        {
            let [r, g, b, a] = dtool_core::commands::pixel_brush::settings(session, op).colour;
            return WellState::Single(if a == 0 { Paint::None } else { Paint::Solid { colour: Colour::new(r, g, b, a) } });
        }
        let Some(session) = workspace.session().filter(|session| !session.selection.ids.is_empty()) else {
            let defaults = dtool_core::commands::defaults::current();
            return WellState::Single(match target {
                ColourTarget::Fill => defaults.fill,
                ColourTarget::Stroke => defaults.stroke.paint,
            });
        };
        if follow_slot && let Some(focus) = focused_slot(workspace, target)
            && let Some(paint) = session.document.get_object(focus.node).and_then(|object| focus.slot.paint(object))
        {
            return WellState::Single(paint.clone());
        }
        let mut paints = session.selection.ids.iter().filter_map(|id| session.document.get_object(*id))
            .map(|object| target.primary(object));
        let Some(first) = paints.next() else { return WellState::None };
        if paints.all(|paint| paint == first) { WellState::Single(first.clone()) } else { WellState::Mixed }
    }

    pub fn set_target(&mut self, target: ColourTarget) {
        if self.target != target {
            self.tint = None;
            self.channel_edit = None;
        }
        self.target = target;
        self.seen = None;
    }

    fn set_colour(&mut self, colour: Colour) {
        if rgb_channels(colour) != rgb_channels(self.colour) {
            self.channel_edit = None;
        }
        if colour.r != colour.g || colour.g != colour.b {
            self.channel_hue = rgb_to_hsl(f32::from(colour.r) / 255.0, f32::from(colour.g) / 255.0, f32::from(colour.b) / 255.0)[0];
        }
        self.colour = colour;
        self.hex = hex_of(&colour);
        self.hex_input.clone_from(&self.hex);
    }

    pub fn set_hex(&mut self, value: &str) -> Result<(), CommandError> {
        let trimmed = value.trim();
        let digits = trimmed.trim_start_matches('#');
        if !matches!(digits.len(), 3 | 4 | 6 | 8) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(CommandError::invalid_params());
        }
        let colour = paint_from_legacy_str(trimmed).as_ref().and_then(solid).ok_or_else(CommandError::invalid_params)?;
        self.channel_edit = None;
        self.set_colour(colour);
        Ok(())
    }

    pub fn set_alpha(&mut self, alpha: u8) {
        self.set_colour(Colour::new(self.colour.r, self.colour.g, self.colour.b, alpha));
    }

    pub fn model_values(&self) -> Result<Vec<f32>, CommandError> {
        if let Some(edit) = &self.channel_edit
            && edit.model == self.model && edit.rgb == rgb_channels(self.colour)
        {
            return Ok(edit.values.clone());
        }
        let (r, g, b) = (self.colour.r as f32 / 255.0, self.colour.g as f32 / 255.0, self.colour.b as f32 / 255.0);
        match self.model {
            PanelModel::Rgb => Ok(vec![self.colour.r as f32, self.colour.g as f32, self.colour.b as f32]),
            PanelModel::Hsl => {
                let mut hsl = rgb_to_hsl(r, g, b);
                if hsl[1] == 0.0 { hsl[0] = self.channel_hue; }
                Ok(hsl.to_vec())
            },
            model => {
                let (target, profile) = model.managed().ok_or_else(CommandError::invalid_params)?;
                let source = Colour::new(self.colour.r, self.colour.g, self.colour.b, self.colour.a).managed();
                let services = dtool_core::colour::services().read().map_err(|_| CommandError::new("COLOUR_PROFILE_MISSING"))?;
                let out = convert(&source, target, &ctx(ProfileStore::SRGB, profile), &services.profiles).map_err(|_| CommandError::new("COLOUR_PROFILE_MISSING"))?;
                Ok(out.values[..model.ranges().len()].to_vec())
            }
        }
    }

    pub fn set_model_values(&mut self, values: &[f32]) -> Result<(), CommandError> {
        let ranges = self.model.ranges();
        if values.len() != ranges.len() || values.iter().zip(ranges).any(|(value, (lo, hi))| !value.is_finite() || value < lo || value > hi) {
            return Err(CommandError::invalid_params());
        }
        let colour = self.colour_for_values(values)?;
        if self.model == PanelModel::Hsl { self.channel_hue = values[0]; }
        self.set_colour(colour);
        // Editing another channel should start from the entered values, not a round trip through RGB.
        if self.model != PanelModel::Rgb {
            self.channel_edit = Some(ChannelEdit { model: self.model, rgb: rgb_channels(colour), values: values.to_vec() });
        }
        Ok(())
    }

    fn colour_for_values(&self, values: &[f32]) -> Result<Colour, CommandError> {
        let alpha = self.colour.a;
        let rgb = match self.model {
            PanelModel::Rgb => [values[0] / 255.0, values[1] / 255.0, values[2] / 255.0],
            PanelModel::Hsl => hsl_to_rgb(values[0], values[1], values[2]),
            model => {
                let (source_model, profile) = model.managed().ok_or_else(CommandError::invalid_params)?;
                let mut padded = [0.0; 4];
                padded[..values.len()].copy_from_slice(values);
                let source = dtool_core::colour::Colour { model: source_model, values: padded, alpha: alpha as f32 / 255.0 };
                let services = dtool_core::colour::services().read().map_err(|_| CommandError::new("COLOUR_PROFILE_MISSING"))?;
                let out = convert(&source, ColourModel::Rgb, &ctx(profile, ProfileStore::SRGB), &services.profiles).map_err(|_| CommandError::new("COLOUR_PROFILE_MISSING"))?;
                [out.values[0], out.values[1], out.values[2]]
            }
        };
        Ok(Colour::new(to_u8(rgb[0]), to_u8(rgb[1]), to_u8(rgb[2]), alpha))
    }

    pub fn apply_command(&self, workspace: &Workspace) -> Option<Command> {
        let session = workspace.session()?;
        if self.target == ColourTarget::Fill && let Some((op, id)) = Self::brush_colour(session) {
            let Colour { r, g, b, a, .. } = self.colour;
            let value = if op != dtool_core::brush::BrushOp::ColourReplace {
                format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
            } else { format!("#{r:02X}{g:02X}{b:02X}") };
            return Some(Command::with_params(crate::context_bar::OPTION_SET, json!({
                "tool": session.active_tool, "id": id, "value": value,
            })));
        }
        let paint = Paint::Solid { colour: self.colour };
        if let Some(focus) = focused_slot(workspace, self.target) {
            let current = focus.slot.paint(session.document.get_object(focus.node)?)?;
            return (*current != paint).then(|| focus.slot.paint_command(focus.node, paint));
        }
        let mut updates = Vec::new();
        for id in &session.selection.ids {
            let object = session.document.get_object(*id)?;
            match self.target {
                ColourTarget::Fill if object.fill != paint => updates.push(json!({ "id": id.0, "fill": paint })),
                ColourTarget::Stroke if object.stroke.paint != paint => {
                    let mut stroke = object.stroke.clone();
                    stroke.paint = paint.clone();
                    updates.push(json!({ "id": id.0, "stroke": stroke }));
                }
                _ => {}
            }
        }
        match updates.len() {
            0 => None,
            1 => Some(Command::with_params("object.update", updates.remove(0))),
            _ => Some(Command::with_params("edit.batch", json!({ "commands": updates.into_iter().map(|params| json!({ "name": "object.update", "params": params })).collect::<Vec<_>>() }))),
        }
    }

    pub fn apply(&mut self, workspace: &mut Workspace) -> bool {
        let Some(command) = self.apply_command(workspace) else { return false };
        if self.run(workspace, command).is_none() {
            return false;
        }
        if let Err(error) = dtool_core::palette::push_recent(self.colour) {
            self.error = Some(error.message_key);
        }
        true
    }

    pub fn contrast_command(&self, workspace: &Workspace) -> Option<Command> {
        let session = workspace.session()?;
        let first = session.document.get_object(*session.selection.ids.first()?)?;
        let other = match self.target {
            ColourTarget::Fill => ColourTarget::Stroke,
            ColourTarget::Stroke => ColourTarget::Fill,
        };
        let other = focused_slot(workspace, other)
            .and_then(|focus| focus.slot.paint(first))
            .unwrap_or_else(|| other.primary(first));
        let other = solid(other)?;
        Some(Command::with_params("color.contrast", json!({ "a": hex_of(&self.colour), "b": hex_of(&other) })))
    }

    pub fn proof_command(&self) -> Command {
        let intent = PROOF_INTENTS.iter().find(|(intent, _, _)| *intent == self.proof.intent).map(|(_, name, _)| *name).unwrap_or("relative_colorimetric");
        Command::with_params("ui.soft_proof.set", json!({ "enabled": self.proof.enabled, "profile": if self.proof.enabled { self.proof.profile.clone() } else { String::new() }, "intent": intent, "bpc": self.proof.bpc, "gamut_check": self.proof.gamut_check }))
    }

    pub fn proof_profiles() -> Vec<String> {
        let services = match dtool_core::colour::services().read() {
            Ok(services) => services,
            Err(_) => return Vec::new(),
        };
        let mut names: Vec<String> = [ColourModel::Cmyk, ColourModel::Rgb, ColourModel::Grey].into_iter().flat_map(|model| services.profiles.list(model).into_iter().map(String::from).collect::<Vec<_>>()).collect();
        names.dedup();
        names
    }

    pub fn run(&mut self, workspace: &mut Workspace, command: Command) -> Option<Value> {
        let (_, result) = workspace.run(Scope::active(), command);
        match result {
            Ok(value) => {
                self.error = None;
                Some(value)
            }
            Err(error) => {
                self.error = Some(error.message_key);
                None
            }
        }
    }

    pub fn recent(&mut self, workspace: &mut Workspace) -> Vec<Colour> {
        let Ok(reply) = workspace.run(Scope::active(), Command::new("color.recent.list")).1 else { return Vec::new() };
        reply["recent"].as_array().map(|items| items.iter().filter_map(|item| item["hex"].as_str().and_then(paint_from_legacy_str)).filter_map(|paint| solid(&paint)).collect()).unwrap_or_default()
    }

    fn apply_paint(&mut self, workspace: &mut Workspace, paint: Paint) {
        if self.target == ColourTarget::Fill
            && let Some((op, _)) = workspace.session().and_then(Self::brush_colour)
        {
            if matches!(paint, Paint::Solid { .. }) {
                self.apply(workspace);
            } else if op != dtool_core::brush::BrushOp::ColourReplace {
                self.set_colour(Colour::new(0, 0, 0, 0));
                self.apply(workspace);
            }
            return;
        }
        let has_selection = !Self::selection(workspace).is_empty();
        match (has_selection, paint) {
            (true, Paint::Solid { .. }) => { self.apply(workspace); }
            (true, Paint::None) => {
                let role = match self.target { ColourTarget::Fill => "fill", ColourTarget::Stroke => "stroke" };
                if let Some(focus) = focused_slot(workspace, self.target) {
                    self.run(workspace, focus.slot.paint_command(focus.node, Paint::None));
                } else {
                    self.run(workspace, Command::with_params(crate::fill_stroke::PAINT_NONE, json!({"role":role})));
                }
            }
            (false, paint) => {
                let (fill, stroke) = match self.target {
                    ColourTarget::Fill => (Some(paint), None),
                    ColourTarget::Stroke => (None, Some(paint)),
                };
                dtool_core::commands::defaults::set_paints(fill, stroke);
            }
            _ => {}
        }
    }

    fn overlapping_swatches(
        &self,
        ui: &mut egui::Ui,
        workspace: &Workspace,
        rail: bool,
    ) -> [(ColourTarget, egui::Response); 2] {
        let size = if rail { egui::vec2(30.0, TOOL_SWATCH_HEIGHT) } else { egui::vec2(33.0, 29.0) };
        let (id, rect) = ui.allocate_space(size);
        let (offset, rear_hit, front_hit) = if rail {
            let split = rect.top() + 14.0;
            (
                egui::vec2(7.0, 12.0),
                egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), split)),
                egui::Rect::from_min_max(egui::pos2(rect.left(), split), rect.max),
            )
        } else {
            let split = rect.left() + 15.0;
            (
                egui::vec2(11.0, 7.0),
                egui::Rect::from_min_max(rect.min, egui::pos2(split, rect.bottom())),
                egui::Rect::from_min_max(egui::pos2(split, rect.top()), rect.max),
            )
        };
        let rear = rect.min + egui::vec2(10.5, 10.5);
        let backing = if rail { dtool_design::theme::surface(ui.ctx(), "bars") } else { ui.visuals().panel_fill };
        let (rear_target, rear_key, front_key) = match self.target {
            ColourTarget::Fill => (ColourTarget::Stroke, "stroke.well", "fill.well"),
            ColourTarget::Stroke => (ColourTarget::Fill, "fill.well", "stroke.well"),
        };
        [
            (rear_target, rear_key, rear, rear_hit),
            (self.target, front_key, rear + offset, front_hit),
        ].map(|(target, key, centre, hit)| {
            let response = ui.interact(hit, id.with(key), egui::Sense::click());
            let active = self.target == target;
            let label = text(key);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), active, label));
            let state = Self::well_state(workspace, target, !rail);
            draw_swatch(ui, centre, crate::fill_stroke::well_value(&state), target == ColourTarget::Stroke,
                active || response.has_focus(), backing);
            let tip = match &state {
                crate::fill_stroke::WellState::Single(paint) => solid(paint).map(|colour| format!("{label} {}", hex_of(&colour))),
                _ => None,
            }.unwrap_or_else(|| label.to_string());
            (target, response.on_hover_text(tip))
        })
    }

    fn paint_targets(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace, editable: bool) -> (Option<ColourTarget>, bool, bool) {
        let next_target = self.overlapping_swatches(ui, workspace, false).into_iter()
            .find_map(|(target, response)| response.clicked().then_some(target));
        let (swap, clear) = ui.add_enabled_ui(editable, |ui| {
            let swap = self.icons.studio_button(ui, "colour-swap", text("paint.swap"), false).clicked();
            let clear = self.icons.studio_button(ui, "colour-no-paint", text("colour.panel.no_paint"), false).clicked();
            (swap, clear)
        }).inner;
        self.sample_controls(ui, workspace, editable);
        (next_target, swap, clear)
    }

    fn sample_controls(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace, editable: bool) {
        let Some(session) = workspace.session_mut().filter(|session| session.has_document) else { return };
        let sampling = session.panel_colour_pick_previous.is_some();
        let button = self.icons.studio_button(ui, "tool-colour-picker", text("tool.eyedropper"), sampling);
        if button.clicked() {
            button.surrender_focus();
            if let Some(previous) = session.panel_colour_pick_previous.take() {
                session.active_tool = previous;
            } else {
                crate::canvas::frame_text_input::commit_pending(session);
                session.panel_colour_pick_previous = Some(std::mem::replace(&mut session.active_tool, "tool.eyedropper".into()));
            }
        }
        let sample = session.panel_sampled_colour;
        let label = text("colour.panel.apply_sample");
        let response = ui.add_enabled_ui(editable && sample.is_some(), |ui| {
            let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(22.0), egui::Sense::click());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
            let value = sample.map_or(dtool_design::WellValue::Mixed, |colour| {
                let colour = dtool_core::paint::display_colour(&colour, None, &dtool_core::paint::DisplayColourCtx::for_optional(Some(&session.document)));
                dtool_design::WellValue::Colour(egui::Color32::from_rgba_unmultiplied(colour.r, colour.g, colour.b, colour.a))
            });
            draw_swatch(ui, rect.center(), value, false, response.has_focus(), ui.visuals().panel_fill);
            response
        }).inner.on_hover_text(sample.map_or_else(|| label.to_string(), |colour| format!("{label} {}", hex_of(&colour))));
        if response.clicked() && let Some(colour) = sample {
            self.apply_swatch_colour(workspace, colour);
        }
    }

    pub(crate) fn swatch_targets(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace, editable: bool) {
        let (target, swap, clear) = self.paint_targets(ui, workspace, editable);
        if let Some(target) = target {
            self.set_target(target);
        }
        if swap {
            self.swap_panel_paints(workspace);
        }
        if clear {
            self.apply_paint(workspace, Paint::None);
        }
    }

    pub(crate) fn apply_swatch_colour(&mut self, workspace: &mut Workspace, colour: Colour) {
        self.set_colour(colour);
        self.apply_paint(workspace, Paint::Solid { colour });
    }

    pub(crate) fn swatch_opacity(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace, editable: bool) {
        self.sync(workspace);
        let has_solid_colour = self.seen_colour.is_some();
        let mut percent = (f32::from(self.colour.a) * 100.0 / 255.0).round() as u8;
        let label = text("context.brush.opacity");
        let response = ui.add_enabled(
            editable && has_solid_colour,
            egui::DragValue::new(&mut percent).range(0..=100).suffix(" %"),
        ).on_hover_text(label);
        if response.changed() {
            let alpha = (f32::from(percent) * 255.0 / 100.0).round() as u8;
            self.set_colour(Colour { a: alpha, ..self.colour });
            self.apply_paint(workspace, Paint::Solid { colour: self.colour });
        }
    }

    pub(crate) fn solid_swatch(workspace: &Workspace, target: ColourTarget) -> Option<Colour> {
        match Self::well_state(workspace, target, true) {
            crate::fill_stroke::WellState::Single(paint) => solid(&paint),
            _ => None,
        }
    }

    pub(crate) fn tool_swatches(&mut self, ui: &mut egui::Ui, workspace: &Workspace) -> Option<ToolColourAction> {
        let selected = Self::selection(workspace);
        let editable = workspace.session().is_some_and(|session| {
            selected.iter().all(|id| session.document.get_object(*id).is_some_and(|object| !object.locked))
        });
        let mut picked = None;
        for (target, response) in self.overlapping_swatches(ui, workspace, true) {
            if response.clicked() {
                picked = Some(ToolColourAction::Open(target));
            }
            egui::Popup::context_menu(&response).show(|ui| {
                ui.add_enabled_ui(editable, |ui| {
                    if !selected.is_empty() && ui.button(text("colour.tool.edit_paint")).clicked() {
                        picked = Some(ToolColourAction::Edit(target));
                        ui.close();
                    }
                    if ui.button(text("colour.panel.no_paint")).clicked() {
                        picked = Some(ToolColourAction::Clear(target));
                        ui.close();
                    }
                    if ui.button(text("paint.default")).clicked() {
                        picked = Some(ToolColourAction::Defaults);
                        ui.close();
                    }
                });
            });
        }
        if ui.add_enabled_ui(editable, |ui| {
            self.icons.studio_button(ui, "colour-swap", text("paint.swap"), false).clicked()
        }).inner {
            picked = Some(ToolColourAction::Swap);
        }
        picked
    }

    pub(crate) fn swap_paints(&mut self, workspace: &mut Workspace) {
        if Self::selection(workspace).is_empty() {
            let defaults = dtool_core::commands::defaults::current();
            dtool_core::commands::defaults::set_paints(Some(defaults.stroke.paint), Some(defaults.fill));
        } else {
            self.run(workspace, Command::with_params(crate::fill_stroke::PAINT_SWAP, json!({})));
        }
    }

    fn swap_panel_paints(&mut self, workspace: &mut Workspace) {
        let active = workspace.session().and_then(|session| {
            let focus = Focus::current(session).filter(|focus| focus.slot.index > 0)?;
            let object = session.document.get_object(focus.node)?;
            Some((focus, object, focus.slot.paint(object)?))
        });
        let Some((focus, object, paint)) = active else {
            self.swap_paints(workspace);
            return;
        };
        let (other, change) = match focus.slot.kind {
            Kind::Fill => {
                let mut stroke = object.stroke.clone();
                stroke.paint = paint.clone();
                (&object.stroke.paint, json!({"id": object.id, "stroke": stroke}))
            }
            Kind::Stroke => (&object.fill, json!({"id": object.id, "fill": paint})),
        };
        if paint == other {
            return;
        }
        let commands = [
            Command::with_params("object.update", change),
            focus.slot.paint_command(focus.node, other.clone()),
        ];
        let swap = Command::with_params("edit.batch", json!({"commands": commands}));
        if self.run(workspace, swap).is_some() {
            self.run(workspace, focus.slot.focus_command(focus.node));
        }
    }

    pub(crate) fn clear_paint(&mut self, workspace: &mut Workspace, target: ColourTarget) {
        self.set_target(target);
        if Self::selection(workspace).is_empty() {
            self.apply_paint(workspace, Paint::None);
        } else {
            let role = match target { ColourTarget::Fill => "fill", ColourTarget::Stroke => "stroke" };
            self.run(workspace, Command::with_params(crate::fill_stroke::PAINT_NONE, json!({"role": role})));
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        self.sync(workspace);
        let pass = ui.ctx().cumulative_pass_nr();
        let (held, escape) = ui.input(|input| (input.pointer.primary_down(), input.key_pressed(egui::Key::Escape)));
        if self.drag.as_ref().is_some_and(|drag| pass.saturating_sub(drag.pass) > 1)
            && let Some(drag) = self.drag.take()
        {
            self.restore_drag(&drag);
        }
        let recent = if workspace.session().is_some() { self.recent(workspace) } else { Vec::new() };
        let editable = workspace.session().is_none_or(|session| session.selection.ids.iter().all(|id| {
            session.document.get_object(*id).is_some_and(|object| !object.locked)
        }));
        let preview_selection = workspace.session().is_some_and(|session| {
            !session.selection.ids.is_empty()
                && (self.target != ColourTarget::Fill || Self::brush_colour(session).is_none())
        });
        let baseline = (preview_selection && held && self.drag.is_none()).then(|| ColourDrag {
            colour: self.colour,
            channel_edit: self.channel_edit.clone(),
            hue: self.channel_hue,
            tint: self.tint.clone(),
            pass,
            cancelled: false,
        });
        let mut paint_action = (None, false, false);
        let mut changed = false;
        let mut proof = None;
        panel_frame(ui, |ui| {
            panel_header(ui, text("panel.colour"));
            // The dock body scrolls, so its clip height is the visible space
            // available to the controls regardless of the scroll offset.
            let dock_height = dtool_design::in_studio(ui).then(|| ui.clip_rect().height());
            let width = ui.available_width().min(ui.clip_rect().width());
            let compact = dock_height.is_some_and(|height| height < 160.0);
            let inline_opacity = dock_height.is_some_and(|height| height < 225.0) && width >= 230.0;
            if dock_height.is_some_and(|height| height < 225.0) {
                ui.spacing_mut().item_spacing.y = 2.0;
            }
            let picker_height = if inline_opacity {
                (dock_height.unwrap_or_default() - 60.0).clamp(18.0, 40.0)
            } else {
                dock_height.map_or(96.0, |height| (height - 108.0).clamp(30.0, 114.0))
            };
            if inline_opacity {
                paint_action = ui.horizontal(|ui| {
                    let paint = self.paint_targets(ui, workspace, editable);
                    changed |= ui.add_enabled_ui(editable, |ui| self.edit_opacity(ui, true)).inner;
                    paint
                }).inner;
            } else {
                paint_action = ui.horizontal_wrapped(|ui| self.paint_targets(ui, workspace, editable)).inner;
            }
            let inline_model = width >= 140.0;
            let inline_hex = width >= 254.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().combo_width = 78.0_f32.min(width);
                let labels = DISPLAY_MODES.map(|(_, key)| text(key));
                let mut index = DISPLAY_MODES.iter().position(|(mode, _)| *mode == self.display).unwrap_or(1);
                if dropdown(ui, "colour_display_mode", &mut index, &labels).changed() {
                    self.display = DISPLAY_MODES[index].0;
                    self.picker.cancel();
                }
                if inline_model && self.display == DisplayMode::Sliders {
                    self.choose_model(ui);
                }
                if inline_hex {
                    changed |= ui.add_enabled_ui(editable, |ui| self.edit_hex(ui)).inner;
                }
            });
            if !inline_model && self.display == DisplayMode::Sliders {
                self.choose_model(ui);
            }
            if !inline_hex {
                changed |= ui.add_enabled_ui(editable, |ui| self.edit_hex(ui)).inner;
            }
            changed |= ui.add_enabled_ui(editable, |ui| {
                self.edit_colour(ui, &recent, picker_height, compact, !inline_opacity)
            }).inner;
            if let Some(ratio) = self.contrast {
                let formatted = format!("{ratio:.1}");
                ui.horizontal(|ui| {
                    ui.label(dtool_strings::format("colour.contrast.ratio", &[("ratio", &formatted)]));
                    if ratio >= 4.5 {
                        ui.label(text("colour.contrast.aa"));
                    }
                    if ratio >= 7.0 {
                        ui.label(text("colour.contrast.aaa"));
                    }
                });
            }
            ui.separator();
            proof = self.show_soft_proof(ui);
            if let Some(error) = &self.error {
                ui.label(text(error));
            }
        });
        if let Some(command) = proof {
            self.run(workspace, command);
        }
        if changed && self.drag.is_none() {
            self.drag = baseline;
        }
        if let Some(mut drag) = self.drag.take() {
            drag.pass = pass;
            drag.cancelled |= escape || !editable || !preview_selection;
            if drag.cancelled {
                self.restore_drag(&drag);
            } else if held {
                if let Some(session) = workspace.session()
                    && let Some(command) = self.apply_command(workspace)
                {
                    crate::app_canvas::stage_geometry_preview(ui, session, command);
                }
            } else {
                self.apply_paint(workspace, Paint::Solid { colour: self.colour });
            }
            if held { self.drag = Some(drag); }
        } else if changed {
            self.apply_paint(workspace, Paint::Solid { colour: self.colour });
        }
        let (next_target, swap, clear) = paint_action;
        if swap {
            self.swap_panel_paints(workspace);
        }
        if clear { self.apply_paint(workspace, Paint::None); }
        if let Some(target) = next_target { self.set_target(target); }
        self.contrast = self.contrast_command(workspace)
            .and_then(|command| workspace.run(Scope::active(), command).1.ok())
            .and_then(|reply| reply["ratio"].as_f64());
    }

    fn restore_drag(&mut self, drag: &ColourDrag) {
        self.set_colour(drag.colour);
        self.channel_edit = drag.channel_edit.clone();
        self.channel_hue = drag.hue;
        self.tint = drag.tint.clone();
        self.picker.cancel();
    }

    fn edit_colour(&mut self, ui: &mut egui::Ui, recent: &[Colour], picker_height: f32, compact: bool, show_opacity: bool) -> bool {
        let mut changed = match self.display {
            DisplayMode::Sliders => self.edit_channels(ui),
            DisplayMode::Tint => self.edit_tint(ui),
            DisplayMode::Wheel | DisplayMode::Boxes => {
                let colour = if self.display == DisplayMode::Wheel {
                    self.picker.show_wheel(ui, self.colour, if compact { 84.0 } else { 164.0 })
                } else {
                    self.picker.show(ui, self.colour, picker_height)
                };
                if let Some(colour) = colour {
                    self.set_colour(colour);
                }
                colour.is_some()
            }
        };
        if show_opacity {
            changed |= self.edit_opacity(ui, false);
        }
        if !recent.is_empty() {
            ui.label(text("glyphs.subset.recent"));
            ui.horizontal_wrapped(|ui| {
                for (index, colour) in recent.iter().enumerate() {
                    let paint = Paint::Solid { colour: *colour };
                    if crate::fill_stroke::paint_well(ui, &paint, &format!("colour_recent_{index}")).clicked() {
                        self.set_colour(*colour);
                        changed = true;
                    }
                }
            });
        }
        changed
    }

    fn edit_tint(&mut self, ui: &mut egui::Ui) -> bool {
        let current = self.colour;
        let current_rgb = [current.r, current.g, current.b];
        if self.tint.as_ref().is_none_or(|tint| [tint.result.r, tint.result.g, tint.result.b] != current_rgb) {
            self.tint = Some(Tint { base: current, result: current, amount: 100.0 });
        }
        let Some(tint) = &mut self.tint else { return false };
        let colours = [egui::Color32::WHITE, egui::Color32::from_rgb(tint.base.r, tint.base.g, tint.base.b)];
        let changed = channel_slider(ui, "T", "colour.tint", &mut tint.amount, 0.0..=100.0, &colours, Some(100.0));
        if changed {
            let component = |value: u8| egui::lerp(255.0..=f32::from(value), tint.amount / 100.0).round() as u8;
            let result = Colour::new(component(tint.base.r), component(tint.base.g), component(tint.base.b), current.a);
            tint.result = result;
            self.set_colour(result);
        }
        changed
    }

    fn edit_opacity(&mut self, ui: &mut egui::Ui, inline: bool) -> bool {
        let mut alpha = f32::from(self.colour.a) / 255.0 * 100.0;
        let opacity_label = format!("{} %", text("context.brush.opacity"));
        let (changed, reset) = if inline {
            let label = ui.add(dtool_design::numeric_caption("%")).on_hover_text(&opacity_label);
            let response = dtool_design::slider(ui, &mut alpha, 0.0..=100.0, "")
                .labelled_by(label.id).on_hover_text(&opacity_label);
            (response.changed(), label.double_clicked())
        } else {
            let label = ui.add(dtool_design::numeric_caption(&opacity_label));
            let response = ui.horizontal(|ui| dtool_design::slider(ui, &mut alpha, 0.0..=100.0, "")).inner;
            (response.labelled_by(label.id).changed(), label.double_clicked())
        };
        if reset {
            self.set_alpha(255);
        } else if changed {
            self.set_alpha((alpha / 100.0 * 255.0).round() as u8);
        }
        changed || reset
    }

    fn edit_hex(&mut self, ui: &mut egui::Ui) -> bool {
        ui.horizontal(|ui| {
            let label = ui.label("HEX");
            let width = ui.available_width().max(0.0).min(82.0);
            let response = ui.add(egui::TextEdit::singleline(&mut self.hex_input)
                .desired_width(width).hint_text(dtool_design::MIXED_TEXT)).labelled_by(label.id);
            if !response.lost_focus() || self.hex_input.is_empty() || self.hex_input == self.hex {
                return false;
            }
            match self.set_hex(&self.hex_input.clone()) {
                Ok(()) => { self.error = None; true }
                Err(error) => { self.error = Some(error.message_key); false }
            }
        }).inner
    }

    fn choose_model(&mut self, ui: &mut egui::Ui) {
        ui.scope(|ui| {
            ui.spacing_mut().combo_width = ui.available_width().max(0.0).min(50.0);
            let labels = PANEL_MODELS.map(PanelModel::label);
            let mut index = PANEL_MODELS.iter().position(|model| *model == self.model).unwrap_or(0);
            let label = text("doc_setup.model");
            let response = dropdown(ui, "colour_panel_model", &mut index, &labels).on_hover_text(label);
            if response.changed() {
                self.model = PANEL_MODELS[index];
                self.channel_edit = None;
            }
        });
    }

    pub(crate) fn context_editor() -> Self {
        Self { model: PanelModel::Rgb, ..Self::default() }
    }

    pub(crate) fn edit_context_colour(&mut self, ui: &mut egui::Ui, colour: &mut Colour) -> bool {
        if self.colour != *colour {
            self.set_colour(*colour);
        }
        ui.horizontal(|ui| {
            self.choose_model(ui);
            self.edit_hex(ui);
        });
        self.edit_channels(ui);
        self.edit_opacity(ui, false);
        if self.colour == *colour { return false; }
        *colour = self.colour;
        true
    }

    fn edit_channels(&mut self, ui: &mut egui::Ui) -> bool {
        match self.model_values() {
            Ok(mut values) => {
                let mut edited = false;
                for (channel, (&(lo, hi), name)) in self.model.ranges().iter().zip(self.model.channel_labels()).enumerate() {
                    let mut sample = values.clone();
                    let steps = if self.model == PanelModel::Hsl && channel == 0 {
                        sample[1] = 100.0;
                        sample[2] = 50.0;
                        6
                    } else if self.model == PanelModel::Hsl && channel == 2 { 2 } else { 1 };
                    let colours: Vec<_> = (0..=steps).map(|step| {
                        sample[channel] = egui::lerp(lo..=hi, step as f32 / steps as f32);
                        let colour = self.colour_for_values(&sample).unwrap_or(self.colour);
                        egui::Color32::from_rgb(colour.r, colour.g, colour.b)
                    }).collect();
                    edited |= ui.push_id((self.model.label(), channel), |ui| {
                        channel_slider(ui, name, "colour.channel", &mut values[channel], lo..=hi, &colours, None)
                    }).inner;
                }
                let applied = edited && self.set_model_values(&values).is_ok();
                let size = egui::vec2(ui.available_width(), 14.0);
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let colour = self.colour;
                ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(colour.r, colour.g, colour.b));
                applied
            }
            Err(error) => {
                ui.label(text(&error.message_key));
                false
            }
        }
    }

    fn show_soft_proof(&mut self, ui: &mut egui::Ui) -> Option<Command> {
        egui::CollapsingHeader::new(text("adjust.soft_proof")).id_salt("colour.soft_proof").show(ui, |ui| {
            let before = self.proof.clone();
            toggle(ui, &mut self.proof.enabled, text("adjust.soft_proof"));
            let profiles = Self::proof_profiles();
            if !profiles.is_empty() {
                let labels: Vec<&str> = profiles.iter().map(String::as_str).collect();
                let mut index = profiles.iter().position(|name| *name == self.proof.profile).unwrap_or(0);
                ui.horizontal(|ui| {
                    ui.label(text("adjust.soft_proof.profile"));
                    if dropdown(ui, "colour_panel_proof_profile", &mut index, &labels).changed() || self.proof.profile.is_empty() {
                        self.proof.profile = profiles[index].clone();
                    }
                });
            }
            let intents: Vec<&str> = PROOF_INTENTS.iter().map(|(_, _, key)| text(key)).collect();
            let mut intent = PROOF_INTENTS.iter().position(|(item, _, _)| *item == self.proof.intent).unwrap_or(1);
            ui.horizontal(|ui| {
                ui.label(text("adjust.soft_proof.intent"));
                if dropdown(ui, "colour_panel_proof_intent", &mut intent, &intents).changed() {
                    self.proof.intent = PROOF_INTENTS[intent].0;
                }
            });
            toggle(ui, &mut self.proof.bpc, text("adjust.soft_proof.bpc"));
            toggle(ui, &mut self.proof.gamut_check, text("adjust.soft_proof.gamut"));
            (self.proof != before).then(|| self.proof_command())
        }).body_returned.flatten()
    }
}

fn channel_slider(
    ui: &mut egui::Ui,
    name: &str,
    id: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    colours: &[egui::Color32],
    reset: Option<f32>,
) -> bool {
    ui.horizontal(|ui| {
        let sense = if reset.is_some() { egui::Sense::click() } else { egui::Sense::hover() };
        let label = ui.add_sized([16.0, ui.spacing().interact_size.y], dtool_design::numeric_caption(name).sense(sense));
        let available = ui.available_width().min(ui.clip_rect().right() - ui.cursor().min.x).max(0.0);
        let numeric_width = available.min(if available < 100.0 { 44.0 } else { 48.0 });
        let slider_width = available - numeric_width - ui.spacing().item_spacing.x;
        let mut changed = false;
        if slider_width >= 22.0 {
            ui.spacing_mut().slider_width = slider_width;
            ui.visuals_mut().widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
            let painter = ui.painter().clone();
            let rail = painter.add(egui::Shape::Noop);
            let slider = ui.add(egui::Slider::new(value, range.clone()).show_value(false).trailing_fill(false)
                .handle_shape(egui::style::HandleShape::Circle)).labelled_by(label.id);
            let track = egui::Rect::from_center_size(slider.rect.center(), egui::vec2(slider.rect.width() - 10.0, 8.0));
            let steps = colours.len() - 1;
            let mut mesh = egui::Mesh::default();
            for (i, pair) in colours.windows(2).enumerate() {
                let piece = egui::Rect::from_min_max(
                    egui::pos2(egui::lerp(track.x_range(), i as f32 / steps as f32), track.top()),
                    egui::pos2(egui::lerp(track.x_range(), (i + 1) as f32 / steps as f32), track.bottom()),
                );
                picker::gradient(&mut mesh, piece, [pair[0], pair[1], pair[1], pair[0]]);
            }
            painter.set(rail, egui::Shape::mesh(mesh));
            changed = slider.changed();
        }
        let spec = NumericSpec::new("").range(f64::from(*range.start())..=f64::from(*range.end()))
            .width(numeric_width).decimals(1).enabled(ui.is_enabled());
        let (field, event) = dtool_design::numeric_field(
            ui, ui.make_persistent_id(id), NumericValue::Value(f64::from(*value)), &spec,
        );
        field.labelled_by(label.id);
        if let Some(reset) = reset.filter(|_| label.double_clicked()) {
            *value = reset;
            changed = true;
        } else if let NumericEvent::Preview(number) | NumericEvent::Commit(number) = event {
            *value = number as f32;
            changed = true;
        }
        changed
    }).inner
}
