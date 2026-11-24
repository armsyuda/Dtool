use dtool_core::{Document, ObjectId};
use serde::{Deserialize, Serialize};
pub use dtool_core::{CanvasFlags, ViewMode};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayItem {
    pub object_id: ObjectId,
    pub bounds: [f32; 4],
    #[serde(default, deserialize_with = "dtool_core::paint::legacy_fill::deserialize")]
    pub fill: Option<dtool_core::paint::Paint>,
    pub opacity: f32,
    pub outline_only: bool,
    pub role: String,
    #[serde(default = "identity_matrix")]
    pub matrix: [f32; 6],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<[f32; 4]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table_edge: Option<TableEdgePattern>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TableEdgePattern {
    pub style: dtool_core::paint::LineStyle,
    pub horizontal: bool,
}

pub const IDENTITY_MATRIX: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn identity_matrix() -> [f32; 6] {
    IDENTITY_MATRIX
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayList {
    pub generation: u64,
    pub items: Vec<DisplayItem>,
    #[serde(default)]
    pub font_generation: u64,
}

pub fn snap_to_pixel(value: f32) -> f32 {
    value.round()
}

pub fn snap_to_half_pixel(value: f32) -> f32 {
    (value * 2.0).round() * 0.5
}

pub fn node_matrix(obj: &dtool_core::object::Object) -> [f32; 6] {
    dtool_core::geometry::object_matrix(obj)
}

fn adjust_bounds(doc: &Document, flags: &CanvasFlags, mut bounds: [f32; 4]) -> Option<[f32; 4]> {
    if flags.force_pixel_alignment {
        bounds = bounds.map(snap_to_pixel);
    }

    if flags.clip_to_canvas {
        let page_width = doc.pages.get(0).map(|p| p.size).unwrap_or([0.0, 0.0])[0];
        let page_height = doc.pages.get(0).map(|p| p.size).unwrap_or([0.0, 0.0])[1];

        let x_min = 0.0;
        let y_min = 0.0;
        let x_max = page_width;
        let y_max = page_height;

        let item_x = bounds[0];
        let item_y = bounds[1];
        let item_w = bounds[2];
        let item_h = bounds[3];

        if item_x >= x_max || item_y >= y_max || item_x + item_w <= x_min || item_y + item_h <= y_min {
            return None;
        }

        let clipped_x = item_x.max(x_min);
        let clipped_y = item_y.max(y_min);
        let clipped_right = (item_x + item_w).min(x_max);
        let clipped_bottom = (item_y + item_h).min(y_max);

        bounds[0] = clipped_x;
        bounds[1] = clipped_y;
        bounds[2] = clipped_right - clipped_x;
        bounds[3] = clipped_bottom - clipped_y;
    }
    Some(bounds)
}

pub fn build_display_list(doc: &Document, flags: &CanvasFlags) -> DisplayList {
    let mut items = Vec::new();
    let outline_only = matches!(flags.view_mode, ViewMode::Outline | ViewMode::Wireframe);

    for obj in &doc.objects {
        if !dtool_core::scene::is_drawn_with_flags(doc, flags, obj.id) {
            continue;
        }
        let opacity = dtool_core::scene::effective_opacity(doc, obj.id);
        let Some(bounds) = adjust_bounds(doc, flags, [obj.pos[0], obj.pos[1], obj.size[0].abs(), obj.size[1].abs()]) else {
            continue;
        };
        let matrix = node_matrix(obj);
        let clip = obj.vector_crop.map(|crop| [obj.pos[0] + crop.rect[0], obj.pos[1] + crop.rect[1], crop.rect[2], crop.rect[3]]);

        if let dtool_core::object::ObjectKind::Dimension(_) = &obj.kind {
            if let Some(geometry) = dtool_core::outline::dimension_geometry(obj) {
                emit_dimension(&mut items, obj, &geometry, outline_only, matrix, clip, opacity);
            }
            continue;
        }

        if let dtool_core::object::ObjectKind::Table(_) = &obj.kind {
            let mut table_list = DisplayList { generation: doc.version, items: Vec::new(), font_generation: dtool_core::text_lines::font_generation() };
            crate::table::emit_table(&mut table_list, doc, obj, flags);
            for mut item in table_list.items {
                let Some(item_bounds) = adjust_bounds(doc, flags, item.bounds) else {
                    continue;
                };
                item.bounds = item_bounds;
                item.matrix = matrix;
                item.clip = clip;
                item.opacity = opacity;
                items.push(item);
            }
            continue;
        }

        let fill = if outline_only { None } else {
            match dtool_core::paint::project_object_paint(dtool_core::paint::PaintRole::SourceObject, obj) {
                dtool_core::paint::Paint::None => None,
                paint => Some(paint),
            }
        };

        let role = match &obj.kind {
            dtool_core::object::ObjectKind::FrameText { .. } => "FrameText",
            dtool_core::object::ObjectKind::ArtText { .. } => "ArtText",
            dtool_core::object::ObjectKind::PathText(_) => "PathText",
            _ => "Default",
        };

        items.push(DisplayItem {
            object_id: obj.id,
            bounds,
            fill,
            opacity,
            outline_only,
            role: role.to_string(),
            matrix,
            clip,
            points: Vec::new(),
            table_edge: None,
            label: None,
        });
    }

    emit_longdoc(doc, flags, outline_only, &mut items);

    for item in &mut items {
        let mut pts = item.points.clone();
        if pts.is_empty() {
            if let Some(object) = doc.get_object(item.object_id) {
                if let Some(path) = dtool_core::outline::object_outline_world(object) {
                    if let Some((points, _)) = dtool_core::outline::flatten_subpaths(&path, 0.25).into_iter().next() {
                        pts = points.into_iter().map(|point| [point[0] as f32, point[1] as f32]).collect();
                    }
                }
            }
        }
        if pts.is_empty() {
            continue;
        }
        if let Some(warped) = dtool_core::commands::warp::live_warp_points(doc, item.object_id, &pts) {
            item.points = warped;
            if !item.points.is_empty() {
                let xs: Vec<f32> = item.points.iter().map(|point| point[0]).collect();
                let ys: Vec<f32> = item.points.iter().map(|point| point[1]).collect();
                let x0 = xs.iter().copied().fold(f32::INFINITY, f32::min);
                let y0 = ys.iter().copied().fold(f32::INFINITY, f32::min);
                let x1 = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let y1 = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                item.bounds = [x0, y0, x1 - x0, y1 - y0];
            }
        }
    }

    DisplayList {
        generation: doc.version,
        items,
        font_generation: dtool_core::text_lines::font_generation(),
    }
}

fn emit_dimension(items: &mut Vec<DisplayItem>, obj: &dtool_core::object::Object, geometry: &dtool_core::outline::DimensionGeometry, outline_only: bool, matrix: [f32; 6], clip: Option<[f32; 4]>, opacity: f32) {
    let paint = if outline_only {
        None
    } else {
        [&obj.stroke.paint, &obj.fill].into_iter().find(|paint| !matches!(paint, dtool_core::paint::Paint::None)).cloned()
    };
    let mut segments = vec![("DimensionLine", geometry.line)];
    segments.extend(geometry.extensions.iter().map(|segment| ("DimensionExtension", *segment)));
    segments.extend(geometry.ticks.iter().map(|segment| ("DimensionTick", *segment)));
    for (role, [a, b]) in segments {
        let points = vec![[a[0] as f32, a[1] as f32], [b[0] as f32, b[1] as f32]];
        let bounds = [points[0][0].min(points[1][0]), points[0][1].min(points[1][1]), (points[0][0] - points[1][0]).abs(), (points[0][1] - points[1][1]).abs()];
        items.push(DisplayItem { object_id: obj.id, bounds, fill: paint.clone(), opacity, outline_only, role: role.to_string(), matrix, clip, points, table_edge: None, label: None });
    }
    let at = [geometry.label_at[0] as f32, geometry.label_at[1] as f32];
    let (sin, cos) = (geometry.label_angle_deg as f32).to_radians().sin_cos();
    items.push(DisplayItem {
        object_id: obj.id,
        bounds: [at[0], at[1], 0.0, 0.0],
        fill: paint,
        opacity: obj.opacity,
        outline_only,
        role: "DimensionLabel".to_string(),
        matrix,
        clip,
        points: vec![at, [at[0] + cos, at[1] + sin]],
        table_edge: None,
        label: Some(geometry.label.clone()),
    });
}

fn push_longdoc(doc: &Document, flags: &CanvasFlags, outline_only: bool, frame: ObjectId, rect: [f32; 4], role: &str, items: &mut Vec<DisplayItem>) {
    let Some(bounds) = adjust_bounds(doc, flags, [rect[0], rect[1], rect[2] - rect[0], rect[3] - rect[1]]) else {
        return;
    };
    let matrix = doc.get_object(frame).map(node_matrix).unwrap_or(IDENTITY_MATRIX);
    if !dtool_core::scene::is_drawn_with_flags(doc, flags, frame) { return; }
    let opacity = dtool_core::scene::effective_opacity(doc, frame);
    items.push(DisplayItem { object_id: frame, bounds, fill: None, opacity, outline_only, role: role.to_string(), matrix, clip: None, points: Vec::new(), table_edge: None, label: None });
}

fn emit_longdoc(doc: &Document, flags: &CanvasFlags, outline_only: bool, items: &mut Vec<DisplayItem>) {
    use dtool_text::notes_layout::{layout_notes, note_line_height};
    for note in layout_notes(doc, note_line_height(doc)) {
        let role = match note.kind {
            dtool_core::longdoc::notes::NoteKind::Footnote => "Footnote",
            dtool_core::longdoc::notes::NoteKind::Endnote => "Endnote",
            dtool_core::longdoc::notes::NoteKind::Sidenote => "Sidenote",
        };
        push_longdoc(doc, flags, outline_only, note.frame, note.rect, role, items);
    }
    for mark in dtool_core::longdoc::marks::layout_marks(doc) {
        push_longdoc(doc, flags, outline_only, mark.frame, mark.rect, mark.kind.role(), items);
    }
}
pub fn invalidate(list: &DisplayList, doc_generation: u64) -> bool {
    list.generation != doc_generation || list.font_generation != dtool_core::text_lines::font_generation()
}

impl DisplayItem {
    pub fn fill_str(&self) -> Option<String> {
        self.fill.as_ref().and_then(dtool_core::paint::paint_to_legacy_str)
    }
}
#[cfg(test)]
mod f1f2_contracts {
    use super::*;
    use dtool_core::object::{Dimension, Object, ObjectKind, VectorCrop};

    #[test]
    fn dimension_emits_line_ticks_label_and_crop_sets_clip() {
        let mut doc = Document::new([200.0, 200.0], dtool_core::document::Units::Px, 72.0, 1);
        let dimension = Dimension { start: [0.0, 0.0], end: [30.0, 0.0], offset: 0.0, length: 30.0, units: "px".to_string() };
        let dim = doc.create_object(Object { pos: [10.0, 10.0], size: [30.0, 0.0], kind: ObjectKind::Dimension(dimension), ..Object::default() });
        let cropped = doc.create_object(Object { pos: [50.0, 60.0], size: [40.0, 40.0], vector_crop: Some(VectorCrop { rect: [5.0, 5.0, 10.0, 10.0] }), ..Object::default() });
        let list = build_display_list(&doc, &CanvasFlags::default());
        let roles: Vec<&str> = list.items.iter().filter(|item| item.object_id == dim).map(|item| item.role.as_str()).collect();
        assert_eq!(roles, ["DimensionLine", "DimensionTick", "DimensionTick", "DimensionLabel"]);
        let label = list.items.iter().find(|item| item.role == "DimensionLabel").unwrap();
        assert_eq!(label.label.as_deref(), Some("30 px"));
        let line = list.items.iter().find(|item| item.role == "DimensionLine").unwrap();
        assert_eq!(line.points, vec![[10.0, 10.0], [40.0, 10.0]]);
        let crop = list.items.iter().find(|item| item.object_id == cropped).unwrap();
        assert_eq!(crop.clip, Some([55.0, 65.0, 10.0, 10.0]));
        let plain = serde_json::to_value(crop).unwrap();
        assert!(plain.get("points").is_none() && plain.get("label").is_none());
    }

    #[test]
    fn font_generation_change_invalidates_display_list() {
        let mut doc = Document::new([100.0, 100.0], dtool_core::document::Units::Px, 72.0, 1);
        doc.create_object(Object { size: [10.0, 10.0], ..Object::default() });
        let list = build_display_list(&doc, &CanvasFlags::default());
        assert_eq!(list.font_generation, dtool_core::text_lines::font_generation());
        assert!(!invalidate(&list, doc.version) || list.font_generation != dtool_core::text_lines::font_generation());
        dtool_core::text_lines::publish_font_generation(list.font_generation + 1);
        assert!(invalidate(&list, doc.version));
        let fresh = build_display_list(&doc, &CanvasFlags::default());
        assert!(fresh.font_generation > list.font_generation);
        let before = dtool_core::text_lines::font_generation();
        dtool_core::text_lines::publish_font_generation(before.saturating_sub(1));
        assert_eq!(dtool_core::text_lines::font_generation(), before);
        let plain: DisplayList = serde_json::from_value(serde_json::json!({ "generation": 1, "items": [] })).unwrap();
        assert_eq!(plain.font_generation, 0);
    }

    #[test]
    fn hidden_parent_omits_child_from_display_list() {
        let mut doc = Document::new([100.0, 100.0], dtool_core::document::Units::Px, 72.0, 1);
        let parent = doc.create_object(Object { pickable: false, size: [100.0, 100.0], ..Object::default() });
        let child = doc.create_object(Object { pos: [10.0, 10.0], size: [20.0, 20.0], parent_id: Some(parent), ..Object::default() });
        let list = build_display_list(&doc, &CanvasFlags::default());
        assert!(list.items.iter().all(|item| item.object_id != child && item.object_id != parent));
        let holder = doc.create_object(Object { opacity: 0.5, size: [1.0, 1.0], ..Object::default() });
        let visible = doc.create_object(Object { pos: [40.0, 40.0], size: [10.0, 10.0], opacity: 0.5, parent_id: Some(holder), ..Object::default() });
        let list = build_display_list(&doc, &CanvasFlags::default());
        let item = list.items.iter().find(|item| item.object_id == visible).unwrap();
        assert!((item.opacity - 0.25).abs() < 1e-6);
    }
}
