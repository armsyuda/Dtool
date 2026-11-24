use crate::icon_cache::IconCache;
use dtool_design::ControlState;
use dtool_protocol::Command;
use dtool_strings::{has, text};
use serde_json::json;

pub const ALIGN_POPUP_COMMAND: &str = "layer.arrange.align";
pub(crate) const ALIGN_LAUNCHER_POSITION: &str = "dtool.align.launcher_position";
fn label(key: &'static str, fallback: &'static str) -> &'static str {
    if has(key) {
        text(key)
    } else {
        text(fallback)
    }
}

pub const ALIGN_EDGES: [&str; 6] = ["left", "centre", "right", "top", "middle", "bottom"];

pub(super) fn action_icon(command: &str) -> Option<&'static str> {
    Some(match command {
        ALIGN_POPUP_COMMAND => "cmd-align",
        "layer.arrange.align.left" => "tool-align-left",
        "layer.arrange.align.centre" => "tool-align-center-h",
        "layer.arrange.align.right" => "tool-align-right",
        "layer.arrange.align.top" => "tool-align-top",
        "layer.arrange.align.middle" => "tool-align-center-v",
        "layer.arrange.align.bottom" => "tool-align-bottom",
        _ => return None,
    })
}

pub(super) fn icon_action(
    ui: &mut egui::Ui,
    icons: &mut IconCache,
    icon: &str,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let state = ControlState::enabled(enabled).tooltip(label);
    let Some(texture) = icons.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui)) else {
        return dtool_design::button_with(ui, label, &state);
    };
    let response = dtool_design::icon_image_button(ui, &texture, &state);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled && ui.is_enabled(), label));
    response
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignTarget {
    #[default]
    Selection,
    Spread,
    Key,
}

impl AlignTarget {
    pub fn param(self) -> &'static str {
        match self {
            AlignTarget::Selection => "selection",
            AlignTarget::Spread => "spread",
            AlignTarget::Key => "key",
        }
    }

    pub fn label(self) -> &'static str {
        let fallback = match self {
            AlignTarget::Selection => "import.cad.selection",
            AlignTarget::Spread => "hyperlink.page",
            AlignTarget::Key => "align.popup.target.key",
        };
        label(self.label_key(), fallback)
    }

    pub fn label_key(self) -> &'static str {
        match self {
            AlignTarget::Selection => "align.popup.target.selection",
            AlignTarget::Spread => "align.popup.target.spread",
            AlignTarget::Key => "align.popup.target.key",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AlignmentPopup {
    pub target: AlignTarget,
    pub spacing: Option<f32>,
}

impl AlignmentPopup {
    pub fn align_enabled(&self, count: usize) -> bool {
        match self.target {
            AlignTarget::Spread => count >= 1,
            _ => count >= 2,
        }
    }

    pub fn distribute_enabled(&self, count: usize) -> bool {
        match (self.target, self.spacing) {
            (_, Some(gap)) => gap.is_finite() && count >= 2,
            (AlignTarget::Spread, None) => count >= 2,
            _ => count >= 3,
        }
    }

    pub fn align_command(&self, edge: &str) -> Option<Command> {
        ALIGN_EDGES.contains(&edge).then(|| Command::with_params(&format!("layer.arrange.align.{edge}"), json!({ "relative_to": self.target.param() })))
    }

    pub fn distribute_command(&self, horizontal: bool) -> Command {
        let axis = if horizontal { "h" } else { "v" };
        let mut params = json!({ "relative_to": self.target.param() });
        if let Some(gap) = self.spacing {
            params["spacing"] = json!(gap);
        }
        Command::with_params(&format!("layer.arrange.distribute.{axis}"), params)
    }

    pub fn show(&mut self, ui: &mut egui::Ui, count: usize) -> Option<Command> {
        self.show_with_icons(ui, count, &mut IconCache::new())
    }

    pub(crate) fn show_with_icons(&mut self, ui: &mut egui::Ui, count: usize, icons: &mut IconCache) -> Option<Command> {
        let mut out = None;
        ui.horizontal(|ui| {
            let target_label = label("align.popup.align_to", "menu.layer.arrange.align");
            let caption = ui.label(target_label);
            let targets = [AlignTarget::Selection, AlignTarget::Spread, AlignTarget::Key].map(|target| (target, target.label()));
            dtool_design::dropdown_value(ui, "align.target", &mut self.target, &targets, &ControlState::default())
                .labelled_by(caption.id);
        });
        ui.horizontal(|ui| {
            let mut fixed = self.spacing.is_some();
            if dtool_design::toggle(ui, &mut fixed, label("align.popup.spacing", "dialog.grid.spacing")).changed() {
                self.spacing = fixed.then_some(0.0);
            }
            if let Some(gap) = self.spacing.as_mut() {
                let mut value = f64::from(*gap);
                if dtool_design::number_field(ui, &mut value, label("align.popup.spacing", "dialog.grid.spacing")).changed() {
                    *gap = value as f32;
                }
            } else {
                ui.label(label("align.popup.spacing.auto", "pref.ui.view_mode.auto"));
            }
        });
        ui.separator();
        egui::Grid::new("align.axes").show(ui, |ui| {
            for (horizontal, edges, heading, distribute_label) in [
                (true, &ALIGN_EDGES[..3], "dialog.guides.kind.horizontal", "menu.layer.arrange.align.distribute_horizontally"),
                (false, &ALIGN_EDGES[3..], "dialog.guides.kind.vertical", "menu.layer.arrange.align.distribute_vertically"),
            ] {
                ui.label(text(heading));
                ui.horizontal(|ui| {
                    for edge in edges {
                        let name = format!("layer.arrange.align.{edge}");
                        let key = format!("menu.{name}");
                        if let Some(icon) = action_icon(&name) {
                            if icon_action(ui, icons, icon, text(&key), self.align_enabled(count)).clicked() {
                                out = self.align_command(edge);
                            }
                        }
                    }
                    ui.separator();
                    let icon = if horizontal { "tool-distribute-h" } else { "tool-distribute-v" };
                    if icon_action(ui, icons, icon, text(distribute_label), self.distribute_enabled(count)).clicked() {
                        out = Some(self.distribute_command(horizontal));
                    }
                });
                ui.end_row();
            }
        });
        out
    }
}
