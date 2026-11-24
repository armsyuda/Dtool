use std::collections::HashMap;
use std::sync::OnceLock;

use egui::Color32;

use crate::theme::ThemeStyle;

const ICONS_JSON: &str = include_str!("../assets/icons.json");
const ICON_APP_MAIN: &[u8] = include_bytes!("../assets/icons/dtool-256.png");

pub const SVG_ICONS: [(&str, &str); 206] = [
    ("app-update", include_str!("../assets/icons/app-update.svg")),
    ("ctx-transform-separately", include_str!("../assets/icons/ctx-transform-separately.svg")),
    ("tool-shape-star", include_str!("../assets/icons/tool-shape-star.svg")),
    ("tool-shape-arrow", include_str!("../assets/icons/tool-shape-arrow.svg")),
    ("tool-shape-callout-ellipse", include_str!("../assets/icons/tool-shape-callout-ellipse.svg")),
    ("tool-shape-callout-rectangle", include_str!("../assets/icons/tool-shape-callout-rectangle.svg")),
    ("tool-shape-donut", include_str!("../assets/icons/tool-shape-donut.svg")),
    ("tool-shape-pie", include_str!("../assets/icons/tool-shape-pie.svg")),
    ("tool-shape-heart", include_str!("../assets/icons/tool-shape-heart.svg")),
    ("tool-shape-cog", include_str!("../assets/icons/tool-shape-cog.svg")),
    ("tool-shape-tear", include_str!("../assets/icons/tool-shape-tear.svg")),
    ("tool-shape-crescent", include_str!("../assets/icons/tool-shape-crescent.svg")),
    ("tool-shape-cloud", include_str!("../assets/icons/tool-shape-cloud.svg")),
    ("tool-shape-segment", include_str!("../assets/icons/tool-shape-segment.svg")),
    ("tool-shape-cat", include_str!("../assets/icons/tool-shape-cat.svg")),
    ("tool-shape-square-star", include_str!("../assets/icons/tool-shape-square-star.svg")),
    ("tool-shape-double-star", include_str!("../assets/icons/tool-shape-double-star.svg")),
    ("tool-shape-diamond", include_str!("../assets/icons/tool-shape-diamond.svg")),
    ("tool-shape-trapezoid", include_str!("../assets/icons/tool-shape-trapezoid.svg")),
    ("tool-shape-spiral", include_str!("../assets/icons/tool-shape-spiral.svg")),
    ("ctx-text-valign-top", include_str!("../assets/icons/ctx-text-valign-top.svg")),
    ("ctx-text-valign-center", include_str!("../assets/icons/ctx-text-valign-center.svg")),
    ("ctx-text-valign-bottom", include_str!("../assets/icons/ctx-text-valign-bottom.svg")),
    ("ctx-text-valign-justify", include_str!("../assets/icons/ctx-text-valign-justify.svg")),
    ("ctx-zoom-in", include_str!("../assets/icons/ctx-zoom-in.svg")),
    ("ctx-zoom-out", include_str!("../assets/icons/ctx-zoom-out.svg")),
    ("ctx-zoom-fit", include_str!("../assets/icons/ctx-zoom-fit.svg")),
    ("ctx-zoom-actual", include_str!("../assets/icons/ctx-zoom-actual.svg")),
    ("ctx-close-curve", include_str!("../assets/icons/ctx-close-curve.svg")),
    ("ctx-text-bold", include_str!("../assets/icons/ctx-text-bold.svg")),
    ("ctx-text-italic", include_str!("../assets/icons/ctx-text-italic.svg")),
    ("ctx-text-underline", include_str!("../assets/icons/ctx-text-underline.svg")),
    ("ctx-text-strike", include_str!("../assets/icons/ctx-text-strike.svg")),
    ("ctx-text-superscript", include_str!("../assets/icons/ctx-text-superscript.svg")),
    ("ctx-text-subscript", include_str!("../assets/icons/ctx-text-subscript.svg")),
    ("colour-swap", include_str!("../assets/icons/colour-swap.svg")),
    ("colour-no-paint", include_str!("../assets/icons/colour-no-paint.svg")),
    ("swatch-add", include_str!("../assets/icons/swatch-add.svg")),
    ("palette-add", include_str!("../assets/icons/palette-add.svg")),
    ("tool-shape-triangle", include_str!("../assets/icons/tool-shape-triangle.svg")),
    ("tool-shape-rounded-rectangle", include_str!("../assets/icons/tool-shape-rounded-rectangle.svg")),
    ("tool-pen-spline", include_str!("../assets/icons/tool-pen-spline.svg")),
    ("tool-pen-b-spline", include_str!("../assets/icons/tool-pen-b-spline.svg")),
    ("tool-pen-polyline", include_str!("../assets/icons/tool-pen-polyline.svg")),
    ("para-list-none", include_str!("../assets/icons/para-list-none.svg")),
    ("para-list-bullet", include_str!("../assets/icons/para-list-bullet.svg")),
    ("para-list-number", include_str!("../assets/icons/para-list-number.svg")),
    ("tool-shape-builder", include_str!("../assets/icons/tool-shape-builder.svg")),
    ("tool-text-path", include_str!("../assets/icons/tool-text-path.svg")),
    ("tool-eraser-background", include_str!("../assets/icons/tool-eraser-background.svg")),
    ("tool-eraser-flood", include_str!("../assets/icons/tool-eraser-flood.svg")),
    ("tool-paint-pixel", include_str!("../assets/icons/tool-paint-pixel.svg")),
    ("tool-perspective", include_str!("../assets/icons/tool-perspective.svg")),
    ("tool-deform", include_str!("../assets/icons/tool-deform.svg")),
    ("tool-measure", include_str!("../assets/icons/tool-measure.svg")),
    ("tool-area", include_str!("../assets/icons/tool-area.svg")),
    ("tool-artboard", include_str!("../assets/icons/tool-artboard.svg")),
    ("tool-qr", include_str!("../assets/icons/tool-qr.svg")),
    ("tool-data-merge-layout", include_str!("../assets/icons/tool-data-merge-layout.svg")),
    ("tool-inpaint", include_str!("../assets/icons/tool-inpaint.svg")),
    ("tool-marquee-column", include_str!("../assets/icons/tool-marquee-column.svg")),
    ("tool-marquee-row", include_str!("../assets/icons/tool-marquee-row.svg")),
    ("tool-object-selection", include_str!("../assets/icons/tool-object-selection.svg")),
    ("tool-red-eye", include_str!("../assets/icons/tool-red-eye.svg")),
    ("tool-selection-brush", include_str!("../assets/icons/tool-selection-brush.svg")),
    ("tool-undo-brush", include_str!("../assets/icons/tool-undo-brush.svg")),
    ("tool-align-bottom", include_str!("../assets/icons/tool-align-bottom.svg")),
    ("tool-align-center-h", include_str!("../assets/icons/tool-align-center-h.svg")),
    ("tool-align-center-v", include_str!("../assets/icons/tool-align-center-v.svg")),
    ("tool-align-left", include_str!("../assets/icons/tool-align-left.svg")),
    ("tool-align-right", include_str!("../assets/icons/tool-align-right.svg")),
    ("tool-align-top", include_str!("../assets/icons/tool-align-top.svg")),
    ("tool-distribute-h", include_str!("../assets/icons/tool-distribute-h.svg")),
    ("tool-distribute-v", include_str!("../assets/icons/tool-distribute-v.svg")),
    ("tool-eraser-smudge", include_str!("../assets/icons/tool-eraser-smudge.svg")),
    ("tool-gradient-conical", include_str!("../assets/icons/tool-gradient-conical.svg")),
    ("tool-gradient-linear", include_str!("../assets/icons/tool-gradient-linear.svg")),
    ("tool-gradient-radial", include_str!("../assets/icons/tool-gradient-radial.svg")),
    ("tool-paint-calligraphy", include_str!("../assets/icons/tool-paint-calligraphy.svg")),
    ("tool-paint-pencil", include_str!("../assets/icons/tool-paint-pencil.svg")),
    ("tool-paint-colour-replace", include_str!("../assets/icons/tool-paint-colour-replace.svg")),
    ("tool-paint-mixer", include_str!("../assets/icons/tool-paint-mixer.svg")),
    ("tool-dodge", include_str!("../assets/icons/tool-dodge.svg")),
    ("tool-tone-brush", include_str!("../assets/icons/tool-tone-brush.svg")),
    ("tool-adjustment-brush", include_str!("../assets/icons/tool-adjustment-brush.svg")),
    ("tool-filter-brush", include_str!("../assets/icons/tool-filter-brush.svg")),
    ("tool-filter-blur", include_str!("../assets/icons/tool-filter-blur.svg")),
    ("tool-filter-sharpen", include_str!("../assets/icons/tool-filter-sharpen.svg")),
    ("tool-filter-median", include_str!("../assets/icons/tool-filter-median.svg")),
    ("tool-burn", include_str!("../assets/icons/tool-burn.svg")),
    ("tool-sponge", include_str!("../assets/icons/tool-sponge.svg")),
    ("tool-pen-pencil", include_str!("../assets/icons/tool-pen-pencil.svg")),
    ("tool-shape-curve", include_str!("../assets/icons/tool-shape-curve.svg")),
    ("tool-transform-rotate", include_str!("../assets/icons/tool-transform-rotate.svg")),
    ("tool-transform-scale", include_str!("../assets/icons/tool-transform-scale.svg")),
    ("tool-transform-shear", include_str!("../assets/icons/tool-transform-shear.svg")),
    ("adj-layer", include_str!("../assets/icons/adj-layer.svg")),
    ("app-logo", include_str!("../assets/icons/app-logo.svg")),
    ("cmd-align", include_str!("../assets/icons/cmd-align.svg")),
    ("cmd-arrange", include_str!("../assets/icons/cmd-arrange.svg")),
    ("cmd-arrange-back", include_str!("../assets/icons/cmd-arrange-back.svg")),
    ("cmd-arrange-front", include_str!("../assets/icons/cmd-arrange-front.svg")),
    ("cmd-assistant", include_str!("../assets/icons/cmd-assistant.svg")),
    ("cmd-bleed", include_str!("../assets/icons/cmd-bleed.svg")),
    ("cmd-bool-add", include_str!("../assets/icons/cmd-bool-add.svg")),
    ("cmd-bool-divide", include_str!("../assets/icons/cmd-bool-divide.svg")),
    ("cmd-bool-intersect", include_str!("../assets/icons/cmd-bool-intersect.svg")),
    ("cmd-bool-subtract", include_str!("../assets/icons/cmd-bool-subtract.svg")),
    ("cmd-bool-xor", include_str!("../assets/icons/cmd-bool-xor.svg")),
    ("cmd-colour-wheel", include_str!("../assets/icons/cmd-colour-wheel.svg")),
    ("cmd-export", include_str!("../assets/icons/cmd-export.svg")),
    ("cmd-export-chip", include_str!("../assets/icons/cmd-export-chip.svg")),
    ("cmd-flip", include_str!("../assets/icons/cmd-flip.svg")),
    ("cmd-flip-vertical", include_str!("../assets/icons/cmd-flip-vertical.svg")),
    ("cmd-gradient-map", include_str!("../assets/icons/cmd-gradient-map.svg")),
    ("cmd-grid", include_str!("../assets/icons/cmd-grid.svg")),
    ("cmd-group", include_str!("../assets/icons/cmd-group.svg")),
    ("layer-folder", include_str!("../assets/icons/layer-folder.svg")),
    ("layer-folder-add", include_str!("../assets/icons/layer-folder-add.svg")),
    ("layer-delete", include_str!("../assets/icons/layer-delete.svg")),
    ("layer-effects", include_str!("../assets/icons/layer-effects.svg")),
    ("cmd-ungroup", include_str!("../assets/icons/cmd-ungroup.svg")),
    ("cmd-guides", include_str!("../assets/icons/cmd-guides.svg")),
    ("cmd-help", include_str!("../assets/icons/cmd-help.svg")),
    ("cmd-histogram", include_str!("../assets/icons/cmd-histogram.svg")),
    ("cmd-more-vertical", include_str!("../assets/icons/cmd-more-vertical.svg")),
    ("cmd-preview-mode", include_str!("../assets/icons/cmd-preview-mode.svg")),
    ("cmd-redo", include_str!("../assets/icons/cmd-redo.svg")),
    ("cmd-ruler", include_str!("../assets/icons/cmd-ruler.svg")),
    ("cmd-snap-magnet", include_str!("../assets/icons/cmd-snap-magnet.svg")),
    ("cmd-snapping", include_str!("../assets/icons/cmd-snapping.svg")),
    ("cmd-tone", include_str!("../assets/icons/cmd-tone.svg")),
    ("cmd-transform-box", include_str!("../assets/icons/cmd-transform-box.svg")),
    ("cmd-undo", include_str!("../assets/icons/cmd-undo.svg")),
    ("ctx-select-contain", include_str!("../assets/icons/ctx-select-contain.svg")),
    ("ctx-select-rect", include_str!("../assets/icons/ctx-select-rect.svg")),
    ("ctx-select-touch", include_str!("../assets/icons/ctx-select-touch.svg")),
    ("ctx-settings", include_str!("../assets/icons/ctx-settings.svg")),
    ("panel-aspect-linked", include_str!("../assets/icons/panel-aspect-linked.svg")),
    ("panel-aspect-unlinked", include_str!("../assets/icons/panel-aspect-unlinked.svg")),
    ("panel-scale-options", include_str!("../assets/icons/panel-scale-options.svg")),
    ("ctx-transform-origin", include_str!("../assets/icons/ctx-transform-origin.svg")),
    ("edit-copy", include_str!("../assets/icons/edit-copy.svg")),
    ("edit-duplicate", include_str!("../assets/icons/edit-duplicate.svg")),
    ("file-new", include_str!("../assets/icons/file-new.svg")),
    ("file-open", include_str!("../assets/icons/file-open.svg")),
    ("file-place", include_str!("../assets/icons/file-place.svg")),
    ("file-print", include_str!("../assets/icons/file-print.svg")),
    ("file-save", include_str!("../assets/icons/file-save.svg")),
    ("layer-blend-options", include_str!("../assets/icons/layer-blend-options.svg")),
    ("layer-mask", include_str!("../assets/icons/layer-mask.svg")),
    ("layer-mask-pixel", include_str!("../assets/icons/layer-mask-pixel.svg")),
    ("layer-mask-invert", include_str!("../assets/icons/layer-mask-invert.svg")),
    ("layer-mask-preview", include_str!("../assets/icons/layer-mask-preview.svg")),
    ("layer-mask-release", include_str!("../assets/icons/layer-mask-release.svg")),
    ("persona-home", include_str!("../assets/icons/persona-home.svg")),
    ("persona-layout", include_str!("../assets/icons/persona-layout.svg")),
    ("persona-pixel", include_str!("../assets/icons/persona-pixel.svg")),
    ("persona-slice", include_str!("../assets/icons/persona-slice.svg")),
    ("persona-vector", include_str!("../assets/icons/persona-vector.svg")),
    ("px-brush", include_str!("../assets/icons/px-brush.svg")),
    ("px-clone", include_str!("../assets/icons/px-clone.svg")),
    ("px-eraser", include_str!("../assets/icons/px-eraser.svg")),
    ("sel-ellipse", include_str!("../assets/icons/sel-ellipse.svg")),
    ("sel-flood", include_str!("../assets/icons/sel-flood.svg")),
    ("sel-lasso", include_str!("../assets/icons/sel-lasso.svg")),
    ("sel-rect", include_str!("../assets/icons/sel-rect.svg")),
    ("tool-artistic-text", include_str!("../assets/icons/tool-artistic-text.svg")),
    ("tool-colour-picker", include_str!("../assets/icons/tool-colour-picker.svg")),
    ("tool-colour-well", include_str!("../assets/icons/tool-colour-well.svg")),
    ("tool-contour", include_str!("../assets/icons/tool-contour.svg")),
    ("tool-corner", include_str!("../assets/icons/tool-corner.svg")),
    ("tool-crop", include_str!("../assets/icons/tool-crop.svg")),
    ("tool-vector-crop", include_str!("../assets/icons/tool-vector-crop.svg")),
    ("tool-ellipse", include_str!("../assets/icons/tool-ellipse.svg")),
    ("tool-frame-text", include_str!("../assets/icons/tool-frame-text.svg")),
    ("tool-gradient", include_str!("../assets/icons/tool-gradient.svg")),
    ("tool-line", include_str!("../assets/icons/tool-line.svg")),
    ("tool-mesh-fill", include_str!("../assets/icons/tool-mesh-fill.svg")),
    ("tool-more", include_str!("../assets/icons/tool-more.svg")),
    ("tool-move", include_str!("../assets/icons/tool-move.svg")),
    ("tool-node", include_str!("../assets/icons/tool-node.svg")),
    ("tool-pen", include_str!("../assets/icons/tool-pen.svg")),
    ("tool-pen-smart", include_str!("../assets/icons/tool-pen-smart.svg")),
    ("tool-picture-frame-ellipse", include_str!("../assets/icons/tool-picture-frame-ellipse.svg")),
    ("tool-picture-frame-rect", include_str!("../assets/icons/tool-picture-frame-rect.svg")),
    ("tool-place-image", include_str!("../assets/icons/tool-place-image.svg")),
    ("tool-point-transform", include_str!("../assets/icons/tool-point-transform.svg")),
    ("tool-polygon", include_str!("../assets/icons/tool-polygon.svg")),
    ("tool-rectangle", include_str!("../assets/icons/tool-rectangle.svg")),
    ("tool-slice", include_str!("../assets/icons/tool-slice.svg")),
    ("tool-stroke-width", include_str!("../assets/icons/tool-stroke-width.svg")),
    ("tool-style-picker", include_str!("../assets/icons/tool-style-picker.svg")),
    ("tool-table", include_str!("../assets/icons/tool-table.svg")),
    ("tool-text-flow", include_str!("../assets/icons/tool-text-flow.svg")),
    ("tool-transparency", include_str!("../assets/icons/tool-transparency.svg")),
    ("tool-vector-brush", include_str!("../assets/icons/tool-vector-brush.svg")),
    ("tool-view", include_str!("../assets/icons/tool-view.svg")),
    ("tool-zoom", include_str!("../assets/icons/tool-zoom.svg")),
    ("ui-separator", include_str!("../assets/icons/ui-separator.svg")),
    ("ui-spacer", include_str!("../assets/icons/ui-spacer.svg")),
    ("tool-knife", include_str!("../assets/icons/tool-knife.svg")),
    ("tool-blend", include_str!("../assets/icons/tool-blend.svg")),
    ("tool-symmetry", include_str!("../assets/icons/tool-symmetry.svg")),
    ("tool-distribute", include_str!("../assets/icons/tool-distribute.svg")),
    ("tool-flood-fill", include_str!("../assets/icons/tool-flood-fill.svg")),
];

pub const TOOL_ICON_ALIASES: [(&str, &str); 64] = [
    ("tool.select.object", "tool-object-selection"),
    ("tool.shape.polygon", "tool-polygon"),
    ("tool.vector_crop", "tool-vector-crop"),
    ("tool.paint.smudge", "tool-eraser-smudge"),
    ("tool.paint.clone", "px-clone"),
    ("tool.retouch", "tool-inpaint"),
    ("tool.boolean", "cmd-bool-add"),
    ("tool.boolean.add", "cmd-bool-add"),
    ("tool.boolean.intersect", "cmd-bool-intersect"),
    ("tool.boolean.subtract", "cmd-bool-subtract"),
    ("tool.boolean.xor", "cmd-bool-xor"),
    ("tool.align", "cmd-align"),
    ("tool.clone", "px-clone"),
    ("tool.eraser", "px-eraser"),
    ("tool.eraser.erase", "px-eraser"),
    ("tool.eraser.background", "tool-eraser-background"),
    ("tool.eraser.flood", "tool-eraser-flood"),
    ("tool.eyedropper", "tool-colour-picker"),
    ("tool.frame_text", "tool-frame-text"),
    ("tool.magic_wand", "sel-flood"),
    ("tool.selection_brush", "tool-selection-brush"),
    ("tool.marquee.ellipse", "sel-ellipse"),
    ("tool.marquee.freehand", "sel-lasso"),
    ("tool.marquee.rectangle", "sel-rect"),
    ("tool.marquee", "sel-rect"),
    ("tool.marquee.row", "tool-marquee-row"),
    ("tool.marquee.column", "tool-marquee-column"),
    ("tool.mesh", "tool-mesh-fill"),
    ("tool.paint.brush", "px-brush"),
    ("tool.paint.colour_replace", "tool-paint-colour-replace"),
    ("tool.paint.mixer", "tool-paint-mixer"),
    ("tool.paint.adjustment", "tool-adjustment-brush"),
    ("tool.paint.tone", "tool-tone-brush"),
    ("tool.paint.filter", "tool-filter-brush"),
    ("tool.paint.blur", "tool-filter-blur"),
    ("tool.sharpen", "tool-filter-sharpen"),
    ("tool.paint.median", "tool-filter-median"),
    ("tool.dodge", "tool-dodge"),
    ("tool.burn", "tool-burn"),
    ("tool.paint.sponge", "tool-sponge"),
    ("tool.paint.vector_brush", "tool-vector-brush"),
    ("tool.paint_brush", "px-brush"),
    ("tool.pen.pen", "tool-pen"),
    ("tool.pen.smart", "tool-pen-smart"),
    ("tool.shape.ellipse", "tool-ellipse"),
    ("tool.shape.line", "tool-line"),
    ("tool.shape.rectangle", "tool-rectangle"),
    ("tool.shapes", "tool-rectangle"),
    ("tool.text", "tool-artistic-text"),
    ("tool.text.artistic", "tool-artistic-text"),
    ("tool.text.frame", "tool-frame-text"),
    ("tool.transform", "tool-point-transform"),
    ("tool.transform.flip", "cmd-flip"),
    ("tool.knife", "tool-knife"),
    ("tool.contour", "tool-contour"),
    ("tool.blend", "tool-blend"),
    ("tool.stroke_width", "tool-stroke-width"),
    ("tool.artboard", "tool-artboard"),
    ("tool.qr", "tool-qr"),
    ("tool.symmetry", "tool-symmetry"),
    ("tool.distribute", "tool-distribute"),
    ("tool.flood_fill", "tool-flood-fill"),
    ("tool.picture_frame", "tool-picture-frame-rect"),
    ("tool.crop", "tool-crop"),
];

fn table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str::<HashMap<String, String>>(ICONS_JSON).unwrap_or_default()
    })
}

fn bytes_for_file(file: &str) -> Option<&'static [u8]> {
    match file {
        "dtool-256.png" => Some(ICON_APP_MAIN),
        _ => file.strip_suffix(".svg").and_then(svg).map(str::as_bytes),
    }
}

pub fn icon(key: &str) -> Option<&'static [u8]> {
    let file = table().get(key)?;
    bytes_for_file(file)
}

pub fn app_icon() -> Option<&'static [u8]> {
    icon("app.main")
}

pub fn icon_file(key: &str) -> Option<&'static str> {
    table().get(key).map(|s| s.as_str())
}

pub fn icon_ids() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = table().keys().map(|key| key.as_str()).collect();
    ids.sort_unstable();
    ids
}

pub fn svg(id: &str) -> Option<&'static str> {
    SVG_ICONS.iter().find(|(key, _)| *key == id).map(|(_, text)| *text)
}

pub fn svg_ids() -> Vec<&'static str> {
    SVG_ICONS.iter().map(|(key, _)| *key).collect()
}

pub fn tool_icon_id(tool_id: &str) -> String {
    tool_id.trim().to_ascii_lowercase().replace(['.', '_', ' '], "-")
}

pub fn tool_icon_for(tool_id: &str) -> Option<&'static str> {
    let tool_id = tool_id.trim();
    if let Some((_, id)) = TOOL_ICON_ALIASES.iter().find(|(tool, _)| *tool == tool_id) {
        return Some(id);
    }
    let id = tool_icon_id(tool_id);
    SVG_ICONS.iter().find(|(key, _)| *key == id).map(|(key, _)| *key)
}

pub fn tool_icon(tool_id: &str) -> Option<&'static [u8]> {
    tool_icon_for(tool_id).and_then(icon)
}

pub fn colour_hex(colour: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", colour.r(), colour.g(), colour.b())
}

pub fn themed_svg(id: &str, colour: Color32) -> Option<String> {
    svg(id).map(|text| text.replace("currentColor", &colour_hex(colour)))
}

pub fn icon_colour(style: ThemeStyle) -> Color32 {
    match style {
        ThemeStyle::Dark => crate::theme::dark().text,
        ThemeStyle::Light => crate::theme::light().text,
    }
}

pub fn current_icon_colour() -> Color32 {
    icon_colour(crate::theme::current_style())
}
