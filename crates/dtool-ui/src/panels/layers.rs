use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::mpsc::TryRecvError;

use dtool_core::document::Document;
use dtool_core::object::ObjectId;
use dtool_core::workspace::{ContextId, Scope, Workspace as CoreWorkspace};
use dtool_design::{panel_frame, panel_header};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::{json, Value};

pub const LAYER_THUMB_PX: f64 = 48.0;
const TEXT_THUMBNAIL_DELAY: std::time::Duration = std::time::Duration::from_millis(150);
type ThumbnailStamp = (ContextId, u64, u64);
type LayerThumbnailJob = super::ThumbnailJob<ThumbnailStamp, Vec<(u64, Option<egui::ColorImage>)>>;

fn render_layer_thumbnail(document: &Document, id: u64) -> Option<egui::ColorImage> {
    let id = ObjectId(id);
    let mut doc = dtool_export::only_objects(document, &[id]);
    // Draw the row's own contents, including a masked or hidden root.
    let subtree = doc.subtree_ids(id);
    doc.objects.retain(|object| subtree.contains(&object.id));
    let root = doc.objects.iter_mut().find(|object| object.id == id)?;
    root.pickable = true;
    root.parent_id = None;
    root.clip_role = dtool_core::object::ClipRole::None;
    let contents: Vec<_> = doc.objects.iter().filter(|object| object.pickable).map(|object| object.id).collect();
    let (x, y, width, height) = dtool_export::selection_region(&doc, &contents)?;
    let scale = LAYER_THUMB_PX as f32 / width.max(height).max(1.0);
    let opts = dtool_export::raster::ExportOptions {
        area: dtool_export::ExportArea::Rect([x, y, width.max(1.0), height.max(1.0)]),
        width: Some((width * scale).round().max(1.0) as u32),
        height: Some((height * scale).round().max(1.0) as u32),
        ..Default::default()
    };
    let rgba = dtool_export::raster::render_preview(&doc, &opts).ok()?;
    if !rgba.pixels().any(|pixel| pixel[3] != 0) {
        return None;
    }
    Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

fn upload_layer_thumbnail(
    ctx: &egui::Context, owner: ContextId, id: u64, image: Option<egui::ColorImage>,
) -> Option<egui::TextureHandle> {
    image.map(|image| ctx.load_texture(format!("layer.{}.{id}", owner.0), image, egui::TextureOptions::LINEAR))
}

const LAYER_TAG_COLOURS: [(&str, &str); 8] = [
    ("#F46A68", "layers.tag.red"),
    ("#F69B51", "layers.tag.orange"),
    ("#EAC758", "layers.tag.yellow"),
    ("#76C778", "layers.tag.green"),
    ("#56C6C8", "layers.tag.cyan"),
    ("#649FF0", "layers.tag.blue"),
    ("#AC8AEF", "layers.tag.violet"),
    ("#E586BF", "layers.tag.pink"),
];

fn tag_colour_picker(ui: &mut egui::Ui, stored: Option<Option<&str>>) -> Option<Option<&'static str>> {
    let mut change = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        let colours = [(None, "layers.tag.none")].into_iter()
            .chain(LAYER_TAG_COLOURS.map(|(hex, label)| (Some(hex), label)));
        for (hex, label) in colours {
            let name = text(label);
            let selected = match (stored, hex) {
                (Some(Some(current)), Some(hex)) => current.eq_ignore_ascii_case(hex),
                (Some(None), None) => true,
                _ => false,
            };
            let (rect, button) = ui.allocate_exact_size(egui::vec2(19.0, 24.0), egui::Sense::click());
            button.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, name));
            let button = button.on_hover_text(name);
            let center = rect.center();
            let colour = hex.and_then(dtool_design::tokens::parse_hex).unwrap_or(egui::Color32::WHITE);
            let painter = ui.painter();
            painter.circle_filled(center, 6.0, colour);
            if hex.is_none() {
                let diagonal = egui::vec2(3.9, -3.9);
                painter.line_segment([center - diagonal, center + diagonal], egui::Stroke::new(1.3, egui::Color32::from_rgb(191, 52, 52)));
            }
            let visuals = ui.style().interact_selectable(&button, selected);
            if selected || button.hovered() || button.has_focus() {
                painter.circle_stroke(center, 8.0, egui::Stroke::new(1.0, visuals.fg_stroke.color));
            }
            if button.clicked() {
                change = Some(hex);
                ui.close();
            }
        }
    });
    change
}

#[derive(Default)]
pub struct LayersPanel {
    pub collapsed: BTreeSet<u64>,
    #[doc(hidden)]
    pub thumbnails: BTreeMap<u64, Option<egui::TextureHandle>>,
    #[doc(hidden)]
    pub thumbnail_stamp: Option<ThumbnailStamp>,
    pending_text_thumbnails: Option<(ThumbnailStamp, std::time::Instant)>,
    background_thumbnails: bool,
    stale_thumbnails: BTreeSet<u64>,
    thumbnail_job: Option<LayerThumbnailJob>,
    pub last_error: Option<String>,
    pub filter: String,
    pub tags: BTreeMap<u64, String>,
    pub tag_colour: BTreeMap<u64, String>,
    pub renaming: Option<LayerRename>,
    pub effects_target: Option<(dtool_core::workspace::ContextId, u64)>,
    pub blend_target: Option<dtool_core::workspace::ContextId>,
    opacity_preview: Option<LayerOpacityPreview>,
    pub mask_icons: crate::icon_cache::IconCache,
    #[doc(hidden)]
    pub reveal_request: Option<(ContextId, ObjectId)>,
    #[doc(hidden)]
    pub revealed_ancestors: Option<(ContextId, BTreeSet<u64>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayerOpacityPreview {
    owner: ContextId,
    generation: u64,
    pub(crate) ids: Vec<ObjectId>,
    pub(crate) value: f32,
}

#[doc(hidden)]
pub struct LayerRename {
    owner: ContextId,
    id: ObjectId,
    name: String,
    focus: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    Above(ObjectId),
    Inside(ObjectId),
    Below(ObjectId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayerRow {
    pub id: u64,
    pub depth: usize,
    pub name: String,
    pub selected: bool,
    pub has_children: bool,
    pub visible: bool,
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TreeNode {
    pub id: u64,
    pub name: String,
    pub parent: Option<u64>,
    pub visible: bool,
    pub locked: bool,
}

pub fn document_nodes(document: &Document) -> Vec<TreeNode> {
    document
        .objects
        .iter()
        .map(|object| TreeNode { id: object.id.0, name: object.name.clone(), parent: object.parent_id.map(|id| id.0), visible: object.pickable, locked: object.locked })
        .collect()
}

pub fn query_nodes(value: &Value) -> Vec<TreeNode> {
    value
        .get("tree")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(TreeNode {
                        id: item.get("id")?.as_u64()?,
                        name: item.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                        parent: item.get("parent_id").and_then(Value::as_u64),
                        visible: item.pointer("/flags/visible").and_then(Value::as_bool).unwrap_or(true),
                        locked: item.pointer("/flags/locked").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn layer_rows(document: &Document, selected: &[ObjectId], collapsed: &BTreeSet<u64>) -> Vec<LayerRow> {
    tree_rows(&document_nodes(document), selected, collapsed)
}

pub fn tree_rows(nodes: &[TreeNode], selected: &[ObjectId], collapsed: &BTreeSet<u64>) -> Vec<LayerRow> {
    let selected: HashSet<_> = selected.iter().copied().collect();
    let mut children: HashMap<_, Vec<_>> = HashMap::new();
    for node in nodes {
        children.entry(node.parent).or_default().push(node);
    }
    let mut pending: Vec<_> = children.remove(&None).unwrap_or_default()
        .into_iter().map(|node| (node, 0)).collect();
    let mut rows = Vec::new();
    while let Some((node, depth)) = pending.pop() {
        let branch = children.remove(&Some(node.id));
        let has_children = branch.is_some();
        let name = if node.name.is_empty() { text("layers.unnamed").to_string() } else { node.name.clone() };
        rows.push(LayerRow { id: node.id, depth, name, selected: selected.contains(&ObjectId(node.id)), has_children, visible: node.visible, locked: node.locked });
        if let Some(children) = branch && !collapsed.contains(&node.id) {
            pending.extend(children.into_iter().map(|child| (child, depth + 1)));
        }
    }
    rows
}

pub fn display_name(id: u64, name: &str) -> String {
    if !dtool_core::commands::layer_meta::has_default_name(name) {
        return name.to_string();
    }
    let key = dtool_core::commands::layer_meta::DEFAULT_NAME_KEY;
    if dtool_strings::has(key) {
        text(key).replace("{n}", &id.to_string())
    } else {
        format!("#{id}")
    }
}

pub fn row_label(row: &LayerRow) -> String {
    let mut label = display_name(row.id, &row.name);
    if !row.visible {
        label = format!("{label} ({})", text("layers.hidden"));
    }
    if row.locked {
        label = format!("{label} ({})", text("layers.locked"));
    }
    label
}

pub fn select_op(selected: bool, modifiers: egui::Modifiers) -> &'static str {
    if modifiers.command && selected {
        "Subtract"
    } else if modifiers.command || modifiers.shift {
        "Add"
    } else {
        "Replace"
    }
}

pub fn filter_rows(rows: &[LayerRow], query: &str, tags: &BTreeMap<u64, String>) -> Vec<LayerRow> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|row| matches_filter(row.id, &row.name, &query, tags))
        .cloned()
        .collect()
}

fn matches_filter(id: u64, name: &str, query: &str, tags: &BTreeMap<u64, String>) -> bool {
    display_name(id, name).to_lowercase().contains(query)
        || tags.get(&id).is_some_and(|tag| tag.to_lowercase().contains(query))
}

fn retain_matching_branches(nodes: &mut Vec<TreeNode>, query: &str, tags: &BTreeMap<u64, String>) {
    let parents: BTreeMap<_, _> = nodes.iter().map(|node| (node.id, node.parent)).collect();
    let mut retained = BTreeSet::new();
    for node in nodes.iter().filter(|node| {
        let name = if node.name.is_empty() { text("layers.unnamed") } else { &node.name };
        matches_filter(node.id, name, query, tags)
    }) {
        let mut current = Some(node.id);
        while let Some(id) = current {
            if !retained.insert(id) {
                break;
            }
            current = parents.get(&id).copied().flatten();
        }
    }
    nodes.retain(|node| retained.contains(&node.id));
}

pub fn drop_zone_at(target: ObjectId, has_children: bool, t: f32) -> DropZone {
    let t = t.clamp(0.0, 1.0);
    if has_children && t > 0.25 && t < 0.75 {
        DropZone::Inside(target)
    } else if t < 0.5 {
        DropZone::Above(target)
    } else {
        DropZone::Below(target)
    }
}

fn drop_zone_on_label(id: ObjectId, can_nest: bool, rect: egui::Rect, point: egui::Pos2) -> DropZone {
    drop_zone_at(id, can_nest, (point.y - rect.top()) / rect.height().max(1.0))
}

pub fn drop_command(dragged: ObjectId, zone: DropZone, nodes: &[TreeNode]) -> Command {
    let parent = match zone {
        DropZone::Inside(id) => Some(id.0),
        DropZone::Above(id) | DropZone::Below(id) => nodes.iter().find(|node| node.id == id.0).and_then(|node| node.parent),
    };
    let mut params = json!({ "ids": [dragged.0], "parent": parent });
    if let DropZone::Above(id) | DropZone::Below(id) = zone {
        params["relative_to"] = json!(id.0);
        params["placement"] = json!(if matches!(zone, DropZone::Above(_)) { "above" } else { "below" });
    }
    Command::with_params("node.reparent", params)
}

pub fn thumb_drop_command(document: &Document, dragged: ObjectId, target: ObjectId) -> Command {
    if document.get_object(target).is_some_and(|object| object.kind == dtool_core::ObjectKind::Artboard) {
        return Command::with_params("node.reparent", json!({ "id": dragged.0, "parent": target.0 }));
    }
    let vector = document.get_object(dragged).is_some_and(|object| matches!(object.kind, dtool_core::object::ObjectKind::Shape | dtool_core::object::ObjectKind::Curve(_) | dtool_core::object::ObjectKind::Compound(_)));
    if vector {
        Command::with_params("layer.vector_mask.set", json!({ "id": dragged.0, "target": target.0 }))
    } else {
        Command::with_params("layer.clip.set", json!({ "id": dragged.0, "parent": target.0 }))
    }
}

pub fn mask_view_command(session: &dtool_core::Session, id: ObjectId) -> Command {
    let id = mask_preview_owner(&session.document, id);
    let target = if session.view.canvas_flags.mask_view == Some(id) { Value::Null } else { json!(id.0) };
    Command::with_params("view.mask_view.set", json!({ "id": target }))
}

fn mask_preview_owner(document: &Document, id: ObjectId) -> ObjectId {
    document.get_object(id)
        .filter(|object| object.clip_role == dtool_core::object::ClipRole::Mask)
        .and_then(|object| object.parent_id)
        .unwrap_or(id)
}

pub fn clip_badge(document: &Document, id: ObjectId) -> &'static str {
    match document.get_object(id).map(|object| object.clip_role) {
        Some(dtool_core::object::ClipRole::Clip) => "◧",
        Some(dtool_core::object::ClipRole::Mask) => "◐",
        _ => "",
    }
}

fn layer_kind_icon(document: &Document, object: &dtool_core::object::Object) -> &'static str {
    use dtool_core::object::ObjectKind;
    use dtool_core::shape::ShapeKind;

    if document.pixel_layers.get(object.id).is_some() {
        return "tool-paint-pixel";
    }
    if dtool_core::commands::layer_kinds::is_fill_layer(object) {
        return "tool-flood-fill";
    }
    match &object.kind {
        ObjectKind::Artboard => "tool-artboard",
        ObjectKind::Group if object.is_layer_folder() => "layer-folder",
        ObjectKind::Group => "cmd-group",
        ObjectKind::Image => "tool-place-image",
        ObjectKind::PictureFrame => {
            use dtool_core::commands::picture_frame::{get_frame, FrameShape};
            if get_frame(document, object.id).is_some_and(|frame| frame.shape == FrameShape::Ellipse) {
                "tool-picture-frame-ellipse"
            } else {
                "tool-picture-frame-rect"
            }
        }
        ObjectKind::Text(_) | ObjectKind::ArtText { .. } => "tool-artistic-text",
        ObjectKind::FrameText { .. } => "tool-frame-text",
        ObjectKind::PathText(_) => "tool-text-path",
        ObjectKind::Curve(_) => "tool-shape-curve",
        ObjectKind::Dimension(_) => "tool-measure",
        ObjectKind::Table(_) => "tool-table",
        ObjectKind::Compound(_) => "cmd-bool-add",
        ObjectKind::Shape => match object.shape.as_ref().map_or(ShapeKind::Rectangle, |shape| shape.kind) {
            ShapeKind::Rectangle => "tool-rectangle",
            ShapeKind::Ellipse => "tool-ellipse",
            ShapeKind::RoundedRectangle => "tool-shape-rounded-rectangle",
            ShapeKind::Triangle => "tool-shape-triangle",
            ShapeKind::Polygon => "tool-polygon",
            ShapeKind::Star => "tool-shape-star",
            ShapeKind::Arrow => "tool-shape-arrow",
            ShapeKind::CalloutEllipse => "tool-shape-callout-ellipse",
            ShapeKind::CalloutRectangle => "tool-shape-callout-rectangle",
            ShapeKind::Donut => "tool-shape-donut",
            ShapeKind::Pie => "tool-shape-pie",
            ShapeKind::Heart => "tool-shape-heart",
            ShapeKind::Cog => "tool-shape-cog",
            ShapeKind::Tear => "tool-shape-tear",
            ShapeKind::Crescent => "tool-shape-crescent",
            ShapeKind::Cloud => "tool-shape-cloud",
            ShapeKind::Segment => "tool-shape-segment",
            ShapeKind::Cat => "tool-shape-cat",
            ShapeKind::SquareStar => "tool-shape-square-star",
            ShapeKind::DoubleStar => "tool-shape-double-star",
            ShapeKind::Diamond => "tool-shape-diamond",
            ShapeKind::Trapezoid => "tool-shape-trapezoid",
            ShapeKind::Spiral => "tool-shape-spiral",
        },
    }
}

pub fn thumbnail_command(id: u64) -> Command {
    Command::with_params("render.node_preview", json!({ "id": id, "max_px": LAYER_THUMB_PX }))
}

pub fn thumbnail_request(id: u64) -> Result<dtool_core::commands::preview::PreviewRequest, dtool_core::CommandError> {
    dtool_core::commands::preview::parse(&thumbnail_command(id).params)
}

pub fn reveal_target(session: &dtool_core::Session) -> Option<ObjectId> {
    let id = session.selection.key()?;
    session.document.get_object(id).map(|_| id)
}

pub fn rename_target(session: &dtool_core::Session) -> Option<ObjectId> {
    let [id] = session.selection.ids.as_slice() else { return None };
    session.document.get_object(*id).filter(|object| !object.locked).map(|_| *id)
}

impl LayersPanel {
    pub fn begin_rename(&mut self, owner: ContextId, document: &Document, id: ObjectId) {
        if let Some(object) = document.get_object(id).filter(|object| !object.locked) {
            self.renaming = Some(LayerRename { owner, id, name: object.name.clone(), focus: true });
        }
    }

    pub fn reveal(&mut self, owner: ContextId, document: &Document, target: ObjectId) {
        let mut parents = BTreeSet::new();
        let mut current = target;
        while let Some(parent) = document.get_object(current).and_then(|object| object.parent_id) {
            if parent == target || !parents.insert(parent.0) {
                break;
            }
            current = parent;
        }
        self.revealed_ancestors = Some((owner, parents));
        self.reveal_request = Some((owner, target));
    }

    fn revealed(&self, workspace: &CoreWorkspace, id: u64) -> bool {
        self.revealed_ancestors.as_ref().is_some_and(|(owner, parents)| {
            Some(*owner) == workspace.active() && parents.contains(&id)
        })
    }

    fn is_collapsed(&self, workspace: &CoreWorkspace, id: u64) -> bool {
        self.collapsed.contains(&id) && !self.revealed(workspace, id)
    }

    pub fn sync_from_document(&mut self, workspace: &CoreWorkspace) {
        self.tags.clear();
        self.tag_colour.clear();
        self.collapsed.clear();
        let Some(session) = workspace.session() else { return };
        for item in &session.document.layer_meta {
            if session.document.get_object(item.object).is_none() {
                continue;
            }
            let id = item.object.0;
            if let Some(tag) = &item.tag {
                self.tags.insert(id, tag.clone());
            }
            if let Some(colour) = &item.colour {
                self.tag_colour.insert(id, colour.clone());
            }
            if item.collapsed {
                self.collapsed.insert(id);
            }
        }
    }

    pub fn set_meta(&mut self, workspace: &mut CoreWorkspace, id: u64, patch: Value) -> bool {
        let mut params = patch;
        params["object"] = json!(id);
        let (_, result) = workspace.run(Scope::active(), Command::with_params(dtool_core::commands::layer_meta::LAYER_META_SET, params));
        match result {
            Ok(_) => {
                self.last_error = None;
                self.sync_from_document(workspace);
                true
            }
            Err(error) => {
                self.last_error = Some(error.message_key);
                false
            }
        }
    }

    pub fn toggle_collapsed(&mut self, workspace: &mut CoreWorkspace, id: u64) -> bool {
        self.sync_from_document(workspace);
        if self.collapsed.contains(&id) && self.revealed(workspace, id) {
            if let Some((_, parents)) = &mut self.revealed_ancestors {
                parents.remove(&id);
            }
            return true;
        }
        let collapsed = !self.is_collapsed(workspace, id);
        self.set_meta(workspace, id, json!({"collapsed": collapsed}))
    }

    fn tree_command(session: &dtool_core::Session) -> Command {
        dtool_core::commands::arrange::layer_panel_tree_command(session, true)
    }

    pub fn collapse_all(&mut self, workspace: &mut CoreWorkspace) -> dtool_core::CommandResult {
        let session = workspace.session().ok_or_else(dtool_core::CommandError::no_document)?;
        let command = Self::tree_command(session);
        let (_, result) = workspace.run(Scope::active(), command);
        let parents: BTreeSet<_> = query_nodes(&result?).into_iter().filter_map(|node| node.parent).collect();
        self.sync_from_document(workspace);
        let ids: Vec<_> = parents.difference(&self.collapsed).copied().collect();
        if !ids.is_empty() {
            let (_, result) = workspace.run(Scope::active(), Command::with_params(
                dtool_core::commands::layer_meta::LAYER_META_SET,
                json!({"ids": ids, "collapsed": true}),
            ));
            result?;
            self.sync_from_document(workspace);
        }
        if self.revealed_ancestors.as_ref().is_some_and(|(owner, _)| Some(*owner) == workspace.active()) {
            self.revealed_ancestors = None;
            self.reveal_request = None;
        }
        Ok(json!({"collapsed": ids.len()}))
    }

    pub fn rows(&self, workspace: &mut CoreWorkspace) -> Vec<LayerRow> {
        let Some(session) = workspace.session() else { return Vec::new(); };
        let selected = session.selection.ids.clone();
        let command = Self::tree_command(session);
        let (_, result) = workspace.run(Scope::active(), command);
        match result {
            Ok(value) => {
                let mut nodes = query_nodes(&value);
                if let Some(session) = workspace.session() {
                    let document = &session.document;
                    for node in nodes.iter_mut().filter(|node| node.name.is_empty()) {
                        if let Some(name) = document.get_object(ObjectId(node.id))
                            .and_then(|object| dtool_core::commands::layer_meta::text_layer_name(document, object))
                        {
                            node.name = name;
                        }
                    }
                }
                let query = self.filter.trim().to_lowercase();
                let collapsed = if query.is_empty() {
                    self.collapsed.iter().copied().filter(|id| !self.revealed(workspace, *id)).collect()
                } else {
                    retain_matching_branches(&mut nodes, &query, &self.tags);
                    BTreeSet::new()
                };
                tree_rows(&nodes, &selected, &collapsed)
            }
            Err(_) => Vec::new(),
        }
    }

    pub fn select(&mut self, workspace: &mut CoreWorkspace, id: u64, op: &str) {
        let command = Command::with_params("selection.set", json!({"ids": [id], "op": op}));
        let (_, result) = workspace.run(Scope::active(), command);
        self.last_error = result.err().map(|error| error.message_key);
    }

    fn run_reported(&mut self, workspace: &mut CoreWorkspace, command: Command) -> bool {
        let (_, result) = workspace.run(Scope::active(), command);
        self.last_error = result.as_ref().err().map(|error| error.message_key.clone());
        result.is_ok()
    }

    pub fn apply_thumb_drop(&mut self, workspace: &mut CoreWorkspace, dragged: ObjectId, target: ObjectId) -> bool {
        let Some(command) = workspace.session().map(|session| thumb_drop_command(&session.document, dragged, target)) else {
            self.last_error = Some(dtool_core::CommandError::no_document().message_key);
            return false;
        };
        self.run_reported(workspace, command)
    }

    pub fn toggle_mask_view(&mut self, workspace: &mut CoreWorkspace, id: ObjectId) -> bool {
        let Some(command) = workspace.session().map(|session| mask_view_command(session, id)) else {
            self.last_error = Some(dtool_core::CommandError::no_document().message_key);
            return false;
        };
        self.run_reported(workspace, command)
    }

    pub fn apply_drop(&mut self, workspace: &mut CoreWorkspace, dragged: ObjectId, zone: DropZone) -> bool {
        let nodes = match workspace.session() {
            Some(session) => document_nodes(&session.document),
            None => {
                self.last_error = Some(dtool_core::CommandError::no_document().message_key);
                return false;
            }
        };
        let command = drop_command(dragged, zone, &nodes);
        let (_, result) = workspace.run(Scope::active(), command);
        match result {
            Ok(_) => {
                self.last_error = None;
                true
            }
            Err(error) => {
                self.last_error = Some(error.message_key);
                false
            }
        }
    }

    pub fn rename(&mut self, workspace: &mut CoreWorkspace, id: u64, name: &str) -> bool {
        let is_artboard = workspace.session().and_then(|session| session.document.get_object(ObjectId(id)))
            .is_some_and(|object| object.kind == dtool_core::ObjectKind::Artboard);
        let command = Command::with_params(
            if is_artboard { "artboard.update" } else { "object.update" },
            json!({ "id": id, "name": name }),
        );
        let (_, result) = workspace.run(Scope::active(), command);
        match result {
            Ok(_) => {
                self.last_error = None;
                self.renaming = None;
                true
            }
            Err(error) => {
                self.last_error = Some(error.message_key);
                false
            }
        }
    }

    pub fn row_thumbnail(&mut self, workspace: &mut CoreWorkspace, id: u64) -> Result<Value, dtool_core::CommandError> {
        let (_, result) = workspace.run(Scope::active(), thumbnail_command(id));
        result
    }

    pub fn set_visible(&mut self, workspace: &mut CoreWorkspace, id: u64, visible: bool) -> bool {
        self.run_reported(workspace, Command::with_params("node.update", json!({"ids":[id], "set":{"visible":visible}})))
    }

    pub fn set_locked(&mut self, workspace: &mut CoreWorkspace, id: u64, locked: bool) -> bool {
        self.run_reported(workspace, Command::with_params("node.lock_state", json!({"ids":[id], "state":if locked {"locked"} else {"unlocked"}})))
    }

    fn prepare_thumbnails(&mut self, ctx: &egui::Context, workspace: &CoreWorkspace) -> bool {
        let Some((owner, session)) = workspace.active().zip(workspace.session()) else {
            self.thumbnail_stamp = None;
            self.pending_text_thumbnails = None;
            self.background_thumbnails = false;
            self.stale_thumbnails.clear();
            self.thumbnails.clear();
            return false;
        };
        let stamp = (owner, session.generation, session.document.version);
        let mut deferred = false;
        if self.thumbnail_stamp != Some(stamp) {
            let same_document = self.thumbnail_stamp.is_some_and(|old| old.0 == stamp.0);
            let editing = session.text_edit.is_some();
            let after_text_edit = same_document && (editing || self.pending_text_thumbnails.is_some());
            if editing && same_document {
                let now = std::time::Instant::now();
                let (pending, deadline) = self.pending_text_thumbnails.get_or_insert((stamp, now + TEXT_THUMBNAIL_DELAY));
                if *pending != stamp {
                    *pending = stamp;
                    *deadline = now + TEXT_THUMBNAIL_DELAY;
                }
                if let Some(remaining) = deadline.checked_duration_since(now) {
                    ctx.request_repaint_after(remaining);
                    deferred = true;
                }
            }
            if !deferred {
                self.pending_text_thumbnails = None;
                self.background_thumbnails = after_text_edit || !session.document.pixel_layers.is_empty();
                self.stale_thumbnails.clear();
                if same_document && self.background_thumbnails {
                    self.stale_thumbnails.extend(self.thumbnails.keys().copied());
                } else {
                    self.thumbnails.clear();
                }
                self.thumbnail_stamp = Some(stamp);
            }
        }
        if let Some(job) = &self.thumbnail_job {
            match job.result.try_recv() {
                Ok(images) => {
                    if job.revision == stamp {
                        for (id, image) in images {
                            self.stale_thumbnails.remove(&id);
                            self.thumbnails.insert(id, upload_layer_thumbnail(ctx, owner, id, image));
                        }
                    }
                }
                Err(TryRecvError::Empty) => return deferred,
                Err(TryRecvError::Disconnected) if job.revision == stamp => self.background_thumbnails = false,
                Err(TryRecvError::Disconnected) => {}
            }
            self.thumbnail_job = None;
        }
        deferred
    }

    fn thumbnail(
        &mut self, ctx: &egui::Context, document: &Document, id: u64,
        deferred: bool, requests: &mut Vec<u64>,
    ) -> Option<egui::TextureHandle> {
        if !deferred && (self.stale_thumbnails.contains(&id) || !self.thumbnails.contains_key(&id)) {
            if self.background_thumbnails {
                requests.push(id);
            } else if let Some((owner, _, _)) = self.thumbnail_stamp {
                let image = render_layer_thumbnail(document, id);
                self.thumbnails.insert(id, upload_layer_thumbnail(ctx, owner, id, image));
                self.stale_thumbnails.remove(&id);
            }
        }
        self.thumbnails.get(&id).cloned().flatten()
    }

    fn start_thumbnails(&mut self, ctx: &egui::Context, workspace: &CoreWorkspace, requests: Vec<u64>) {
        if requests.is_empty() || self.thumbnail_job.is_some() {
            return;
        }
        let Some((stamp, session)) = self.thumbnail_stamp.zip(workspace.session()) else { return };
        let doc = session.document.clone();
        match LayerThumbnailJob::start(ctx, "dtool-layer-thumbnails", stamp, move || {
            requests.into_iter().map(|id| (id, render_layer_thumbnail(&doc, id))).collect()
        }) {
            Ok(job) => self.thumbnail_job = Some(job),
            Err(_) => {
                self.background_thumbnails = false;
                ctx.request_repaint();
            }
        }
    }

    pub fn request_effects(&mut self, workspace: &mut CoreWorkspace, id: u64) -> bool {
        let Some(owner) = workspace.active() else { return false };
        if !workspace.session().is_some_and(|s| s.document.get_object(ObjectId(id)).is_some()) { return false; }
        self.select(workspace, id, "Replace");
        self.effects_target = Some((owner, id));
        true
    }

    pub(crate) fn opacity_preview(&self, ctx: &egui::Context, session: &dtool_core::Session) -> Option<LayerOpacityPreview> {
        self.opacity_preview.as_ref().filter(|preview| {
            session.context == Some(preview.owner) && session.generation == preview.generation
                && session.selection.ids == preview.ids && ctx.input(|input| input.pointer.primary_down())
        }).cloned()
    }

    fn show_compositing(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        use crate::dialogs::blend_options::{blend_set_command, mode_label, MODES};
        use dtool_design::{ControlState, NumericEvent, NumericSpec, NumericValue, MIXED_TEXT};

        let selected: Vec<_> = workspace.session().into_iter().flat_map(|session| {
            session.selection.ids.iter().filter_map(move |id| session.document.get_object(*id))
        }).collect();
        let ids: Vec<_> = selected.iter().map(|object| object.id).collect();
        let enabled = !selected.is_empty() && selected.iter().all(|object| !object.locked);
        let all_groups = selected.iter().all(|object| matches!(object.kind, dtool_core::object::ObjectKind::Group));
        let current_opacity = selected.first().map(|object| object.opacity);
        let average = if selected.is_empty() { 100.0 } else {
            selected.iter().map(|object| f64::from(object.opacity) * 100.0).sum::<f64>() / selected.len() as f64
        };
        let opacity = match current_opacity {
            Some(first) if selected.iter().all(|object| object.opacity == first) => NumericValue::Value(f64::from(first) * 100.0),
            _ => NumericValue::Mixed,
        };
        let current_mode = selected.first().map(|object| object.blend_mode);
        let mut mode = match current_mode {
            Some(first) if selected.iter().all(|object| object.blend_mode == first) => format!("{first:?}"),
            _ => String::new(),
        };
        let modes: Vec<_> = MODES.iter().copied().filter(|name| *name != "PassThrough" || all_groups)
            .map(|name| (name.to_string(), mode_label(name))).collect();
        let options: Vec<_> = modes.iter().map(|(name, label)| (name.clone(), label.as_str())).collect();

        let draft = workspace.session().and_then(|session| self.opacity_preview(ui.ctx(), session))
            .map(|preview| f64::from(preview.value) * 100.0);
        let (blend, opacity_event) = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let opacity_label = text("layers.opacity");
            let available = ui.available_width();
            let label = if available >= 210.0 { opacity_label } else { "" };
            let field_id = ui.make_persistent_id(("layers.opacity", workspace.active(), &ids));
            let spec = NumericSpec::new(label).range(0.0..=100.0)
                .width(if available < 155.0 { 43.0 } else { 54.0 })
                .suffix("%").mixed_hint(MIXED_TEXT).mixed_origin(average).enabled(enabled);
            let shown = draft.map_or(opacity, NumericValue::Value);
            let (response, mut event) = dtool_design::numeric_field(ui, field_id, shown, &spec);
            response.on_hover_text(opacity_label);
            let popup_label = text("layers.opacity.slider");
            let popup = ui.add_enabled(enabled, egui::Button::new("⌄").small().min_size(egui::vec2(14.0, 20.0)))
                .on_hover_text(popup_label);
            popup.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled && ui.is_enabled(), popup_label));
            egui::Popup::menu(&popup).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                let mut value = draft.unwrap_or(average) as f32;
                let rail = dtool_design::slider_rail(ui, &mut value, 0.0..=100.0, 145.0, opacity_label, enabled);
                let pressed = |key| ui.input(|input| input.key_pressed(key));
                let saved = self.opacity_preview.as_ref().filter(|preview| {
                    workspace.active() == Some(preview.owner) && preview.ids == ids
                        && workspace.session().is_some_and(|session| session.generation == preview.generation)
                });
                if pressed(egui::Key::Escape) {
                    event = NumericEvent::Cancel;
                } else if rail.drag_stopped() {
                    event = NumericEvent::Commit(if rail.changed() { f64::from(value) }
                        else { saved.map_or(f64::from(value), |preview| f64::from(preview.value) * 100.0) });
                } else if rail.changed() {
                    event = if rail.is_pointer_button_down_on() { NumericEvent::Preview(f64::from(value)) }
                        else { NumericEvent::Commit(f64::from(value)) };
                }
            });
            ui.spacing_mut().combo_width = (ui.available_width() - 28.0).clamp(42.0, 160.0);
            let mixed = mode.is_empty();
            let blend = dtool_design::dropdown_value(
                ui, "layers.blend", &mut mode, &options,
                &ControlState::enabled(enabled).mixed(mixed).tooltip(text("layers.blend")),
            ).changed();
            if self.action_icon(ui, "layer-blend-options", text("menu.layer.blend_options"), enabled) {
                self.blend_target = workspace.active();
            }
            (blend, event)
        }).inner;

        match opacity_event {
            NumericEvent::Preview(value) if enabled => {
                self.opacity_preview = workspace.session().zip(workspace.active()).map(|(session, owner)| LayerOpacityPreview {
                    owner, generation: session.generation, ids, value: (value / 100.0) as f32,
                });
                ui.ctx().request_repaint();
            }
            NumericEvent::None if ui.input(|input| input.pointer.primary_down()) => {}
            _ => {
                if self.opacity_preview.take().is_some() { ui.ctx().request_repaint(); }
            }
        }
        if !enabled { return; }
        let new_opacity = if let NumericEvent::Commit(value) = opacity_event { Some(value / 100.0) } else { None };
        let mut commands = Vec::new();
        for object in selected {
            if blend && format!("{:?}", object.blend_mode) != mode {
                commands.push(blend_set_command(object.id.0, &mode));
            }
            if let Some(value) = new_opacity.filter(|value| (*value as f32) != object.opacity) {
                commands.push(Command::with_params("object.update", json!({"id": object.id.0, "opacity": value})));
            }
        }
        if commands.len() == 1 {
            self.run_reported(workspace, commands.remove(0));
        } else if !commands.is_empty() {
            self.run_reported(workspace, Command::with_params("edit.batch", json!({"commands": commands})));
        }
    }

    fn action_icon(&mut self, ui: &mut egui::Ui, icon: &str, label: &str, enabled: bool) -> bool {
        let state = dtool_design::ControlState::enabled(enabled).tooltip(label);
        let response = match self.mask_icons.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui)) {
            Some(texture) => dtool_design::icon_image_button(ui, &texture, &state),
            None => dtool_design::button_with(ui, label, &state),
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled && ui.is_enabled(), label));
        response.clicked()
    }

    fn mask_menu(
        &mut self,
        ui: &mut egui::Ui,
        session: &mut dtool_core::Session,
    ) -> Option<Result<serde_json::Value, dtool_core::CommandError>> {
        use crate::mask_actions::{self, MaskAction};
        let label = text("menu.layer.mask.actions");
        let state = dtool_design::ControlState::default().tooltip(label);
        let selected = mask_actions::mask_view_on(session);
        let response = match self.mask_icons.themed_texture(ui.ctx(), "layer-mask", dtool_design::icon_texture_px(ui)) {
            Some(texture) => dtool_design::icon_image_button_selected(ui, &texture, selected, &state),
            None => dtool_design::button_with(ui, label, &state),
        };
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
        egui::Popup::menu(&response).show(|ui| {
            mask_actions::show_mask_actions_with(ui, session, |ui, action, label, enabled, selected| {
                let icon = match action {
                    Some(MaskAction::Invert) => "layer-mask-invert",
                    Some(MaskAction::FromSelection) => "layer-mask",
                    Some(MaskAction::Release) => "layer-mask-release",
                    None => "layer-mask-preview",
                };
                let size = dtool_design::size("icon.size.sm");
                let button = match self.mask_icons.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui)) {
                    Some(texture) => egui::Button::image_and_text(
                        egui::Image::new(&texture).fit_to_exact_size(egui::Vec2::splat(size)),
                        label,
                    ),
                    None => egui::Button::new(label),
                };
                let response = ui.add_enabled(enabled, button.selected(selected).frame_when_inactive(selected));
                if response.clicked() {
                    ui.close();
                }
                response.clicked()
            })
        }).and_then(|popup| popup.inner)
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        panel_frame(ui, |ui| {
            panel_header(ui, text("panel.layers"));
            self.sync_from_document(workspace);
            let reveal = self.reveal_request.take()
                .filter(|(owner, _)| Some(*owner) == workspace.active())
                .map(|(_, target)| target.0);
            if let Some(id) = reveal
                && !self.filter.trim().is_empty()
                && !self.rows(workspace).iter().any(|row| row.id == id)
            {
                self.filter.clear();
            }
            let filter_changed = ui.horizontal(|ui| {
                let has_filter = !self.filter.is_empty();
                let reserved = if has_filter { 22.0 + ui.spacing().item_spacing.x } else { 0.0 };
                let search = ui.add(egui::TextEdit::singleline(&mut self.filter)
                    .hint_text(text("layers.filter")).desired_width((ui.available_width() - reserved).max(0.0)));
                let clear = if has_filter {
                    let caption = text("layers.filter.clear");
                    let button = ui.add_sized([22.0, search.rect.height()], egui::Button::new("×"));
                    button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), caption));
                    button.on_hover_text(caption).clicked()
                } else {
                    false
                };
                let escape = (search.has_focus() || search.lost_focus())
                    && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                if clear || escape {
                    self.filter.clear();
                }
                search.changed() || clear || escape
            }).inner;
            let searching = !self.filter.trim().is_empty();
            self.show_compositing(ui, workspace);
            ui.separator();
            crate::compound_canvas::show_operands(ui, workspace);
            let rows = self.rows(workspace);
            if rows.is_empty() {
                ui.label(text(if searching { "layers.filter.empty" } else { "layers.empty" }));
            }
            let mut clicked = None;
            let mut toggled = None;
            let mut dropped = None;
            let mut rename = None;
            let mut rename_cancelled = false;
            let mut rename_submitted = None;
            if self.renaming.as_ref().is_some_and(|edit| Some(edit.owner) != workspace.active()) {
                self.renaming = None;
            }
            let mut thumb_dropped = None;
            let mut mask_view = None;
            let mut requested_effects = None;
            let mut row_command = None;
            let mut visibility = None;
            let mut locked = None;
            let thumb = 22.0;
            let footer_width = ui.available_width().min(ui.clip_rect().width());
            let button_width = dtool_design::size("icon.size.md") + ui.spacing().button_padding.x * 2.0;
            let split_footer = footer_width < (button_width + ui.spacing().item_spacing.x) * 7.0 + 8.0;
            let footer_id = ui.make_persistent_id(("layers.footer_height", split_footer));
            let button_row_height = dtool_design::size("icon.size.md") + ui.spacing().button_padding.y * 2.0 + ui.spacing().item_spacing.y;
            let default_footer_height = button_row_height * if split_footer { 2.0 } else { 1.0 }
                + ui.spacing().item_spacing.y * 2.0;
            let footer_height = ui.data(|data| data.get_temp::<f32>(footer_id)).unwrap_or(default_footer_height);
            let rows_height = (ui.available_height() - footer_height).max(0.0);
            let (pressed, released) = crate::input::primary_button_positions(ui);
            let drag_id = ui.id().with(("layers-row-press", workspace.active()));
            let mut pointer_drag = ui.data(|data| data.get_temp::<(ObjectId, egui::Pos2)>(drag_id));
            if pressed.is_some() {
                pointer_drag = None;
            }
            let mut targets = Vec::new();
            let valid_drop = |dragged, target| {
                workspace.session().is_some_and(|session| !dtool_core::commands::arrange::would_cycle(session, dragged, target))
            };
            let thumbnails_deferred = self.prepare_thumbnails(ui.ctx(), workspace);
            let mut thumbnail_requests = Vec::new();
            let row_height = 28.0;
            let mut scroll = egui::ScrollArea::vertical().id_salt("layers.rows").max_height(rows_height)
                .auto_shrink([false, !rows_height.is_finite()]);
            if let Some(index) = reveal.and_then(|id| rows.iter().position(|row| row.id == id)) {
                let step = row_height + ui.spacing().item_spacing.y;
                scroll = scroll.vertical_scroll_offset(index as f32 * step);
            } else if filter_changed {
                scroll = scroll.vertical_scroll_offset(0.0);
            }
            let tag_colours = workspace.session().map(|session| {
                dtool_core::commands::layer_meta::effective_tag_colours(&session.document)
            }).unwrap_or_default();
            scroll.show_rows(ui, row_height, rows.len(), |ui, visible| {
                self.thumbnails.retain(|id,_| rows[visible.clone()].iter().any(|row| row.id == *id));
                self.stale_thumbnails.retain(|id| self.thumbnails.contains_key(id));
                for index in visible {
                    let row = &rows[index];
                    let id = ObjectId(row.id);
                    let document = workspace.session().map(|session| &session.document);
                    let object = document.and_then(|doc| doc.get_object(id));
                    let is_artboard = object.is_some_and(|object| object.kind == dtool_core::ObjectKind::Artboard);
                    let is_group = object.is_some_and(|object| matches!(object.kind, dtool_core::object::ObjectKind::Group));
                    let can_nest = is_group || is_artboard || row.has_children
                        || searching && document.is_some_and(|doc| doc.objects.iter().any(|child| child.parent_id == Some(id)));
                    let page_number = document.filter(|doc| is_artboard && doc.pages.len() > 1)
                        .map(|doc| dtool_core::data_merge::layout::object_page_index(doc, id) + 1);
                    let is_folder = object.is_some_and(dtool_core::object::Object::is_layer_folder);
                    let kind = document.zip(object).map(|(doc, object)| layer_kind_icon(doc, object));
                    let kind_icon = kind.and_then(|icon| self.mask_icons.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui)));
                    let has_pixel_mask = document.is_some_and(|doc| doc.layer_masks.iter().any(|mask| mask.object == id));
                    let texture = document.filter(|_| !is_folder && !is_artboard).and_then(|doc| {
                        self.thumbnail(ui.ctx(), doc, row.id, thumbnails_deferred, &mut thumbnail_requests)
                    });
                    let surface = dtool_design::theme::surface(ui.ctx(), "panel.bg");
                    let accent = ui.visuals().selection.bg_fill;
                    let tile = surface.lerp_to_gamma(ui.visuals().text_color(), 0.11);
                    let mut indicator = None;
                    let mut guide_left = None;
                    let fill = if row.selected { surface.lerp_to_gamma(accent, 0.22) } else if index % 2 == 0 { surface } else { surface.gamma_multiply(0.93) };
                    let painted = egui::Frame::new().fill(fill).inner_margin(egui::Margin { left: 8, ..egui::Margin::symmetric(3, 1) }).show(ui, |ui| { ui.horizontal(|ui| {
                        let gap = 2.0;
                        ui.spacing_mut().item_spacing.x = gap;
                        let disclosure_width = 16.0;
                        let kind_width = 12.0;
                        let state_width = 16.0;
                        let effect_width = dtool_design::size("icon.size.md") + ui.spacing().button_padding.x * 2.0;
                        let trailing_width = effect_width + state_width * 2.0 + gap * 3.0;
                        let leading_width = disclosure_width + kind_width + thumb + gap * 3.0;
                        let preferred_name_width = 72.0;
                        let indent_budget = (ui.available_width() - leading_width - trailing_width - preferred_name_width).max(0.0);
                        let indent_step = (indent_budget / 5.0).min(dtool_design::space("md"));
                        let indent = (row.depth as f32 * indent_step).min(indent_budget);
                        if indent > 0.0 {
                            ui.add_space(indent);
                        }
                        guide_left = Some(ui.cursor().left());
                        if row.has_children {
                            let collapsed = !searching && self.is_collapsed(workspace, row.id);
                            let action = text(if searching {
                                "layers.filter.branch"
                            } else if collapsed {
                                "layers.expand"
                            } else {
                                "layers.collapse"
                            });
                            let name = display_name(row.id, &row.name);
                            let caption = format!("{action} · {name}");
                            let button = ui.push_id(("layers.disclosure", id), |ui| {
                                ui.add_sized(egui::vec2(disclosure_width, 16.0),
                                    egui::Button::new("").frame(false)
                                        .sense(if searching { egui::Sense::hover() } else { egui::Sense::click() }))
                            }).inner;
                            let offsets = if collapsed {
                                [[-1.5, -3.0], [1.5, 0.0], [-1.5, 3.0]]
                            } else {
                                [[-3.0, -1.5], [0.0, 1.5], [3.0, -1.5]]
                            };
                            let points = offsets.map(|[x, y]| button.rect.center() + egui::vec2(x, y));
                            ui.painter().add(egui::Shape::line(points.to_vec(), ui.style().interact(&button).fg_stroke));
                            let role = if searching { egui::Role::Label } else { egui::Role::Button };
                            button.widget_info(|| egui::WidgetInfo::labeled(role, ui.is_enabled(), &caption));
                            if button.on_hover_text(caption).clicked() && !searching {
                                toggled = Some(row.id);
                            }
                        } else {
                            ui.allocate_space(egui::vec2(disclosure_width, 16.0));
                        }
                        let clip_role = object.map(|object| object.clip_role);
                        let clip_marker = match clip_role {
                            Some(dtool_core::object::ClipRole::Clip) => Some(("cmd-bool-intersect", text("layers.thumbnail.clip"))),
                            Some(dtool_core::object::ClipRole::Mask) => Some(("layer-mask", text("layers.thumbnail.mask"))),
                            _ => None,
                        };
                        let (kind_rect, _) = ui.allocate_exact_size(egui::vec2(kind_width, 16.0), egui::Sense::hover());
                        if let Some(icon) = &kind_icon {
                            let ink = if row.visible { egui::Color32::WHITE } else { egui::Color32::WHITE.gamma_multiply(0.55) };
                            ui.painter().image(icon.id(), egui::Rect::from_center_size(kind_rect.center(), egui::vec2(12.0, 12.0)),
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), ink);
                        }
                        let thumb_sense = egui::Sense::click_and_drag();
                        let thumb_response = match texture.as_ref().or(kind_icon.as_ref()) {
                            Some(texture) => {
                                let size = thumb - 4.0;
                                ui.add_sized(egui::vec2(thumb, thumb), egui::Button::image(egui::Image::new(texture)
                                    .fit_to_exact_size(texture.size_vec2() * (size / texture.size_vec2().max_elem()))).fill(tile).sense(thumb_sense))
                            }
                            None => ui.add_sized(egui::vec2(thumb, thumb), egui::Button::new("").fill(tile).sense(thumb_sense)),
                        };
                        let thumbnail_kind = if let Some(page) = page_number {
                            text("layers.artboard_page").replace("{n}", &page.to_string())
                        } else if let Some((_, label)) = clip_marker {
                            label.to_owned()
                        } else {
                            text(match kind {
                                Some("tool-picture-frame-ellipse") => "tool.picture_frame.ellipse",
                                Some("tool-picture-frame-rect") => "tool.picture_frame.rect",
                                Some("cmd-group") => "menu.layer.group",
                                _ if is_folder => "layers.folder",
                                _ if is_artboard => "tool.artboard",
                                _ => "layers.thumbnail",
                            }).to_owned()
                        };
                        let mut thumbnail_label = format!("{} · {thumbnail_kind}", display_name(row.id, &row.name));
                        if has_pixel_mask {
                            thumbnail_label.push_str(" · ");
                            thumbnail_label.push_str(text("layers.thumbnail.pixel_mask"));
                        }
                        thumb_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &thumbnail_label));
                        let thumb_response = thumb_response.on_hover_text(thumbnail_label);
                        if let Some(page) = page_number {
                            let caption = ui.painter().layout_no_wrap(format!("P{page}"),
                                egui::FontId::proportional(9.0), ui.visuals().text_color());
                            let rect = egui::Rect::from_min_size(thumb_response.rect.right_bottom() - caption.size(), caption.size());
                            ui.painter().rect_filled(rect.expand2(egui::vec2(2.0, 0.0)), 2.0, surface);
                            ui.painter().galley(rect.min, caption, ui.visuals().text_color());
                        }
                        let pixel_marker = has_pixel_mask.then_some(("layer-mask-pixel", text("layers.thumbnail.pixel_mask")));
                        for (index, (icon, _)) in clip_marker.into_iter().chain(pixel_marker).enumerate() {
                            let level = index + usize::from(page_number.is_some());
                            let rect = egui::Rect::from_center_size(
                                thumb_response.rect.right_bottom() - egui::vec2(5.0, 5.0 + level as f32 * 12.0),
                                egui::vec2(12.0, 12.0),
                            );
                            ui.painter().rect_filled(rect, 2.0, surface);
                            if let Some(texture) = self.mask_icons.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui)) {
                                ui.painter().image(texture.id(), rect,
                                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                            } else {
                                let fallback = match icon {
                                    "layer-mask-pixel" => "▦",
                                    "layer-mask" => "◐",
                                    _ => "◧",
                                };
                                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, fallback,
                                    egui::FontId::proportional(10.0), ui.visuals().text_color());
                            }
                        }
                        if thumb_response.clicked() {
                            if ui.input(|input| input.modifiers.alt) {
                                mask_view = Some(id);
                            } else {
                                clicked = Some((row.id, select_op(row.selected, ui.input(|input| input.modifiers))));
                            }
                        }
                        thumb_response.dnd_set_drag_payload(id);
                        if let Some(point) = pressed.filter(|point| thumb_response.interact_rect.contains(*point)) {
                            pointer_drag = Some((id, point));
                        }
                        if let Some(payload) = thumb_response.dnd_hover_payload::<ObjectId>().filter(|payload| valid_drop(**payload, id)) {
                            ui.painter().rect_stroke(thumb_response.rect, 2.0, egui::Stroke::new(1.5, accent), egui::StrokeKind::Inside);
                            if thumb_response.dnd_release_payload::<ObjectId>().is_some() {
                                thumb_dropped = Some((*payload, id));
                            }
                        }
                        let name_size = egui::vec2((ui.available_width() - trailing_width).max(0.0), 23.0);
                        let label = if let Some(edit) = self.renaming.as_mut().filter(|edit| edit.id == id) {
                            let response = ui.add_sized(name_size, egui::TextEdit::singleline(&mut edit.name)
                                .id_salt(("layers.rename", edit.owner, id)));
                            if std::mem::take(&mut edit.focus) {
                                response.request_focus();
                                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                                    let end = egui::text::CCursor::new(edit.name.chars().count());
                                    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::default(), end)));
                                    state.store(ui.ctx(), response.id);
                                }
                            } else if (response.has_focus() || response.lost_focus()) &&
                                ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
                            {
                                response.surrender_focus();
                                rename_cancelled = true;
                            } else if response.lost_focus() {
                                if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                                    rename_submitted = Some((id.0, edit.name.clone()));
                                } else {
                                    rename_cancelled = true;
                                }
                            }
                            None
                        } else {
                            Some(ui.push_id(("layers.row", row.id), |ui| {
                                let name = display_name(row.id, &row.name);
                                let ink = if row.visible { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
                                let (rect, response) = ui.allocate_exact_size(name_size, egui::Sense::click_and_drag());
                                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), &name));
                                let label = egui::WidgetText::from(egui::RichText::new(&name).color(ink))
                                    .into_galley(ui, Some(egui::TextWrapMode::Truncate), rect.width(), egui::TextStyle::Body);
                                ui.painter().with_clip_rect(rect).galley(
                                    egui::pos2(rect.left(), rect.center().y - label.size().y * 0.5),
                                    label,
                                    ink,
                                );
                                response.on_hover_cursor(egui::CursorIcon::Default).on_hover_text(name)
                            }).inner)
                        };
                        if let Some(label) = &label {
                            label.dnd_set_drag_payload(id);
                            if let Some(point) = pressed.filter(|point| label.interact_rect.contains(*point)) {
                                pointer_drag = Some((id, point));
                            }
                            targets.push((id, can_nest, label.rect, label.interact_rect, thumb_response.interact_rect));
                            if label.clicked() {
                                clicked = Some((row.id, select_op(row.selected, ui.input(|input| input.modifiers))));
                            }
                            if label.double_clicked() && !row.locked {
                                rename = Some(id);
                            }
                        }
                        let menu_target = label.as_ref().map_or_else(|| thumb_response.clone(), |label| label.union(thumb_response.clone()));
                        if menu_target.secondary_clicked() && !row.selected {
                            clicked = Some((row.id, "Replace"));
                        }
                        menu_target.context_menu(|ui| crate::menu_tree::context_command_menu(ui, |ui| {
                            use crate::menu_tree::{command_menu_item, command_menu_tooltip};
                            use crate::menus::layer_lock::{LAYER_HIDE, LAYER_LOCK, LAYER_SHOW, LAYER_UNLOCK, command_for};

                            let Some(session) = workspace.session() else { return };
                            let ids = if row.selected { session.selection.ids.as_slice() } else { std::slice::from_ref(&id) };
                            let selected = || ids.iter().filter_map(|&id| session.document.get_object(id));
                            let source = document.and_then(|doc| doc.master_pages.source_of(id)
                                .and_then(|source| doc.get_object(source)));
                            if source.is_some() && command_menu_item(ui, text("panel.master.reset"), true).clicked() {
                                row_command = Some(Command::with_params("master.object.reset", serde_json::json!({"id": id})));
                                ui.close();
                            }
                            if command_menu_item(ui, text("menu.layer.rename_layer"), !row.locked).clicked() {
                                rename = Some(id);
                                ui.close();
                            }
                            for (action, key, available) in [
                                (LAYER_LOCK, "menu.layer.lock", selected().any(|object| !object.locked)),
                                (LAYER_UNLOCK, "menu.layer.unlock", selected().any(|object| object.locked)),
                                (LAYER_HIDE, "menu.layer.hide", selected().any(|object| object.pickable)),
                                (LAYER_SHOW, "menu.layer.show", selected().any(|object| !object.pickable)),
                            ] {
                                if available && command_menu_item(ui, text(key), true).clicked() {
                                    let nodes: Vec<_> = ids.iter().map(|id| id.0).collect();
                                    row_command = command_for(action, &nodes);
                                    ui.close();
                                }
                            }
                            for (name, key) in [
                                (dtool_core::commands::arrange::NODE_HIDE_OTHERS, "layers.hide_others"),
                                (dtool_core::commands::arrange::NODE_SHOW_OTHERS, "layers.show_others"),
                            ] {
                                let command = Command::with_params(name, json!({"nodes": ids}));
                                if command_menu_item(ui, text(key), session.can_execute(&command)).clicked() {
                                    row_command = Some(command);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            let mask_below = Command::with_params(dtool_core::clip_mask::VECTOR_MASK_APPLY_BELOW, json!({"id": id.0}));
                            let can_mask_below = ids.len() == 1 && session.can_execute(&mask_below);
                            if command_menu_item(ui, text("menu.layer.create_clipping_mask"), can_mask_below).clicked() {
                                row_command = Some(mask_below);
                                ui.close();
                            }
                            if let Some(session) = workspace.session() {
                                let owner = mask_preview_owner(&session.document, id);
                                let viewing = session.view.canvas_flags.mask_view == Some(owner);
                                if viewing || dtool_core::clip_mask::has_any_mask(&session.document, owner) {
                                    let label = text("layer.mask.view_only");
                                    let response = ui.selectable_label(viewing, label);
                                    if command_menu_tooltip(ui, response, label, false, None).clicked() {
                                        mask_view = Some(id);
                                        ui.close();
                                    }
                                }
                            }
                            let release = match clip_role {
                                Some(dtool_core::object::ClipRole::Mask) => Some((
                                    dtool_core::clip_mask::VECTOR_MASK_RELEASE,
                                    "menu.layer.release_vector_mask",
                                )),
                                Some(dtool_core::object::ClipRole::Clip) => Some((
                                    dtool_core::clip_mask::CLIP_RELEASE,
                                    "menu.layer.release_clipping",
                                )),
                                _ => None,
                            };
                            if let Some((name, key)) = release
                                && command_menu_item(ui, text(key), !row.locked).clicked()
                            {
                                row_command = Some(Command::with_params(name, json!({"id": id.0})));
                                ui.close();
                            }
                            let editable = selected().all(|object| !object.locked);
                            let groups = selected().all(|object| object.kind == dtool_core::ObjectKind::Group);
                            for (name, key, enabled) in [
                                ("edit.duplicate", "menu.edit.duplicate", editable),
                                ("node.group", "menu.layer.group", editable && ids.len() > 1),
                                ("node.ungroup", "menu.layer.ungroup", editable && groups),
                                ("node.delete", "menu.edit.delete", editable),
                            ] {
                                if command_menu_item(ui, text(key), enabled).clicked() {
                                    row_command = Some(Command::new(name));
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if command_menu_item(ui, text("menu.layer.layer_effects"), !row.locked).clicked() {
                                requested_effects = Some(row.id);
                                ui.close();
                            }
                            ui.separator();
                            for (attribute, key) in [
                                ("TagColour", "layers.select.same_tag_colour"),
                                ("Name", "layers.select.same_name"),
                            ] {
                                let command = Command::with_params(
                                    dtool_core::commands::selection_edit::EDIT_SELECT_SAME,
                                    json!({"attribute": attribute, "layer_panel": true, "reference": id.0}),
                                );
                                if command_menu_item(ui, text(key), session.can_execute(&command)).clicked() {
                                    row_command = Some(command);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            let stored = |id| dtool_core::commands::layer_meta::meta(&session.document, id)
                                .and_then(|entry| entry.colour.as_deref());
                            let first = stored(id);
                            let same_colour = ids.iter().all(|&id| match (first, stored(id)) {
                                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                                (a, b) => a == b,
                            });
                            if let Some(colour) = tag_colour_picker(ui, same_colour.then_some(first)) {
                                row_command = Some(Command::with_params(
                                    dtool_core::commands::layer_meta::LAYER_META_SET,
                                    json!({"ids": ids, "colour": colour}),
                                ));
                            }
                        }));
                        if workspace.session().is_some_and(|s| s.document.get_object(id).is_some_and(|o| !o.effects.is_empty())) {
                            if self.action_icon(ui, "layer-effects", text("menu.layer.layer_effects"), !row.locked)
                            {
                                requested_effects = Some(row.id);
                            }
                        } else {
                            ui.allocate_space(egui::vec2(effect_width, 16.0));
                        }
                        if let Some(label) = label.as_ref()
                            && let Some(payload) = label.dnd_hover_payload::<ObjectId>()
                        {
                            if valid_drop(*payload, id)
                                && let Some(point) = ui.input(|input| input.pointer.interact_pos())
                            {
                                let zone = drop_zone_on_label(id, can_nest, label.rect, point);
                                indicator = Some(zone);
                                if label.dnd_release_payload::<ObjectId>().is_some() {
                                    dropped = Some((*payload, zone));
                                }
                            }
                        }
                        let mut is_locked = row.locked;
                        if dtool_design::lock_toggle(ui, &mut is_locked).on_hover_text(text(if row.locked { "menu.layer.unlock" } else { "menu.layer.lock" })).changed() { locked = Some((row.id, is_locked)); }
                        let mut is_visible = row.visible;
                        if dtool_design::visibility_toggle(ui, &mut is_visible).on_hover_text(text(if row.visible { "menu.layer.hide" } else { "menu.layer.show" })).changed() { visibility = Some((row.id, is_visible)); }
                    }); });
                    let row_rect = painted.response.rect;
                    if let Some(colour) = tag_colours.get(&id).and_then(|hex| dtool_design::tokens::parse_hex(hex)) {
                        let stripe = egui::Rect::from_min_max(
                            row_rect.left_top() + egui::vec2(4.0, 2.0),
                            row_rect.left_bottom() + egui::vec2(7.0, -2.0),
                        );
                        ui.painter().rect_filled(stripe, 1.0, colour);
                    }
                    let divider = surface.lerp_to_gamma(ui.visuals().text_color(), 0.17);
                    ui.painter().hline(row_rect.x_range(), row_rect.bottom(), egui::Stroke::new(1.0, divider));
                    if let Some(zone) = indicator {
                        let left = guide_left.unwrap_or(row_rect.left()).clamp(row_rect.left(), row_rect.right());
                        let rect = egui::Rect::from_min_max(egui::pos2(left, row_rect.top()), row_rect.right_bottom());
                        let stroke = egui::Stroke::new(2.0, accent);
                        match zone {
                            DropZone::Above(_) | DropZone::Below(_) => {
                                let y = if matches!(zone, DropZone::Above(_)) { rect.top() } else { rect.bottom() };
                                ui.painter().hline(rect.x_range(), y, stroke);
                                ui.painter().circle_filled(egui::pos2(left, y), 2.5, accent);
                            }
                            DropZone::Inside(_) => { ui.painter().rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside); }
                        }
                    }
                    if row.selected {
                        let marker = egui::Rect::from_min_size(row_rect.min, egui::vec2(2.0, row_rect.height()));
                        ui.painter().rect_filled(marker, 0.0, accent);
                    }
                }
            });
            self.start_thumbnails(ui.ctx(), workspace, thumbnail_requests);
            // A fast drag can press and release in the same UI frame, before egui
            // has installed its drag payload. Resolve from the recorded row hitboxes.
            if let Some(point) = released {
                if dropped.is_none() && thumb_dropped.is_none() {
                    if let Some((dragged, start)) = pointer_drag.filter(|(_, start)| start.distance(point) >= 4.0) {
                        for (id, can_nest, label, label_hitbox, thumbnail) in targets {
                            if thumbnail.contains(point) && valid_drop(dragged, id) {
                                thumb_dropped = Some((dragged, id));
                                break;
                            }
                            if label_hitbox.contains(point) && valid_drop(dragged, id) {
                                dropped = Some((dragged, drop_zone_on_label(id, can_nest, label, point)));
                                break;
                            }
                        }
                    }
                }
                pointer_drag = None;
            }
            ui.data_mut(|data| {
                if let Some(gesture) = pointer_drag {
                    data.insert_temp(drag_id, gesture);
                } else {
                    data.remove::<(ObjectId, egui::Pos2)>(drag_id);
                }
            });
            if let Some((id, visible)) = visibility { self.set_visible(workspace, id, visible); }
            if let Some((id, locked)) = locked { self.set_locked(workspace, id, locked); }
            if let Some(id) = toggled {
                self.toggle_collapsed(workspace, id);
            }
            if let Some((dragged, target)) = thumb_dropped {
                self.apply_thumb_drop(workspace, dragged, target);
            }
            if let Some(id) = requested_effects { self.request_effects(workspace, id); }
            if let Some(id) = mask_view {
                self.toggle_mask_view(workspace, id);
            }
            if let Some((id, op)) = clicked {
                self.select(workspace, id, op);
            }
            if let Some(command) = row_command {
                self.run_reported(workspace, command);
            }
            if rename_cancelled {
                self.renaming = None;
            }
            if let Some((id, name)) = rename_submitted {
                let changed = workspace.session().and_then(|session| session.document.get_object(ObjectId(id)))
                    .is_some_and(|object| object.name != name);
                self.renaming = None;
                if changed {
                    self.rename(workspace, id, &name);
                }
            }
            if let (Some(id), Some(owner), Some(session)) = (rename, workspace.active(), workspace.session()) {
                self.begin_rename(owner, &session.document, id);
                ui.ctx().request_repaint();
            }
            if let Some((dragged, zone)) = dropped {
                self.apply_drop(workspace, dragged, zone);
            }
            let footer_top = ui.cursor().min.y;
            let mut footer_effects = None;
            if let Some(session) = workspace.session_mut() {
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                if self.action_icon(ui, "layer-folder-add", text("menu.layer.new_folder"), session.has_document) {
                    let command = Command::with_params("node.folder.create", json!({ "name": text("layers.folder") }));
                    self.last_error = session.run(&command).err().map(|error| error.message_key);
                }
                let ids = &session.selection.ids;
                let editable = !ids.is_empty() && ids.iter().all(|id| session.document.get_object(*id).is_some_and(|object| !object.locked));
                if self.action_icon(ui, "layer-effects", text("menu.layer.layer_effects"), editable && ids.len() == 1) {
                    footer_effects = ids.first().map(|id| id.0);
                }
                let groups = ids.iter().all(|id| session.document.get_object(*id).is_some_and(|object| matches!(object.kind, dtool_core::object::ObjectKind::Group)));
                for (name, label, icon, enabled) in [
                    ("node.group", "menu.layer.group", "cmd-group", editable && ids.len() > 1),
                    ("node.ungroup", "menu.layer.ungroup", "cmd-ungroup", editable && groups),
                ] {
                    if self.action_icon(ui, icon, text(label), enabled) {
                        let command = Command::with_params(name, json!({}));
                        self.last_error = session.run(&command).err().map(|error| error.message_key);
                    }
                }
                if split_footer {
                    ui.end_row();
                } else {
                    ui.separator();
                }
                if let Some(result) = self.mask_menu(ui, session) {
                    self.last_error = result.err().map(|error| error.message_key);
                }
                let pixel_layer = Command::with_params(dtool_core::commands::layer_kinds::LAYER_PIXEL_ADD,
                    json!({ "current_surface": true }));
                if self.action_icon(ui, "tool-paint-pixel", text("menu.pixel.new_layer"), session.can_execute(&pixel_layer)) {
                    self.last_error = session.run(&pixel_layer).err().map(|error| error.message_key);
                }
                let editable = session.can_execute_named("node.delete");
                if self.action_icon(ui, "layer-delete", text("menu.edit.delete"), editable) {
                    self.last_error = session.run(&Command::new("node.delete")).err().map(|error| error.message_key);
                }
                });
            }
            if let Some(id) = footer_effects {
                self.request_effects(workspace, id);
            }
            if let Some(key) = &self.last_error {
                ui.label(text(key));
            }
            let actual_footer_height = ui.cursor().min.y - footer_top;
            if (actual_footer_height - footer_height).abs() > 0.5 {
                ui.data_mut(|data| data.insert_temp(footer_id, actual_footer_height));
                ui.ctx().request_repaint();
            }
        });
    }
}
