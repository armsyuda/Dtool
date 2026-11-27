use std::collections::BTreeMap;

use serde_json::{json, Value};

pub const QUERY: &str = "ui.context_toolbar.query";
pub const OPTION_SET: &str = "ui.context_option.set";
pub const OPTIONS_SET: &str = "ui.tool.options.set";

pub fn is_advanced_shape_tool(id: &str) -> bool {
    id.strip_prefix("tool.shape.").is_some_and(|kind| {
        crate::commands::shape::B19_RECIPE_KEYS.iter().any(|(key, _)| *key == kind)
    })
}

pub const TOOL_IDS: &[&str] = &[
    "tool.view",
    "tool.zoom",
    "tool.move",
    "tool.select.object",
    "tool.node",
    "tool.stroke_width",
    "tool.point_transform",
    "tool.contour",
    "tool.corner",
    "tool.pen",
    "tool.pen.pen",
    "tool.pen.pencil",
    "tool.pen.spline",
    "tool.pen.smart",
    "tool.pen.b_spline",
    "tool.pen.polyline",
    "tool.shapes",
    "tool.shape.rectangle",
    "tool.shape.ellipse",
    "tool.shape.triangle",
    "tool.shape.rounded_rectangle",
    "tool.shape.polygon",
    "tool.shape.star",
    "tool.shape.arrow",
    "tool.shape.callout_ellipse",
    "tool.shape.callout_rectangle",
    "tool.shape.donut",
    "tool.shape.pie",
    "tool.shape.heart",
    "tool.shape.cog",
    "tool.shape.tear",
    "tool.shape.crescent",
    "tool.shape.cloud",
    "tool.shape.segment",
    "tool.shape.cat",
    "tool.shape.square_star",
    "tool.shape.double_star",
    "tool.shape.diamond",
    "tool.shape.trapezoid",
    "tool.shape.spiral",
    "tool.shape.line",
    "tool.picture_frame",
    "tool.picture_frame.rect",
    "tool.picture_frame.ellipse",
    "tool.shape.curve",
    "tool.shape_builder",
    "tool.knife",
    "tool.text",
    "tool.text.artistic",
    "tool.text.frame",
    "tool.text.path",
    "tool.table",
    "tool.transform",
    "tool.transform.rotate",
    "tool.transform.scale",
    "tool.transform.shear",
    "tool.transform.flip",
    "tool.crop",
    "tool.vector_crop",
    "tool.slice",
    "tool.eyedropper",
    "tool.style_picker",
    "tool.flood_fill",
    "tool.gradient",
    "tool.gradient.linear",
    "tool.gradient.radial",
    "tool.gradient.conical",
    "tool.transparency",
    "tool.mesh",
    "tool.blend",
    "tool.boolean",
    "tool.boolean.add",
    "tool.boolean.subtract",
    "tool.boolean.intersect",
    "tool.boolean.xor",
    "tool.boolean.divide",
    "tool.eraser",
    "tool.eraser.erase",
    "tool.eraser.smudge",
    "tool.eraser.background",
    "tool.eraser.flood",
    "tool.paint_brush",
    "tool.paint.brush",
    "tool.paint.mixer",
    "tool.paint.adjustment",
    "tool.paint.tone",
    "tool.paint.filter",
    "tool.paint.blur",
    "tool.sharpen",
    "tool.paint.median",
    "tool.dodge",
    "tool.burn",
    "tool.paint.sponge",
    "tool.paint.colour_replace",
    "tool.paint.pencil",
    "tool.paint.pixel",
    "tool.paint.vector_brush",
    "tool.paint.calligraphy",
    "tool.paint.smudge",
    "tool.paint.clone",
    "tool.marquee",
    "tool.marquee.rectangle",
    "tool.marquee.ellipse",
    "tool.marquee.row",
    "tool.marquee.column",
    "tool.marquee.freehand",
    "tool.selection_brush",
    "tool.magic_wand",
    "tool.retouch",
    crate::commands::red_eye::TOOL,
    "tool.perspective",
    "tool.deform",
    "tool.measure",
    "tool.area",
    "tool.qr",
    "tool.artboard",
    "tool.data_merge_layout",
    "tool.symmetry",
    "tool.align",
    "tool.align.left",
    "tool.align.center_h",
    "tool.align.right",
    "tool.align.top",
    "tool.align.center_v",
    "tool.align.bottom",
    "tool.distribute",
    "tool.distribute.h",
    "tool.distribute.v",
];

const ALIASES: &[(&str, &str)] = &[
    ("fill", "tool.flood_fill"),
    ("paint_brush", "tool.paint_brush"),
    ("erase_brush", "tool.eraser.erase"),
    ("background_erase_brush", "tool.eraser.background"),
    ("mixer_brush", "tool.paint.mixer"),
    ("tone_brush", "tool.paint.tone"),
    ("dodge_brush", "tool.dodge"),
    ("burn_brush", "tool.burn"),
    ("sponge_brush", "tool.paint.sponge"),
    ("blur_brush", "tool.paint.blur"),
    ("sharpen_brush", "tool.sharpen"),
    ("median_brush", "tool.paint.median"),
    ("colour_replace_brush", "tool.paint.colour_replace"),
    ("flood_erase", "tool.eraser.flood"),
    ("pixel", "tool.paint.pixel"),
    ("smudge_brush", "tool.paint.smudge"),
    ("clone_brush", "tool.paint.clone"),
    ("tool.clone", "tool.paint.clone"),
    ("lasso", "tool.marquee.freehand"),
    ("flood_select", "tool.magic_wand"),
    ("frame_text", "tool.text.frame"),
    ("art_text", "tool.text.artistic"),
    ("artistic_text", "tool.text.artistic"),
];

pub fn canonical_tool(id: &str) -> Option<&'static str> {
    if let Some((_, target)) = ALIASES.iter().find(|(alias, _)| *alias == id) {
        return Some(target);
    }
    let full = if id.starts_with("tool.") { id.to_string() } else { format!("tool.{id}") };
    TOOL_IDS.iter().copied().find(|known| *known == full)
}

pub fn active_id(id: &str) -> String {
    canonical_tool(id).map(String::from).unwrap_or_else(|| id.to_string())
}

pub fn same_tool(a: &str, b: &str) -> bool {
    match (canonical_tool(a), canonical_tool(b)) {
        (Some(left), Some(right)) => left == right,
        _ => a == b,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OptionKind {
    Toggle { default: bool },
    Number { min: f64, max: f64, step: f64, default: f64 },
    Choice { values: &'static [&'static str], default: &'static str },
    Text { default: &'static str },
    Action { command: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolOptionSpec {
    pub id: &'static str,
    pub key: &'static str,
    pub kind: OptionKind,
}

impl ToolOptionSpec {
    pub fn default_value(&self) -> Value {
        match self.kind {
            OptionKind::Toggle { default } => json!(default),
            OptionKind::Number { default, .. } => json!(default),
            OptionKind::Choice { default, .. } => json!(default),
            OptionKind::Text { default } => json!(default),
            OptionKind::Action { .. } => Value::Null,
        }
    }

    pub fn is_action(&self) -> bool {
        matches!(self.kind, OptionKind::Action { .. })
    }

    pub fn normalize(&self, value: &Value) -> Option<Value> {
        match self.kind {
            OptionKind::Toggle { .. } => value.as_bool().map(Value::Bool),
            OptionKind::Number { min, max, .. } => {
                let number = value.as_f64()?;
                (number.is_finite() && number >= min && number <= max).then(|| {
                    json!(if matches!(self.id, TABLE_ROWS | TABLE_COLUMNS) { number.round() } else { number })
                })
            }
            OptionKind::Choice { values, .. } => {
                let text = value.as_str()?.to_lowercase();
                values.iter().find(|choice| **choice == text).map(|choice| json!(choice))
            }
            OptionKind::Text { .. } => {
                let text = value.as_str()?;
                if self.id == "brush.replacement_colour" {
                    let digits = text.strip_prefix('#')?;
                    return (digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
                        .then(|| json!(text.to_ascii_uppercase()));
                }
                if matches!(self.id, "brush.mixer_colour" | "brush.tone_colour") {
                    let digits = text.strip_prefix('#')?;
                    return (digits.len() == 8 && digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
                        .then(|| json!(text.to_ascii_uppercase()));
                }
                Some(json!(text))
            }
            OptionKind::Action { .. } => Some(Value::Null),
        }
    }

    pub fn describe(&self, value: &Value) -> Value {
        let mut out = json!({ "id": self.id, "key": self.key });
        let map = out.as_object_mut().expect("object");
        match self.kind {
            OptionKind::Toggle { .. } => {
                map.insert("kind".into(), json!("toggle"));
                map.insert("value".into(), value.clone());
            }
            OptionKind::Number { min, max, step, .. } => {
                map.insert("kind".into(), json!("number"));
                map.insert("value".into(), value.clone());
                map.insert("min".into(), json!(min));
                map.insert("max".into(), json!(max));
                map.insert("step".into(), json!(step));
            }
            OptionKind::Choice { values, .. } => {
                map.insert("kind".into(), json!("choice"));
                map.insert("value".into(), value.clone());
                map.insert("values".into(), json!(values));
            }
            OptionKind::Text { .. } => {
                map.insert("kind".into(), json!("text"));
                map.insert("value".into(), value.clone());
            }
            OptionKind::Action { command } => {
                map.insert("kind".into(), json!("action"));
                map.insert("command".into(), json!(command));
            }
        }
        out
    }
}

const fn toggle(id: &'static str, key: &'static str, default: bool) -> ToolOptionSpec {
    ToolOptionSpec { id, key, kind: OptionKind::Toggle { default } }
}

const fn number(id: &'static str, key: &'static str, min: f64, max: f64, step: f64, default: f64) -> ToolOptionSpec {
    ToolOptionSpec { id, key, kind: OptionKind::Number { min, max, step, default } }
}

const fn choice(id: &'static str, key: &'static str, values: &'static [&'static str], default: &'static str) -> ToolOptionSpec {
    ToolOptionSpec { id, key, kind: OptionKind::Choice { values, default } }
}

const fn text_field(id: &'static str, key: &'static str, default: &'static str) -> ToolOptionSpec {
    ToolOptionSpec { id, key, kind: OptionKind::Text { default } }
}

const fn action(id: &'static str, key: &'static str, command: &'static str) -> ToolOptionSpec {
    ToolOptionSpec { id, key, kind: OptionKind::Action { command } }
}

const SELECTION_OPS: &[&str] = &["new", "add", "subtract", "intersect"];
const TEXT_ALIGN: &[&str] = &["left", "centre", "right", "justify_left", "justify_centre", "justify_right", "justify_all"];
const TEXT_VALIGN: &[&str] = &["top", "centre", "bottom", "justify"];

pub const MOVE_SHOW_ORIGIN: &str = "move.show_transform_origin";
pub const MOVE_TRANSFORM_SEPARATELY: &str = "move.transform_separately";
pub const MOVE_SHOW_SELECTION_BOX: &str = "move.show_selection_box";

pub const MOVE_OPTIONS: &[ToolOptionSpec] = &[
    toggle(MOVE_TRANSFORM_SEPARATELY, "context.move.transform_separately", false),
    toggle(MOVE_SHOW_SELECTION_BOX, "context.move.show_selection_box", true),
    toggle(MOVE_SHOW_ORIGIN, "context.move.show_transform_origin", false),
    action("move.reset_origin", "context.move.reset_transform_origin", "transform.origin.reset"),
    action("move.align", "context.move.align", "layer.arrange.align"),
    action("move.align.left", "context.move.align.left", "layer.arrange.align.left"),
    action("move.align.centre", "context.move.align.centre", "layer.arrange.align.centre"),
    action("move.align.right", "context.move.align.right", "layer.arrange.align.right"),
    action("move.align.top", "context.move.align.top", "layer.arrange.align.top"),
    action("move.align.middle", "context.move.align.middle", "layer.arrange.align.middle"),
    action("move.align.bottom", "context.move.align.bottom", "layer.arrange.align.bottom"),
    action("move.convert_to_curves", "context.move.convert_to_curves", "vector.convert_to_curves"),
];

pub const VIEW_OPTIONS: &[ToolOptionSpec] = &[
    action("view.zoom_fit", "context.view.zoom_fit", "view.zoom.fit"),
    action("view.zoom_actual", "context.view.zoom_actual", "view.zoom.actual"),
];

pub const ZOOM_OPTIONS: &[ToolOptionSpec] = &[
    choice("zoom.mode", "context.zoom.mode", &["in", "out"], "in"),
    number("zoom.level", "context.zoom.level", 1.0, crate::ZOOM_MAX as f64 * 100.0, 1.0, 100.0),
    action("zoom.in", "context.zoom.in", "view.zoom.in"),
    action("zoom.out", "context.zoom.out", "view.zoom.out"),
    action("zoom.fit", "context.zoom.fit", "view.zoom.fit"),
    action("zoom.actual", "context.zoom.actual", "view.zoom.actual"),
];

pub const SHAPE_STROKE_WIDTH: &str = "shape.stroke_width";
pub const PEN_STROKE_WIDTH: &str = "pen.stroke_width";
const STROKE_WIDTH_LABEL: &str = "context.shape.stroke_width";

pub const SHAPE_FROM_CENTER: &str = "shape.from_center";
pub const SHAPE_CORNER_RADIUS: &str = "shape.corner_radius";
pub const SHAPE_SIDES: &str = "shape.sides";

const SHAPE_WIDTH: ToolOptionSpec = number(SHAPE_STROKE_WIDTH, STROKE_WIDTH_LABEL, 0.0, 1000.0, 0.1, 1.0);
const SHAPE_CENTER: ToolOptionSpec = toggle(SHAPE_FROM_CENTER, "context.shape.from_center", false);
const SHAPE_TO_CURVES: ToolOptionSpec = action("shape.convert_to_curves", "context.shape.convert_to_curves", "vector.convert_to_curves");

pub const SHAPE_OPTIONS: &[ToolOptionSpec] = &[SHAPE_WIDTH, SHAPE_CENTER, SHAPE_TO_CURVES];
const LINE_OPTIONS: &[ToolOptionSpec] = &[SHAPE_WIDTH, SHAPE_TO_CURVES];
const ROUNDED_RECTANGLE_OPTIONS: &[ToolOptionSpec] = &[
    SHAPE_WIDTH,
    SHAPE_CENTER,
    number(SHAPE_CORNER_RADIUS, "context.shape.corner_radius", 0.0, 50.0, 0.1, 25.0),
    SHAPE_TO_CURVES,
];
const POLYGON_OPTIONS: &[ToolOptionSpec] = &[
    SHAPE_WIDTH,
    SHAPE_CENTER,
    number(SHAPE_SIDES, "context.shape.sides", 3.0, 100.0, 1.0, 6.0),
    SHAPE_TO_CURVES,
];

pub const TABLE_ROWS: &str = "table.rows";
pub const TABLE_COLUMNS: &str = "table.columns";

pub const TABLE_OPTIONS: &[ToolOptionSpec] = &[
    number(TABLE_ROWS, "ctx.table.rows", 1.0, crate::table::commands::MAX_ROWS_COLS as f64, 1.0, 3.0),
    number(TABLE_COLUMNS, "ctx.table.columns", 1.0, crate::table::commands::MAX_ROWS_COLS as f64, 1.0, 3.0),
];

pub const TEXT_OPTIONS: &[ToolOptionSpec] = &[
    text_field("text.font_family", "context.text.font_family", ""),
    text_field("text.font_style", "context.text.font_style", ""),
    number("text.font_size", "context.text.font_size", 0.5, 1296.0, 0.5, 12.0),
    choice("text.align", "context.text.align", TEXT_ALIGN, "left"),
    choice("text.vertical_align", "context.text.vertical_align", TEXT_VALIGN, "top"),
];

pub const ART_TEXT_VERTICAL: &str = "text.art.vertical";
const ART_TEXT_DIRECTION: ToolOptionSpec = toggle(ART_TEXT_VERTICAL, "context.text.vertical", false);

pub const CURVE_ENERGY: &str = "node.curvature_comb";
const CURVE_ENERGY_TOGGLE: ToolOptionSpec = toggle(CURVE_ENERGY, "context.curve_energy.show", false);

const PEN_WIDTH: ToolOptionSpec = number(PEN_STROKE_WIDTH, STROKE_WIDTH_LABEL, 0.0, 1000.0, 0.1, 1.0);
const PEN_PREVIEW: ToolOptionSpec = toggle("pen.rubber_band", "context.pen.rubber_band", false);
const PEN_CLOSE: ToolOptionSpec = action("pen.close_curve", "context.pen.close_curve", "path.close");
pub const PEN_CONSTRUCTION_SNAP: &str = "pen.snap_handle_construction";

pub const PEN_OPTIONS: &[ToolOptionSpec] = &[
    PEN_WIDTH,
    choice("pen.mode", "context.pen.mode", &["pen", "smart", "polygon", "line", "spline", "b_spline"], "pen"),
    PEN_PREVIEW,
    toggle(PEN_CONSTRUCTION_SNAP, "ctx.node.snap_handle_construction", false),
    PEN_CLOSE,
    CURVE_ENERGY_TOGGLE,
];

const SPLINE_OPTIONS: &[ToolOptionSpec] = &[PEN_WIDTH, PEN_PREVIEW, PEN_CLOSE, CURVE_ENERGY_TOGGLE];

pub const PENCIL_OPTIONS: &[ToolOptionSpec] = &[
    number(PEN_STROKE_WIDTH, STROKE_WIDTH_LABEL, 0.0, 1000.0, 0.1, 1.0),
    toggle("pencil.extend", "context.pencil.extend", true),
    toggle("pencil.auto_close", "context.pencil.auto_close", true),
    CURVE_ENERGY_TOGGLE,
];

pub const VECTOR_BRUSH_OPTIONS: &[ToolOptionSpec] = &[
    number("brush.width", "context.vector_brush.width", 0.1, 1000.0, 0.1, 6.0),
    number("brush.opacity", "context.brush.opacity", 0.0, 100.0, 1.0, 100.0),
];

pub const STROKE_WIDTH_OPTIONS: &[ToolOptionSpec] = &[
    toggle("stroke_width.lock_width", "context.stroke_width.lock_width", false),
    toggle("stroke_width.lock_order", "context.stroke_width.lock_order", true),
    toggle("stroke_width.snap_ends", "context.stroke_width.snap_ends", false),
    action("stroke_width.reset", "context.stroke_width.reset", "stroke.profile.reset"),
];

const NODE_CORNER_OPTIONS: [ToolOptionSpec; 4] = [
    choice("node.convert", "context.node.convert", &["sharp", "smooth", "smart"], "smooth"),
    toggle("node.transform_mode", "context.node.transform_mode", false),
    action("node.close_curve", "context.node.close_curve", "path.close"),
    CURVE_ENERGY_TOGGLE,
];

pub const NODE_HIDE_TRANSFORM_BOX: &str = "node.hide_transform_box";
pub const NODE_SHOW_ALIGNMENT_HANDLES: &str = "node.show_alignment_handles";
pub const NODE_BOX_FROM_CURVES: &str = "node.box_from_curves";
pub const NODE_TRANSFORM_SEPARATELY: &str = "node.transform_separately";

pub const NODE_OPTIONS: &[ToolOptionSpec] = &{
    let [convert, transform, close, energy] = NODE_CORNER_OPTIONS;
    [
        convert, transform, close, energy,
        toggle("node.show_transform_origin", "context.move.show_transform_origin", false),
        toggle(NODE_HIDE_TRANSFORM_BOX, "context.node.hide_transform_box", false),
        toggle(NODE_SHOW_ALIGNMENT_HANDLES, "context.node.show_alignment_handles", false),
        toggle(NODE_TRANSFORM_SEPARATELY, "context.move.transform_separately", false),
        toggle(NODE_BOX_FROM_CURVES, "context.node.box_from_curves", false),
        toggle("node.show_orientation", "ctx.node.show_orientation", false),
        toggle("node.align_to_nodes", "ctx.node.align_to_nodes", false),
        toggle("node.snap_to_geometry", "ctx.node.snap_to_geometry", false),
        toggle("node.snap_all_selected", "ctx.node.snap_all_selected", false),
        toggle("node.snap_handle_global", "ctx.node.snap_handle_global", true),
        toggle("node.snap_handle_construction", "ctx.node.snap_handle_construction", false),
    ]
};

pub const POINT_TRANSFORM_OPTIONS: &[ToolOptionSpec] = &[
    toggle("point_transform.snap_to_points", "context.point_transform.snap_to_points", true),
    toggle("point_transform.rotate_scale", "context.point_transform.rotate_scale", true),
];

pub const KNIFE_OPTIONS: &[ToolOptionSpec] = &[
    choice("knife.mode", "context.knife.mode", &["straight", "freehand"], "straight"),
    toggle("knife.close_curves", "context.knife.close_curves", true),
    toggle("knife.auto_close", "context.knife.auto_close", true),
    number("knife.smoothness", "context.knife.smoothness", 0.0, 100.0, 1.0, 0.0),
    number("knife.stabiliser", "context.knife.stabiliser", 0.0, 100.0, 1.0, 0.0),
];

pub const SHAPE_BUILDER_OPTIONS: &[ToolOptionSpec] = &[
    choice("shape_builder.action", "context.shape_builder.action", &["add", "delete", "create"], "add"),
    choice("shape_builder.drag_method", "context.shape_builder.drag_method", &["freehand", "line", "marquee"], "freehand"),
    choice("shape_builder.cleanup", "context.shape_builder.cleanup", &["none", "internal", "connected", "unused"], "none"),
    toggle("shape_builder.first_style", "context.shape_builder.first_style", true),
];

pub const MARQUEE_OPTIONS: &[ToolOptionSpec] = &[
    choice("marquee.op", "context.marquee.op", SELECTION_OPS, "new"),
    number("marquee.feather", "context.marquee.feather", 0.0, 1000.0, 0.1, 0.0),
    toggle("marquee.antialias", "context.marquee.antialias", true),
    toggle("marquee.from_center", "context.marquee.from_center", false),
    number("marquee.width", "context.marquee.width", 0.0, 100000.0, 1.0, 0.0),
    number("marquee.height", "context.marquee.height", 0.0, 100000.0, 1.0, 0.0),
    action("selection.refine", "selection.refine_button", "selection.refine"),
];

pub const LASSO_OPTIONS: &[ToolOptionSpec] = &[
    choice("marquee.op", "context.marquee.op", SELECTION_OPS, "new"),
    number("marquee.feather", "context.marquee.feather", 0.0, 1000.0, 0.1, 0.0),
    toggle("marquee.antialias", "context.marquee.antialias", true),
    choice("lasso.mode", "context.lasso.mode", &["freehand", "polygonal", "magnetic"], "freehand"),
    action("selection.refine", "selection.refine_button", "selection.refine"),
];

pub const WAND_OPTIONS: &[ToolOptionSpec] = &[
    choice("marquee.op", "context.marquee.op", SELECTION_OPS, "new"),
    number("wand.tolerance", "context.wand.tolerance", 0.0, 100.0, 1.0, 32.0),
    toggle("wand.contiguous", "context.wand.contiguous", true),
    choice("wand.source", "context.wand.source", &["current_layer", "all_layers"], "current_layer"),
    toggle("marquee.antialias", "context.marquee.antialias", true),
    action("selection.refine", "selection.refine_button", "selection.refine"),
];

pub const SELECTION_BRUSH_OPTIONS: &[ToolOptionSpec] = &[
    choice("marquee.op", "context.marquee.op", &["add", "subtract"], "add"),
    number("brush.width", "context.brush.width", 1.0, 2048.0, 1.0, 64.0),
    toggle("selection_brush.snap_to_edges", "context.selection_brush.snap_to_edges", false),
    toggle("selection_brush.all_layers", "context.wand.all_layers", false),
    toggle("selection_brush.soft_edges", "context.selection_brush.soft_edges", false),
    action("selection.refine", "selection.refine_button", "selection.refine"),
];

pub const BRUSH_OPTIONS: &[ToolOptionSpec] = &[
    number("brush.width", "context.brush.width", 1.0, 2048.0, 1.0, 64.0),
    number("brush.opacity", "context.brush.opacity", 0.0, 100.0, 1.0, 100.0),
    number("brush.flow", "context.brush.flow", 0.0, 100.0, 1.0, 100.0),
    number("brush.hardness", "context.brush.hardness", 0.0, 100.0, 1.0, 80.0),
];

const BRUSH_SYMMETRY_COUNT: ToolOptionSpec = number("brush.symmetry", "brush.symmetry", 1.0, 8.0, 1.0, 1.0);
const BRUSH_SYMMETRY_MIRROR: ToolOptionSpec = toggle("brush.mirror", "brush.mirror", false);
const BRUSH_SYMMETRY_LOCK: ToolOptionSpec = toggle("brush.symmetry_lock", "brush.symmetry_lock", false);
const BRUSH_SYMMETRY_X: ToolOptionSpec = number("brush.symmetry_x", "brush.symmetry_x", 0.0, 100.0, 0.1, 50.0);
const BRUSH_SYMMETRY_Y: ToolOptionSpec = number("brush.symmetry_y", "brush.symmetry_y", 0.0, 100.0, 0.1, 50.0);

const PIXEL_PAINT_OPTIONS: &[ToolOptionSpec] = &[
    BRUSH_OPTIONS[0], BRUSH_OPTIONS[1], BRUSH_OPTIONS[2], BRUSH_OPTIONS[3],
    toggle("brush.pressure_size", "brush.pressure_size", false),
    toggle("brush.protect_alpha", "brush.protect_alpha", false),
    toggle("brush.wet_edges", "brush.wet_edges", false),
    BRUSH_SYMMETRY_COUNT, BRUSH_SYMMETRY_MIRROR, BRUSH_SYMMETRY_LOCK, BRUSH_SYMMETRY_X, BRUSH_SYMMETRY_Y,
];

const PIXEL_ERASE_OPTIONS: &[ToolOptionSpec] = &[
    BRUSH_OPTIONS[0], BRUSH_OPTIONS[1], BRUSH_OPTIONS[2], BRUSH_OPTIONS[3], PIXEL_PAINT_OPTIONS[4],
    BRUSH_SYMMETRY_COUNT, BRUSH_SYMMETRY_MIRROR, BRUSH_SYMMETRY_LOCK, BRUSH_SYMMETRY_X, BRUSH_SYMMETRY_Y,
];

const SINGLE_PIXEL_BRUSH_OPTIONS: &[ToolOptionSpec] = &[BRUSH_OPTIONS[1], BRUSH_OPTIONS[2]];

pub const CALLIGRAPHY_TOOL: &str = "tool.paint.calligraphy";
pub const CALLIGRAPHY_ANGLE: &str = "brush.calligraphy.angle";
pub const CALLIGRAPHY_ROUNDNESS: &str = "brush.calligraphy.roundness";
pub(crate) const CALLIGRAPHY_DEFAULT_ANGLE: f32 = -35.0;
pub(crate) const CALLIGRAPHY_DEFAULT_ROUNDNESS: f32 = 28.0;
const CALLIGRAPHY_OPTIONS: &[ToolOptionSpec] = &[
    BRUSH_OPTIONS[0], BRUSH_OPTIONS[1],
    number(CALLIGRAPHY_ANGLE, CALLIGRAPHY_ANGLE, -180.0, 180.0, 1.0, CALLIGRAPHY_DEFAULT_ANGLE as f64),
    number(CALLIGRAPHY_ROUNDNESS, CALLIGRAPHY_ROUNDNESS, 1.0, 100.0, 1.0, CALLIGRAPHY_DEFAULT_ROUNDNESS as f64),
    BRUSH_OPTIONS[2], BRUSH_OPTIONS[3], PIXEL_PAINT_OPTIONS[4], PIXEL_PAINT_OPTIONS[5],
];

pub const CLONE_TOOL: &str = "tool.paint.clone";
pub const CLONE_PICK_SOURCE: &str = "brush.clone.pick_source";
pub const CLONE_ALIGNED: &str = "brush.clone.aligned";
const CLONE_BRUSH_OPTIONS: &[ToolOptionSpec] = &[
    toggle(CLONE_PICK_SOURCE, CLONE_PICK_SOURCE, false),
    BRUSH_OPTIONS[0], BRUSH_OPTIONS[1], BRUSH_OPTIONS[2], BRUSH_OPTIONS[3],
    PIXEL_PAINT_OPTIONS[4], PIXEL_PAINT_OPTIONS[5],
    toggle(CLONE_ALIGNED, CLONE_ALIGNED, true),
];

const BACKGROUND_ERASE_OPTIONS: &[ToolOptionSpec] = &[
    BRUSH_OPTIONS[0], BRUSH_OPTIONS[1], BRUSH_OPTIONS[2], BRUSH_OPTIONS[3],
    number("fill.tolerance", "context.fill.tolerance", 0.0, 100.0, 1.0, 10.0),
    toggle("fill.contiguous", "context.wand.contiguous", true),
    toggle("brush.sample_continuously", "context.brush.sample_continuously", false),
];

pub const MIXER_TOOL: &str = "tool.paint.mixer";
const MIXER_OPTIONS: &[ToolOptionSpec] = &[
    BRUSH_OPTIONS[0],
    number("brush.flow", "context.brush.flow", 0.0, 100.0, 1.0, 50.0),
    number("brush.strength", "brush.strength", 0.0, 100.0, 1.0, 90.0),
    action("brush.load", "brush.load", "pixel.brush.mixer.load"),
    action("brush.clean", "brush.clean", "pixel.brush.mixer.clean"),
    toggle("brush.auto_load", "brush.auto_load", false),
    toggle("brush.auto_clean", "brush.auto_clean", false),
    choice("brush.model", "brush.mixer_model", &["ryb", "rgb"], "ryb"),
    text_field("brush.mixer_colour", "brush.mixer_colour", "#000000FF"),
];

const TONAL_BRUSH_OPTIONS: &[ToolOptionSpec] = &[
    BRUSH_OPTIONS[0],
    number("brush.opacity", "context.brush.opacity", 0.0, 100.0, 1.0, 25.0),
    BRUSH_OPTIONS[2], BRUSH_OPTIONS[3],
    toggle("brush.stabilizer", "brush.stabilizer", false),
    choice("brush.tonal_range", "brush.tonal_range", &["shadows", "midtones", "highlights"], "midtones"),
    toggle("brush.protect_hue", "brush.protect_hue", false),
    toggle("brush.wet_edges", "brush.wet_edges", false),
];

pub const TONE_TOOL: &str = "tool.paint.tone";
const NONDESTRUCTIVE_TONE_OPTIONS: &[ToolOptionSpec] = &[
    number("brush.width", "context.brush.width", 1.0, 2048.0, 1.0, 1024.0),
    TONAL_BRUSH_OPTIONS[1], BRUSH_OPTIONS[2], BRUSH_OPTIONS[3],
    choice("brush.tone_mode", "brush.tone_mode", &["dodge_burn", "blend", "inverse_blend"], "dodge_burn"),
    choice("brush.tone_blend", "brush.tone_blend", &["overlay", "soft_light", "hard_light", "screen", "multiply"], "overlay"),
    choice("brush.tone_range", "brush.tone_range", &["full", "shadows", "midtones", "highlights"], "full"),
    action("brush.tone_add", "brush.tone_add", "pixel.brush.tone.add"),
    text_field("brush.tone_colour", "brush.mixer_colour", "#FFFFFFFF"),
];

const SPONGE_BRUSH_OPTIONS: &[ToolOptionSpec] = &[
    BRUSH_OPTIONS[0],
    number("brush.opacity", "context.brush.opacity", 0.0, 100.0, 1.0, 30.0),
    BRUSH_OPTIONS[2], BRUSH_OPTIONS[3],
    TONAL_BRUSH_OPTIONS[4],
    choice("brush.sponge_mode", "brush.sponge", &["saturate", "desaturate", "vibrance"], "saturate"),
    TONAL_BRUSH_OPTIONS[7],
];

const FILTER_BRUSH_OPTIONS: &[ToolOptionSpec] = &[
    BRUSH_OPTIONS[0], BRUSH_OPTIONS[1], BRUSH_OPTIONS[2], BRUSH_OPTIONS[3],
    TONAL_BRUSH_OPTIONS[4],
    BRUSH_SYMMETRY_COUNT, BRUSH_SYMMETRY_MIRROR, BRUSH_SYMMETRY_LOCK, BRUSH_SYMMETRY_X, BRUSH_SYMMETRY_Y,
];

const SHARPEN_BRUSH_OPTIONS: &[ToolOptionSpec] = &[
    FILTER_BRUSH_OPTIONS[0], FILTER_BRUSH_OPTIONS[1], FILTER_BRUSH_OPTIONS[2], FILTER_BRUSH_OPTIONS[3],
    FILTER_BRUSH_OPTIONS[4],
    choice("brush.sharpen_mode", "brush.sharpen", &["clarity", "unsharp", "harsh"], "clarity"),
    TONAL_BRUSH_OPTIONS[7],
    BRUSH_SYMMETRY_COUNT, BRUSH_SYMMETRY_MIRROR, BRUSH_SYMMETRY_LOCK, BRUSH_SYMMETRY_X, BRUSH_SYMMETRY_Y,
];

const SMUDGE_BRUSH_OPTIONS: &[ToolOptionSpec] = &[
    BRUSH_OPTIONS[0],
    number("brush.flow", "context.brush.flow", 0.0, 100.0, 1.0, 10.0),
    number("brush.strength", "brush.strength", 0.0, 100.0, 1.0, 50.0),
    MIXER_OPTIONS[7],
    BRUSH_SYMMETRY_COUNT, BRUSH_SYMMETRY_MIRROR, BRUSH_SYMMETRY_LOCK, BRUSH_SYMMETRY_X, BRUSH_SYMMETRY_Y,
];

const COLOUR_REPLACE_OPTIONS: &[ToolOptionSpec] = &[
    BRUSH_OPTIONS[0], BRUSH_OPTIONS[1], BRUSH_OPTIONS[2], BRUSH_OPTIONS[3],
    BACKGROUND_ERASE_OPTIONS[4], BACKGROUND_ERASE_OPTIONS[5], BACKGROUND_ERASE_OPTIONS[6],
    text_field("brush.replacement_colour", "context.brush.replacement_colour", "#000000"),
];

const PIXEL_PENCIL_OPTIONS: &[ToolOptionSpec] = &[
    BRUSH_OPTIONS[1], BRUSH_OPTIONS[2],
    toggle("brush.protect_alpha", "brush.protect_alpha", false),
    BRUSH_SYMMETRY_COUNT, BRUSH_SYMMETRY_MIRROR, BRUSH_SYMMETRY_LOCK, BRUSH_SYMMETRY_X, BRUSH_SYMMETRY_Y,
];

const ONE_PIXEL_MARQUEE_OPTIONS: &[ToolOptionSpec] = &[
    choice("marquee.op", "context.marquee.op", SELECTION_OPS, "new"),
    number("marquee.feather", "context.marquee.feather", 0.0, 1000.0, 0.1, 0.0),
    action("selection.refine", "selection.refine_button", "selection.refine"),
];

pub const FILL_OPTIONS: &[ToolOptionSpec] = &[
    toggle("fill.context_fill", "context.fill.context_fill", false),
    choice("fill.fill_type", "context.fill.fill_type", &["solid", "context"], "solid"),
    number("fill.pigment_blend", "context.fill.pigment_blend", 0.0, 1.0, 0.01, 1.0),
    number("fill.tolerance", "context.fill.tolerance", 0.0, 100.0, 1.0, 32.0),
    toggle("fill.contiguous", "context.wand.contiguous", true),
    toggle("fill.antialias", "context.marquee.antialias", true),
    choice("fill.blend", "context.fill.blend", &["replace", "normal"], "replace"),
    choice("fill.source", "context.fill.source", &["current_layer", "all_layers"], "current_layer"),
];

const FLOOD_ERASE_OPTIONS: &[ToolOptionSpec] = &[
    number("fill.tolerance", "context.fill.tolerance", 0.0, 100.0, 1.0, 32.0),
    toggle("fill.contiguous", "context.wand.contiguous", true),
    toggle("fill.antialias", "context.marquee.antialias", true),
    choice("fill.source", "context.fill.source", &["current_layer", "all_layers"], "current_layer"),
];

pub const GRADIENT_OPTIONS: &[ToolOptionSpec] = &[
    choice("gradient.type", "context.gradient.type", &["linear", "elliptical", "radial", "conical", "bitmap"], "linear"),
    choice("gradient.target", "context.gradient.target", &["fill", "stroke"], "fill"),
];

pub const TRANSPARENCY_OPTIONS: &[ToolOptionSpec] = &[
    choice("gradient.type", "context.gradient.type", &["linear", "elliptical", "radial", "conical"], "linear"),
];

pub const CROP_OPTIONS: &[ToolOptionSpec] = &[
    choice("crop.mode", "context.crop.mode", &["unconstrained", "custom_ratio", "original_ratio", "absolute"], "unconstrained"),
];

pub const STYLE_PICKER_OPTIONS: &[ToolOptionSpec] = &[
    toggle("fill", "context.style_picker.fill", true),
    toggle("stroke", "context.style_picker.stroke", true),
    toggle("opacity", "context.style_picker.opacity", true),
    toggle("effects", "context.style_picker.effects", true),
    toggle("paint_order", "context.style_picker.paint_order", true),
    toggle("blend_mode", "context.style_picker.blend_mode", true),
];

pub const ARTBOARD_TOOL: &str = "tool.artboard";
pub const ARTBOARD_SOURCE: &str = "artboard.source";
pub const ARTBOARD_WIDTH: &str = "artboard.width";
pub const ARTBOARD_HEIGHT: &str = "artboard.height";
pub const ARTBOARD_ASPECT: &str = "artboard.aspect";
pub const ARTBOARD_SPACING: &str = "artboard.spacing";
pub const ARTBOARD_OPTIONS: &[ToolOptionSpec] = &[
    choice(ARTBOARD_SOURCE, "artboard.source", &["document", "selection", "custom"], "document"),
    number(ARTBOARD_WIDTH, "artboard.width", 0.001, 1_000_000.0, 1.0, 100.0),
    number(ARTBOARD_HEIGHT, "artboard.height", 0.001, 1_000_000.0, 1.0, 100.0),
    toggle(ARTBOARD_ASPECT, "artboard.aspect", false),
    number(ARTBOARD_SPACING, "artboard.spacing", 0.0, 1_000_000.0, 1.0, 20.0),
    action("artboard.insert", "artboard.insert", "artboard.create"),
];

pub const EMPTY_OPTIONS: &[ToolOptionSpec] = &[];

pub fn options_for(tool: &str) -> &'static [ToolOptionSpec] {
    let Some(tool) = canonical_tool(tool) else { return EMPTY_OPTIONS };
    match tool {
        "tool.move" | "tool.select.object" => MOVE_OPTIONS,
        ARTBOARD_TOOL => ARTBOARD_OPTIONS,
        "tool.view" => VIEW_OPTIONS,
        "tool.table" => TABLE_OPTIONS,
        "tool.zoom" => ZOOM_OPTIONS,
        "tool.node" => NODE_OPTIONS,
        "tool.corner" => &NODE_CORNER_OPTIONS,
        "tool.stroke_width" => STROKE_WIDTH_OPTIONS,
        "tool.point_transform" => POINT_TRANSFORM_OPTIONS,
        "tool.pen.pencil" => PENCIL_OPTIONS,
        "tool.pen.spline" | "tool.pen.b_spline" => SPLINE_OPTIONS,
        "tool.paint.vector_brush" => VECTOR_BRUSH_OPTIONS,
        "tool.knife" => KNIFE_OPTIONS,
        "tool.shape_builder" => SHAPE_BUILDER_OPTIONS,
        "tool.style_picker" => STYLE_PICKER_OPTIONS,
        "tool.flood_fill" => FILL_OPTIONS,
        "tool.eraser.flood" => FLOOD_ERASE_OPTIONS,
        "tool.eraser.background" => BACKGROUND_ERASE_OPTIONS,
        "tool.eraser.erase" | "tool.eraser" => PIXEL_ERASE_OPTIONS,
        "tool.paint.brush" | "tool.paint_brush" => PIXEL_PAINT_OPTIONS,
        "tool.paint.pixel" => SINGLE_PIXEL_BRUSH_OPTIONS,
        CALLIGRAPHY_TOOL => CALLIGRAPHY_OPTIONS,
        CLONE_TOOL => CLONE_BRUSH_OPTIONS,
        MIXER_TOOL => MIXER_OPTIONS,
        "tool.paint.adjustment" | "tool.dodge" | "tool.burn" => TONAL_BRUSH_OPTIONS,
        TONE_TOOL => NONDESTRUCTIVE_TONE_OPTIONS,
        "tool.paint.sponge" => SPONGE_BRUSH_OPTIONS,
        "tool.paint.filter" | "tool.paint.blur" | "tool.paint.median" => FILTER_BRUSH_OPTIONS,
        "tool.sharpen" => SHARPEN_BRUSH_OPTIONS,
        "tool.paint.smudge" | "tool.eraser.smudge" => SMUDGE_BRUSH_OPTIONS,
        "tool.paint.colour_replace" => COLOUR_REPLACE_OPTIONS,
        "tool.crop" => CROP_OPTIONS,
        "tool.magic_wand" => WAND_OPTIONS,
        "tool.selection_brush" => SELECTION_BRUSH_OPTIONS,
        "tool.marquee.freehand" => LASSO_OPTIONS,
        "tool.marquee.row" | "tool.marquee.column" => ONE_PIXEL_MARQUEE_OPTIONS,
        "tool.paint.pencil" => PIXEL_PENCIL_OPTIONS,
        "tool.shape.line" | "tool.shape.curve" => LINE_OPTIONS,
        "tool.shape.rounded_rectangle" => ROUNDED_RECTANGLE_OPTIONS,
        "tool.shape.polygon" => POLYGON_OPTIONS,
        _ if tool == "tool.shapes" || tool.starts_with("tool.shape.") => SHAPE_OPTIONS,
        _ if tool == "tool.text" || tool.starts_with("tool.text.") => TEXT_OPTIONS,
        _ if tool == "tool.pen" || tool.starts_with("tool.pen.") => PEN_OPTIONS,
        _ if tool.starts_with("tool.marquee.") => MARQUEE_OPTIONS,
        "tool.transparency" => TRANSPARENCY_OPTIONS,
        _ if tool == "tool.gradient" || tool.starts_with("tool.gradient.") => GRADIENT_OPTIONS,
        _ if tool == "tool.paint_brush" || tool.starts_with("tool.paint.") || tool == "tool.eraser" || tool.starts_with("tool.eraser.") => BRUSH_OPTIONS,
        _ => EMPTY_OPTIONS,
    }
}

const BRUSH_EDITOR: ToolOptionSpec = action("brush.editor", "brush_editor.more", crate::commands::pixel_presets::EDITOR_OPEN);

pub fn find_option(tool: &str, id: &str) -> Option<&'static ToolOptionSpec> {
    if id == BRUSH_EDITOR.id && crate::commands::pixel_presets::editor_tool(tool).is_some() {
        return Some(&BRUSH_EDITOR);
    }
    if id == ART_TEXT_VERTICAL && matches!(canonical_tool(tool), Some("tool.text.artistic" | "tool.text")) {
        return Some(&ART_TEXT_DIRECTION);
    }
    let options = options_for(tool);
    options
        .iter()
        .find(|spec| spec.id == id)
        .or_else(|| {
            let suffix = format!(".{id}");
            options.iter().find(|spec| spec.id.ends_with(&suffix))
        })
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolOptionStore {
    raster_tips: std::collections::HashMap<String, crate::brush::BrushTip>,
    values: BTreeMap<String, Value>,
}

impl ToolOptionStore {
    pub fn raster_tip(&self, tool: &str) -> Option<&crate::brush::BrushTip> {
        self.raster_tips.get(canonical_tool(tool)?)
    }

    pub(crate) fn set_raster_tip(&mut self, tool: &str, tip: crate::brush::BrushTip) {
        if let Some(tool) = canonical_tool(tool) { self.raster_tips.insert(tool.to_owned(), tip); }
    }

    fn slot(tool: &str, id: &str) -> String {
        if id == CURVE_ENERGY {
            return id.to_owned();
        }
        format!("{tool}|{id}")
    }

    pub fn get(&self, tool: &str, spec: &ToolOptionSpec) -> Value {
        canonical_tool(tool)
            .and_then(|tool| self.values.get(&Self::slot(tool, spec.id)).cloned())
            .unwrap_or_else(|| match (spec.id, tool) {
                ("gradient.type", "tool.gradient.radial") => json!("radial"),
                ("gradient.type", "tool.gradient.conical") => json!("conical"),
                _ => spec.default_value(),
            })
    }

    pub fn set(&mut self, tool: &str, id: &str, value: &Value) -> Option<(&'static ToolOptionSpec, Value)> {
        let tool = canonical_tool(tool)?;
        let spec = find_option(tool, id)?;
        if spec.is_action() {
            return None;
        }
        let value = spec.normalize(value)?;
        self.values.insert(Self::slot(tool, spec.id), value.clone());
        Some((spec, value))
    }

    pub fn value(&self, tool: &str, id: &str) -> Option<Value> {
        let tool = canonical_tool(tool)?;
        let spec = find_option(tool, id)?;
        if spec.is_action() {
            return None;
        }
        Some(self.get(tool, spec))
    }

    pub(crate) fn explicit_number(&self, tool: &str, id: &str) -> Option<f64> {
        self.values.get(&Self::slot(tool, id))?.as_f64()
    }

    pub fn number(&self, tool: &str, id: &str) -> Option<f64> {
        self.value(tool, id).and_then(|value| value.as_f64())
    }

    pub fn flag(&self, tool: &str, id: &str) -> Option<bool> {
        self.value(tool, id).and_then(|value| value.as_bool())
    }

    pub fn choice(&self, tool: &str, id: &str) -> Option<String> {
        self.value(tool, id).and_then(|value| value.as_str().map(String::from))
    }

    pub fn set_number(&mut self, tool: &str, id: &str, value: f64) -> bool {
        self.set(tool, id, &json!(value)).is_some()
    }

    pub fn set_flag(&mut self, tool: &str, id: &str, value: bool) -> bool {
        self.set(tool, id, &json!(value)).is_some()
    }

    pub fn set_choice(&mut self, tool: &str, id: &str, value: &str) -> bool {
        self.set(tool, id, &json!(value)).is_some()
    }

    pub fn reset_tool(&mut self, tool: &str) {
        if let Some(tool) = canonical_tool(tool) {
            let prefix = format!("{tool}|");
            self.values.retain(|slot, _| !slot.starts_with(&prefix));
        }
    }

    pub fn query(&self, tool: &str) -> Value {
        let Some(tool) = canonical_tool(tool) else {
            return json!({ "tool": Value::Null, "ids": Vec::<&str>::new(), "options": Vec::<Value>::new() });
        };
        let editor = crate::commands::pixel_presets::editor_tool(tool).map(|_| &BRUSH_EDITOR);
        let direction = find_option(tool, ART_TEXT_VERTICAL);
        let options = options_for(tool).iter().chain(editor).chain(direction);
        let ids: Vec<&str> = options.clone().map(|spec| spec.id).collect();
        let described: Vec<Value> = options.map(|spec| spec.describe(&self.get(tool, spec))).collect();
        json!({ "tool": tool, "ids": ids, "options": described })
    }
}

pub const KNIFE_AUTO_CLOSE_GAP: f64 = 1.0;

pub fn prepare_knife_cut(cut: Vec<[f64; 2]>, auto_close: bool, smoothness: f64, stabiliser: f64) -> Vec<[f64; 2]> {
    let cut = stabilise_knife_cut(cut, stabiliser);
    let mut cut = if smoothness > 0.0 { smooth_knife_cut(&cut, smoothness) } else { cut };
    if auto_close {
        cut = auto_close_knife_cut(&cut);
    }
    cut
}

fn stabilise_knife_cut(cut: Vec<[f64; 2]>, stabiliser: f64) -> Vec<[f64; 2]> {
    let strength = stabiliser.clamp(0.0, 100.0) / 100.0;
    if !(strength > 0.0) || cut.len() < 3 {
        return cut;
    }
    let radius = (8.0 * strength).ceil() as usize;
    let mut averaged: Vec<_> = cut.iter().enumerate().map(|(index, &point)| {
        if index == 0 || index + 1 == cut.len() {
            return point;
        }
        let samples = &cut[index.saturating_sub(radius)..(index + radius + 1).min(cut.len())];
        let sum = samples.iter().fold([0.0, 0.0], |[x, y], p| [x + p[0], y + p[1]]);
        std::array::from_fn(|axis| point[axis] + strength * (sum[axis] / samples.len() as f64 - point[axis]))
    }).collect();
    averaged.dedup();
    averaged
}

fn auto_close_knife_cut(cut: &[[f64; 2]]) -> Vec<[f64; 2]> {
    if cut.len() < 3 {
        return cut.to_vec();
    }
    let first = cut[0];
    let last = cut[cut.len() - 1];
    let dx = first[0] - last[0];
    let dy = first[1] - last[1];
    let distance = (dx * dx + dy * dy).sqrt();
    if distance > 0.0 && distance <= KNIFE_AUTO_CLOSE_GAP {
        let mut closed = cut.to_vec();
        closed.push(first);
        closed
    } else {
        cut.to_vec()
    }
}

fn smooth_knife_cut(cut: &[[f64; 2]], smoothness: f64) -> Vec<[f64; 2]> {
    let rounds = ((smoothness / 25.0).floor() as usize).min(4);
    if rounds == 0 || cut.len() < 3 {
        return cut.to_vec();
    }
    let mut points = cut.to_vec();
    for _ in 0..rounds {
        if points.len() < 3 {
            break;
        }
        let mut next = Vec::with_capacity(points.len() * 2);
        next.push(points[0]);
        for window in points.windows(2) {
            let a = window[0];
            let b = window[1];
            next.push([a[0] * 0.75 + b[0] * 0.25, a[1] * 0.75 + b[1] * 0.25]);
            next.push([a[0] * 0.25 + b[0] * 0.75, a[1] * 0.25 + b[1] * 0.75]);
        }
        *next.last_mut().unwrap() = *points.last().unwrap();
        points = next;
    }
    points
}
