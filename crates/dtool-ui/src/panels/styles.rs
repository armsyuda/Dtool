use dtool_core::paint::Paint;
use dtool_core::workspace::Workspace;
use dtool_protocol::Command;
use serde_json::{json, Value};

pub const STYLES_PANEL_KEY: &str = "styles";
pub const KEY_TITLE: &str = "panel.styles";
pub const KEY_CREATE: &str = "styles.create";
pub const KEY_DELETE: &str = "button.delete";
pub const KEY_EMPTY: &str = "styles.empty";
pub const KEY_UNCATEGORISED: &str = "styles.category.none";

#[derive(Debug, Clone, PartialEq)]
pub struct StyleRow {
    pub id: u64,
    pub name: String,
    pub category: String,
    pub fill: Option<Paint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StyleCategory {
    pub name: String,
    pub styles: Vec<StyleRow>,
}

#[derive(Default)]
pub struct StylesPanel {
    pub new_name: String,
    pub new_category: String,
    pub last_error: Option<String>,
}

pub fn label(key: &str) -> Option<String> {
    dtool_strings::has(key).then(|| dtool_strings::text(key).to_string())
}

pub fn parse_styles(value: &Value) -> Vec<StyleRow> {
    value
        .get("styles")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(StyleRow {
                        id: item.get("id")?.as_u64()?,
                        name: item.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                        category: item.get("category").and_then(Value::as_str).unwrap_or("").to_string(),
                        fill: item.get("style").and_then(|style| style.get("fill")).and_then(|fill| serde_json::from_value(fill.clone()).ok()),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn categories(rows: &[StyleRow]) -> Vec<StyleCategory> {
    let mut out: Vec<StyleCategory> = Vec::new();
    for row in rows {
        match out.iter_mut().find(|category| category.name == row.category) {
            Some(category) => category.styles.push(row.clone()),
            None => out.push(StyleCategory { name: row.category.clone(), styles: vec![row.clone()] }),
        }
    }
    out
}

pub fn thumbnail_colour(row: &StyleRow) -> egui::Color32 {
    row.fill.as_ref().and_then(crate::fill_stroke::paint_well_colour).unwrap_or_else(|| dtool_design::color("panel.bg"))
}

impl StylesPanel {
    fn run(&mut self, workspace: &mut Workspace, name: &str, params: Value) -> Option<Value> {
        let Some(session) = workspace.session_mut() else {
            self.last_error = Some("error.no_document".to_string());
            return None;
        };
        match session.run(&Command::with_params(name, params)) {
            Ok(value) => {
                self.last_error = None;
                Some(value)
            }
            Err(error) => {
                self.last_error = Some(error.message_key);
                None
            }
        }
    }

    pub fn list(&mut self, workspace: &mut Workspace) -> Vec<StyleRow> {
        let Some(session) = workspace.session_mut() else {
            return Vec::new();
        };
        session.run(&Command::with_params("style.list", json!({}))).map(|value| parse_styles(&value)).unwrap_or_default()
    }

    pub fn apply(&mut self, workspace: &mut Workspace, id: u64) -> bool {
        self.run(workspace, "style.apply", json!({ "style_id": id })).is_some()
    }

    pub fn create_from_selection(&mut self, workspace: &mut Workspace) -> Option<u64> {
        let mut params = json!({});
        let name = self.new_name.trim();
        if !name.is_empty() {
            params["name"] = json!(name);
        }
        let category = self.new_category.trim();
        if !category.is_empty() {
            params["category"] = json!(category);
        }
        let id = self.run(workspace, "style.create", params).and_then(|value| value["style_id"].as_u64());
        if id.is_some() {
            self.new_name.clear();
        }
        id
    }

    pub fn delete(&mut self, workspace: &mut Workspace, id: u64) -> bool {
        self.run(workspace, "style.delete", json!({ "style_id": id })).is_some()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        let rows = self.list(workspace);
        let groups = categories(&rows);
        let mut apply = None;
        let mut delete = None;
        let mut create = false;
        dtool_design::panel_frame(ui, |ui| {
            if let Some(title) = label(KEY_TITLE) {
                dtool_design::panel_header(ui, title);
            }
            if groups.is_empty() {
                if let Some(text) = label(KEY_EMPTY) {
                    ui.label(text);
                }
            }
            for group in &groups {
                let heading = if group.name.is_empty() { label(KEY_UNCATEGORISED).unwrap_or_default() } else { group.name.clone() };
                ui.push_id(("style_category", group.name.as_str()), |ui| {
                    egui::CollapsingHeader::new(heading).default_open(true).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(dtool_design::space("xs"), dtool_design::space("xs"));
                            for row in &group.styles {
                                let response = dtool_design::colour_well(ui, thumbnail_colour(row), &format!("style.{}", row.id)).on_hover_text(row.name.as_str());
                                if response.clicked() {
                                    apply = Some(row.id);
                                }
                                if let Some(text) = label(KEY_DELETE) {
                                    response.context_menu(|ui| {
                                        if dtool_design::button(ui, text).clicked() {
                                            delete = Some(row.id);
                                            ui.close();
                                        }
                                    });
                                }
                            }
                        });
                    });
                });
            }
            ui.add_space(dtool_design::space("sm"));
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.new_name);
                ui.text_edit_singleline(&mut self.new_category);
                if let Some(text) = label(KEY_CREATE) {
                    create = dtool_design::button(ui, text).clicked();
                }
            });
            if let Some(key) = self.last_error.as_deref().and_then(label) {
                ui.colored_label(dtool_design::color("error.red"), key);
            }
        });
        if let Some(id) = apply {
            self.apply(workspace, id);
        }
        if let Some(id) = delete {
            self.delete(workspace, id);
        }
        if create {
            self.create_from_selection(workspace);
        }
    }
}