use dtool_core::constraints::{Constraint, Fit, target_parent};
use dtool_core::workspace::Workspace;
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::{Value, json};

pub const PANEL_KEY: &str = "constraints";
pub const WINDOW_PANEL: &str = "window.vector.constraints";

#[derive(Default)]
pub struct ConstraintsPanel {
    error: Option<String>,
}

impl ConstraintsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        let Some(session) = workspace.session_mut() else {
            ui.label(text("constraints.select_child"));
            return;
        };
        let entries: Vec<_> = session.selection.ids.iter().filter_map(|&id| target_parent(&session.document, id)).collect();
        if entries.is_empty() {
            ui.label(text("constraints.select_child"));
            return;
        }
        let rules: Vec<_> = entries.iter().map(|(child, _)| child.constraint.unwrap_or_default()).collect();
        let first = entries[0].1;
        if entries.iter().all(|(_, parent)| parent.id == first.id) {
            let name = if first.name.is_empty() { text("constraints.parent") } else { &first.name };
            ui.label(format!("{name}  ·  {:.1} × {:.1}", first.size[0].abs(), first.size[1].abs()));
        } else {
            ui.weak(text("constraints.multiple_parents"));
        }
        let mut picked = None;
        let mut button = |ui: &mut egui::Ui, key: &'static str, label: &'static str, get: fn(&Constraint) -> bool| {
            let current = get(&rules[0]);
            let mixed = rules.iter().any(|rule| get(rule) != current);
            let response = ui.add(egui::Button::selectable(current && !mixed, if mixed {
                format!("{} · —", text(label))
            } else {
                text(label).to_owned()
            }));
            if response.clicked() {
                picked = Some(json!({"patch": {(key): mixed || !current}}));
            }
        };
        ui.horizontal_wrapped(|ui| {
            button(ui, "left", "constraints.left", |rule| rule.left);
            button(ui, "right", "constraints.right", |rule| rule.right);
        });
        ui.horizontal_wrapped(|ui| {
            button(ui, "top", "constraints.top", |rule| rule.top);
            button(ui, "bottom", "constraints.bottom", |rule| rule.bottom);
        });
        ui.horizontal_wrapped(|ui| {
            button(ui, "scale_w", "constraints.scale_w", |rule| rule.scale_w);
            button(ui, "scale_h", "constraints.scale_h", |rule| rule.scale_h);
        });
        ui.separator();
        let fit = rules[0].fit;
        let mixed = rules.iter().any(|rule| rule.fit != fit);
        egui::ComboBox::from_id_salt("constraints.fit")
            .selected_text(if mixed { "—" } else { fit_label(fit) })
            .show_ui(ui, |ui| {
                for mode in [Fit::None, Fit::Min, Fit::Max] {
                    if ui.selectable_label(!mixed && fit == mode, fit_label(mode)).clicked() {
                        picked = Some(json!({"patch": {"fit": mode}}));
                    }
                }
            }).response.on_hover_text(text("constraints.fit"));
        if ui.add_enabled(entries.iter().any(|(child, _)| child.constraint.is_some()),
            egui::Button::new(text("constraints.clear"))).clicked()
        {
            picked = Some(json!({"constraint": Value::Null}));
        }
        if let Some(params) = picked {
            self.error = session.run(&Command::with_params("constraint.set", params)).err().map(|error| error.message_key);
        }
        if let Some(error) = &self.error {
            ui.label(text(error));
        }
    }
}

fn fit_label(fit: Fit) -> &'static str {
    text(match fit {
        Fit::None => "constraints.fit.none",
        Fit::Min => "constraints.fit.min",
        Fit::Max => "constraints.fit.max",
    })
}
