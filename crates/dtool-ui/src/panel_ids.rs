use crate::dock::{default_side, DockSide, PanelId};
use dtool_strings::text;

pub const NAMED_PANEL_COUNT: usize = 14;

pub const NAMED_PANELS: [PanelId; NAMED_PANEL_COUNT] = [
    PanelId::Layers,
    PanelId::Properties,
    PanelId::Console,
    PanelId::Pages,
    PanelId::History,
    PanelId::Navigator,
    PanelId::Colour,
    PanelId::Swatches,
    PanelId::Stroke,
    PanelId::Character,
    PanelId::Paragraph,
    PanelId::Effects,
    PanelId::AiLog,
    PanelId::Resources,
];

pub fn remaining_panels() -> [PanelId; 65] { [
    PanelId::Other("align".to_string()),
    PanelId::Other(crate::panels::appearance::PANEL_KEY.to_string()),
    PanelId::Other("arrange".to_string()),
    PanelId::Other("artboards".to_string()),
    PanelId::Other("asset_browser".to_string()),
    PanelId::Other("auto_align".to_string()),
    PanelId::Other("auto_rotate".to_string()),
    PanelId::Other("auto_scale".to_string()),
    PanelId::Other("balance".to_string()),
    PanelId::Other("blend_modes".to_string()),
    PanelId::Other("brushes".to_string()),
    PanelId::Other("color_adjustments".to_string()),
    PanelId::Other("color_balance".to_string()),
    PanelId::Other("color_curves".to_string()),
    PanelId::Other("color_lookup".to_string()),
    PanelId::Other("color_matrix".to_string()),
    PanelId::Other("color_picker".to_string()),
    PanelId::Other("color_wheel".to_string()),
    PanelId::Other("constraints".to_string()),
    PanelId::Other("crop".to_string()),
    PanelId::Other("curves".to_string()),
    PanelId::Other("distribute".to_string()),
    PanelId::Other("document_setup".to_string()),
    PanelId::Other("export".to_string()),
    PanelId::Other("fill".to_string()),
    PanelId::Other("filters".to_string()),
    PanelId::Other("font_manager".to_string()),
    PanelId::Other("gradient".to_string()),
    PanelId::Other("grid".to_string()),
    PanelId::Other("guides".to_string()),
    PanelId::Other("image_adjustments".to_string()),
    PanelId::Other("layer_states".to_string()),
    PanelId::Other("layout".to_string()),
    PanelId::Other("line_style".to_string()),
    PanelId::Other("live_effects".to_string()),
    PanelId::Other("mask".to_string()),
    PanelId::Other("mesh".to_string()),
    PanelId::Other("node_editor".to_string()),
    PanelId::Other("objects".to_string()),
    PanelId::Other("opacity".to_string()),
    PanelId::Other("outline".to_string()),
    PanelId::Other("page_setup".to_string()),
    PanelId::Other("path_operations".to_string()),
    PanelId::Other("patterns".to_string()),
    PanelId::Other("perspective".to_string()),
    PanelId::Other("pixel_view".to_string()),
    PanelId::Other("preview".to_string()),
    PanelId::Other("rasterize".to_string()),
    PanelId::Other("rotate".to_string()),
    PanelId::Other("scale".to_string()),
    PanelId::Other("search".to_string()),
    PanelId::Other("selection".to_string()),
    PanelId::Other("shape_builder".to_string()),
    PanelId::Other("slices".to_string()),
    PanelId::Other("snapshots".to_string()),
    PanelId::Other("snap".to_string()),
    PanelId::Other("style".to_string()),
    PanelId::Other("symbols".to_string()),
    PanelId::Other(crate::panels::table::PANEL_KEY.to_string()),
    PanelId::Other(crate::panels::table_formats::PANEL_KEY.to_string()),
    PanelId::Other("text_effects".to_string()),
    PanelId::Other("text_frame".to_string()),
    PanelId::Other("transform".to_string()),
    PanelId::Other("vector_view".to_string()),
    PanelId::Other("zoom".to_string()),
]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelEntry {
    pub id: PanelId,
    pub key: String,
    pub title_key: String,
    pub side: DockSide,
}

impl PanelId {
    pub fn title_key(&self) -> String {
        format!("panel.{}", self.key())
    }

    pub fn title(&self) -> String {
        text(&self.title_key()).to_string()
    }

    pub fn is_named(&self) -> bool {
        NAMED_PANELS.contains(self)
    }

    pub fn entry(&self) -> PanelEntry {
        PanelEntry {
            id: self.clone(),
            key: self.key().to_string(),
            title_key: self.title_key(),
            side: default_side(self),
        }
    }
}

pub fn named_panel_ids() -> Vec<PanelId> {
    NAMED_PANELS.to_vec()
}

/// Panels available in the workspace editor and in saved custom workspaces.
pub fn studio_panel_ids() -> Vec<PanelId> {
    let mut ids = named_panel_ids();
    for id in crate::panels::WINDOW_PANELS.iter().filter_map(|action| crate::panels::window_panel(action)) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

pub fn remaining_panel_ids() -> Vec<PanelId> {
    remaining_panels().to_vec()
}

pub fn all_panel_ids() -> Vec<PanelId> {
    let mut ids = named_panel_ids();
    for id in remaining_panel_ids() {
        if !ids.iter().any(|known| known.key() == id.key()) {
            ids.push(id);
        }
    }
    ids
}

pub fn panel_registry(extra: &[PanelId]) -> Vec<PanelEntry> {
    let mut entries: Vec<PanelEntry> = NAMED_PANELS.iter().map(PanelId::entry).collect();
    for id in extra {
        if !entries.iter().any(|entry| entry.id.key() == id.key()) {
            entries.push(id.entry());
        }
    }
    entries
}

pub fn canonical(id: PanelId) -> PanelId {
    match id {
        PanelId::Other(key) => PanelId::from_key(&key),
        named => named,
    }
}

pub fn panel_title(id: &PanelId) -> String {
    id.title()
}
