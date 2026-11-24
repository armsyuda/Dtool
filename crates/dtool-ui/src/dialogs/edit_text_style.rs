use crate::dialog_chrome::{show_modal, ClosePolicy, DialogResult};
use dtool_core::text_styles::{NamedTextStyle, StyleKind};
use dtool_core::{Command, Session};
use dtool_strings::{format, text};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    Ok,
    ApplyToSelection,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditCategory {
    #[default]
    Style,
    Character,
    Typography,
    Paragraph,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditTextStyleDialog {
    pub name: String,
    pub original_name: String,
    pub kind: StyleKind,
    pub based_on: Option<String>,
    pub next_style: Option<String>,
    pub props: Map<String, Value>,
    pub apply_to_selection: bool,
    pub category: EditCategory,
}

impl EditTextStyleDialog {
    pub fn from_style(style: &NamedTextStyle) -> Self {
        Self {
            name: style.name.clone(),
            original_name: style.name.clone(),
            kind: style.kind,
            based_on: style.based_on.clone(),
            next_style: style.next_style.clone(),
            props: style.props.clone(),
            apply_to_selection: false,
            category: EditCategory::Style,
        }
    }

    pub fn define_command(&self) -> Command {
        Command::with_params(
            "text.style.define",
            json!({
                "id": self.name.trim(),
                "kind": self.kind,
                "based_on": self.based_on,
                "next_style": self.next_style,
                "props": self.props,
            }),
        )
    }

    pub fn can_confirm(&self) -> bool {
        !self.name.trim().is_empty()
    }

    pub fn reset_formatting(&mut self) {
        self.props.clear();
    }

    pub fn summary_lines(&self, session: &Session) -> Vec<String> {
        let resolved = session.document.text_styles.resolve(self.kind, &self.original_name).unwrap_or_else(|_| self.props.clone());
        let mut keys: Vec<_> = resolved.keys().cloned().collect();
        keys.sort();
        keys.into_iter()
            .map(|key| format!("{key}: {}", value_label(resolved.get(&key).unwrap_or(&Value::Null))))
            .collect()
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &Session) -> Option<EditAction> {
        let title = format("styles.edit", &[("name", self.name.as_str())]);
        let can = self.can_confirm();
        let mut apply_clicked = false;
        let (result, _) = show_modal(ctx, "dtool.edit_text_style", &title, ClosePolicy::CancelOnly, can, |ui| {
            ui.set_min_size(egui::vec2(710.0, 764.0));
            ui.horizontal(|ui| {
                ui.label(text("styles.name"));
                ui.text_edit_singleline(&mut self.name);
            });
            ui.horizontal(|ui| {
                ui.label(text("styles.based_on"));
                let mut current = self.based_on.clone().unwrap_or_default();
                let mut names: Vec<String> = session.document.text_styles.names(self.kind).into_iter().filter(|name| name != &self.original_name).collect();
                names.insert(0, String::new());
                let selected = names.iter().position(|name| name == &current).unwrap_or(0);
                egui::ComboBox::from_id_salt("styles.based_on")
                    .selected_text(if current.is_empty() { text("styles.based_on.none").to_string() } else { current.clone() })
                    .show_ui(ui, |ui| {
                        for (index, name) in names.iter().enumerate() {
                            let label = if name.is_empty() { text("styles.based_on.none").to_string() } else { name.clone() };
                            if ui.selectable_label(index == selected, label).clicked() {
                                self.based_on = if name.is_empty() { None } else { Some(name.clone()) };
                            }
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label(text("styles.next"));
                let mut names: Vec<String> = session.document.text_styles.names(self.kind);
                names.insert(0, String::new());
                let current = self.next_style.clone().unwrap_or_default();
                egui::ComboBox::from_id_salt("styles.next")
                    .selected_text(if current.is_empty() { text("styles.next.none").to_string() } else { current.clone() })
                    .show_ui(ui, |ui| {
                        for name in &names {
                            let label = if name.is_empty() { text("styles.next.none").to_string() } else { name.clone() };
                            if ui.selectable_label(!name.is_empty() && self.next_style.as_deref() == Some(name.as_str()), label).clicked() {
                                self.next_style = if name.is_empty() { None } else { Some(name.clone()) };
                            }
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label(text("styles.type"));
                if ui.selectable_label(self.kind == StyleKind::Paragraph, text("styles.kind.paragraph")).clicked() {
                    self.kind = StyleKind::Paragraph;
                }
                if ui.selectable_label(self.kind == StyleKind::Character, text("styles.kind.character")).clicked() {
                    self.kind = StyleKind::Character;
                }
            });
            ui.horizontal(|ui| {
                for (category, key) in [
                    (EditCategory::Style, "styles.category.style"),
                    (EditCategory::Character, "styles.category.character"),
                    (EditCategory::Typography, "styles.category.typography"),
                    (EditCategory::Paragraph, "styles.category.paragraph"),
                ] {
                    if ui.selectable_label(self.category == category, text(key)).clicked() {
                        self.category = category;
                    }
                }
            });
            if ui.button(text("styles.reset_formatting")).clicked() {
                self.reset_formatting();
            }
            ui.label(text("styles.summary"));
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for line in self.summary_lines(session) {
                    ui.label(line);
                }
            });
            ui.checkbox(&mut self.apply_to_selection, text("styles.apply_to_selection"));
            ui.horizontal(|ui| {
                if ui.add_enabled(can, egui::Button::new(text("styles.ok"))).clicked() {
                    ui.ctx().memory_mut(|memory| memory.data.insert_temp(egui::Id::new("dtool.edit_text_style.ok"), true));
                }
                if ui.button(text("styles.cancel")).clicked() {
                    ui.ctx().memory_mut(|memory| memory.data.insert_temp(egui::Id::new("dtool.edit_text_style.cancel"), true));
                }
                if ui.add_enabled(can, egui::Button::new(text("styles.apply_to_selection"))).clicked() {
                    apply_clicked = true;
                }
            });
        });
        if apply_clicked {
            return Some(EditAction::ApplyToSelection);
        }
        if ctx.memory(|memory| memory.data.get_temp::<bool>(egui::Id::new("dtool.edit_text_style.cancel")).unwrap_or(false)) {
            ctx.memory_mut(|memory| memory.data.remove::<bool>(egui::Id::new("dtool.edit_text_style.cancel")));
            return Some(EditAction::Cancel);
        }
        match result {
            DialogResult::Confirm => Some(EditAction::Ok),
            DialogResult::Cancel => Some(EditAction::Cancel),
            DialogResult::Open => None,
        }
    }
}

fn value_label(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}
