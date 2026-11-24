use dtool_core::workspace::Workspace;
use dtool_protocol::Command;
use serde_json::{json, Value};

pub const ASSETS_PANEL_KEY: &str = "assets";
pub const KEY_TITLE: &str = "panel.assets";
pub const KEY_ADD_CATEGORY: &str = "assets.add_category";
pub const KEY_ADD_GROUP: &str = "assets.add_group";
pub const KEY_ADD_SELECTION: &str = "assets.add_selection";
pub const KEY_INSERT: &str = "assets.insert";
pub const KEY_DELETE: &str = "button.delete";
pub const KEY_EMPTY: &str = "assets.empty";

#[derive(Debug, Clone, PartialEq)]
pub struct AssetRow {
    pub id: u64,
    pub name: String,
    pub category: String,
    pub group: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CategoryRow {
    pub name: String,
    pub groups: Vec<String>,
}

#[derive(Default)]
pub struct AssetsPanel {
    pub category: Option<String>,
    pub group: Option<String>,
    pub new_category: String,
    pub new_group: String,
    pub last_error: Option<String>,
}

pub fn label(key: &str) -> Option<String> {
    dtool_strings::has(key).then(|| dtool_strings::text(key).to_string())
}

pub fn parse_library(value: &Value) -> (Vec<CategoryRow>, Vec<AssetRow>) {
    let categories = value
        .get("categories")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = item.get("name")?.as_str()?.to_string();
                    let groups = item.get("groups").and_then(Value::as_array).map(|groups| groups.iter().filter_map(|g| g.as_str().map(str::to_string)).collect()).unwrap_or_default();
                    Some(CategoryRow { name, groups })
                })
                .collect()
        })
        .unwrap_or_default();
    let assets = value
        .get("assets")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(AssetRow {
                        id: item.get("id")?.as_u64()?,
                        name: item.get("name")?.as_str()?.to_string(),
                        category: item.get("category")?.as_str()?.to_string(),
                        group: item.get("group").and_then(Value::as_str).unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    (categories, assets)
}

impl AssetsPanel {
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

    pub fn library(&mut self, workspace: &mut Workspace) -> (Vec<CategoryRow>, Vec<AssetRow>) {
        let Some(session) = workspace.session_mut() else {
            return Default::default();
        };
        session.run(&Command::with_params("asset.list", json!({}))).map(|value| parse_library(&value)).unwrap_or_default()
    }

    pub fn add_category(&mut self, workspace: &mut Workspace) -> bool {
        let name = self.new_category.trim().to_string();
        if name.is_empty() {
            return false;
        }
        let done = self.run(workspace, "asset.add_category", json!({ "name": name })).is_some();
        if done {
            self.category = Some(name);
            self.new_category.clear();
        }
        done
    }

    pub fn add_group(&mut self, workspace: &mut Workspace) -> bool {
        let (Some(category), name) = (self.category.clone(), self.new_group.trim().to_string()) else {
            return false;
        };
        if name.is_empty() {
            return false;
        }
        let done = self.run(workspace, "asset.add_group", json!({ "category": category, "name": name })).is_some();
        if done {
            self.group = Some(name);
            self.new_group.clear();
        }
        done
    }

    pub fn add_selection(&mut self, workspace: &mut Workspace) -> Option<u64> {
        let category = self.category.clone()?;
        let group = self.group.clone().unwrap_or_default();
        self.run(workspace, "asset.add", json!({ "category": category, "group": group })).and_then(|value| value["id"].as_u64())
    }

    pub fn insert(&mut self, workspace: &mut Workspace, id: u64, pos: Option<[f32; 2]>) -> Vec<u64> {
        let params = match pos {
            Some(pos) => json!({ "asset_id": id, "pos": pos }),
            None => json!({ "asset_id": id }),
        };
        self.run(workspace, "asset.place", params)
            .and_then(|value| value.get("ids").and_then(Value::as_array).map(|ids| ids.iter().filter_map(Value::as_u64).collect()))
            .unwrap_or_default()
    }

    pub fn delete(&mut self, workspace: &mut Workspace, id: u64) -> bool {
        self.run(workspace, "asset.delete", json!({ "asset_id": id })).is_some()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        let (categories, assets) = self.library(workspace);
        if self.category.as_ref().is_none_or(|name| !categories.iter().any(|c| &c.name == name)) {
            self.category = categories.first().map(|c| c.name.clone());
            self.group = None;
        }
        dtool_design::panel_frame(ui, |ui| {
            if let Some(title) = label(KEY_TITLE) {
                dtool_design::panel_header(ui, title);
            }
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.new_category);
                if let Some(text) = label(KEY_ADD_CATEGORY) {
                    if dtool_design::button(ui, text).clicked() {
                        self.add_category(workspace);
                    }
                }
            });
            let mut insert = None;
            let mut delete = None;
            let mut pick = None;
            for category in &categories {
                let open = self.category.as_deref() == Some(category.name.as_str());
                if ui.selectable_label(open, &category.name).clicked() {
                    pick = Some((category.name.clone(), None));
                }
                if !open {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.new_group);
                    if let Some(text) = label(KEY_ADD_GROUP) {
                        if dtool_design::button(ui, text).clicked() {
                            self.add_group(workspace);
                        }
                    }
                });
                for group in &category.groups {
                    let selected = self.group.as_deref() == Some(group.as_str());
                    ui.horizontal(|ui| {
                        ui.add_space(dtool_design::space("md"));
                        if ui.selectable_label(selected, group).clicked() {
                            pick = Some((category.name.clone(), Some(group.clone())));
                        }
                    });
                }
                let rows: Vec<&AssetRow> = assets.iter().filter(|row| row.category == category.name && self.group.as_ref().is_none_or(|g| &row.group == g)).collect();
                if rows.is_empty() {
                    if let Some(text) = label(KEY_EMPTY) {
                        ui.label(text);
                    }
                }
                for row in rows {
                    ui.horizontal(|ui| {
                        ui.add_space(dtool_design::space("md"));
                        let item = ui.selectable_label(false, &row.name);
                        if item.double_clicked() {
                            insert = Some(row.id);
                        }
                        if let Some(text) = label(KEY_INSERT) {
                            if dtool_design::button(ui, text).clicked() {
                                insert = Some(row.id);
                            }
                        }
                        if let Some(text) = label(KEY_DELETE) {
                            if dtool_design::button(ui, text).clicked() {
                                delete = Some(row.id);
                            }
                        }
                    });
                }
            }
            if let Some(text) = label(KEY_ADD_SELECTION) {
                let enabled = self.category.is_some() && workspace.session().is_some_and(|s| !s.selection.ids.is_empty());
                if ui.add_enabled(enabled, egui::Button::new(text)).clicked() {
                    self.add_selection(workspace);
                }
            }
            if let Some((category, group)) = pick {
                self.category = Some(category);
                self.group = group;
            }
            if let Some(id) = insert {
                self.insert(workspace, id, None);
            }
            if let Some(id) = delete {
                self.delete(workspace, id);
            }
            if let Some(key) = &self.last_error {
                if let Some(text) = label(key) {
                    ui.label(text);
                }
            }
        });
    }
}