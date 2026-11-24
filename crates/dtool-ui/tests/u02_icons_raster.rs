#[allow(dead_code)]
#[path = "../src/icon_cache.rs"]
mod icon_cache;

use dtool_design::icons::{svg_ids, tool_icon, tool_icon_for, TOOL_ICON_ALIASES};
use egui::Color32;
use icon_cache::{rasterize_icon, IconCache};

const TOOLS_PANEL: &str = include_str!("../src/tools_panel.rs");

const TOOL_CRATE_IDS: [&str; 13] = [
    "tool.clone",
    "tool.frame_text",
    "tool.inpaint",
    "tool.magic_wand",
    "tool.marquee.column",
    "tool.marquee.ellipse",
    "tool.marquee.freehand",
    "tool.marquee.rectangle",
    "tool.marquee.row",
    "tool.object_selection",
    "tool.red_eye",
    "tool.selection_brush",
    "tool.undo_brush",
];

fn all_tool_ids() -> Vec<String> {
    let mut ids: Vec<String> = TOOLS_PANEL
        .split("ToolId(\"")
        .skip(1)
        .filter_map(|part| part.split('"').next())
        .filter(|id| id.starts_with("tool."))
        .map(str::to_string)
        .collect();
    ids.extend(TOOL_CRATE_IDS.iter().map(|id| id.to_string()));
    ids.extend(dtool_core::context_toolbar::TOOL_IDS.iter().map(|id| id.to_string()));
    ids.sort();
    ids.dedup();
    ids
}

#[test]
fn every_tool_id_is_mapped_or_listed_unmapped() {
    let tools = all_tool_ids();
    let icons = svg_ids();
    let mut unmapped = Vec::new();
    for tool in &tools {
        match tool_icon_for(tool) {
            Some(id) => {
                assert!(icons.contains(&id), "{tool} -> {id}");
                assert!(tool_icon(tool).is_some(), "{tool}");
            }
            None => unmapped.push(tool.as_str()),
        }
    }
    assert!(unmapped.is_empty(), "{unmapped:?}");
    for (tool, _) in TOOL_ICON_ALIASES {
        assert!(tools.iter().any(|known| known == tool), "{tool}");
    }
    assert_eq!(tool_icon_for("tool.move"), Some("tool-move"));
    assert_eq!(tool_icon_for("tool.shape.rectangle"), Some("tool-rectangle"));
    assert_eq!(tool_icon_for("tool.marquee.rectangle"), Some("sel-rect"));
    assert_eq!(tool_icon_for("tool.eyedropper"), Some("tool-colour-picker"));
}

#[test]
fn all_embedded_icons_render_to_non_empty_pixels() {
    let colour = Color32::from_rgb(0xF0, 0xF0, 0xF0);
    let mut failed = Vec::new();
    for id in svg_ids() {
        match rasterize_icon(id, colour, 48) {
            Some(raster) if raster.size == [48, 48] && raster.rgba.len() == 48 * 48 * 4 && raster.covered_pixels() > 0 => {}
            other => failed.push(format!("{id}: {:?}", other.map(|r| (r.size, r.covered_pixels())))),
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}

#[test]
fn theme_colour_reaches_the_pixels() {
    let raster = rasterize_icon("tool-rectangle", Color32::from_rgb(255, 0, 0), 48).unwrap();
    let solid: Vec<&[u8]> = raster.rgba.chunks_exact(4).filter(|pixel| pixel[3] == 255).collect();
    assert!(!solid.is_empty());
    assert!(solid.iter().all(|pixel| pixel[0] > 200 && pixel[1] < 40 && pixel[2] < 40));
    let image = raster.color_image();
    assert_eq!(image.size, [48, 48]);
}

#[test]
fn cache_renders_once_per_icon_colour_and_size() {
    let mut cache = IconCache::new();
    let white = Color32::WHITE;
    assert!(cache.raster("tool-move", white, 32).is_some());
    assert!(cache.raster("tool-move", white, 32).is_some());
    assert_eq!(cache.renders(), 1);
    assert!(cache.raster("tool-move", Color32::BLACK, 32).is_some());
    assert!(cache.raster("tool-move", white, 48).is_some());
    assert_eq!(cache.renders(), 3);
    assert!(cache.tool_raster("tool.move", white, 32).is_some());
    assert_eq!(cache.renders(), 3);
    assert!(cache.tool_raster("tool.nonexistent", white, 32).is_none());
    assert!(cache.raster("no-such-icon", white, 32).is_none());
    assert!(cache.raster("no-such-icon", white, 32).is_none());
    assert_eq!(cache.renders(), 4);
    assert_eq!(cache.len(), 4);
    cache.clear();
    assert!(cache.is_empty());
}
