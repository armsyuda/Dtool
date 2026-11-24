use dtool_protocol::CommandError;
use egui_tiles::{Tile, TileId, Tiles, Tree};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelId {
    Layers,
    Properties,
    Console,
    Pages,
    History,
    Navigator,
    Colour,
    Swatches,
    Stroke,
    Character,
    Paragraph,
    Effects,
    AiLog,
    Resources,
    Other(String),
}

impl PanelId {
    pub fn key(&self) -> &str {
        match self {
            PanelId::Layers => "layers",
            PanelId::Properties => "properties",
            PanelId::Console => "console",
            PanelId::Pages => "pages",
            PanelId::History => "history",
            PanelId::Navigator => "navigator",
            PanelId::Colour => "colour",
            PanelId::Swatches => "swatches",
            PanelId::Stroke => "stroke",
            PanelId::Character => "character",
            PanelId::Paragraph => "paragraph",
            PanelId::Effects => "effects",
            PanelId::AiLog => "ai_log",
            PanelId::Resources => "resources",
            PanelId::Other(id) => id.as_str(),
        }
    }

    pub fn from_key(key: &str) -> PanelId {
        match key {
            "layers" => PanelId::Layers,
            "properties" => PanelId::Properties,
            "console" => PanelId::Console,
            "pages" => PanelId::Pages,
            "history" => PanelId::History,
            "navigator" => PanelId::Navigator,
            "colour" | "color" => PanelId::Colour,
            "swatches" => PanelId::Swatches,
            "stroke" => PanelId::Stroke,
            "character" => PanelId::Character,
            "paragraph" => PanelId::Paragraph,
            "effects" => PanelId::Effects,
            "ai_log" | "ailog" => PanelId::AiLog,
            "resources" => PanelId::Resources,
            other => PanelId::Other(other.to_string()),
        }
    }

    pub fn from_value(value: &Value) -> Option<PanelId> {
        match value {
            Value::String(key) if !key.is_empty() => Some(PanelId::from_key(key)),
            Value::Null => None,
            other => serde_json::from_value(other.clone()).ok().map(crate::panel_ids::canonical),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelState {
    pub id: PanelId,
    pub visible: bool,
    pub floated: bool,
    pub os_window: bool,
    pub tab_group: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DockSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl DockSide {
    pub const ALL: [DockSide; 4] = [DockSide::Left, DockSide::Right, DockSide::Top, DockSide::Bottom];

    pub fn key(self) -> &'static str {
        match self {
            DockSide::Left => "left",
            DockSide::Right => "right",
            DockSide::Top => "top",
            DockSide::Bottom => "bottom",
        }
    }

    pub fn from_key(key: &str) -> Option<DockSide> {
        DockSide::ALL.iter().copied().find(|side| side.key() == key)
    }

    pub fn menu_key(self) -> &'static str {
        match self {
            DockSide::Left => "menu.window.panels.show_left",
            DockSide::Right => "menu.window.panels.show_right",
            DockSide::Top => "menu.window.panels.show_top",
            DockSide::Bottom => "menu.window.panels.show_bottom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockColumns {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl Default for DockColumns {
    fn default() -> Self {
        Self {
            left: true,
            right: true,
            top: true,
            bottom: true,
        }
    }
}

impl DockColumns {
    pub fn is_shown(&self, side: DockSide) -> bool {
        match side {
            DockSide::Left => self.left,
            DockSide::Right => self.right,
            DockSide::Top => self.top,
            DockSide::Bottom => self.bottom,
        }
    }

    pub fn set(&mut self, side: DockSide, shown: bool) {
        match side {
            DockSide::Left => self.left = shown,
            DockSide::Right => self.right = shown,
            DockSide::Top => self.top = shown,
            DockSide::Bottom => self.bottom = shown,
        }
    }

    pub fn any_shown(&self) -> bool {
        self.left || self.right || self.top || self.bottom
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layout {
    pub tiles: serde_json::Value,
    pub panels: Vec<PanelState>,
    #[serde(default)]
    pub columns: DockColumns,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dock_sizes: BTreeMap<String, f32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub floating_positions: BTreeMap<String, FloatingPanelPositions>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FloatingPanelPositions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    embedded: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutPreset {
    pub name: String,
    pub layout: Layout,
}

pub fn default_side(id: &PanelId) -> DockSide {
    match id {
        PanelId::Layers | PanelId::Pages | PanelId::Navigator => DockSide::Left,
        PanelId::Console | PanelId::History => DockSide::Top,
        PanelId::AiLog | PanelId::Resources => DockSide::Bottom,
        _ => DockSide::Right,
    }
}

pub fn side_tree(side: DockSide, panes: Vec<PanelId>) -> Tree<PanelId> {
    let mut tiles: Tiles<PanelId> = Tiles::default();
    let children: Vec<TileId> = panes.into_iter().map(|pane| tiles.insert_pane(pane)).collect();
    let root = tiles.insert_tab_tile(children);
    Tree::new(egui::Id::new(("dtool.dock", side.key())), root, tiles)
}

fn pane_tile(tree: &Tree<PanelId>, id: &PanelId) -> Option<TileId> {
    tree.tiles.iter().find_map(|(tile_id, tile)| match tile {
        Tile::Pane(pane) if pane == id => Some(*tile_id),
        _ => None,
    })
}

fn invalid_params() -> CommandError {
    CommandError::new("INVALID_PARAMS")
}

fn not_found() -> CommandError {
    CommandError::new("NOT_FOUND")
}

impl Default for Layout {
    fn default() -> Self {
        let mut layout = Layout {
            tiles: Value::Object(Map::new()),
            panels: Vec::new(),
            columns: DockColumns::default(),
            dock_sizes: BTreeMap::new(),
            floating_positions: BTreeMap::new(),
        };
        layout.store_tree(DockSide::Left, &side_tree(DockSide::Left, vec![PanelId::Layers]));
        layout.store_tree(DockSide::Right, &side_tree(DockSide::Right, vec![PanelId::Properties]));
        layout.panels.push(PanelState { id: PanelId::Layers, visible: true, floated: false, os_window: false, tab_group: None });
        layout.panels.push(PanelState { id: PanelId::Properties, visible: true, floated: false, os_window: false, tab_group: None });
        layout
    }
}

impl Layout {
    pub fn get(&self) -> &Layout {
        self
    }

    pub fn set(&mut self, layout: Layout) {
        *self = layout;
        self.canonicalise();
        self.sync_tree_visibility();
    }

    pub fn tree(&self, side: DockSide) -> Option<Tree<PanelId>> {
        self.tiles
            .get(side.key())
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    pub fn store_tree(&mut self, side: DockSide, tree: &Tree<PanelId>) {
        if !self.tiles.is_object() {
            self.tiles = Value::Object(Map::new());
        }
        if let (Some(map), Ok(value)) = (self.tiles.as_object_mut(), serde_json::to_value(tree)) {
            map.insert(side.key().to_string(), value);
        }
    }

    pub fn side_of(&self, id: &PanelId) -> Option<DockSide> {
        self.panel_location(id).map(|(side, _, _)| side)
    }

    fn panel_location(&self, id: &PanelId) -> Option<(DockSide, Tree<PanelId>, TileId)> {
        DockSide::ALL.into_iter().find_map(|side| {
            let tree = self.tree(side)?;
            let pane = pane_tile(&tree, id)?;
            Some((side, tree, pane))
        })
    }

    pub fn panels_on(&self, side: DockSide) -> Vec<PanelId> {
        match self.tree(side) {
            Some(tree) => tree
                .tiles
                .iter()
                .filter_map(|(_, tile)| match tile {
                    Tile::Pane(pane) => Some(pane.clone()),
                    _ => None,
                })
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn dock_panel(&mut self, id: &PanelId, side: DockSide) {
        if self.side_of(id) == Some(side) {
            return;
        }
        if let Some(old_side) = self.side_of(id) {
            if let Some(mut tree) = self.tree(old_side) {
                if let Some(tile_id) = pane_tile(&tree, id) {
                    tree.remove_recursively(tile_id);
                }
                self.store_tree(old_side, &tree);
            }
        }
        let mut tree = match self.tree(side) {
            Some(tree) => tree,
            None => side_tree(side, Vec::new()),
        };
        let new_tile = tree.tiles.insert_pane(id.clone());
        let root = tree.root();
        let mut attached = false;
        if let Some(root_id) = root {
            if let Some(Tile::Container(container)) = tree.tiles.get_mut(root_id) {
                container.add_child(new_tile);
                attached = true;
            }
        }
        if !attached {
            let root_id = tree.tiles.insert_tab_tile(vec![new_tile]);
            tree.root = Some(root_id);
        }
        self.store_tree(side, &tree);
        if self.panel_state(id).is_none() {
            self.panels.push(PanelState {
                id: id.clone(),
                visible: true,
                floated: false,
                os_window: false,
                tab_group: None,
            });
        }
        self.sync_tree_visibility();
    }

    pub fn ensure_active_tabs(&mut self) {
        for side in DockSide::ALL {
            if let Some(mut tree) = self.tree(side) {
                ensure_active_tabs(&mut tree.tiles);
                self.store_tree(side, &tree);
            }
        }
    }

    pub fn sync_tree_visibility(&mut self) {
        for side in DockSide::ALL {
            if let Some(mut tree) = self.tree(side) {
                for state in &self.panels {
                    if let Some(tile_id) = pane_tile(&tree, &state.id) {
                        tree.tiles.set_visible(tile_id, state.visible && !state.floated);
                    }
                }
                ensure_active_tabs(&mut tree.tiles);
                self.store_tree(side, &tree);
            }
        }
    }

    pub fn panel_state(&self, id: &PanelId) -> Option<&PanelState> {
        self.panels.iter().find(|p| p.id == *id)
    }

    pub fn panel_state_mut(&mut self, id: &PanelId) -> Option<&mut PanelState> {
        self.panels.iter_mut().find(|p| p.id == *id)
    }

    pub fn is_panel_shown(&self, id: &PanelId) -> bool {
        let Some(state) = self.panel_state(id) else {
            return false;
        };
        if !state.visible {
            return false;
        }
        if state.floated {
            return true;
        }
        match self.side_of(id) {
            Some(side) => self.columns.is_shown(side),
            None => false,
        }
    }

    pub fn has_visible_docked_panels(&self, side: DockSide) -> bool {
        self.columns.is_shown(side) && self.panels.iter().any(|panel| {
            panel.visible && !panel.floated && self.side_of(&panel.id) == Some(side)
        })
    }

    pub fn set_panel_visible(&mut self, id: &PanelId, visible: bool) {
        if let Some(state) = self.panel_state_mut(id) {
            state.visible = visible;
        } else {
            self.panels.push(PanelState {
                id: id.clone(),
                visible,
                floated: false,
                os_window: false,
                tab_group: None,
            });
        }
        if visible {
            let side = match self.side_of(id) {
                Some(side) => side,
                None => {
                    let side = default_side(id);
                    self.dock_panel(id, side);
                    side
                }
            };
            let floated = self.panel_state(id).map(|state| state.floated).unwrap_or(false);
            if !floated {
                self.columns.set(side, true);
            }
        }
        self.sync_tree_visibility();
    }

    pub fn set_side_shown(&mut self, side: DockSide, shown: bool) {
        self.columns.set(side, shown);
    }

    pub fn toggle_side(&mut self, side: DockSide) {
        let shown = self.columns.is_shown(side);
        self.columns.set(side, !shown);
    }

    pub fn hide_all(&mut self) {
        for side in DockSide::ALL {
            self.columns.set(side, false);
        }
    }

    pub fn all_hidden(&self) -> bool {
        !self.columns.any_shown()
    }

    pub fn show_left(&mut self) {
        self.set_side_shown(DockSide::Left, true);
    }

    pub fn hide_left(&mut self) {
        self.set_side_shown(DockSide::Left, false);
    }

    pub fn show_right(&mut self) {
        self.set_side_shown(DockSide::Right, true);
    }

    pub fn hide_right(&mut self) {
        self.set_side_shown(DockSide::Right, false);
    }

    pub fn show_top(&mut self) {
        self.set_side_shown(DockSide::Top, true);
    }

    pub fn hide_top(&mut self) {
        self.set_side_shown(DockSide::Top, false);
    }

    pub fn show_bottom(&mut self) {
        self.set_side_shown(DockSide::Bottom, true);
    }

    pub fn hide_bottom(&mut self) {
        self.set_side_shown(DockSide::Bottom, false);
    }

    pub fn panel_state_json(&self, id: &PanelId) -> Value {
        let state = self.panel_state(id);
        let location = self.panel_location(id);
        // Tile ids are local to a dock side. A floating panel can still
        // have a pane in that tree for when it returns to the dock.
        let tab_group = state.filter(|state| !state.floated).and(location.as_ref()).and_then(|(_, tree, pane)| {
            let parent = tree.tiles.parent_of(*pane)?;
            matches!(tree.tiles.get(parent), Some(Tile::Container(egui_tiles::Container::Tabs(_))))
                .then_some(parent.0)
        });
        json!({
            "id": id.key(),
            "visible": state.map(|s| s.visible).unwrap_or(false),
            "floated": state.map(|s| s.floated).unwrap_or(false),
            "os_window": state.map(|s| s.os_window).unwrap_or(false),
            "tab_group": tab_group,
            "side": location.map(|(side, _, _)| side.key()),
            "shown": self.is_panel_shown(id),
        })
    }

    pub fn layout_json(&self) -> Result<Value, CommandError> {
        serde_json::to_value(self).map_err(|_| CommandError::new("INTERNAL_ERROR"))
    }

    fn command_panel_set(&mut self, params: &Value) -> Result<Value, CommandError> {
        let id = params.get("id").and_then(PanelId::from_value).ok_or_else(invalid_params)?;
        if let Some(side) = params.get("side").and_then(|v| v.as_str()) {
            let side = DockSide::from_key(side).ok_or_else(invalid_params)?;
            self.dock_panel(&id, side);
        }
        if let Some(visible) = params.get("visible") {
            let visible = visible.as_bool().ok_or_else(invalid_params)?;
            self.set_panel_visible(&id, visible);
        }
        let os_window = match params.get("os_window") {
            Some(value) => Some(value.as_bool().ok_or_else(invalid_params)?),
            None => None,
        };
        match params.get("floated") {
            Some(value) => {
                let floated = value.as_bool().ok_or_else(invalid_params)?;
                if floated {
                    self.float_panel(&id, os_window.unwrap_or(false));
                } else {
                    self.redock_panel(&id);
                }
            }
            None => {
                if let Some(os_window) = os_window {
                    if let Some(state) = self.panel_state_mut(&id) {
                        if state.floated {
                            state.os_window = os_window;
                        }
                    }
                }
            }
        }
        Ok(self.panel_state_json(&id))
    }

    pub fn float_panel(&mut self, id: &PanelId, os_window: bool) {
        if self.side_of(id).is_none() {
            let side = default_side(id);
            self.dock_panel(id, side);
        }
        if let Some(state) = self.panel_state_mut(id) {
            state.visible = true;
            state.floated = true;
            state.os_window = os_window;
        }
        self.sync_tree_visibility();
    }

    pub(crate) fn float_panel_at(&mut self, id: &PanelId, pos: egui::Pos2) {
        self.float_panel(id, false);
        self.remember_floating_position(id, false, pos);
    }

    fn remember_floating_position(&mut self, id: &PanelId, native: bool, pos: egui::Pos2) {
        if !pos.x.is_finite() || !pos.y.is_finite() {
            return;
        }
        let positions = self.floating_positions.entry(id.key().to_owned()).or_default();
        let slot = if native { &mut positions.native } else { &mut positions.embedded };
        *slot = Some([pos.x, pos.y]);
    }

    pub fn redock_panel(&mut self, id: &PanelId) {
        let mut docked = false;
        if let Some(state) = self.panel_state_mut(id) {
            state.floated = false;
            state.os_window = false;
            docked = state.visible;
        }
        if docked {
            if let Some(side) = self.side_of(id) {
                self.columns.set(side, true);
            }
        }
        self.sync_tree_visibility();
    }

    pub fn redock_to(&mut self, id: &PanelId, side: DockSide) {
        self.dock_panel(id, side);
        self.redock_panel(id);
    }

    pub(crate) fn redock_into_tabs(&mut self, id: &PanelId, side: DockSide, group: TileId) {
        let Some(mut tree) = self.tree(side) else { return };
        if !matches!(tree.tiles.get(group), Some(Tile::Container(egui_tiles::Container::Tabs(_)))) {
            self.redock_to(id, side);
            return;
        }
        let tile = match pane_tile(&tree, id) {
            Some(tile) => {
                if tree.tiles.parent_of(tile) != Some(group) {
                    tree.move_tile_to_container(tile, group, usize::MAX, false);
                }
                tile
            }
            None => {
                if let Some(old_side) = self.side_of(id)
                    && let Some(mut old_tree) = self.tree(old_side)
                    && let Some(old_tile) = pane_tile(&old_tree, id)
                {
                    old_tree.remove_recursively(old_tile);
                    self.store_tree(old_side, &old_tree);
                }
                let tile = tree.tiles.insert_pane(id.clone());
                if let Some(Tile::Container(egui_tiles::Container::Tabs(tabs))) = tree.tiles.get_mut(group) {
                    tabs.add_child(tile);
                }
                tile
            }
        };
        if let Some(Tile::Container(egui_tiles::Container::Tabs(tabs))) = tree.tiles.get_mut(group) {
            tabs.set_active(tile);
        }
        self.store_tree(side, &tree);
        self.redock_panel(id);
    }

    pub(crate) fn redock_as_group(&mut self, id: &PanelId, side: DockSide, adjacent: Option<(TileId, bool)>) {
        let Some(state) = self.panel_state(id) else { return };
        if !state.visible { return; }
        let mut tree = self.tree(side).unwrap_or_else(|| side_tree(side, Vec::new()));
        let direction = match side {
            DockSide::Left | DockSide::Right => egui_tiles::LinearDir::Vertical,
            DockSide::Top | DockSide::Bottom => egui_tiles::LinearDir::Horizontal,
        };
        let source_side = self.side_of(id);
        let source = pane_tile(&tree, id);
        if let Some((target, _)) = adjacent
            && let Some(Tile::Container(egui_tiles::Container::Tabs(tabs))) = tree.tiles.get(target)
            && source.is_some_and(|source| tabs.children.as_slice() == [source])
        {
            self.redock_panel(id);
            return;
        }

        if let Some(source) = source {
            tree.remove_recursively(source);
        }
        let pane = tree.tiles.insert_pane(id.clone());
        let group = tree.tiles.insert_tab_tile(vec![pane]);
        let reference = adjacent.map(|(tile, _)| tile).filter(|tile| tree.tiles.get(*tile).is_some()).or(tree.root);
        if let Some(reference) = reference {
            let before = adjacent.is_some_and(|(_, before)| before);
            let parent = tree.tiles.parent_of(reference);
            if adjacent.is_none()
                && let Some(Tile::Container(egui_tiles::Container::Linear(linear))) = tree.tiles.get_mut(reference)
                && linear.dir == direction
            {
                linear.add_child(group);
            } else if let Some(parent) = parent
                && let Some(Tile::Container(egui_tiles::Container::Linear(linear))) = tree.tiles.get_mut(parent)
                && linear.dir == direction
                && let Some(index) = linear.children.iter().position(|child| *child == reference)
            {
                linear.children.insert(index + usize::from(!before), group);
            } else {
                let children = if before { [group, reference] } else { [reference, group] };
                let split = egui_tiles::Linear::new_binary(direction, children, 0.5);
                let split = tree.tiles.insert_new(Tile::Container(egui_tiles::Container::Linear(split)));
                if let Some(parent) = parent {
                    if let Some(Tile::Container(container)) = tree.tiles.get_mut(parent) {
                        container.replace_child(reference, split);
                    }
                } else {
                    tree.root = Some(split);
                }
            }
        } else {
            tree.root = Some(group);
        }
        if let Some(old_side) = source_side.filter(|old| *old != side)
            && let Some(mut old_tree) = self.tree(old_side)
            && let Some(old_tile) = pane_tile(&old_tree, id)
        {
            old_tree.remove_recursively(old_tile);
            self.store_tree(old_side, &old_tree);
        }
        self.store_tree(side, &tree);
        self.redock_panel(id);
    }

    pub fn floating_panels(&self) -> Vec<PanelState> {
        self.panels
            .iter()
            .filter(|state| state.visible && state.floated)
            .cloned()
            .collect()
    }

    pub fn on_header_drag_released(&mut self, id: &PanelId, pointer: egui::Pos2, dock_rect: egui::Rect) -> bool {
        if dock_rect.contains(pointer) {
            return false;
        }
        self.float_panel(id, false);
        true
    }

    pub fn show_floating(&mut self, ctx: &egui::Context, body: impl FnMut(&mut egui::Ui, &PanelId)) -> Option<(PanelId, egui::Pos2)> {
        self.show_floating_drag(ctx, body)
            .and_then(|(panel, pointer, released)| released.then_some((panel, pointer)))
    }

    pub(crate) fn show_floating_drag(&mut self, ctx: &egui::Context, mut body: impl FnMut(&mut egui::Ui, &PanelId)) -> Option<(PanelId, egui::Pos2, bool)> {
        let mut closed: Vec<PanelId> = Vec::new();
        let mut drag = None;
        for state in self.floating_panels() {
            let id = state.id.clone();
            let positions = self.floating_positions.get(id.key()).cloned().unwrap_or_default();
            if state.os_window {
                let viewport_id = egui::ViewportId::from_hash_of(("dtool.panel", id.key()));
                let title = crate::panel_ids::panel_title(&id);
                let mut builder = egui::ViewportBuilder::default()
                    .with_title(title)
                    .with_inner_size([320.0, 350.0]);
                let already_open = ctx.input(|input| input.raw.viewports.contains_key(&viewport_id));
                let observed_key = egui::Id::new(("dtool.panel.observed_native_position", id.key()));
                if let Some(pos) = positions.native {
                    if !already_open {
                        builder = builder.with_position(pos);
                    } else if ctx.data(|data| data.get_temp::<[f32; 2]>(observed_key)) != Some(pos) {
                        ctx.send_viewport_cmd_to(viewport_id, egui::ViewportCommand::OuterPosition(pos.into()));
                    }
                }
                let root = ctx.input(|input| input.viewport().inner_rect).map(|rect| (rect.min, ctx.pixels_per_point()));
                let mut outer_position = None;
                let close = ctx.show_viewport_immediate(viewport_id, builder, |ctx, class| {
                    if class != egui::ViewportClass::EmbeddedWindow {
                        outer_position = ctx.input(|input| input.viewport().outer_rect.map(|rect| rect.min));
                    }
                    let mut dock = false;
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let label = dtool_strings::text("menu.window.arrange.dock");
                        let response = ui.add(egui::Button::new(label).sense(egui::Sense::click_and_drag()));
                        dock = response.clicked();
                        if let Some((origin, root_scale)) = root
                            && let Some((pointer, _, released)) = floating_header_drag(ui.ctx(), response.id, &response, false)
                            && let Some(inner) = ui.input(|input| input.viewport().inner_rect)
                        {
                            let root_pos = (inner.min + pointer.to_vec2()).to_vec2() * (ui.ctx().pixels_per_point() / root_scale) - origin.to_vec2();
                            drag = Some((id.clone(), root_pos.to_pos2(), released));
                        }
                        body(ui, &id);
                    });
                    dock || ctx.input(|input| input.viewport().close_requested())
                });
                if let Some(pos) = outer_position {
                    self.remember_floating_position(&id, true, pos);
                    ctx.data_mut(|data| data.insert_temp(observed_key, [pos.x, pos.y]));
                }
                if close {
                    closed.push(id);
                }
            } else {
                let title = dtool_strings::text(&format!("panel.{}", id.key())).to_string();
                let mut open = true;
                let window_id = egui::Id::new(("dtool.panel.float", id.key()));
                let mut window = egui::Window::new(title).id(window_id).open(&mut open);
                if let Some(pos) = positions.embedded.map(egui::Pos2::from)
                    && ctx.memory(|memory| memory.area_rect(window_id).map(|rect| rect.min)) != Some(pos)
                {
                    // The title-bar drag handler keeps the existing position on this frame.
                    window = window.drag_area(egui::WindowDrag::Off).current_pos(pos);
                }
                if let Some(size) = floating_resize(ctx, window_id) {
                    window = window.fixed_size(size);
                }
                if let Some(shown) = window.show(ctx, |ui| body(ui, &id)) {
                    let rect = shown.response.rect;
                    let resize_area = shown.inner.is_some().then_some(rect);
                    ctx.data_mut(|data| {
                        data.insert_temp(window_id.with("resize_area"), resize_area);
                    });
                    if let Some(rect) = resize_area {
                        let painter = ctx.layer_painter(shown.response.layer_id);
                        let corner = rect.right_bottom() - egui::vec2(4.0, 4.0);
                        let stroke = egui::Stroke::new(1.0, dtool_design::color("icon").gamma_multiply(0.7));
                        for length in [4.0, 8.0, 12.0] {
                            painter.line_segment([
                                corner - egui::vec2(length, 0.0),
                                corner - egui::vec2(0.0, length),
                            ], stroke);
                        }
                    }
                    self.remember_floating_position(&id, false, rect.min);
                    if open && let Some((pointer, position, released)) = floating_header_drag(ctx, window_id, &shown.response, true) {
                        if released {
                            // A press and release can arrive in one frame without moving the egui window.
                            self.remember_floating_position(&id, false, position);
                            ctx.request_repaint();
                        }
                        drag = Some((id.clone(), pointer, released));
                    }
                }
                if !open {
                    closed.push(id);
                }
            }
        }
        let refocus = closed.iter().any(|id| self.panel_state(id).is_some_and(|state| state.os_window));
        for id in closed {
            self.redock_panel(&id);
        }
        if refocus {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
        }
        drag
    }

    fn command_panel_float(&mut self, params: &Value) -> Result<Value, CommandError> {
        let id = params.get("id").and_then(PanelId::from_value).ok_or_else(invalid_params)?;
        let os_window = match params.get("os_window") {
            Some(value) => value.as_bool().ok_or_else(invalid_params)?,
            None => false,
        };
        self.float_panel(&id, os_window);
        Ok(self.panel_state_json(&id))
    }

    pub fn run_command(&mut self, name: &str, params: &Value) -> Option<Result<Value, CommandError>> {
        match name {
            "ui.layout.get" => Some(self.layout_json()),
            "ui.layout.set" => Some(match params.get("layout") {
                Some(value) => match serde_json::from_value::<Layout>(value.clone()) {
                    Ok(layout) => {
                        self.set(layout);
                        self.layout_json()
                    }
                    Err(_) => Err(invalid_params()),
                },
                None => Err(invalid_params()),
            }),
            "ui.panel.state" => Some(match params.get("id").and_then(PanelId::from_value) {
                Some(id) if self.panel_state(&id).is_some() => Ok(self.panel_state_json(&id)),
                Some(_) => Err(not_found()),
                None => Err(invalid_params()),
            }),
            "ui.panel.set" => Some(self.command_panel_set(params)),
            "ui.panel.float" => Some(self.command_panel_float(params)),
            _ => None,
        }
    }

    pub fn reset_workspace(&mut self) {
        *self = Layout::default();
    }

    pub fn hide_panels(&mut self) {
        self.hide_all();
    }

    pub fn load_preset(&mut self, preset: LayoutPreset) {
        *self = preset.layout;
        self.sync_tree_visibility();
    }

    fn canonicalise(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut panels = Vec::new();
        for mut state in std::mem::take(&mut self.panels) {
            state.id = crate::panel_ids::canonical(state.id);
            if seen.insert(state.id.clone()) {
                panels.push(state);
            }
        }
        self.panels = panels;
        for side in DockSide::ALL {
            let Some(mut tree) = self.tree(side) else { continue };
            let tile_ids: Vec<_> = tree.tiles.iter().map(|(id, _)| *id).collect();
            for tile_id in tile_ids {
                if let Some(Tile::Pane(pane)) = tree.tiles.get_mut(tile_id) {
                    *pane = crate::panel_ids::canonical(pane.clone());
                }
            }
            let mut seen_panes = std::collections::HashSet::new();
            let mut drop_ids = Vec::new();
            for (tile_id, tile) in tree.tiles.iter() {
                if let Tile::Pane(pane) = tile {
                    if !seen_panes.insert(pane.key().to_string()) {
                        drop_ids.push(*tile_id);
                    }
                }
            }
            for tile_id in drop_ids {
                tree.remove_recursively(tile_id);
            }
            self.store_tree(side, &tree);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct FloatingResize {
    start: egui::Pos2,
    size: egui::Vec2,
    axes: egui::Vec2,
}

fn floating_resize(ctx: &egui::Context, id: egui::Id) -> Option<egui::Vec2> {
    let rect = ctx.data(|data| data.get_temp::<Option<egui::Rect>>(id.with("resize_area"))).flatten()?;
    let drag_key = id.with("resize_drag");
    let (press, release, pointer, down, active) = ctx.input(|input| {
        let mut press = None;
        let mut release = None;
        for event in &input.events {
            if let egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } = event {
                if *pressed { press = Some(*pos); } else { release = Some(*pos); }
            }
        }
        (press, release, input.pointer.interact_pos(), input.pointer.primary_down(),
            input.focused && !input.key_pressed(egui::Key::Escape))
    });
    let edges = |point: egui::Pos2| {
        let corner = egui::Rect::from_min_max(rect.right_bottom() - egui::Vec2::splat(16.0), rect.right_bottom());
        let wide = corner.contains(point) || rect.right() - point.x < 5.0;
        let tall = corner.contains(point) || rect.bottom() - point.y < 5.0;
        egui::vec2(f32::from(wide), f32::from(tall))
    };
    let layer = egui::LayerId::new(egui::Order::Middle, id);
    if let Some(start) = press.filter(|point| rect.contains(*point) && ctx.layer_id_at(*point) == Some(layer)) {
        let axes = edges(start);
        if axes != egui::Vec2::ZERO {
            ctx.data_mut(|data| data.insert_temp(drag_key, FloatingResize { start, size: rect.size(), axes }));
        }
    }
    let drag = if !active || !down || release.is_some() {
        ctx.data_mut(|data| data.remove_temp::<FloatingResize>(drag_key))
    } else {
        ctx.data(|data| data.get_temp::<FloatingResize>(drag_key))
    };
    let cursor = |axes: egui::Vec2| match (axes.x > 0.0, axes.y > 0.0) {
        (true, true) => egui::CursorIcon::ResizeNwSe,
        (true, false) => egui::CursorIcon::ResizeHorizontal,
        (false, true) => egui::CursorIcon::ResizeVertical,
        (false, false) => egui::CursorIcon::Default,
    };
    if let Some(point) = pointer.filter(|point| rect.contains(*point) && ctx.layer_id_at(*point) == Some(layer)) {
        let axes = edges(point);
        if axes != egui::Vec2::ZERO {
            ctx.set_cursor_icon(cursor(axes));
        }
    }
    let drag = drag.filter(|_| active)?;
    let end = release.or(pointer.filter(|_| down))?;
    let movement = end - drag.start;
    let delta = egui::vec2(movement.x * drag.axes.x, movement.y * drag.axes.y);
    if delta.length() < 2.0 {
        return None;
    }
    ctx.set_cursor_icon(cursor(drag.axes));
    Some((drag.size + delta).max(egui::vec2(160.0, 130.0)).min(ctx.content_rect().max - rect.min))
}

fn floating_header_drag(
    ctx: &egui::Context,
    id: egui::Id,
    response: &egui::Response,
    embedded: bool,
) -> Option<(egui::Pos2, egui::Pos2, bool)> {
    let key = id.with("header_drag");
    let mut title = response.rect;
    title.max.y = title.max.y.min(title.min.y + dtool_design::size("panel.header.height"));
    // The native window may have moved on this frame. Hit-test presses against
    // the title rectangle that was displayed when the press occurred.
    let rect_key = id.with("last_title_rect");
    let previous = ctx.data_mut(|data| {
        let previous = data.get_temp::<egui::Rect>(rect_key).unwrap_or(title);
        data.insert_temp(rect_key, title);
        previous
    });
    let (press, release, pointer, down, active) = ctx.input(|input| {
        let mut press = None;
        let mut release = None;
        for event in &input.events {
            if let egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } = event {
                if *pressed { press = Some(*pos); } else { release = Some(*pos); }
            }
        }
        (press, release, input.pointer.interact_pos(), input.pointer.primary_down(),
            input.focused && !input.key_pressed(egui::Key::Escape))
    });
    let drag_strip = if embedded {
        // Leave the collapse and close buttons at either end of the title bar clickable.
        previous.shrink2(egui::vec2(previous.height(), 0.0))
    } else {
        previous
    };
    if let Some(press) = press.filter(|pos| drag_strip.contains(*pos))
        && ctx.layer_id_at(press) == Some(response.layer_id)
    {
        ctx.data_mut(|data| data.insert_temp(key, (press, previous.min)));
    }
    let origin = if !active || !down || release.is_some() {
        ctx.data_mut(|data| data.remove_temp::<(egui::Pos2, egui::Pos2)>(key))
    } else {
        ctx.data(|data| data.get_temp::<(egui::Pos2, egui::Pos2)>(key))
    };
    let (start, window_pos) = origin.filter(|_| active)?;
    let end = release.or(pointer.filter(|_| down))?;
    (start.distance(end) > 6.0).then_some((end, window_pos + (end - start), release.is_some()))
}

impl Layout {
    pub fn last_layout_path() -> std::path::PathBuf {
        crate::settings::UiSettings::default_path().with_file_name("layout.json")
    }

    pub fn workspace_default_path() -> std::path::PathBuf {
        crate::settings::UiSettings::default_path().with_file_name("workspace_default.json")
    }

    pub fn load_file(path: &std::path::Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let mut layout: Self = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
        layout.canonicalise();
        layout.sync_tree_visibility();
        Ok(layout)
    }

    pub fn save_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        let temporary = path.with_extension("json.tmp");
        let result = (|| {
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, path)
        })();
        if result.is_err() { let _ = std::fs::remove_file(&temporary); }
        result
    }
}

pub fn ensure_active_tabs<P>(tiles: &mut Tiles<P>) {
    let fixes: Vec<(TileId, Option<TileId>)> = tiles
        .iter()
        .filter_map(|(id, tile)| match tile {
            Tile::Container(egui_tiles::Container::Tabs(tabs)) => {
                let next = tabs.next_active(tiles);
                (next != tabs.active).then_some((*id, next))
            }
            _ => None,
        })
        .collect();
    for (id, next) in fixes {
        if let Some(Tile::Container(egui_tiles::Container::Tabs(tabs))) = tiles.get_mut(id) {
            tabs.active = next;
        }
    }
}
