use dtool_design::{color, colour_well, dropdown, toggle};
use dtool_strings::text;
use crate::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FillType { Solid, Context }

impl Default for FillType {
    fn default() -> Self { Self::Solid }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FillToolOptions {
    pub context_fill: bool,
    pub fill_type: FillType,
    pub colour: [u8; 4],
    pub pigment_blend: f32,
}

impl Default for FillToolOptions {
    fn default() -> Self {
        Self { context_fill: false, fill_type: FillType::Solid, colour: color("text").to_array(), pigment_blend: 1.0 }
    }
}

pub(crate) fn colour_editor(ui: &mut egui::Ui, rgba: &mut [u8; 4], id: &str) -> bool {
    let before = *rgba;
    let fill = egui::Rgba::from_srgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]).into();
    let response = colour_well(ui, fill, id);
    egui::Popup::menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        for (index, key) in ["colour.channel_red", "colour.channel_green", "colour.channel_blue", "colour.channel_alpha"].iter().enumerate() {
            let mut channel = f64::from(rgba[index]);
            dtool_design::number_field(ui, &mut channel, &text(key));
            if channel.is_finite() { rgba[index] = channel.round().clamp(0.0, 255.0) as u8; }
        }
    });
    before != *rgba
}

pub struct FillToolBar {
    pub options: FillToolOptions,
}

impl Default for FillToolBar {
    fn default() -> Self { Self { options: FillToolOptions::default() } }
}

impl FillToolBar {
    pub fn show(&mut self, ui: &mut egui::Ui, ws: &mut Workspace) {
        if let Some(session) = ws.core.session_mut() {
            self.show_session(ui, session);
        }
    }

    pub fn sync_from(&mut self, session: &dtool_core::Session) {
        let stored = dtool_core::fill_tool::FillToolOptions::from_store(&session.tool_options);
        self.options.context_fill = stored.context_fill;
        self.options.fill_type = if stored.fill_type == dtool_core::fill_tool::FillType::Context { FillType::Context } else { FillType::Solid };
        self.options.pigment_blend = stored.pigment_blend;
    }

    pub fn show_session(&mut self, ui: &mut egui::Ui, session: &mut dtool_core::Session) {
        self.sync_from(session);
        let mut changed = toggle(ui, &mut self.options.context_fill, text("fill_tool.context_fill")).changed();
        ui.label(text("fill_tool.type"));
        let mut selected = if self.options.fill_type == FillType::Context { 1 } else { 0 };
        let solid = text("fill_tool.type_solid");
        let context = text("fill_tool.type_context");
        if dropdown(ui, "fill-tool-type", &mut selected, &[solid, context]).changed() {
            self.options.fill_type = if selected == 1 { FillType::Context } else { FillType::Solid };
            changed = true;
        }
        changed |= colour_editor(ui, &mut self.options.colour, "fill-tool-colour");
        changed |= dtool_design::slider(ui, &mut self.options.pigment_blend, 0.0..=1.0, &text("fill_tool.pigment_blend")).changed();
        if changed {
            let _ = session.run_named("ui.tool.options.set", serde_json::json!({"tool": "fill", "options": self.options}));
        }
    }
}
