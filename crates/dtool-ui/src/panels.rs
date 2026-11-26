pub mod books;
pub mod colour;
pub mod character;
pub mod fields;
pub mod anchors;
pub mod hyperlinks;
pub mod index;
pub mod layer_states;
pub mod layers;
pub mod notes;
pub mod paragraph;
pub mod pages;
pub mod toc;
pub mod swatches;
pub mod table;
pub mod table_formats;
pub mod text_frame;
pub mod xrefs;
pub mod session_route;
pub mod glyph_browser;
pub mod text_styles;
pub mod assets;
pub mod symbols;
mod constraints;
pub mod styles;
pub mod text_wrap;

pub(crate) struct ThumbnailJob<Revision, Images> {
    pub(crate) revision: Revision,
    pub(crate) result: std::sync::mpsc::Receiver<Images>,
}

impl<Revision, Images: Send + 'static> ThumbnailJob<Revision, Images> {
    pub(crate) fn start(
        ctx: &egui::Context, name: &str, revision: Revision,
        render: impl FnOnce() -> Images + Send + 'static,
    ) -> std::io::Result<Self> {
        let (sender, result) = std::sync::mpsc::channel();
        let wake = ctx.clone();
        let viewport = ctx.viewport_id();
        std::thread::Builder::new().name(name.into()).spawn(move || {
            let images = render();
            let _ = sender.send(images);
            wake.request_repaint_of(viewport);
        })?;
        Ok(Self { revision, result })
    }
}
pub mod quick_fx;
pub mod appearance;
pub mod path_brushes;
pub mod pixel_brushes;

use crate::dock::{DockSide, Layout, PanelId};
use crate::presets::PresetStore;
use std::path::Path;
use crate::panel_ids::{all_panel_ids, named_panel_ids, panel_registry, panel_title, remaining_panel_ids, PanelEntry};
use crate::transform_panel::TransformPanel;
use dtool_design::{panel_frame, panel_header};
use dtool_protocol::CommandError;
use egui_tiles::{Behavior, Tile, TileId, UiResponse};
use serde_json::{json, Value};

pub type PanelBody = Box<dyn FnMut(&mut egui::Ui)>;

pub const TRANSFORM_PANEL_KEY: &str = "transform";

#[derive(Default)]
pub struct PanelRegistry {
    bodies: Vec<(PanelId, PanelBody)>,
    pub swatches: swatches::SwatchesPanel,
    pub paragraph: paragraph::ParagraphPanel,
    pub character: character::CharacterPanel,
    pub text_frame: text_frame::TextFramePanel,
    pub table: table::TablePanel,
    pub table_formats: table_formats::TableFormatsPanel,
    pub layers: layers::LayersPanel,
    pub layer_states: layer_states::LayerStatesPanel,
    pub stroke: crate::stroke_panel::StrokePanel,
    pub glyph_browser: glyph_browser::GlyphBrowserState,
    pub colour: colour::ColourPanel,
    pub assets: assets::AssetsPanel,
    pub symbols: symbols::SymbolsPanel,
    constraints: constraints::ConstraintsPanel,
    pub styles: styles::StylesPanel,
    pub path_brushes: path_brushes::PathBrushesPanel,
    pub pixel_brushes: pixel_brushes::Panel,
    pub text_wrap: text_wrap::TextWrapPanel,
    pub quick_fx: quick_fx::QuickFxPanel,
    pub text_styles: text_styles::TextStylesPanel,
    pub transform: TransformPanel,
    pub navigator: crate::navigator::NavigatorPanel,
    pub pages: pages::PagesPanel,
    pub canvas_size: [f32; 2],
}

pub const WINDOW_SWATCHES: &str = "window.general.swatches";
pub const WINDOW_COLOUR: &str = "window.general.colour";
pub const WINDOW_PARAGRAPH: &str = "window.text.paragraph";
pub const WINDOW_CHARACTER: &str = "window.text.character";
pub const WINDOW_TEXT_FRAME: &str = "window.text.text_frame";
pub const TEXT_FRAME_PANEL_KEY: &str = "text_frame";
pub const WINDOW_GLYPH_BROWSER: &str = "window.text.glyph_browser";
pub const WINDOW_ASSETS: &str = "window.general.assets";
pub const WINDOW_STYLES: &str = "window.vector.styles";
pub const WINDOW_TEXT_WRAP: &str = "text.text_wrap.show_text_wrap_settings";
pub const WINDOW_TEXT_STYLES: &str = text_styles::WINDOW_TEXT_STYLES;
pub const WINDOW_TABLE: &str = "window.table.table";
pub const WINDOW_TABLE_FORMATS: &str = "window.table.table_formats";

pub fn window_panel(action: &str) -> Option<PanelId> {
    match action {
        symbols::WINDOW_PANEL => Some(PanelId::Other(symbols::PANEL_KEY.to_string())),
        constraints::WINDOW_PANEL => Some(PanelId::Other(constraints::PANEL_KEY.to_string())),
        pixel_brushes::WINDOW_PANEL => Some(PanelId::Other(pixel_brushes::PANEL_KEY.to_owned())),
        path_brushes::WINDOW_PANEL => Some(PanelId::Other(path_brushes::PANEL_KEY.to_string())),
        "window.vector.appearance" => Some(PanelId::Other(appearance::PANEL_KEY.to_string())),
        "window.vector.quick_fx" => Some(PanelId::Effects),
        "window.layout.pages" => Some(PanelId::Pages),
        WINDOW_SWATCHES => Some(PanelId::Swatches),
        WINDOW_COLOUR => Some(PanelId::Colour),
        WINDOW_PARAGRAPH => Some(PanelId::Paragraph),
        WINDOW_CHARACTER => Some(PanelId::Character),
        WINDOW_TEXT_FRAME => Some(PanelId::Other(TEXT_FRAME_PANEL_KEY.to_string())),
        WINDOW_GLYPH_BROWSER => Some(PanelId::Other(glyph_browser::GLYPH_BROWSER_PANEL_KEY.to_string())),
        WINDOW_ASSETS => Some(PanelId::Other(assets::ASSETS_PANEL_KEY.to_string())),
        WINDOW_STYLES => Some(PanelId::Other(styles::STYLES_PANEL_KEY.to_string())),
        WINDOW_TEXT_WRAP => Some(PanelId::Other(text_wrap::TEXT_WRAP_PANEL_KEY.to_string())),
        WINDOW_TEXT_STYLES => Some(PanelId::Other(text_styles::TEXT_STYLES_PANEL_KEY.to_string())),
        WINDOW_TABLE => Some(PanelId::Other(table::PANEL_KEY.to_string())),
        WINDOW_TABLE_FORMATS => Some(PanelId::Other(table_formats::PANEL_KEY.to_string())),
        _ => None,
    }
}

pub const WINDOW_PANELS: &[&str] = &[symbols::WINDOW_PANEL, path_brushes::WINDOW_PANEL, pixel_brushes::WINDOW_PANEL, "window.vector.appearance", "window.vector.quick_fx", "window.layout.pages", WINDOW_SWATCHES, WINDOW_COLOUR, WINDOW_PARAGRAPH, WINDOW_CHARACTER, WINDOW_TEXT_FRAME, WINDOW_GLYPH_BROWSER, WINDOW_ASSETS, WINDOW_STYLES, WINDOW_TEXT_WRAP, WINDOW_TEXT_STYLES, WINDOW_TABLE, WINDOW_TABLE_FORMATS];

thread_local! {
    static SHOWN_WINDOW_PANELS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn record_window_panels(layout: &Layout) {
    let shown: Vec<String> = WINDOW_PANELS.iter().filter(|action| window_panel(action).is_some_and(|id| layout.is_panel_shown(&id))).map(|action| action.to_string()).collect();
    SHOWN_WINDOW_PANELS.with(|cell| *cell.borrow_mut() = shown);
}

pub fn window_panel_checked(action: &str) -> bool {
    SHOWN_WINDOW_PANELS.with(|cell| cell.borrow().iter().any(|item| item == action))
}

pub fn toggle_panel(layout: &mut Layout, id: &PanelId) -> bool {
    let shown = if layout.is_panel_shown(id) {
        layout.set_panel_visible(id, false);
        layout.sync_tree_visibility();
        false
    } else {
        open_panel(layout, id);
        layout.sync_tree_visibility();
        true
    };
    record_window_panels(layout);
    shown
}

pub fn workspace_panel(ui: &mut egui::Ui, id: &PanelId, registry: &mut PanelRegistry, workspace: &mut dtool_core::workspace::Workspace) -> bool {
    match id {
        PanelId::Other(key) if key == pixel_brushes::PANEL_KEY => {
            registry.pixel_brushes.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == path_brushes::PANEL_KEY => {
            registry.path_brushes.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == appearance::PANEL_KEY => {
            appearance::show(ui, workspace);
            true
        }
        PanelId::Pages => {
            let view_size = workspace.active().and_then(|id| workspace.get(id))
                .and_then(|context| context.session.view_canvas_size).unwrap_or(registry.canvas_size);
            if let Some(command) = registry.pages.show(ui, workspace.session(), view_size) {
                if let Err(error) = workspace.run(dtool_core::Scope::active(), command).1 {
                    ui.weak(dtool_strings::text(&error.message_key));
                }
            }
            true
        }
        PanelId::Navigator => {
            let commands = std::cell::RefCell::new(Vec::new());
            let view_size = workspace.session().and_then(|session| session.view_canvas_size)
                .unwrap_or(registry.canvas_size).map(|n| n.max(1.0));
            registry.navigator.show(ui, workspace.session(), view_size,
                &|delta| commands.borrow_mut().push(dtool_core::Command::with_params("view.pan", json!({"delta":delta}))),
                &|zoom| commands.borrow_mut().push(dtool_core::Command::with_params("view.zoom.set", json!({"zoom":zoom, "view_size":view_size}))),
                &|show| commands.borrow_mut().push(dtool_core::Command::with_params("guides.show", json!({"show":show}))),
                &|show| commands.borrow_mut().push(dtool_core::Command::with_params("view.flags.set", json!({"show_bleed":show}))));
            if let Some(session) = workspace.session_mut() {
                let before = (session.view.zoom, session.view.scroll);
                for command in commands.into_inner() {
                    if let Err(error) = session.run(&command) { ui.label(dtool_strings::text(&error.message_key)); }
                }
                registry.navigator.navigation_changed |= before != (session.view.zoom, session.view.scroll);
            }
            true
        }
        PanelId::Other(key) if key == TRANSFORM_PANEL_KEY => {
            let active = workspace.active();
            if let (Some(context), Some(session)) = (active, workspace.session_mut()) {
                registry.transform.bind_selection(ui, Some((context, session.selection.ids.clone())));
                registry.transform.show_with_session(ui, session);
            } else {
                registry.transform.bind_selection(ui, None);
                ui.add_enabled_ui(false, |ui| { registry.transform.show(ui); });
            }
            true
        }
        PanelId::Swatches => {
            registry.swatches.show_with_colour(ui, workspace, &mut registry.colour);
            true
        }
        PanelId::Paragraph => {
            registry.paragraph.show(ui, workspace);
            true
        }
        PanelId::Character => {
            registry.character.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == TEXT_FRAME_PANEL_KEY => {
            registry.text_frame.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == table::PANEL_KEY => {
            registry.table.show(ui, workspace.session_mut());
            true
        }
        PanelId::Other(key) if key == table_formats::PANEL_KEY => {
            registry.table_formats.show(ui, workspace);
            true
        }
        PanelId::Effects => { registry.quick_fx.show(ui, workspace); true }
        PanelId::Layers => {
            registry.layers.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == glyph_browser::GLYPH_BROWSER_PANEL_KEY => {
            registry.glyph_browser.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == layer_states::LAYER_STATES_PANEL_KEY => {
            registry.layer_states.show(ui, workspace);
            true
        }
        PanelId::Stroke => {
            registry.stroke.render_mut(ui, workspace);
            true
        }
        PanelId::Colour => {
            registry.colour.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == constraints::PANEL_KEY => {
            registry.constraints.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == symbols::PANEL_KEY => {
            registry.symbols.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == assets::ASSETS_PANEL_KEY => {
            registry.assets.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == styles::STYLES_PANEL_KEY => {
            registry.styles.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == text_wrap::TEXT_WRAP_PANEL_KEY => {
            registry.text_wrap.show(ui, workspace);
            true
        }
        PanelId::Other(key) if key == text_styles::TEXT_STYLES_PANEL_KEY => {
            registry.text_styles.show(ui, workspace);
            true
        }
        _ => false,
    }
}

impl PanelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_existing_bodies() -> Self {
        let mut registry = Self::new();
        let mut transform = TransformPanel::default();
        registry.register(
            PanelId::Other(TRANSFORM_PANEL_KEY.to_string()),
            Box::new(move |ui| {
                transform.show(ui);
            }),
        );
        registry
    }

    pub fn register(&mut self, id: PanelId, body: PanelBody) {
        match self.bodies.iter_mut().find(|(known, _)| *known == id) {
            Some(slot) => slot.1 = body,
            None => self.bodies.push((id, body)),
        }
    }

    pub fn has_body(&self, id: &PanelId) -> bool {
        matches!(id, PanelId::Layers | PanelId::Navigator | PanelId::Effects | PanelId::Swatches | PanelId::Paragraph | PanelId::Character | PanelId::Stroke | PanelId::Colour) || matches!(id, PanelId::Other(key) if key == TEXT_FRAME_PANEL_KEY || key == glyph_browser::GLYPH_BROWSER_PANEL_KEY || key == assets::ASSETS_PANEL_KEY || key == symbols::PANEL_KEY || key == constraints::PANEL_KEY || key == styles::STYLES_PANEL_KEY || key == path_brushes::PANEL_KEY || key == pixel_brushes::PANEL_KEY || key == text_wrap::TEXT_WRAP_PANEL_KEY || key == text_styles::TEXT_STYLES_PANEL_KEY || key == table::PANEL_KEY || key == table_formats::PANEL_KEY) || self.bodies.iter().any(|(known, _)| known == id)
    }

    pub fn ids(&self) -> Vec<PanelId> {
        let mut ids = all_panel_ids();
        ids.push(PanelId::Other(path_brushes::PANEL_KEY.to_string()));
        ids.push(PanelId::Other(pixel_brushes::PANEL_KEY.to_owned()));
        let text_styles_id = PanelId::Other(text_styles::TEXT_STYLES_PANEL_KEY.to_string());
        if !ids.iter().any(|id| id.key() == text_styles::TEXT_STYLES_PANEL_KEY) {
            ids.push(text_styles_id);
        }
        for (id, _) in &self.bodies {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }

    pub fn entries(&self) -> Vec<PanelEntry> {
        let mut extra: Vec<PanelId> = self.bodies.iter().map(|(id, _)| id.clone()).collect();
        extra.push(PanelId::Other(path_brushes::PANEL_KEY.to_string()));
        extra.push(PanelId::Other(pixel_brushes::PANEL_KEY.to_owned()));
        extra.push(PanelId::Other(constraints::PANEL_KEY.to_string()));
        extra.push(PanelId::Other(table::PANEL_KEY.to_string()));
        extra.push(PanelId::Other(table_formats::PANEL_KEY.to_string()));
        let text_styles_id = PanelId::Other(text_styles::TEXT_STYLES_PANEL_KEY.to_string());
        if !extra.iter().any(|id| id.key() == text_styles::TEXT_STYLES_PANEL_KEY) {
            extra.push(text_styles_id);
        }
        panel_registry(&extra)
    }

    pub fn show_body(&mut self, ui: &mut egui::Ui, id: &PanelId) -> bool {
        match self.bodies.iter_mut().find(|(known, _)| known == id) {
            Some((_, body)) => {
                body(ui);
                true
            }
            None => false,
        }
    }
}

fn sense_header_drag(ui: &mut egui::Ui, id: &PanelId) -> Option<(PanelId, egui::Pos2)> {
    let response = panel_header(ui, panel_title(id)).interact(egui::Sense::drag());
    if response.drag_stopped() {
        response
            .interact_pointer_pos()
            .or_else(|| ui.ctx().pointer_latest_pos())
            .map(|pos| (id.clone(), pos))
    } else {
        None
    }
}

fn apply_header_release(layout: &mut Layout, release: Option<(PanelId, egui::Pos2)>, dock_rect: egui::Rect) {
    if let Some((id, pointer)) = release {
        layout.on_header_drag_released(&id, pointer, dock_rect);
    }
}

pub fn panel_chrome(ui: &mut egui::Ui, id: &PanelId, registry: &mut PanelRegistry) -> Option<(PanelId, egui::Pos2)> {
    let mut release = None;
    panel_frame(ui, |ui| {
        release = sense_header_drag(ui, id);
        registry.show_body(ui, id);
    });
    release
}

// Every studio tree, including the session-backed tree, uses the same chrome.
macro_rules! studio_chrome {
    () => {
        fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
            egui_tiles::SimplificationOptions { all_panes_must_have_tabs: true, ..Default::default() }
        }
        fn tab_bar_height(&self, _: &egui::Style) -> f32 { dtool_design::size("panel.header.height") }
        fn gap_width(&self, _: &egui::Style) -> f32 { 3.0 }
        fn tab_bar_color(&self, visuals: &egui::Visuals) -> egui::Color32 { visuals.panel_fill.lerp_to_gamma(egui::Color32::BLACK, 0.23) }
        fn tab_bg_color(&self, visuals: &egui::Visuals, _: &egui_tiles::Tiles<PanelId>, _: TileId, state: &egui_tiles::TabState) -> egui::Color32 {
            if state.active { visuals.panel_fill } else { egui::Color32::TRANSPARENT }
        }
        fn tab_outline_stroke(&self, visuals: &egui::Visuals, _: &egui_tiles::Tiles<PanelId>, _: TileId, state: &egui_tiles::TabState) -> egui::Stroke {
            if state.active { egui::Stroke::NONE } else { egui::Stroke::new(1.0, visuals.panel_fill) }
        }
        fn tab_bar_hline_stroke(&self, visuals: &egui::Visuals) -> egui::Stroke { egui::Stroke::new(1.0, visuals.panel_fill.lerp_to_gamma(egui::Color32::BLACK, 0.38)) }
    };
}

pub struct DockBehavior<'a> {
    pub registry: &'a mut PanelRegistry,
    pub header_release: &'a mut Option<(PanelId, egui::Pos2)>,
}

impl Behavior<PanelId> for DockBehavior<'_> {
    studio_chrome!();
    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile_id: TileId, pane: &mut PanelId) -> UiResponse {
        let id = pane.clone();
        panel_frame(ui, |ui| {
            if let Some(release) = sense_header_drag(ui, &id) {
                *self.header_release = Some(release);
            }
            self.registry.show_body(ui, &id);
        });
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &PanelId) -> egui::WidgetText {
        panel_title(pane).into()
    }
}

pub fn register_named_panels(layout: &mut Layout, registry: &PanelRegistry) {
    for id in registry.ids() {
        if layout.panel_state(&id).is_none() {
            let side = crate::dock::default_side(&id);
            layout.dock_panel(&id, side);
            if let Some(state) = layout.panel_state_mut(&id) {
                state.visible = false;
            }
        }
    }
    layout.sync_tree_visibility();
}

pub fn show_side(ui: &mut egui::Ui, layout: &mut Layout, side: DockSide, registry: &mut PanelRegistry) {
    if !layout.columns.is_shown(side) {
        return;
    }
    if let Some(mut tree) = layout.tree(side) {
        let mut header_release = None;
        let mut behavior = DockBehavior { registry, header_release: &mut header_release };
        let dock_rect = ui.max_rect();
        tree.ui(&mut behavior, ui);
        apply_header_release(layout, header_release, dock_rect);
        layout.store_tree(side, &tree);
    }
}

pub fn focus_panel(layout: &mut Layout, id: &PanelId) -> bool {
    let Some(side) = layout.side_of(id) else {
        return false;
    };
    let Some(mut tree) = layout.tree(side) else {
        return false;
    };
    let focused = tree.make_active(|_, tile| matches!(tile, Tile::Pane(pane) if pane == id));
    layout.store_tree(side, &tree);
    focused
}

pub fn open_panel(layout: &mut Layout, id: &PanelId) {
    layout.set_panel_visible(id, true);
    focus_panel(layout, id);
}

pub fn run_command(layout: &mut Layout, registry: &PanelRegistry, name: &str, params: &Value) -> Option<Result<Value, CommandError>> {
    let panel = || {
        params
            .get("id")
            .and_then(PanelId::from_value)
            .ok_or_else(|| CommandError::new("INVALID_PARAMS"))
    };
    let addresses_panel = matches!(name, "ui.panel.open" | "ui.panel.query" | "ui.panel.state" | "ui.panel.float")
        || (name == "ui.panel.set" && !dtool_core::commands::palettes::is_swatches_set(params));
    if addresses_panel {
        if let Some(id) = params.get("id").and_then(PanelId::from_value) {
            let known = registry.entries().iter().any(|entry| entry.id.key() == id.key()) || layout.panel_state(&id).is_some();
            if !known {
                return Some(Err(CommandError::new("NOT_FOUND")));
            }
        }
    }
    match name {
        "ui.panel.list" => Some(Ok(json!({
            "panels": registry
                .entries()
                .iter()
                .map(|entry| layout.panel_state_json(&entry.id))
                .collect::<Vec<Value>>()
        }))),
        "ui.panel.open" => Some(panel().map(|id| {
            open_panel(layout, &id);
            layout.panel_state_json(&id)
        })),
        "ui.panel.query" => Some(panel().map(|id| layout.panel_state_json(&id))),
        "ui.panel.set" if dtool_core::commands::palettes::is_swatches_set(params) => Some(dtool_core::commands::palettes::apply_view_set(&params["set"]).and_then(|state| {
            let id = PanelId::Swatches;
            let mut layout_params = params.clone();
            layout_params["id"] = json!(dtool_core::commands::palettes::SWATCHES_PANEL);
            if ["side", "visible", "floated", "os_window"].iter().any(|key| params.get(*key).is_some()) {
                layout.run_command(name, &layout_params).unwrap_or_else(|| Ok(Value::Null))?;
            }
            Ok(json!({ "panel": dtool_core::commands::palettes::SWATCHES_PANEL, "state": state, "layout": layout.panel_state_json(&id) }))
        })),
        "ui.panel.state" | "ui.panel.set" => layout.run_command(name, params),
        _ => None,
    }
}

pub fn run_ui_dock_command(
    layout: &mut Layout,
    registry: &PanelRegistry,
    store: &mut PresetStore,
    name: &str,
    params: &Value,
    persist_path: Option<&Path>,
) -> Option<Result<Value, CommandError>> {
    let result = run_command(layout, registry, name, params)
        .or_else(|| layout.run_command(name, params))
        .or_else(|| store.run_command(name, params, layout));
    if matches!(result, Some(Ok(_))) {
        layout.ensure_active_tabs();
    }
    if let (Some(path), Some(Ok(_))) = (persist_path, result.as_ref()) {
        if matches!(name, "ui.preset.save" | "ui.preset.delete") {
            let _ = store.persist(path);
        }
    }
    result
}

struct StrokeDockBehavior<'a> {
    registry: &'a mut PanelRegistry,
    stroke: &'a mut crate::stroke_panel::StrokePanel,
    workspace: &'a mut dtool_core::workspace::Workspace,
}
impl Behavior<PanelId> for StrokeDockBehavior<'_> {
    studio_chrome!();
    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile_id: TileId, pane: &mut PanelId) -> UiResponse {
        if *pane == PanelId::Stroke { self.stroke.render_mut(ui, self.workspace); }
        else if !workspace_panel(ui, pane, self.registry, self.workspace) { panel_chrome(ui, pane, self.registry); }
        UiResponse::None
    }
    fn tab_title_for_pane(&mut self, pane: &PanelId) -> egui::WidgetText { panel_title(pane).into() }
}

pub fn show_side_with_stroke(ui: &mut egui::Ui, layout: &mut Layout, side: DockSide, registry: &mut PanelRegistry, stroke: &mut crate::stroke_panel::StrokePanel, workspace: &mut dtool_core::workspace::Workspace) {
    if !layout.columns.is_shown(side) { return; }
    if let Some(mut tree) = layout.tree(side) {
        tree.ui(&mut StrokeDockBehavior { registry, stroke, workspace }, ui);
        layout.store_tree(side, &tree);
    }
}

pub fn show_floating_with_stroke(ctx: &egui::Context, layout: &mut Layout, registry: &mut PanelRegistry, stroke: &mut crate::stroke_panel::StrokePanel, workspace: &mut dtool_core::workspace::Workspace) {
    layout.show_floating(ctx, |ui, id| {
        if *id == PanelId::Stroke { stroke.render_mut(ui, workspace); }
        else if !workspace_panel(ui, id, registry, workspace) { panel_chrome(ui, id, registry); }
    });
}

pub const SNAPSHOTS_PANEL_KEY: &str = "snapshots";

pub struct SessionPanels<'a> {
    pub slices: &'a mut crate::slices_panel::SlicesPanel,
    pub history: &'a mut crate::history_panel::HistoryPanel,
    pub snapshots: &'a mut crate::snapshots_panel::SnapshotsPanel,
}

impl SessionPanels<'_> {
    pub fn show(&mut self, ui: &mut egui::Ui, id: &PanelId, registry: &mut PanelRegistry, workspace: &mut dtool_core::workspace::Workspace, header_release: &mut Option<(PanelId, egui::Pos2)>) {
        let session_panel = *id == PanelId::Other("slices".into()) || *id == PanelId::History || *id == PanelId::Other(SNAPSHOTS_PANEL_KEY.into());
        if !session_panel {
            if !workspace_panel(ui, id, registry, workspace) {
                if let Some(release) = panel_chrome(ui, id, registry) { *header_release = Some(release); }
            }
            return;
        }
        let Some(session) = workspace.session_mut() else { return };
        match id {
            PanelId::History => { panel_frame(ui, |ui| { if let Some(release) = sense_header_drag(ui, id) { *header_release = Some(release); } self.history.show(ui, session); }); }
            PanelId::Other(key) if key == SNAPSHOTS_PANEL_KEY => { panel_frame(ui, |ui| { if let Some(release) = sense_header_drag(ui, id) { *header_release = Some(release); } self.snapshots.show(ui, session); }); }
            _ => { self.slices.show(ui, session); }
        }
    }
}

struct SliceDockBehavior<'a> {
    side: DockSide,
    registry: &'a mut PanelRegistry,
    panels: SessionPanels<'a>,
    workspace: &'a mut dtool_core::workspace::Workspace,
    header_release: &'a mut Option<(PanelId, egui::Pos2)>,
    closed_panel: &'a mut Option<PanelId>,
    dropped_here: &'a mut bool,
}

#[derive(Clone)]
pub(crate) struct DockTabDrag {
    pub panel: PanelId,
    pub source: DockSide,
    pub origin: egui::Pos2,
}

impl DockTabDrag {
    pub fn memory_key() -> egui::Id {
        egui::Id::new("dtool.dock.tab_drag")
    }
}

impl Behavior<PanelId> for SliceDockBehavior<'_> {
    studio_chrome!();
    fn on_edit(&mut self, action: egui_tiles::EditAction) {
        if matches!(action, egui_tiles::EditAction::TileDropped) {
            *self.dropped_here = true;
        }
    }
    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile_id: TileId, pane: &mut PanelId) -> UiResponse {
        dtool_design::studio_content_with(ui, !matches!(pane, PanelId::Layers | PanelId::Swatches), |ui| {
            self.panels.show(ui, pane, self.registry, self.workspace, self.header_release);
        });
        UiResponse::None
    }
    fn tab_title_for_pane(&mut self, pane: &PanelId) -> egui::WidgetText { panel_title(pane).into() }
    fn on_tab_button(&mut self, tiles: &mut egui_tiles::Tiles<PanelId>, id: TileId, response: egui::Response) -> egui::Response {
        if let Some(Tile::Pane(panel)) = tiles.get(id) {
            if response.clicked_by(egui::PointerButton::Middle) {
                *self.closed_panel = Some(panel.clone());
            }
            if response.enabled() {
                let origin = response.ctx.input(|input| input.events.iter().rev().find_map(|event| {
                    match event {
                        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. } => Some(*pos),
                        _ => None,
                    }
                }));
                if let Some(origin) = origin.filter(|pos| response.interact_rect.contains(*pos))
                    && response.ctx.layer_id_at(origin) == Some(response.layer_id)
                {
                    let drag = DockTabDrag { panel: panel.clone(), source: self.side, origin };
                    response.ctx.data_mut(|data| data.insert_temp(DockTabDrag::memory_key(), drag));
                }
            }
        }
        response
    }
    fn top_bar_right_ui(
        &mut self,
        tiles: &egui_tiles::Tiles<PanelId>,
        ui: &mut egui::Ui,
        _: TileId,
        tabs: &egui_tiles::Tabs,
        _: &mut f32,
    ) {
        let Some(Tile::Pane(panel)) = tabs.active.and_then(|id| tiles.get(id)) else { return };
        let label = format!("{} · {}", panel_title(panel), dtool_strings::text("menu.file.close"));
        let response = dtool_design::icon_button_with(ui, "×", &dtool_design::ControlState::default().tooltip(&label));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &label));
        if response.clicked() {
            *self.closed_panel = Some(panel.clone());
        }
    }
}

pub fn show_side_with_slices(ui: &mut egui::Ui, layout: &mut Layout, side: DockSide, registry: &mut PanelRegistry, panels: SessionPanels<'_>, workspace: &mut dtool_core::workspace::Workspace) -> (Vec<(TileId, egui::Rect)>, bool) {
    if !layout.columns.is_shown(side) { return (Vec::new(), false); }
    if let Some(mut tree) = layout.tree(side) {
        let mut header_release = None;
        let mut closed_panel = None;
        let mut dropped_here = false;
        let dock_rect = ui.max_rect();
        tree.ui(&mut SliceDockBehavior { side, registry, panels, workspace, header_release: &mut header_release, closed_panel: &mut closed_panel, dropped_here: &mut dropped_here }, ui);
        layout.store_tree(side, &tree);
        apply_header_release(layout, header_release, dock_rect);
        if let Some(panel) = closed_panel {
            layout.set_panel_visible(&panel, false);
        }
        let groups = tree.tiles.iter().filter_map(|(&id, tile)| match tile {
            Tile::Container(egui_tiles::Container::Tabs(_)) => tree.tiles.rect(id).map(|rect| (id, rect)),
            _ => None,
        }).collect();
        return (groups, dropped_here);
    }
    (Vec::new(), false)
}

pub(crate) fn show_floating_with_slices(ctx: &egui::Context, layout: &mut Layout, registry: &mut PanelRegistry, mut panels: SessionPanels<'_>, workspace: &mut dtool_core::workspace::Workspace) -> Option<(PanelId, egui::Pos2, bool)> {
    record_window_panels(layout);
    layout.show_floating_drag(ctx, |ui, id| {
        let mut header_release = None;
        panels.show(ui, id, registry, workspace, &mut header_release);
    })
}
