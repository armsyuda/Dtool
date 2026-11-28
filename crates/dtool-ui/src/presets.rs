use crate::dock::{side_tree, DockColumns, DockSide, Layout, LayoutPreset, PanelId, PanelState};
use dtool_design::{button, menu_item};
use dtool_protocol::CommandError;
use dtool_strings::text;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum WorkspacePersona {
    #[default]
    Vector,
    Pixel,
    Layout,
    Slice,
    Astrophotography,
    ColourGrading,
    Compositing,
    Retouching,
    Scripting,
    Typography,
}

impl WorkspacePersona {
    pub const ALL: [WorkspacePersona; 10] = [
        WorkspacePersona::Vector,
        WorkspacePersona::Pixel,
        WorkspacePersona::Layout,
        WorkspacePersona::Slice,
        WorkspacePersona::Astrophotography,
        WorkspacePersona::ColourGrading,
        WorkspacePersona::Compositing,
        WorkspacePersona::Retouching,
        WorkspacePersona::Scripting,
        WorkspacePersona::Typography,
    ];

    pub fn key(self) -> &'static str {
        match self {
            WorkspacePersona::Vector => "vector",
            WorkspacePersona::Pixel => "pixel",
            WorkspacePersona::Layout => "layout",
            WorkspacePersona::Slice => "slice",
            WorkspacePersona::Astrophotography => "astrophotography",
            WorkspacePersona::ColourGrading => "colour_grading",
            WorkspacePersona::Compositing => "compositing",
            WorkspacePersona::Retouching => "retouching",
            WorkspacePersona::Scripting => "scripting",
            WorkspacePersona::Typography => "typography",
        }
    }

    pub fn from_key(key: &str) -> Option<WorkspacePersona> {
        WorkspacePersona::ALL.iter().copied().find(|persona| persona.key() == key)
    }

    pub fn is_planned(self) -> bool {
        matches!(
            self,
            WorkspacePersona::Astrophotography
                | WorkspacePersona::ColourGrading
                | WorkspacePersona::Compositing
                | WorkspacePersona::Retouching
                | WorkspacePersona::Scripting
                | WorkspacePersona::Typography
        )
    }

    pub fn listed() -> Vec<WorkspacePersona> {
        WorkspacePersona::ALL.to_vec()
    }

    pub fn label_key(self) -> String {
        format!("workspace.{}", self.key())
    }

    pub fn columns(self) -> DockColumns {
        let sides = default_panel_sides(self);
        let shown = |side: DockSide| sides.iter().any(|(s, panes)| *s == side && !panes.is_empty());
        DockColumns {
            left: shown(DockSide::Left),
            right: shown(DockSide::Right),
            top: shown(DockSide::Top),
            bottom: shown(DockSide::Bottom),
        }
    }
}

fn other(key: &str) -> PanelId {
    PanelId::from_key(key)
}

// Shared studio groups, derived from design handoff v1.0.7. Only live panel
// routes are included; saved user layouts continue to deserialize unchanged.
fn studio_groups(persona: WorkspacePersona) -> Vec<Vec<Vec<&'static str>>> {
    if persona == WorkspacePersona::Pixel {
        vec![vec![vec!["swatches"], vec!["stroke"], vec!["character"], vec!["paragraph", "text_styles"]],
             vec![vec!["colour"], vec!["layers", "brushes"], vec!["transform", "navigator", "history"]]]
    } else {
        vec![vec![vec!["swatches"], vec!["stroke"], vec!["character"], vec!["paragraph", "text_styles"]],
             vec![vec!["colour"], vec!["layers", "effects"], vec!["transform", "navigator", "history"]]]
    }
}

fn reference_studio_layout(persona: WorkspacePersona) -> Layout {
    use egui_tiles::{Linear, LinearDir, Tiles, Tree};
    let mut layout = Layout { tiles: Value::Object(Map::new()), panels: Vec::new(),
        columns: DockColumns { left: true, right: true, top: false, bottom: false },
        dock_sizes: Default::default(), floating_positions: Default::default() };
    let left = if persona == WorkspacePersona::Layout { vec![PanelId::Pages, other("assets"), other("styles")] }
        else { vec![other("assets"), other("styles")] };
    let mut register = |id: PanelId| {
        if !layout.panels.iter().any(|p| p.id == id) {
            layout.panels.push(PanelState { id, visible: true, floated: false, os_window: false, tab_group: None });
        }
    };
    for id in &left { register(id.clone()); }
    let mut tiles = Tiles::default();
    let mut columns = Vec::new();
    for (column_index, groups) in studio_groups(persona).into_iter().enumerate() {
        let mut children = Vec::new();
        for group in groups {
            let panes = group.into_iter().map(|key| {
                let id = crate::panel_ids::canonical(other(key));
                register(id.clone());
                tiles.insert_pane(id)
            }).collect();
            children.push(tiles.insert_tab_tile(panes));
        }
        let mut column = Linear::new(LinearDir::Vertical, children.clone());
        let shares: &[f32] = if column_index == 0 { &[0.22, 0.31, 0.24, 0.23] } else { &[0.22, 0.60, 0.18] };
        for (child, share) in children.into_iter().zip(shares) { column.shares.set_share(child, *share); }
        columns.push(tiles.insert_container(column));
    }
    let root = tiles.insert_horizontal_tile(columns);
    let tree = Tree::new(egui::Id::new(("dtool.dock", "right")), root, tiles);
    layout.store_tree(DockSide::Left, &side_tree(DockSide::Left, left));
    layout.store_tree(DockSide::Right, &tree);
    layout.sync_tree_visibility();
    layout
}

pub fn default_panel_sides(persona: WorkspacePersona) -> Vec<(DockSide, Vec<PanelId>)> {
    match persona {
        WorkspacePersona::Vector => vec![
            (DockSide::Left, vec![other("brushes"), other("asset_browser"), other("style")]),
            (
                DockSide::Right,
                vec![
                    PanelId::Colour,
                    PanelId::Swatches,
                    other("stroke"),
                    PanelId::Character,
                    PanelId::Paragraph,
                    other("appearance"),
                    PanelId::Layers,
                    PanelId::Effects,
                    other("transform"),
                    PanelId::Navigator,
                    PanelId::History,
                ],
            ),
        ],
        WorkspacePersona::Pixel => vec![
            (DockSide::Left, vec![other("macro"), other("library")]),
            (
                DockSide::Right,
                vec![
                    other("histogram"),
                    PanelId::Colour,
                    PanelId::Layers,
                    other("channels"),
                    other("brushes"),
                    other("stock"),
                    PanelId::Navigator,
                    other("transform"),
                    PanelId::History,
                ],
            ),
        ],
        WorkspacePersona::Layout => vec![
            (DockSide::Left, vec![PanelId::Pages, other("asset_browser")]),
            (
                DockSide::Right,
                vec![
                    PanelId::Colour,
                    PanelId::Swatches,
                    other("stroke"),
                    PanelId::Character,
                    PanelId::Paragraph,
                    other("text_styles"),
                    PanelId::Layers,
                    other("transform"),
                    PanelId::Navigator,
                    PanelId::History,
                ],
            ),
        ],
        WorkspacePersona::Slice => vec![
            (DockSide::Right, vec![other("export"), PanelId::Layers, other("transform"), PanelId::History]),
        ],
        WorkspacePersona::Astrophotography => vec![
            (
                DockSide::Right,
                vec![other("histogram"), PanelId::Layers, other("channels"), PanelId::Navigator, PanelId::History],
            ),
        ],
        WorkspacePersona::ColourGrading => vec![
            (
                DockSide::Right,
                vec![
                    other("histogram"),
                    other("scope"),
                    other("color_adjustments"),
                    PanelId::Colour,
                    PanelId::Layers,
                    PanelId::Navigator,
                ],
            ),
        ],
        WorkspacePersona::Compositing => vec![
            (
                DockSide::Right,
                vec![
                    PanelId::Layers,
                    other("channels"),
                    other("brushes"),
                    PanelId::Effects,
                    PanelId::Navigator,
                    PanelId::History,
                ],
            ),
        ],
        WorkspacePersona::Retouching => vec![
            (DockSide::Left, vec![other("macro")]),
            (
                DockSide::Right,
                vec![other("brushes"), PanelId::Colour, PanelId::Layers, PanelId::Navigator, PanelId::History],
            ),
        ],
        WorkspacePersona::Scripting => vec![
            (DockSide::Right, vec![PanelId::Layers, PanelId::Properties, PanelId::History]),
            (DockSide::Bottom, vec![PanelId::Console]),
        ],
        WorkspacePersona::Typography => vec![
            (DockSide::Left, vec![other("font_manager")]),
            (
                DockSide::Right,
                vec![
                    PanelId::Character,
                    PanelId::Paragraph,
                    other("text_styles"),
                    other("glyph_browser"),
                    other("text_effects"),
                    PanelId::Layers,
                ],
            ),
        ],
    }
}

pub fn default_layout(persona: WorkspacePersona) -> Layout {
    if matches!(persona, WorkspacePersona::Vector | WorkspacePersona::Pixel | WorkspacePersona::Layout) {
        return reference_studio_layout(persona);
    }
    let mut layout = Layout {
        tiles: Value::Object(Map::new()),
        panels: Vec::new(),
        columns: persona.columns(),
        dock_sizes: Default::default(),
        floating_positions: Default::default(),
    };
    for (side, panes) in default_panel_sides(persona) {
        for id in &panes {
            if layout.panel_state(id).is_none() {
                layout.panels.push(PanelState {
                    id: id.clone(),
                    visible: true,
                    floated: false,
                    os_window: false,
                    tab_group: None,
                });
            }
        }
        layout.store_tree(side, &side_tree(side, panes));
    }
    layout.sync_tree_visibility();
    layout
}

pub fn default_presets() -> Vec<LayoutPreset> {
    WorkspacePersona::listed()
        .iter()
        .map(|persona| LayoutPreset {
            name: persona.key().to_string(),
            layout: default_layout(*persona),
        })
        .collect()
}

fn workspace_or_default<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<WorkspacePersona, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PresetStore {
    pub presets: Vec<LayoutPreset>,
    #[serde(default, deserialize_with = "workspace_or_default")]
    pub workspace: WorkspacePersona,
    #[serde(default)]
    pub saved_layout: Option<Layout>,
}

fn invalid_params() -> CommandError {
    CommandError::new("INVALID_PARAMS")
}

fn not_found() -> CommandError {
    CommandError::new("NOT_FOUND")
}

fn name_param(params: &Value) -> Result<String, CommandError> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    if name.is_empty() {
        Err(invalid_params())
    } else {
        Ok(name)
    }
}

impl PresetStore {
    pub fn list(&self) -> Vec<String> {
        self.presets.iter().map(|preset| preset.name.clone()).collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.presets.iter().any(|preset| preset.name == name)
    }

    pub fn save(&mut self, name: &str, layout: &Layout) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        match self.presets.iter_mut().find(|preset| preset.name == name) {
            Some(preset) => preset.layout = layout.clone(),
            None => self.presets.push(LayoutPreset {
                name: name.to_string(),
                layout: layout.clone(),
            }),
        }
        true
    }

    pub fn load(&self, name: &str) -> Option<Layout> {
        self.presets
            .iter()
            .find(|preset| preset.name == name)
            .map(|preset| preset.layout.clone())
    }

    pub fn apply(&self, name: &str, layout: &mut Layout) -> bool {
        match self.load(name) {
            Some(saved) => {
                layout.set(saved);
                true
            }
            None => false,
        }
    }

    pub fn delete(&mut self, name: &str) -> bool {
        let before = self.presets.len();
        self.presets.retain(|preset| preset.name != name);
        self.presets.len() != before
    }

    pub fn rename(&mut self, from: &str, to: &str) -> bool {
        let to = to.trim();
        if to.is_empty() || (to != from && self.contains(to)) {
            return false;
        }
        match self.presets.iter_mut().find(|preset| preset.name == from) {
            Some(preset) => {
                preset.name = to.to_string();
                true
            }
            None => false,
        }
    }

    pub fn workspace(&self) -> WorkspacePersona {
        self.workspace
    }

    pub fn workspace_default(&self) -> Layout {
        default_layout(self.workspace)
    }

    pub fn choose_workspace(&mut self, persona: WorkspacePersona, layout: &mut Layout) {
        if persona.is_planned() {
            return;
        }
        self.workspace = persona;
        layout.set(default_layout(persona));
        self.saved_layout = Some(layout.clone());
    }

    pub fn reset(&mut self, layout: &mut Layout) {
        layout.set(default_layout(self.workspace));
        self.saved_layout = Some(layout.clone());
    }

    pub fn remember_layout(&mut self, layout: &Layout) {
        self.saved_layout = Some(layout.clone());
    }

    pub fn restore_layout(&self, layout: &mut Layout) {
        match &self.saved_layout {
            Some(saved) => layout.set(saved.clone()),
            None => layout.set(default_layout(self.workspace)),
        }
    }

    pub fn persisted_path() -> std::path::PathBuf {
        crate::settings::UiSettings::default_path().with_file_name("presets.json")
    }

    pub fn load_startup() -> Self {
        Self::load_persisted(&Self::persisted_path())
    }

    pub fn persist(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let temporary = path.with_extension("json.tmp");
        let result = (|| {
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    pub fn load_persisted(path: &Path) -> PresetStore {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|body| serde_json::from_str::<PresetStore>(&body).ok())
            .unwrap_or_default()
            .without_removed()
    }

    pub fn without_removed(mut self) -> PresetStore {
        if self.workspace.is_planned() {
            self.workspace = WorkspacePersona::default();
        }
        self
    }

    pub fn reset_and_persist(&mut self, layout: &mut Layout, path: &Path) -> std::io::Result<()> {
        self.reset(layout);
        self.persist(path)
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn from_json(value: &Value) -> PresetStore {
        serde_json::from_value::<PresetStore>(value.clone()).unwrap_or_default().without_removed()
    }

    pub fn run_command(&mut self, name: &str, params: &Value, layout: &mut Layout) -> Option<Result<Value, CommandError>> {
        match name {
            "ui.preset.list" => Some(Ok(json!({ "presets": self.list() }))),
            "ui.preset.save" => Some(name_param(params).and_then(|preset| {
                if self.save(&preset, layout) {
                    Ok(json!({ "name": preset, "presets": self.list() }))
                } else {
                    Err(invalid_params())
                }
            })),
            "ui.preset.load" => Some(name_param(params).and_then(|preset| {
                if self.apply(&preset, layout) {
                    layout.layout_json()
                } else {
                    Err(not_found())
                }
            })),
            "ui.preset.delete" => Some(name_param(params).and_then(|preset| {
                if self.delete(&preset) {
                    Ok(json!({ "presets": self.list() }))
                } else {
                    Err(not_found())
                }
            })),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AddPresetDialog {
    pub open: bool,
    pub name: String,
}

impl AddPresetDialog {
    pub fn open(&mut self) {
        self.open = true;
        self.name.clear();
    }

    pub fn show(&mut self, ctx: &egui::Context, store: &mut PresetStore, layout: &Layout) {
        if !self.open {
            return;
        }
        let mut close = false;
        crate::dialog_chrome::show_floating(ctx, "dtool.dialog.add_preset", text("menu.window.panels.add_preset"), |ui| {
            ui.horizontal(|ui| {
                ui.label(text("dialog.preset.name"));
                ui.text_edit_singleline(&mut self.name);
            });
            ui.horizontal(|ui| {
                let ok = ui.add_enabled_ui(!self.name.trim().is_empty(), |ui| button(ui, text("button.ok"))).inner;
                if ok.clicked() && store.save(&self.name, layout) {
                    let _ = store.persist(&PresetStore::persisted_path());
                    close = true;
                }
                if button(ui, text("button.cancel")).clicked() {
                    close = true;
                }
            });
        });
        if close {
            self.open = false;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ManagePresetsDialog {
    pub open: bool,
    pub selected: Option<usize>,
    pub rename_to: String,
}

impl ManagePresetsDialog {
    pub fn open(&mut self) {
        self.open = true;
        self.selected = None;
        self.rename_to.clear();
    }

    pub fn show(&mut self, ctx: &egui::Context, store: &mut PresetStore) {
        if !self.open {
            return;
        }
        let mut close = false;
        crate::dialog_chrome::show_floating(ctx, "dtool.dialog.manage_presets", text("menu.window.panels.manage_presets"), |ui| {
            let names = store.list();
            for (index, name) in names.iter().enumerate() {
                let selected = self.selected == Some(index);
                if ui.selectable_label(selected, name.as_str()).clicked() {
                    self.selected = Some(index);
                    self.rename_to = name.clone();
                }
            }
            let current = self.selected.and_then(|index| names.get(index).cloned());
            ui.horizontal(|ui| {
                ui.label(text("dialog.preset.name"));
                ui.add_enabled(current.is_some(), egui::TextEdit::singleline(&mut self.rename_to));
            });
            ui.horizontal(|ui| {
                let has_selection = current.is_some();
                let rename = ui.add_enabled_ui(has_selection, |ui| button(ui, text("button.rename"))).inner;
                if rename.clicked() {
                    if let Some(name) = current.as_deref() {
                        if store.rename(name, &self.rename_to) {
                            let _ = store.persist(&PresetStore::persisted_path());
                        }
                    }
                }
                let delete = ui.add_enabled_ui(has_selection, |ui| button(ui, text("button.delete"))).inner;
                if delete.clicked() {
                    if let Some(name) = current.as_deref() {
                        if store.delete(name) {
                            let _ = store.persist(&PresetStore::persisted_path());
                        }
                        self.selected = None;
                        self.rename_to.clear();
                    }
                }
                if button(ui, text("button.close")).clicked() {
                    close = true;
                }
            });
        });
        if close {
            self.open = false;
        }
    }
}

pub fn presets_menu_items(ui: &mut egui::Ui, store: &PresetStore, layout: &mut Layout, add: &mut AddPresetDialog, manage: &mut ManagePresetsDialog) {
    let names = store.list();
    for name in &names {
        if menu_item(ui, name.as_str()).clicked() {
            store.apply(name, layout);
            ui.close();
        }
    }
    if !names.is_empty() {
        ui.separator();
    }
    if menu_item(ui, text("menu.window.panels.add_preset")).clicked() {
        add.open();
        ui.close();
    }
    let manage_item = ui.add_enabled_ui(!names.is_empty(), |ui| menu_item(ui, text("menu.window.panels.manage_presets"))).inner;
    if manage_item.clicked() {
        manage.open();
        ui.close();
    }
}

pub fn get_menu_label_window_panels_reset() -> String {
    text("menu.window.panels.reset").to_string()
}

pub fn panels_reset_menu_item(ui: &mut egui::Ui, store: &mut PresetStore, layout: &mut Layout, path: Option<&Path>) -> bool {
    if menu_item(ui, get_menu_label_window_panels_reset().as_str()).clicked() {
        store.reset(layout);
        if let Some(path) = path {
            let _ = store.persist(path);
        }
        ui.close();
        return true;
    }
    false
}

pub fn workspace_menu_items(ui: &mut egui::Ui, store: &mut PresetStore, layout: &mut Layout) -> Option<WorkspacePersona> {
    let mut chosen = None;
    for persona in WorkspacePersona::listed() {
        let label = text(&persona.label_key()).to_string();
        if persona.is_planned() {
            let label = format!("{} ({})", label, text("menu.planned.suffix"));
            ui.add_enabled(false, egui::Button::new(label).frame(false));
            continue;
        }
        if menu_item(ui, label.as_str()).clicked() {
            store.choose_workspace(persona, layout);
            chosen = Some(persona);
            ui.close();
        }
    }
    chosen
}

pub fn open_preset_dialog(action: &str, add: &mut AddPresetDialog, manage: &mut ManagePresetsDialog) -> bool {
    match action {
        "window.panels.add_preset" => {
            add.open();
            true
        }
        "window.panels.manage_presets" => {
            manage.open();
            true
        }
        _ => false,
    }
}
