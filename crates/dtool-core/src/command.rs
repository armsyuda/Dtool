#[path = "commands/native_open.rs"]
pub mod native_open;
#[path = "commands/document_files.rs"]
pub mod document_files;
#[path = "commands/autosave.rs"]
pub mod autosave_commands;
use crate::document::{Document, Units};
use crate::object::{Object, ObjectId, ObjectCreateArgs, ObjectUpdateArgs, ObjectDeleteArgs, validate_fill, validate_opacity};
use crate::transform::{Anchor, FlipAxis, RotateDirection};
use crate::selection::{Selection, MarqueeMode, SelectOp, CycleDirection, selection_marquee_with_flags, selection_hit, selection_enter, selection_exit, selection_cycle, PixelSelection, SelectionOp, parse_length, rect_mask, document_px_size};
use crate::clipboard::{AppClipboard, ClipboardKind};
use crate::snap_prefs::{SnapPrefsService, SnapPrefsSetArgs};
use crate::guides::{Guide, GuideError, GuideId, GuideKind, Guides};
use serde::{Deserialize, Serialize};
use dtool_protocol::{Command, CommandBus, CommandError, CommandResult, ChangeName, CommandCatalog};
pub use crate::commands::duplicate::{EditDuplicateDragArgs, EDIT_DUPLICATE_DRAG};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocNewArgs {
    pub width: f32,
    pub height: f32,
    pub units: Units,
    pub dpi: f32,
    pub pages: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocUnitsSetArgs {
    pub units: Units,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocScaleSetArgs {
    pub drawing_scale: [f32; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformFlipArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<u64>>,
    pub axis: FlipAxis,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformRotate90Args {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<u64>>,
    pub direction: RotateDirection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionSetArgs {
    pub ids: Vec<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<SelectOp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<u64>,
}

impl SelectionSetArgs {
    fn apply(self, selection: &mut Selection) {
        selection.set(self.ids.into_iter().map(ObjectId).collect(), self.op.unwrap_or_default());
        if let Some(key) = self.key_id {
            selection.set_key(ObjectId(key));
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionMarqueeArgs {
    pub rect: [f32; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<MarqueeMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<SelectOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionHitArgs {
    pub point: [f32; 2],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionCycleArgs {
    pub direction: CycleDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditPasteArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DuplicateMode {
    Layer,
    Linked,
    Selection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditDuplicateArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<DuplicateMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeySendArgs {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifiers: Option<crate::shortcuts::Modifiers>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidesAddArgs {
    pub kind: GuideKind,
    pub position: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_id: Option<crate::document::PageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidesUpdateArgs {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<GuideKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidesDeleteArgs {
    pub id: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GuidesFindArgs {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub page_id: Option<crate::document::PageId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GuidesShowArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_columns: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuideStateArgs {
    pub guide: Guide,
}

fn guide_error(err: GuideError) -> CommandError {
    match err {
        GuideError::NotFound => CommandError { code: "NOT_FOUND".to_string(), message_key: "error.not_found".to_string() },
        GuideError::InvalidParams | GuideError::Locked => CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditPasteSpecialArgs {
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectInsertArgs {
    pub objects: Vec<Object>,
}

pub const PASTE_FORMAT_DTOOL: &str = "dtool";
pub const PASTE_FORMAT_RTF: &str = "Rich Text Format";
pub const PASTE_FORMAT_UNICODE_TEXT: &str = "CF_UNICODETEXT";
pub const PASTE_FORMAT_TEXT: &str = "CF_TEXT";

pub fn paste_special_formats(payload: &crate::clipboard::ClipboardPayload) -> Vec<String> {
    let mut formats = Vec::new();
    match payload.kind {
        ClipboardKind::Objects => {
            if payload.objects.as_ref().map_or(false, |o| !o.is_empty()) {
                formats.push(PASTE_FORMAT_DTOOL.to_string());
            }
        }
        ClipboardKind::Text => {
            if payload.text.as_ref().map_or(false, |t| !t.is_empty()) {
                if payload.style.is_some() {
                    formats.push(PASTE_FORMAT_RTF.to_string());
                }
                formats.push(PASTE_FORMAT_UNICODE_TEXT.to_string());
                formats.push(PASTE_FORMAT_TEXT.to_string());
            }
        }
        _ => {}
    }
    formats
}

pub fn plain_text(input: &str) -> String {
    let unified = input.replace("\r\n", "\n").replace('\r', "\n");
    unified
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect()
}

pub fn plain_text_object(content: &str, at: Option<[f32; 2]>) -> Object {
    let lines: Vec<&str> = content.split('\n').collect();
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1).max(1) as f32;
    let line_count = lines.len().max(1) as f32;
    let mut obj = Object::default();
    obj.name = content.lines().next().unwrap_or("").chars().take(32).collect();
    obj.pos = at.unwrap_or([0.0, 0.0]);
    obj.size = [longest * 8.0, line_count * 16.0];
    obj.kind = crate::object::ObjectKind::Text(content.to_string());
    obj
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformSelectionBoxArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<u64>>,
    pub origin: [f32; 2],
    #[serde(default)]
    pub translate: [f32; 2],
    #[serde(default = "unit_scale")]
    pub scale: [f32; 2],
    #[serde(default)]
    pub rotation_deg: f32,
    #[serde(default)]
    pub shear_deg: f32,
    /// Rotation and shear of the basis used for a single layer's handles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axes: Option<[f32; 2]>,
    /// Reflow directly resized text frames. Groups continue scaling their contents.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reflow_frame_text: bool,
    /// Apply the same transform around each selected root's centre.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub separate_origins: bool,
}

fn unit_scale() -> [f32; 2] {
    [1.0, 1.0]
}

impl Default for TransformSelectionBoxArgs {
    fn default() -> Self {
        Self { ids: None, origin: [0.0; 2], translate: [0.0; 2], scale: unit_scale(),
            rotation_deg: 0.0, shear_deg: 0.0, axes: None, reflow_frame_text: false, separate_origins: false }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransformSetArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shear_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_locked: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformQueryArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<u64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectRestoreArgs {
    pub objects: Vec<Object>,
}

pub fn anchor_fraction(anchor: Anchor) -> [f32; 2] {
    match anchor {
        Anchor::TopLeft => [0.0, 0.0],
        Anchor::Top => [0.5, 0.0],
        Anchor::TopRight => [1.0, 0.0],
        Anchor::Left => [0.0, 0.5],
        Anchor::Centre => [0.5, 0.5],
        Anchor::Right => [1.0, 0.5],
        Anchor::BottomLeft => [0.0, 1.0],
        Anchor::Bottom => [0.5, 1.0],
        Anchor::BottomRight => [1.0, 1.0],
    }
}

pub fn selection_bounds(doc: &Document, ids: &[ObjectId]) -> Option<[f32; 4]> {
    let mut min = [f32::MAX, f32::MAX];
    let mut max = [f32::MIN, f32::MIN];
    let mut any = false;
    for id in ids {
        if let Some(obj) = doc.get_object(*id) {
            let [x0, y0, w, h] = transform_geometry(doc, obj).map_or_else(
                || crate::geometry::object_world_bounds(obj),
                |visual| crate::geometry::object_world_bounds(&visual),
            );
            let (x1, y1) = (x0 + w, y0 + h);
            min[0] = min[0].min(x0);
            min[1] = min[1].min(y0);
            max[0] = max[0].max(x1);
            max[1] = max[1].max(y1);
            any = true;
        }
    }
    if any {
        Some([min[0], min[1], max[0] - min[0], max[1] - min[1]])
    } else {
        None
    }
}

pub fn box_transform_point(args: &TransformSelectionBoxArgs, point: [f32; 2]) -> [f32; 2] {
    if args.axes.is_some() {
        return crate::geometry::apply_matrix(&selection_box_matrix(args), point);
    }
    let lx = (point[0] - args.origin[0]) * args.scale[0];
    let ly = (point[1] - args.origin[1]) * args.scale[1];
    let lx = lx + args.shear_deg.to_radians().tan() * ly;
    let (s, c) = args.rotation_deg.to_radians().sin_cos();
    let rx = lx * c - ly * s;
    let ry = lx * s + ly * c;
    [rx + args.origin[0] + args.translate[0], ry + args.origin[1] + args.translate[1]]
}

fn wrap_degrees(mut deg: f32) -> f32 {
    while deg > 180.0 {
        deg -= 360.0;
    }
    while deg <= -180.0 {
        deg += 360.0;
    }
    deg
}

fn finite2(v: [f32; 2]) -> bool {
    v[0].is_finite() && v[1].is_finite()
}

pub(crate) fn resolve_target_ids(doc: &Document, ids: &Option<Vec<u64>>, selection: &Selection) -> Result<Vec<ObjectId>, CommandError> {
    let targets: Vec<ObjectId> = match ids {
        Some(list) => list.iter().map(|id| ObjectId(*id)).collect(),
        None => selection.ids.clone(),
    };
    if targets.is_empty() {
        return Err(CommandError::invalid_params());
    }
    for id in &targets {
        match doc.get_object(*id) {
            Some(obj) => {
                if obj.locked {
                    return Err(CommandError::invalid_params());
                }
            }
            None => return Err(CommandError::not_found()),
        }
    }
    Ok(targets)
}

pub fn execute_transform_selection_box(doc: &mut Document, ids: &[ObjectId], args: &TransformSelectionBoxArgs) -> Result<(), CommandError> {
    let members = transform_members(doc, ids);
    if args.separate_origins && ids.len() > 1 {
        let mut updated = Vec::new();
        for (root, targets, local) in separate_transform_roots(doc, ids, args, &members) {
            let mut grouped: std::collections::HashSet<_> = members.get(&root)
                .map(|ids| ids.iter().copied().collect()).unwrap_or_default();
            if doc.get_object(root).is_some_and(Object::is_group) {
                grouped.insert(root);
            }
            updated.extend(plan_transform_box(doc, &targets, &local, &grouped)?);
        }
        return apply_transformed_objects(doc, updated);
    }
    let grouped = selected_groups(doc, ids).into_iter().chain(members.values().flatten().copied()).collect();
    execute_transform_box(doc, &extend_transform_targets(ids, &members), args, &grouped)
}

/// Transform explicit layers when their parent mapping is handled by the caller.
pub(crate) fn execute_transform_layer_box(doc: &mut Document, ids: &[ObjectId], args: &TransformSelectionBoxArgs) -> Result<(), CommandError> {
    execute_transform_box(doc, ids, args, &std::collections::HashSet::new())
}

fn execute_transform_box(
    doc: &mut Document, ids: &[ObjectId], args: &TransformSelectionBoxArgs,
    grouped: &std::collections::HashSet<ObjectId>,
) -> Result<(), CommandError> {
    let updated = plan_transform_box(doc, ids, args, grouped)?;
    apply_transformed_objects(doc, updated)
}

fn plan_transform_box(
    doc: &Document, ids: &[ObjectId], args: &TransformSelectionBoxArgs,
    grouped: &std::collections::HashSet<ObjectId>,
) -> Result<Vec<(Object, [f64; 2])>, CommandError> {
    if !finite2(args.origin) || !finite2(args.translate) || !finite2(args.scale) || !args.rotation_deg.is_finite() || !args.shear_deg.is_finite() {
        return Err(CommandError::invalid_params());
    }
    if args.scale[0] == 0.0 || args.scale[1] == 0.0 || args.shear_deg.abs() >= 90.0
        || args.axes.is_some_and(|axes| !finite2(axes) || axes[1].abs() >= 90.0)
    {
        return Err(CommandError::invalid_params());
    }
    let map = selection_box_matrix(args);
    let member_maps = if args.scale != [1.0, 1.0] { constraint_maps(doc, ids, grouped, &map)? } else { Default::default() };
    let factors = args.scale.map(|value| f64::from(value.abs()));
    let mut updated = Vec::new();
    for id in ids {
        let obj = doc.get_object(*id).ok_or(CommandError::not_found())?;
        if grouped.contains(id) {
            let follows_group = obj.path_text_data().is_some_and(|path| grouped.contains(&path.path));
            let member_map = member_maps.get(id).unwrap_or(&map);
            let path_map = obj.path_text_data().and_then(|path| member_maps.get(&path.path)).unwrap_or(member_map);
            updated.push(transform_group_member(obj, if follows_group { path_map } else { member_map }, follows_group, false)?);
            continue;
        }
        if args.axes.is_some_and(|axes| axes != [obj.rotation_deg, obj.shear_deg])
            || args.axes.is_some() && (args.shear_deg != 0.0 || args.rotation_deg != 0.0)
        {
            updated.push(transform_group_member(obj, &map, false, args.reflow_frame_text)?);
            continue;
        }
        let mut next = obj.clone();
        let half = [obj.size[0].abs() * 0.5, obj.size[1].abs() * 0.5];
        let centre = [obj.pos[0] + half[0], obj.pos[1] + half[1]];
        let new_centre = box_transform_point(args, centre);
        let new_size = [obj.size[0] * args.scale[0], obj.size[1] * args.scale[1]];
        next.size = new_size;
        next.pos = [new_centre[0] - new_size[0].abs() * 0.5, new_centre[1] - new_size[1].abs() * 0.5];
        next.rotation_deg = wrap_degrees(obj.rotation_deg + args.rotation_deg);
        next.shear_deg = (obj.shear_deg + args.shear_deg).clamp(-89.0, 89.0);
        scale_text_geometry(&mut next, args.scale, args.reflow_frame_text)?;
        scale_curve_geometry(&mut next, factors);
        updated.push((next, factors));
    }
    Ok(updated)
}

fn apply_transformed_objects(doc: &mut Document, updated: Vec<(Object, [f64; 2])>) -> Result<(), CommandError> {
    for (next, factors) in updated {
        let id = next.id;
        if let Some(obj) = doc.find_object_mut(id.0) {
            *obj = next;
        }
        scale_object_paint(doc, id, factors)?;
    }
    Ok(())
}

fn scale_text_geometry(object: &mut Object, scale: [f32; 2], reflow_frame_text: bool) -> Result<(), CommandError> {
    if matches!(object.kind, crate::ObjectKind::ArtText { .. })
        || (matches!(object.kind, crate::ObjectKind::FrameText { .. }) && !reflow_frame_text)
    {
        let next = std::array::from_fn(|axis| object.text_scale[axis] * scale[axis].abs());
        if !finite2(next) || next.iter().any(|&side| side <= 0.0) {
            return Err(CommandError::invalid_params());
        }
        object.text_scale = next;
    }
    Ok(())
}

fn scale_curve_geometry(object: &mut Object, [sx, sy]: [f64; 2]) {
    let crate::object::ObjectKind::Curve(curve) = &mut object.kind else { return };
    let scale = |point: &mut [f64; 2]| {
        point[0] *= sx;
        point[1] *= sy;
    };
    for contour in std::iter::once(&mut curve.contour).chain(curve.holes.iter_mut()) {
        for point in &mut contour.points {
            scale(&mut point.pos);
            point.handle_in.iter_mut().chain(point.handle_out.iter_mut()).for_each(scale);
            if let Some(corner) = &mut point.corner {
                corner.radius *= sx.min(sy);
            }
        }
    }
}

fn scale_object_paint(doc: &mut Document, id: ObjectId, factors: [f64; 2]) -> Result<(), CommandError> {
    if factors[0] == 1.0 && factors[1] == 1.0 { return Ok(()); }
    if !(factors[0].is_finite() && factors[1].is_finite() && factors[0] > 0.0 && factors[1] > 0.0) { return Ok(()); }
    doc.apply_transform_to_object(id, factors[0], factors[1], 0.0)
}

fn selected_groups(doc: &Document, ids: &[ObjectId]) -> Vec<ObjectId> {
    ids.iter().copied().filter(|id| doc.get_object(*id).is_some_and(Object::is_group)).collect()
}

/// Groups and attached vector masks participate once in their owner's transform.
pub fn transform_target_ids(doc: &Document, ids: &[ObjectId]) -> Vec<ObjectId> {
    extend_transform_targets(ids, &transform_members(doc, ids))
}

fn extend_transform_targets(ids: &[ObjectId], members: &std::collections::HashMap<ObjectId, Vec<ObjectId>>) -> Vec<ObjectId> {
    let mut targets = ids.to_vec();
    targets.extend(ids.iter().filter_map(|id| members.get(id)).flatten().copied());
    let mut seen = std::collections::HashSet::new();
    targets.retain(|id| seen.insert(*id));
    targets
}

fn transform_members(doc: &Document, ids: &[ObjectId]) -> std::collections::HashMap<ObjectId, Vec<ObjectId>> {
    let mut sets: std::collections::HashMap<_, Vec<_>> = selected_groups(doc, ids).into_iter()
        .map(|id| (id, crate::commands::arrange::descendant_ids(doc, &[id]).into_iter().skip(1).collect()))
        .collect();
    let owners: std::collections::HashSet<_> = ids.iter().copied()
        .filter(|id| !sets.contains_key(id)).collect();
    for mask in doc.objects().iter().filter(|object| object.clip_role == crate::object::ClipRole::Mask) {
        if let Some(owner) = mask.parent_id.filter(|id| owners.contains(id)) {
            sets.entry(owner).or_default().extend(crate::commands::arrange::descendant_ids(doc, &[mask.id]));
        }
    }
    sets
}

fn separate_transform_roots(
    doc: &Document, ids: &[ObjectId], args: &TransformSelectionBoxArgs,
    members: &std::collections::HashMap<ObjectId, Vec<ObjectId>>,
) -> Vec<(ObjectId, Vec<ObjectId>, TransformSelectionBoxArgs)> {
    let children: std::collections::HashSet<_> = members.values().flatten().copied().collect();
    let mut assigned = std::collections::HashSet::new();
    let mut template = args.clone();
    template.ids = None;
    template.separate_origins = false;
    ids.iter().copied().filter(|id| !children.contains(id)).map(|id| {
        let targets = std::iter::once(id).chain(members.get(&id).into_iter().flatten().copied())
            .filter(|id| assigned.insert(*id)).collect();
        let origin = doc.get_object(id).map_or(args.origin, crate::geometry::box_centre);
        let local = TransformSelectionBoxArgs { origin, ..template.clone() };
        (id, targets, local)
    }).collect()
}

type MemberMaps = std::collections::HashMap<ObjectId, [f32; 6]>;

pub fn selection_box_matrix(args: &TransformSelectionBoxArgs) -> [f32; 6] {
    let [a, b, c, d] = crate::geometry::object_linear(args.rotation_deg, args.shear_deg);
    let [sx, sy] = args.scale;
    let linear = [a * sx, b * sx, c * sy, d * sy, 0.0, 0.0];
    let Some([rotation, shear]) = args.axes else {
        let [x, y] = box_transform_point(args, [0.0, 0.0]);
        return [linear[0], linear[1], linear[2], linear[3], x, y];
    };
    use crate::geometry::{apply_matrix, compose, invert_matrix, object_linear};
    let [a, b, c, d] = object_linear(rotation, shear);
    let basis = [a, b, c, d, 0.0, 0.0];
    let Some(inverse) = invert_matrix(&basis) else { return linear };
    let mut map = compose(&basis, &compose(&linear, &inverse));
    let moved = apply_matrix(&map, args.origin);
    map[4] = args.origin[0] + args.translate[0] - moved[0];
    map[5] = args.origin[1] + args.translate[1] - moved[1];
    map
}

fn group_layout(
    doc: &Document, root: &Object, next_root: &Object, members: &[ObjectId], map: &[f32; 6], output: &mut MemberMaps,
) -> Result<(), CommandError> {
    if !members.iter().any(|id| doc.get_object(*id).is_some_and(|object| object.constraint.is_some())) {
        return Ok(());
    }
    let mut resized = std::collections::HashMap::from([(root.id, next_root.clone())]);
    for id in members {
        let member = doc.get_object(*id).ok_or_else(CommandError::not_found)?;
        let parent = member.parent_id.and_then(|id| doc.get_object(id));
        let parent_map = parent.and_then(|object| output.get(&object.id)).unwrap_or(map);
        let child_map = parent.and_then(|object| {
            crate::constraints::child_resize_map(object, resized.get(&object.id)?, member)
        }).unwrap_or(*parent_map);
        output.insert(*id, child_map);
        if member.is_group() {
            resized.insert(*id, transform_group_member(member, &child_map, false, false)?.0);
        }
    }
    Ok(())
}

fn constraint_maps(
    doc: &Document, ids: &[ObjectId], grouped: &std::collections::HashSet<ObjectId>, map: &[f32; 6],
) -> Result<MemberMaps, CommandError> {
    let mut output = MemberMaps::new();
    if !grouped.iter().any(|id| doc.get_object(*id).is_some_and(|object| object.constraint.is_some())) {
        return Ok(output);
    }
    for id in ids {
        let Some(root) = doc.get_object(*id).filter(|object| object.is_group()
            && !object.parent_id.is_some_and(|parent| grouped.contains(&parent))) else { continue };
        let next = transform_group_member(root, map, false, false)?.0;
        let members = crate::commands::arrange::descendant_ids(doc, &[*id]);
        group_layout(doc, root, &next, &members[1..], map, &mut output)?;
    }
    Ok(output)
}

pub fn selection_constraint_maps(doc: &Document, ids: &[ObjectId], args: &TransformSelectionBoxArgs) -> MemberMaps {
    if args.separate_origins && ids.len() > 1 {
        let members = transform_members(doc, ids);
        let mut maps = MemberMaps::new();
        for (root, targets, local) in separate_transform_roots(doc, ids, args, &members) {
            let map = selection_box_matrix(&local);
            maps.extend(targets.into_iter().map(|id| (id, map)));
            maps.extend(selection_constraint_maps(doc, &[root], &local));
        }
        return maps;
    }
    if args.scale == [1.0, 1.0] {
        return MemberMaps::new();
    }
    let grouped = crate::commands::arrange::descendant_ids(doc, &selected_groups(doc, ids)).into_iter().collect();
    constraint_maps(doc, ids, &grouped, &selection_box_matrix(args)).unwrap_or_default()
}

fn group_member_transform(from: &Object, to: &Object, scale: [f64; 2]) -> Result<[f32; 6], CommandError> {
    use crate::geometry::{box_centre, compose, invert_matrix, object_matrix};
    if scale == [1.0, 1.0] && from.size == to.size
        && from.rotation_deg == to.rotation_deg && from.shear_deg == to.shear_deg
    {
        return Ok([1.0, 0.0, 0.0, 1.0, to.pos[0] - from.pos[0], to.pos[1] - from.pos[1]]);
    }
    let [ox, oy] = box_centre(from);
    let [nx, ny] = box_centre(to);
    let [sx, sy] = scale.map(|value| value as f32);
    let local = [sx, 0.0, 0.0, sy, nx - sx * ox, ny - sy * oy];
    let inverse = invert_matrix(&object_matrix(from)).ok_or_else(CommandError::invalid_params)?;
    Ok(compose(&object_matrix(to), &compose(&local, &inverse)))
}

/// The object's unflipped local axes decompose into rotation, horizontal shear
/// and local scales. Store a reflection in the signed dimensions.
fn transform_group_member(object: &Object, map: &[f32; 6], map_path_text: bool, reflow_frame_text: bool) -> Result<(Object, [f64; 2]), CommandError> {
    use crate::geometry::{apply_matrix, box_centre, compose, object_linear};
    if map[..4] == [1.0, 0.0, 0.0, 1.0] {
        let mut next = object.clone();
        next.pos = apply_matrix(map, object.pos);
        return finite2(next.pos).then_some((next, [1.0, 1.0])).ok_or_else(CommandError::invalid_params);
    }
    let [a, b, c, d] = object_linear(object.rotation_deg, object.shear_deg);
    let [a, b, c, d, _, _] = compose(map, &[a, b, c, d, 0.0, 0.0]);
    let sx = a.hypot(b);
    let sy = (a * d - b * c) / sx;
    let shear = (a * c + b * d) / (sx * sy);
    let centre = apply_matrix(map, box_centre(object));
    let mut next = object.clone();
    next.size = [object.size[0] * sx, object.size[1] * sy];
    next.pos = [centre[0] - next.size[0].abs() * 0.5, centre[1] - next.size[1].abs() * 0.5];
    next.rotation_deg = b.atan2(a).to_degrees();
    next.shear_deg = shear.atan().to_degrees();
    if !finite2(next.pos) || !finite2(next.size) || !next.shear_deg.is_finite()
        || !next.rotation_deg.is_finite() || !sx.is_finite() || !sy.is_finite() || sx == 0.0 || sy == 0.0
    {
        return Err(CommandError::invalid_params());
    }
    let factors = [f64::from(sx), f64::from(sy.abs())];
    scale_text_geometry(&mut next, [sx, sy], reflow_frame_text)?;
    if map_path_text {
        let matrix = compose(map, &crate::geometry::text_matrix(object));
        let [a, b, c, d, _, _] = matrix;
        if crate::geometry::invert_matrix(&[a, b, c, d, 0.0, 0.0]).is_none() {
            return Err(CommandError::invalid_params());
        }
        next.path_text_linear = ([a, b, c, d] != [1.0, 0.0, 0.0, 1.0]).then_some([a, b, c, d]);
    }
    scale_curve_geometry(&mut next, factors);
    Ok((next, factors))
}

pub fn execute_transform_set(doc: &mut Document, ids: &[ObjectId], args: &TransformSetArgs) -> Result<(), CommandError> {
    apply_transform_set(doc, ids, args, true)
}

fn apply_transform_set(doc: &mut Document, ids: &[ObjectId], args: &TransformSetArgs, use_content_bounds: bool) -> Result<(), CommandError> {
    let values = [args.x, args.y, args.w, args.h, args.rotation_deg, args.shear_deg];
    for v in values.iter().flatten() {
        if !v.is_finite() {
            return Err(CommandError::invalid_params());
        }
    }
    if args.w.map_or(false, |w| w <= 0.0) || args.h.map_or(false, |h| h <= 0.0) {
        return Err(CommandError::invalid_params());
    }
    if args.shear_deg.map_or(false, |s| s.abs() >= 90.0) {
        return Err(CommandError::invalid_params());
    }
    let anchor = args.anchor.unwrap_or(Anchor::TopLeft);
    let frac = anchor_fraction(anchor);
    let aspect = args.aspect_locked.unwrap_or(false);
    let groups = transform_members(doc, ids);
    let grouped: std::collections::HashSet<_> = groups.values().flatten().copied().collect();
    let mut updated: Vec<Object> = Vec::new();
    let mut factors: Vec<(ObjectId, [f64; 2])> = Vec::new();
    let mut visited = std::collections::HashSet::new();
    for id in ids.iter().filter(|&&id| !grouped.contains(&id) && visited.insert(id)) {
        let obj = doc.get_object(*id).ok_or(CommandError::not_found())?;
        let visual = use_content_bounds.then(|| transform_geometry(doc, obj)).flatten();
        let from = visual.as_ref().unwrap_or(obj);
        let mut next = from.clone();
        let old_w = from.size[0].abs();
        let old_h = from.size[1].abs();
        let mut w = old_w;
        let mut h = old_h;
        if let Some(v) = args.w {
            w = v;
            if aspect && args.h.is_none() && old_w > 0.0 {
                h = old_h * v / old_w;
            }
        }
        if let Some(v) = args.h {
            h = v;
            if aspect && args.w.is_none() && old_h > 0.0 {
                w = old_w * v / old_h;
            }
        }
        let pivot = [from.pos[0] + old_w * frac[0], from.pos[1] + old_h * frac[1]];
        let pivot = [args.x.unwrap_or(pivot[0]), args.y.unwrap_or(pivot[1])];
        next.size = [w * from.size[0].signum(), h * from.size[1].signum()];
        next.pos = [pivot[0] - w * frac[0], pivot[1] - h * frac[1]];
        if let Some(r) = args.rotation_deg {
            next.rotation_deg = wrap_degrees(r);
        }
        if let Some(s) = args.shear_deg {
            next.shear_deg = s;
        }
        let scale = [if old_w > 0.0 { (w / old_w) as f64 } else { 1.0 }, if old_h > 0.0 { (h / old_h) as f64 } else { 1.0 }];
        if !finite2(next.pos) || !finite2(next.size) {
            return Err(CommandError::invalid_params());
        }
        if let Some(members) = groups.get(id).filter(|_| {
            next.pos != from.pos || next.size != from.size
                || next.rotation_deg != from.rotation_deg || next.shear_deg != from.shear_deg
        }) {
            let map = group_member_transform(from, &next, scale)?;
            if visual.is_some() {
                next = transform_group_member(obj, &map, false, false)?.0;
            }
            let mut member_maps = MemberMaps::new();
            if scale != [1.0, 1.0] {
                group_layout(doc, obj, &next, members, &map, &mut member_maps)?;
            }
            for id in members {
                let member = doc.get_object(*id).ok_or_else(CommandError::not_found)?;
                let follows_group = member.path_text_data().is_some_and(|path| path.path == obj.id || members.contains(&path.path));
                let member_map = member_maps.get(id).unwrap_or(&map);
                let path_map = member.path_text_data().and_then(|path| member_maps.get(&path.path)).unwrap_or(member_map);
                let (member, factor) = transform_group_member(member, if follows_group { path_map } else { member_map }, follows_group, false)?;
                factors.push((member.id, factor));
                updated.push(member);
            }
        } else if visual.is_some() {
            next = obj.clone();
        }
        factors.push((obj.id, scale));
        updated.push(next);
    }
    for next in updated {
        if let Some(obj) = doc.find_object_mut(next.id.0) {
            *obj = next;
        }
    }
    for (id, factor) in factors {
        scale_object_paint(doc, id, factor)?;
    }
    Ok(())
}

pub fn execute_object_restore(doc: &mut Document, args: &ObjectRestoreArgs) -> Result<(), CommandError> {
    for saved in &args.objects {
        if doc.get_object(saved.id).is_none() {
            return Err(CommandError::not_found());
        }
    }
    for saved in &args.objects {
        if let Some(obj) = doc.find_object_mut(saved.id.0) {
            *obj = saved.clone();
        }
    }
    Ok(())
}

pub fn transform_query(doc: &Document, ids: &[ObjectId], anchor: Anchor, aspect_locked: bool) -> serde_json::Value {
    let frac = anchor_fraction(anchor);
    let rows: Vec<[f32; 6]> = ids
        .iter()
        .filter_map(|id| doc.get_object(*id))
        .map(|o| {
            let [x, y, w, h] = transform_box(doc, o);
            [x + w * frac[0], y + h * frac[1], w, h, o.rotation_deg, o.shear_deg]
        })
        .collect();
    let first = rows.first().copied().unwrap_or([0.0; 6]);
    let mixed: Vec<bool> = (0..6).map(|i| rows.iter().any(|r| (r[i] - first[i]).abs() > 0.0001)).collect();
    serde_json::json!({
        "pos": [first[0], first[1]],
        "size": [first[2], first[3]],
        "rotation_deg": first[4],
        "shear_deg": first[5],
        "anchor": anchor,
        "aspect_locked": aspect_locked,
        "mixed": {
            "x": mixed[0],
            "y": mixed[1],
            "w": mixed[2],
            "h": mixed[3],
            "rotation_deg": mixed[4],
            "shear_deg": mixed[5]
        },
        "count": rows.len()
    })
}

/// The unrotated rectangle around the visual centre, with the object's axes.
pub fn transform_box(doc: &Document, object: &Object) -> [f32; 4] {
    let visual = transform_geometry(doc, object);
    let object = visual.as_ref().unwrap_or(object);
    [object.pos[0], object.pos[1], object.size[0].abs(), object.size[1].abs()]
}

fn transform_geometry(doc: &Document, object: &Object) -> Option<Object> {
    let [x, y, w, h] = crate::scene_hit::symbol_content_bounds(doc, object, None)?;
    if !(w > 0.0 && h > 0.0) { return None; }
    let mut visual = object.clone();
    let centre = crate::geometry::apply_matrix(&crate::geometry::object_matrix(object), [x + w * 0.5, y + h * 0.5]);
    visual.pos = [centre[0] - w * 0.5, centre[1] - h * 0.5];
    visual.size = [w.copysign(object.size[0]), h.copysign(object.size[1])];
    Some(visual)
}

pub fn execute_doc_new(args: DocNewArgs) -> Result<Document, CommandError> {
    if args.width <= 0.0 || args.height <= 0.0 || args.dpi <= 0.0 || args.pages == 0 {
        return Err(CommandError::invalid_params());
    }

    let mut doc = Document::new([args.width, args.height], args.units, args.dpi, args.pages);
    doc.setup.landscape = args.width > args.height;
    let bleed = new_document_bleed(doc.units, doc.dpi);
    doc.setup.bleed = crate::document::SetupMargins { enabled: true, left: bleed, right: bleed, top: bleed, bottom: bleed, linked: true };
    crate::table::styles::seed_new_document(&mut doc);
    Ok(doc)
}

pub const NEW_DOCUMENT_BLEED_MM: f32 = 3.0;

pub fn new_document_bleed(units: crate::document::Units, dpi: f32) -> f32 {
    let inches = NEW_DOCUMENT_BLEED_MM / 25.4;
    match units {
        crate::document::Units::Mm => NEW_DOCUMENT_BLEED_MM,
        crate::document::Units::Cm => NEW_DOCUMENT_BLEED_MM / 10.0,
        crate::document::Units::In => inches,
        crate::document::Units::Pt => inches * 72.0,
        crate::document::Units::Px => inches * if dpi.is_finite() && dpi > 0.0 { dpi } else { 72.0 },
    }
}

pub fn execute_doc_units_set(doc: &mut Document, args: DocUnitsSetArgs) -> Result<(), CommandError> {
    doc.set_units(args.units);
    Ok(())
}

pub fn execute_doc_scale_set(doc: &mut Document, args: DocScaleSetArgs) -> Result<(), CommandError> {
    if args.drawing_scale[0] <= 0.0 || args.drawing_scale[1] <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    doc.set_drawing_scale(args.drawing_scale).map_err(|_| CommandError::invalid_params())?;
    Ok(())
}

fn execute_layer_pose(
    doc: &mut Document, ids: &[ObjectId], update: impl Fn(&mut Object),
) -> Result<(), CommandError> {
    let groups = transform_members(doc, ids);
    let descendants: std::collections::HashSet<_> = groups.values().flatten().copied().collect();
    let mut visited = std::collections::HashSet::new();
    let mut objects = Vec::new();
    for id in ids.iter().filter(|&&id| !descendants.contains(&id) && visited.insert(id)) {
        let original = doc.get_object(*id).ok_or_else(CommandError::not_found)?;
        let mut next = original.clone();
        update(&mut next);
        if let Some(members) = groups.get(id) {
            let map = group_member_transform(original, &next, [1.0, 1.0])?;
            for id in members {
                let object = doc.get_object(*id).ok_or_else(CommandError::not_found)?;
                let follows_group = object.path_text_data().is_some_and(|path| path.path == original.id || members.contains(&path.path));
                objects.push(transform_group_member(object, &map, follows_group, false)?);
            }
        }
        objects.push((next, [1.0, 1.0]));
    }
    apply_transformed_objects(doc, objects)
}

fn layer_pose_ids(doc: &Document, ids: Option<&[u64]>) -> Vec<ObjectId> {
    match ids {
        Some(ids) => ids.iter().copied().map(ObjectId).collect(),
        None => doc.objects().iter().map(|object| object.id).collect(),
    }
}

pub fn execute_transform_flip(doc: &mut Document, args: TransformFlipArgs) -> Result<(), CommandError> {
    let anchor = args.anchor.unwrap_or(Anchor::Centre);
    let ids = layer_pose_ids(doc, args.ids.as_deref());
    if let Some(origin) = args.origin {
        let scale = match args.axis {
            FlipAxis::Horizontal => [-1.0, 1.0],
            FlipAxis::Vertical => [1.0, -1.0],
        };
        return execute_transform_selection_box(doc, &ids, &TransformSelectionBoxArgs {
            origin, scale, axes: Some([0.0, 0.0]), ..Default::default()
        });
    }
    execute_layer_pose(doc, &ids, |object| crate::transform::flip(object, args.axis, anchor))
}

pub fn execute_transform_rotate_90(doc: &mut Document, args: TransformRotate90Args) -> Result<(), CommandError> {
    let anchor = args.anchor.unwrap_or(Anchor::Centre);
    let ids = layer_pose_ids(doc, args.ids.as_deref());
    if let Some(origin) = args.origin {
        let rotation_deg = match args.direction {
            RotateDirection::Right => 90.0,
            RotateDirection::Left => -90.0,
        };
        return execute_transform_selection_box(doc, &ids, &TransformSelectionBoxArgs {
            origin, rotation_deg, ..Default::default()
        });
    }
    execute_layer_pose(doc, &ids, |object| crate::transform::rotate_90(object, args.direction, anchor))
}

pub fn execute_object_create(doc: &mut Document, args: ObjectCreateArgs) -> Result<ObjectId, CommandError> {
    if args.size[0] <= 0.0 || args.size[1] <= 0.0 {
        return Err(CommandError::invalid_params());
    }

    if !args.fill.as_ref().map_or(true, validate_fill) || !args.stroke.as_ref().map_or(true, crate::paint::validate_stroke) {
        return Err(CommandError::invalid_params());
    }

    let opacity = args.opacity.unwrap_or(1.0);
    if !validate_opacity(opacity) {
        return Err(CommandError::invalid_params());
    }

    let mut obj = Object::default();
    
    obj.name = match args.name {
        Some(n) => n,
        None => String::new(),
    };
    
    obj.pos = args.pos;
    obj.size = args.size;
    obj.fill = args.fill.unwrap_or_default();
    if let Some(stroke) = args.stroke {
        obj.stroke = stroke;
    }
    obj.opacity = opacity;
    crate::artboard::edit::assign_new_object(doc, &mut obj, args.pos);

    let id = doc.create_object(obj);
    Ok(id)
}

pub fn execute_object_update(doc: &mut Document, args: ObjectUpdateArgs) -> Result<(), CommandError> {
    let id = ObjectId(args.id);

    if doc.get_object(id).is_none() {
        return Err(CommandError::not_found());
    }

    if let Some(size) = args.size {
        if size[0] <= 0.0 || size[1] <= 0.0 {
            return Err(CommandError::invalid_params());
        }
    }

    if let Some(opacity) = args.opacity {
        if !validate_opacity(opacity) {
            return Err(CommandError::invalid_params());
        }
    }

    if let Some(ref fill_opt) = args.fill {
        if !validate_fill(fill_opt) {
            return Err(CommandError::invalid_params());
        }
    }

    if !args.stroke.as_ref().map_or(true, crate::paint::validate_stroke) {
        return Err(CommandError::invalid_params());
    }

    if let Some(name) = args.name {
        if let Some(obj) = doc.find_object_mut(args.id) {
            obj.name = name;
        }
    }

    if let Some(pos) = args.pos {
        if let Some(obj) = doc.find_object_mut(args.id) {
            obj.pos = pos;
        }
    }

    if let Some(size) = args.size {
        if let Some(obj) = doc.find_object_mut(args.id) {
            obj.size = size;
        }
    }

    if let Some(fill) = args.fill {
        if let Some(obj) = doc.find_object_mut(args.id) {
            obj.fill = fill;
        }
    }

    if let Some(stroke) = args.stroke {
        if let Some(obj) = doc.find_object_mut(args.id) {
            obj.stroke = stroke;
        }
    }

    if let Some(opacity) = args.opacity {
        if let Some(obj) = doc.find_object_mut(args.id) {
            obj.opacity = opacity;
        }
    }

    if let Some(transparency) = args.transparency {
        if let Some(obj) = doc.find_object_mut(args.id) {
            obj.transparency = transparency;
        }
    }

    Ok(())
}

pub fn execute_object_delete(doc: &mut Document, args: ObjectDeleteArgs) -> Result<(), CommandError> {
    let id = ObjectId(args.id);

    if doc.get_object(id).is_none() {
        return Err(CommandError::not_found());
    }

    let doomed = doc.subtree_ids(id);
    let mut stories: Vec<crate::document::StoryId> = Vec::new();
    for object in doc.objects.iter().filter(|object| doomed.contains(&object.id)) {
        stories.extend(object.story_id());
        if let crate::object::ObjectKind::Table(table) = &object.kind {
            stories.extend(table.cells.iter().map(|cell| cell.story));
        }
    }
    doc.delete_object(id)?;
    for story in stories {
        crate::frame::drop_story_if_unowned(doc, story);
    }
    Ok(())
}

pub const APP_NAME: &str = "dtool";
pub const APP_NAME_KEY: &str = "app.title";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_LICENCES_KEY: &str = "about.licences";
pub const APP_LICENCES: &[&str] = &[
    "dtool-core",
    "dtool-ui",
    "dtool-strings",
    "egui",
    "serde",
    "anyhow",
];

pub fn invalid_params_error() -> CommandError {
    CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() }
}

pub fn command_params_empty(params: &serde_json::Value) -> bool {
    match params {
        serde_json::Value::Null => true,
        serde_json::Value::Object(map) => map.is_empty(),
        _ => false,
    }
}

pub fn check_no_params(params: &serde_json::Value) -> Result<(), CommandError> {
    if command_params_empty(params) {
        Ok(())
    } else {
        Err(invalid_params_error())
    }
}

pub fn app_about() -> serde_json::Value {
    serde_json::json!({
        "name": APP_NAME,
        "name_key": APP_NAME_KEY,
        "version": APP_VERSION,
        "licences_key": APP_LICENCES_KEY,
        "licences": APP_LICENCES
    })
}

pub fn app_version() -> serde_json::Value {
    serde_json::json!({
        "name": APP_NAME,
        "version": APP_VERSION
    })
}

pub const PIXEL_SELECTION_RESTORE: &str = "selection.pixel.restore";

#[derive(Debug, Clone, PartialEq)]
pub struct SelectionRectArgs {
    pub rect: [f32; 4],
    pub op: SelectionOp,
    pub antialias: bool,
}

pub fn parse_selection_rect(params: &serde_json::Value, dpi: f32) -> Result<SelectionRectArgs, CommandError> {
    let map = params.as_object().ok_or_else(invalid_params_error)?;
    let rect_value = match map.get("rect") {
        Some(value) => value,
        None => params,
    };
    let rect_map = rect_value.as_object().ok_or_else(invalid_params_error)?;
    let mut rect = [0.0f32; 4];
    for (slot, key) in ["x", "y", "w", "h"].iter().enumerate() {
        let value = rect_map.get(*key).ok_or_else(invalid_params_error)?;
        rect[slot] = parse_length(value, dpi)?;
    }
    let op = match map.get("op") {
        None | Some(serde_json::Value::Null) => SelectionOp::New,
        Some(value) => serde_json::from_value::<SelectionOp>(value.clone()).map_err(|_| invalid_params_error())?,
    };
    let antialias = match map.get("antialias") {
        None | Some(serde_json::Value::Null) => true,
        Some(value) => value.as_bool().ok_or_else(invalid_params_error)?,
    };
    Ok(SelectionRectArgs { rect, op, antialias })
}

pub fn pixel_selection_summary(doc: &Document) -> serde_json::Value {
    let (width, height) = match &doc.pixel_selection {
        Some(sel) => (sel.width, sel.height),
        None => document_px_size(doc).unwrap_or((0, 0)),
    };
    match &doc.pixel_selection {
        Some(sel) if !sel.is_empty() => serde_json::json!({
            "empty": false,
            "bounds": sel.bounds(),
            "area_px": sel.area(),
            "width": width,
            "height": height,
            "page_id": sel.page_id
        }),
        _ => serde_json::json!({
            "empty": true,
            "bounds": serde_json::Value::Null,
            "area_px": 0.0,
            "width": width,
            "height": height
        }),
    }
}

pub fn apply_selection_rect(doc: &mut Document, params: &serde_json::Value) -> Result<(), CommandError> {
    let surface = crate::selection::PixelSurface::for_command(doc, params)?;
    let [width, height] = surface.size;
    let args = parse_selection_rect(params, doc.dpi)?;
    let mut mask = match params.get("shape").and_then(serde_json::Value::as_str).unwrap_or("rectangle") {
        "rectangle" => rect_mask(width, height, args.rect, args.antialias),
        "ellipse" => crate::selection::ellipse_mask(width, height, args.rect, args.antialias),
        _ => return Err(invalid_params_error()),
    };
    if let Some(value) = params.get("feather") {
        let radius = parse_length(value, doc.dpi)?;
        if !(0.0..=1000.0).contains(&radius) { return Err(invalid_params_error()); }
        if radius > 0.0 { mask.feather(radius); }
    }
    mask.page_id = params["page_id"].as_u64().map(crate::document::PageId);
    crate::selection::combine_page_mask(doc, mask, args.op)
}

pub fn apply_pixel_selection_restore(doc: &mut Document, params: &serde_json::Value) -> Result<(), CommandError> {
    let value = params.get("selection").cloned().unwrap_or(serde_json::Value::Null);
    let restored: Option<PixelSelection> = serde_json::from_value(value).map_err(|_| invalid_params_error())?;
    doc.pixel_selection = restored;
    if let Some(channels) = params.get("spare_channels") {
        doc.spare_channels = serde_json::from_value(channels.clone()).map_err(|_| invalid_params_error())?;
    }
    Ok(())
}

pub fn apply_selection_save(doc: &mut Document, params: &serde_json::Value) -> Result<serde_json::Value, CommandError> {
    let to = params.get("to").and_then(|v| v.as_str());
    let path = params.get("path").and_then(|v| v.as_str()).filter(|p| !p.is_empty());
    let dest = match to {
        Some("channel") => "channel",
        Some("file") => "file",
        None if path.is_some() => "file",
        None => "channel",
        Some(_) => return Err(invalid_params_error()),
    };
    let coverage = match &doc.pixel_selection {
        Some(sel) if !sel.is_empty() => sel.clone(),
        _ => return Err(CommandError::new("NOTHING_SELECTED")),
    };
    if dest == "channel" {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string())
            .unwrap_or_else(|| crate::selection::default_spare_channel_name(&doc.spare_channels));
        crate::selection::push_spare_channel(&mut doc.spare_channels, name.clone(), coverage);
        Ok(serde_json::json!({ "to": "channel", "name": name }))
    } else {
        let path = path.ok_or_else(invalid_params_error)?;
        let written = crate::selection::write_afselection_file(std::path::Path::new(path), &coverage)?;
        Ok(serde_json::json!({ "to": "file", "path": written }))
    }
}

pub fn apply_selection_load(doc: &mut Document, params: &serde_json::Value) -> Result<(), CommandError> {
    let from = params.get("from").and_then(|v| v.as_str());
    let path = params.get("path").and_then(|v| v.as_str()).filter(|p| !p.is_empty());
    let source = match from {
        Some("channel") => "channel",
        Some("file") => "file",
        None if path.is_some() => "file",
        None => "channel",
        Some(_) => return Err(invalid_params_error()),
    };
    let op = match params.get("op") {
        None | Some(serde_json::Value::Null) => SelectionOp::New,
        Some(value) => serde_json::from_value::<SelectionOp>(value.clone()).map_err(|_| invalid_params_error())?,
    };
    let mut incoming = if source == "channel" {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(invalid_params_error)?;
        crate::selection::spare_channel(&doc.spare_channels, name)?.clone()
    } else {
        let path = path.ok_or_else(invalid_params_error)?;
        crate::selection::read_afselection_file(std::path::Path::new(path))?
    };
    if params.get("page_id").is_some() {
        let surface = crate::selection::PixelSurface::for_command(doc, params)?;
        incoming.page_id = Some(surface.page_id);
    }
    if !crate::selection::selection_size_matches_document(doc, &incoming) {
        return Err(CommandError::invalid_params());
    }
    crate::selection::combine_page_mask(doc, incoming, op)
}

fn run_selection_io(session: &mut Session, command: &Command) -> Result<serde_json::Value, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let before_selection = session.document.pixel_selection.clone();
    let before_channels = session.document.spare_channels.clone();
    let mut reply = match command.name.as_str() {
        "selection.save" => apply_selection_save(&mut session.document, &command.params)?,
        "selection.load" => {
            apply_selection_load(&mut session.document, &command.params)?;
            pixel_selection_summary(&session.document)
        }
        _ => return Err(CommandError::not_implemented()),
    };
    if reply["to"] == "file" {
        reply["doc_version"] = serde_json::json!(session.generation);
        reply["changes"] = serde_json::json!([]);
        return Ok(reply);
    }
    let restore = |selection: &Option<PixelSelection>, channels: &Vec<(String, PixelSelection)>| {
        Command::with_params(PIXEL_SELECTION_RESTORE, serde_json::json!({
            "selection": selection,
            "spare_channels": channels,
        }))
    };
    let undo_cmd = restore(&before_selection, &before_channels);
    let redo_cmd = restore(&session.document.pixel_selection, &session.document.spare_channels);
    session.undo.push(crate::undo::Change {
        name: command.name.clone(),
        redo: redo_cmd,
        undo: undo_cmd,
    });
    session.generation += 1;
    session.document.version += 1;
    if let Some(map) = reply.as_object_mut() {
        map.insert("doc_version".to_string(), serde_json::json!(session.generation));
        map.insert("changes".to_string(), serde_json::json!([{ "name": command.name }]));
    }
    Ok(reply)
}

pub struct Change {
    pub name: String,
    pub redo: Command,
    pub undo: Command,
}

pub fn preflight(session: &Session, command: &Command) -> Result<(), CommandError> {
    match command.name.as_str() {
        "edit.batch" => {
            let commands = command.params.get("commands")
                .and_then(|v| v.as_array())
                .ok_or_else(|| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
            if commands.is_empty() {
                return Err(CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() });
            }

            for sub_cmd in commands {
                let cmd: Command = serde_json::from_value(sub_cmd.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let not_implemented = session.catalog.resolve(&cmd.name).is_some_and(|spec| !spec.implemented);
                let executable = crate::commands::layer_style::batch_preflight(session, &cmd).unwrap_or_else(|| session.can_execute(&cmd));
                if cmd.name == crate::table::styles::DEFAULT_SAVE || not_implemented || !executable || preflight_targets(session, &cmd).is_err() {
                    return Err(CommandError { code: "BATCH_PREFLIGHT_FAILED".to_string(), message_key: "error.batch_preflight_failed".to_string() });
                }
            }
            Ok(())
        },
        _ => {
            if let Some(spec) = session.catalog.resolve(&command.name) {
                if !spec.implemented {
                    return Err(CommandError::not_implemented());
                }
            }
            preflight_targets(session, command)?;
            if !session.can_execute(command) {
                if !session.catalog.contains(&command.name) {
                    return Err(CommandError { code: "UNKNOWN_COMMAND".to_string(), message_key: "error.unknown_command".to_string() });
                }
                return Err(CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() });
            }
            Ok(())
        }
    }
}

const OBJECT_TARGET_COMMANDS: &[&str] = &[
    "object.update",
    "object.delete",
    "transform.set",
    "transform.selection_box",
    "transform.origin.set",
    "transform.origin.reset",
    "transform.flip",
    "transform.rotate_90",
    "doc.object.query",
];

pub fn preflight_targets(session: &Session, command: &Command) -> Result<(), CommandError> {
    if !OBJECT_TARGET_COMMANDS.contains(&command.name.as_str()) {
        return Ok(());
    }
    let exists = |value: &serde_json::Value| value.as_u64().is_some_and(|id| session.document.get_object(ObjectId(id)).is_some());
    if let Some(id) = command.params.get("id") {
        if !id.is_null() && !exists(id) {
            return Err(CommandError::invalid_params());
        }
    }
    if let Some(ids) = command.params.get("ids") {
        match ids {
            serde_json::Value::Null => {}
            serde_json::Value::Array(items) => {
                if !items.iter().all(exists) {
                    return Err(CommandError::invalid_params());
                }
            }
            _ => return Err(CommandError::invalid_params()),
        }
    }
    Ok(())
}

struct BatchRollback {
    document: Document,
    generation: u64,
    undo: crate::undo::UndoStack,
    selection: Selection,
    table_selection: crate::table::selection::TableSelection,
    guides: Guides,
    view: crate::view_state::ViewState,
    clipboard: std::sync::Arc<crate::clipboard::ClipboardPayload>,
    last_duplicate_ids: Vec<ObjectId>,
    last_transform_delta: Option<[f32; 2]>,
    last_duplicate_steps: Vec<TransformSelectionBoxArgs>,
    text_edit: Option<TextEditState>,
    active_tool: String,
    tool_options: crate::context_toolbar::ToolOptionStore,
    mixer_load: crate::brush::MixerLoad,
    appearance_focus: Option<crate::commands::appearance::Focus>,
    history: crate::history_store::SessionHistory,
    effect_previews: Vec<crate::commands::layer_style::StagedEffect>,
    last_deselected: Option<PixelSelection>,
}

impl BatchRollback {
    fn capture(session: &Session) -> Self {
        Self {
            document: session.document.clone(),
            generation: session.generation,
            undo: session.undo.clone(),
            selection: session.selection.clone(),
            table_selection: session.table_selection.clone(),
            guides: session.guides.clone(),
            view: session.view.clone(),
            clipboard: session.clipboard.get_payload(),
            last_duplicate_ids: session.last_duplicate_ids.clone(),
            last_transform_delta: session.last_transform_delta,
            last_duplicate_steps: session.last_duplicate_steps.clone(),
            text_edit: session.text_edit.clone(),
            active_tool: session.active_tool.clone(),
            tool_options: session.tool_options.clone(),
            mixer_load: session.mixer_load,
            appearance_focus: session.appearance_focus,
            history: session.history.clone(),
            effect_previews: crate::commands::layer_style::snapshot_staged(session),
            last_deselected: session.last_deselected.clone(),
        }
    }

    fn restore(self, session: &mut Session) {
        session.document = self.document;
        session.generation = self.generation;
        session.undo = self.undo;
        session.selection = self.selection;
        session.table_selection = self.table_selection;
        session.guides = self.guides;
        session.view = self.view;
        session.clipboard.restore_payload(self.clipboard);
        session.last_duplicate_ids = self.last_duplicate_ids;
        session.last_transform_delta = self.last_transform_delta;
        session.last_duplicate_steps = self.last_duplicate_steps;
        session.text_edit = self.text_edit;
        session.active_tool = self.active_tool;
        session.tool_options = self.tool_options;
        session.mixer_load = self.mixer_load;
        session.appearance_focus = self.appearance_focus;
        session.history = self.history;
        session.last_deselected = self.last_deselected;
        crate::commands::layer_style::restore_staged(session, self.effect_previews);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextEditSnapshot {
    pub story: crate::document::Story,
    pub(crate) linked_text: Option<crate::commands::symbols::LinkedTextState>,
    pub(crate) marks: Option<crate::longdoc::marks::StoryMarks>,
    pub caret: usize,
    pub anchor: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextEditState {
    pub story: crate::table::model::StoryId,
    pub object: Option<ObjectId>,
    pub caret: usize,
    pub anchor: usize,
    pub before: Option<TextEditSnapshot>,
}

#[derive(Debug)]
pub struct Session {
    pub history: crate::history_store::SessionHistory,
    pub generation: u64,
    pub undo: crate::undo::UndoStack,
    pub document: Document,
    pub catalog: CommandCatalog,
    pub selection: Selection,
    pub table_selection: crate::table::selection::TableSelection,
    pub clipboard: AppClipboard,
    pub last_duplicate_ids: Vec<ObjectId>,
    pub last_transform_delta: Option<[f32; 2]>,
    pub last_duplicate_steps: Vec<TransformSelectionBoxArgs>,
    /// Last aspect lock sent with transform.set; reported by transform.query (session state, not undoable).
    pub transform_aspect_locked: bool,
    transform_origin: Option<(Vec<ObjectId>, [f32; 2])>,
    pub snap_prefs: SnapPrefsService,
    pub symbol_sync: bool,
    pub active_tool: String,
    pub panel_colour_pick_previous: Option<String>,
    pub panel_sampled_colour: Option<crate::paint::Colour>,
    pub tool_options: crate::context_toolbar::ToolOptionStore,
    pub quick_mask: crate::commands::quickmask::State,
    pub(crate) mixer_load: crate::brush::MixerLoad,
    pub(crate) last_deselected: Option<PixelSelection>,
    pub appearance_focus: Option<crate::commands::appearance::Focus>,
    pub guides: Guides,
    pub view: crate::view_state::ViewState,
    pub view_canvas_size: Option<[f32; 2]>,
    pub view_target_hint: Option<crate::view::ViewTarget>,
    pub previous_viewed_page: Option<crate::document::PageId>,
    pub(crate) display_dpi: Option<f32>,
    pub has_document: bool,
    pub text_edit: Option<TextEditState>,
    pub autosave: crate::autosave::Autosave,
    pub change_log: crate::query::ChangeLog,
    pub edit: Option<crate::edit_session::EditSession>,
    pub text_composing: Option<String>,
    pub context: Option<crate::workspace::ContextId>,
    pub macro_recorder: crate::macro_recorder::MacroRecorder,
    pub(crate) effect_preview_owner: std::sync::Arc<()>,
}

pub const VIEW_PAN: &str = "view.pan";
pub const VIEW_ZOOM_SET: &str = "view.zoom.set";
pub const ZOOM_STEP: f32 = 2.0;
pub const ZOOM_PRESETS: [f32; 4] = [0.5, 1.0, 2.0, 4.0];

#[derive(Debug, Clone, Deserialize)]
pub struct ViewZoomSetArgs {
    pub zoom: f32,
    #[serde(default)]
    pub around: Option<[f32; 2]>,
    #[serde(default)]
    pub view_size: Option<[f32; 2]>,
}

pub fn parse_view_zoom_set(params: &serde_json::Value) -> Result<ViewZoomSetArgs, CommandError> {
    let args: ViewZoomSetArgs = serde_json::from_value(params.clone()).map_err(|_| invalid_params_error())?;
    if !args.zoom.is_finite() || args.zoom <= 0.0 {
        return Err(invalid_params_error());
    }
    Ok(args)
}

pub fn parse_view_zoom_step(params: &serde_json::Value, zoom: f32, zoom_in: bool) -> Result<ViewZoomSetArgs, CommandError> {
    #[derive(serde::Deserialize)]
    struct Step {
        #[serde(default)]
        factor: Option<f32>,
        #[serde(default)]
        around: Option<[f32; 2]>,
        #[serde(default)]
        view_size: Option<[f32; 2]>,
    }
    let params = if params.is_null() { serde_json::json!({}) } else { params.clone() };
    let step: Step = serde_json::from_value(params).map_err(|_| invalid_params_error())?;
    let factor = step.factor.unwrap_or(2.0);
    if !factor.is_finite() || factor <= 0.0 {
        return Err(invalid_params_error());
    }
    let target = if zoom_in { zoom * factor } else { zoom / factor };
    Ok(ViewZoomSetArgs { zoom: target, around: step.around, view_size: step.view_size })
}

pub fn parse_view_delta(params: &serde_json::Value) -> Result<[f32; 2], CommandError> {
    let arr = params
        .get("delta")
        .and_then(|v| v.as_array())
        .ok_or_else(invalid_params_error)?;
    if arr.len() != 2 {
        return Err(invalid_params_error());
    }
    let x = arr[0].as_f64().ok_or_else(invalid_params_error)? as f32;
    let y = arr[1].as_f64().ok_or_else(invalid_params_error)? as f32;
    if !x.is_finite() || !y.is_finite() {
        return Err(invalid_params_error());
    }
    Ok([x, y])
}

pub fn view_pan_apply(view: &mut crate::view_state::ViewState, delta: [f32; 2]) {
    view.scroll[0] += delta[0];
    view.scroll[1] += delta[1];
}

pub fn view_rotation_wrap(degrees: f32) -> f32 {
    let r = degrees.rem_euclid(360.0);
    if r >= 360.0 { 0.0 } else { r }
}

impl Session {
    fn push_guide_undo(&mut self, name: &str, redo: Command, undo_cmd: Command) {
        self.undo.push(crate::undo::Change {
            name: name.to_string(),
            redo,
            undo: undo_cmd,
        });
        self.generation += 1;
        self.document.version += 1;
    }

    pub fn paste_special_formats(&self) -> Vec<String> {
        paste_special_formats(&self.clipboard.get_payload())
    }

    pub fn can_execute_named(&self, name: &str) -> bool {
        let command = Command { name: name.to_string(), params: serde_json::json!({}) };
        self.can_execute(&command)
    }

    pub fn run_named(&mut self, name: &str, params: serde_json::Value) -> Result<serde_json::Value, CommandError> {
        let command = Command { name: name.to_string(), params };
        self.run(&command)
    }

    pub fn edit_active(&self) -> bool {
        self.edit.is_some()
    }

    pub fn preview(&mut self, key: &str, command: &Command) -> CommandResult {
        if self.edit.as_ref().is_some_and(|edit| edit.key != key || edit.tool_at_begin != self.active_tool) {
            self.end_edit(crate::edit_session::EditEnd::Commit)?;
        }
        let snapshot = crate::edit_session::snapshot_for(self, command);
        let depth = self.undo.applied_steps();
        let generation = self.generation;
        let result = self.run(command)?;
        let pushed = self.undo.applied_steps().saturating_sub(depth);
        let (undo, redo) = if pushed == 1 {
            let change = self.undo.pop_last().ok_or_else(crate::edit_session::not_previewable)?;
            (change.undo, change.redo)
        } else if pushed > 1 {
            let mut changes = Vec::new();
            while self.undo.applied_steps() > depth {
                if let Some(change) = self.undo.pop_last() {
                    changes.push(change);
                }
            }
            for change in changes.iter() {
                self.restore_command(&change.undo)?;
            }
            self.generation += 1;
            return Err(crate::edit_session::not_previewable());
        } else if let Some(pair) = snapshot {
            pair
        } else {
            return Ok(result);
        };
        match self.edit.as_mut() {
            Some(edit) => edit.last_redo = redo,
            None => {
                self.edit = Some(crate::edit_session::EditSession {
                    key: key.to_string(),
                    name: command.name.clone(),
                    first_undo: undo,
                    last_redo: redo,
                    doc_generation_at_begin: generation,
                    tool_at_begin: self.active_tool.clone(),
                });
            }
        }
        Ok(result)
    }

    pub fn end_edit(&mut self, end: crate::edit_session::EditEnd) -> CommandResult {
        let Some(edit) = self.edit.take() else {
            return Ok(serde_json::json!({ "doc_version": self.generation, "changes": [] }));
        };
        match end {
            crate::edit_session::EditEnd::Commit => {
                self.undo.push(crate::undo::Change { name: edit.name.clone(), undo: edit.first_undo, redo: edit.last_redo });
                self.generation += 1;
                Ok(serde_json::json!({ "doc_version": self.generation, "changes": [{ "name": edit.name }] }))
            }
            crate::edit_session::EditEnd::Cancel => {
                self.restore_command(&edit.first_undo)?;
                self.generation += 1;
                self.document.version += 1;
                Ok(serde_json::json!({ "doc_version": self.generation, "changes": [{ "name": edit.name }], "cancelled": true }))
            }
        }
    }

    fn restore_command(&mut self, command: &Command) -> Result<(), CommandError> {
        match self.apply_command(command) {
            Err(error) if error.code == "NOT_IMPLEMENTED" => {
                let depth = self.undo.applied_steps();
                self.run_dispatch(command)?;
                while self.undo.applied_steps() > depth {
                    self.undo.pop_last();
                }
                Ok(())
            }
            other => other,
        }
    }

    pub fn selection_bounds(&self) -> Option<[f32; 4]> {
        selection_bounds(&self.document, &self.selection.ids)
    }

    fn origin_selection_matches(&self, ids: &[ObjectId]) -> bool {
        ids.len() == self.selection.ids.len() && self.selection.ids.iter().all(|id| ids.contains(id))
    }

    fn custom_transform_origin(&self, ids: &[ObjectId]) -> Option<[f32; 2]> {
        if let [id] = ids {
            let object = self.document.get_object(*id)?;
            let local = object.transform_origin?;
            let extent = object.size.map(|size| if size == 0.0 { 1.0 } else { size.abs() });
            let point = std::array::from_fn(|axis| object.pos[axis] + extent[axis] * local[axis]);
            return Some(crate::geometry::apply_matrix(&crate::geometry::object_matrix(object), point));
        }
        self.transform_origin.as_ref()
            .filter(|(stored, _)| stored.len() == ids.len() && ids.iter().all(|id| stored.contains(id)))
            .map(|(_, point)| *point)
    }

    fn update_transform_origin(&mut self, command: &Command) -> CommandResult {
        let point: Option<[f32; 2]> = if command.name == "transform.origin.set" {
            let point = serde_json::from_value(command.params["point"].clone()).map_err(|_| CommandError::invalid_params())?;
            if !finite2(point) { return Err(CommandError::invalid_params()); }
            Some(point)
        } else {
            None
        };
        let targets = &self.selection.ids;
        if let [id] = targets.as_slice() {
            let id = *id;
            let before = self.document.get_object(id).filter(|object| !object.locked).cloned()
                .ok_or_else(CommandError::invalid_params)?;
            let origin = if let Some(point) = point {
                let local = crate::geometry::doc_to_object_local(&before, point).ok_or_else(CommandError::invalid_params)?;
                let extent = before.size.map(|size| if size == 0.0 { 1.0 } else { size.abs() });
                let origin = std::array::from_fn(|axis| (local[axis] - before.pos[axis]) / extent[axis]);
                if !finite2(origin) { return Err(CommandError::invalid_params()); }
                Some(origin)
            } else {
                None
            };
            if before.transform_origin == origin {
                return Ok(serde_json::json!({"changes":[]}));
            }
            let object = self.document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
            object.transform_origin = origin;
            let redo = Command::with_params("object.restore", serde_json::json!({"objects":[object]}));
            self.push_restore_undo(&command.name, redo, vec![before]);
        } else if !targets.is_empty() {
            if let Some(point) = point {
                self.transform_origin = Some((targets.clone(), point));
            } else if self.custom_transform_origin(targets).is_some() {
                self.transform_origin = None;
            }
        } else {
            return Err(CommandError::invalid_params());
        }
        Ok(serde_json::json!({"doc_version":self.generation,"changes":[{"name":command.name}]}))
    }

    pub fn selection_transform_origin(&self) -> Option<[f32; 2]> {
        self.transform_origin_for(&self.selection.ids)
    }

    fn transform_origin_for(&self, ids: &[ObjectId]) -> Option<[f32; 2]> {
        let tool = if self.active_tool == "tool.select.object" { "tool.select.object" } else { "tool.move" };
        if !self.origin_selection_matches(ids) || !self.tool_options
            .flag(tool, crate::context_toolbar::MOVE_SHOW_ORIGIN).unwrap_or(false)
        {
            return None;
        }
        if let Some(point) = self.custom_transform_origin(ids) { return Some(point); }
        let [left, top, width, height] = self.selection_bounds()?;
        Some([left + width * 0.5, top + height * 0.5])
    }

    pub(crate) fn snapshot_objects(&self, ids: &[ObjectId]) -> Vec<Object> {
        ids.iter().filter_map(|id| self.document.get_object(*id).cloned()).collect()
    }

    pub(crate) fn push_restore_undo(&mut self, name: &str, redo: Command, old_objects: Vec<Object>) {
        let undo_cmd = Command {
            name: "object.restore".to_string(),
            params: serde_json::to_value(ObjectRestoreArgs { objects: old_objects }).unwrap_or_default(),
        };
        self.undo.push(crate::undo::Change {
            name: name.to_string(),
            redo,
            undo: undo_cmd,
        });
        self.generation += 1;
        self.document.version += 1;
    }

    pub fn with_clipboard(document: Document, clipboard: AppClipboard) -> Self {
        let mut session = Session::new(document);
        session.clipboard = clipboard;
        session
    }

    pub fn new(mut document: Document) -> Self {
        document.hydrate_ids();
        Session {
            autosave: crate::autosave::Autosave::new(),
            generation: 0,
            undo: crate::undo::UndoStack::new(),
            document,
            catalog: CommandCatalog::new(),
            selection: Selection::default(),
            table_selection: crate::table::selection::TableSelection::default(),
            clipboard: AppClipboard::new(),
            history: crate::history_store::SessionHistory::default(),
            last_duplicate_ids: Vec::new(),
            last_transform_delta: None,
            last_duplicate_steps: Vec::new(),
            transform_aspect_locked: false,
            transform_origin: None,
            snap_prefs: SnapPrefsService::new(),
            symbol_sync: true,
            active_tool: "tool.move".to_string(),
            panel_colour_pick_previous: None,
            panel_sampled_colour: None,
            tool_options: crate::context_toolbar::ToolOptionStore::default(),
            appearance_focus: None,
            guides: Guides::default(),
            view: crate::view_state::ViewState::default(),
            view_canvas_size: None,
            view_target_hint: None,
            previous_viewed_page: None,
            display_dpi: None,
                    has_document: true,
            text_edit: None,
            change_log: crate::query::ChangeLog::default(),
            edit: None,
            text_composing: None,
            context: None,
            macro_recorder: crate::macro_recorder::MacroRecorder::default(),
            effect_preview_owner: std::sync::Arc::new(()),
            quick_mask: Default::default(),
            mixer_load: Default::default(),
            last_deselected: None,
        }
    }

    pub fn document(&self) -> Option<&Document> {
        if self.has_document { Some(&self.document) } else { None }
    }

    pub fn document_mut(&mut self) -> Option<&mut Document> {
        if self.has_document { Some(&mut self.document) } else { None }
    }

    pub fn query(&self, since: Option<u64>) -> Result<serde_json::Value, CommandError> {
        let current_version = self.generation;
        match since {
            None => crate::query::query_snapshot(current_version, &self.document),
            Some(s) => {
                if !self.change_log.is_kept(s, current_version) {
                    return Err(crate::query::invalid_params_error());
                }
                let changes = self.change_log.changes_since(s);
                let mut delta = crate::query::query_delta(current_version, changes);
                if let Some(map) = delta.as_object_mut() {
                    map.insert("tables".to_string(), crate::query::embedded_tables(&self.document));
                }
                Ok(delta)
            }
        }
    }


    pub fn view_rotation_result(&self, name: &str) -> serde_json::Value {
        serde_json::json!({
            "doc_version": self.generation,
            "changes": [{ "name": name }],
            "rotation_deg": self.view.rotation_deg
        })
    }

    pub fn view_rotate_set(&mut self, degrees: f32) -> Result<serde_json::Value, CommandError> {
        if !degrees.is_finite() {
            return Err(invalid_params_error());
        }
        self.view.rotation_deg = view_rotation_wrap(degrees);
        Ok(self.view_rotation_result("view.rotate.set"))
    }

    pub fn view_pan(&mut self, delta: [f32; 2]) -> serde_json::Value {
        let before = self.view.clone();
        view_pan_apply(&mut self.view, delta);
        crate::commands::pages::remember_view_change(self, &before);
        serde_json::json!({
            "doc_version": self.generation,
            "changes": [{ "name": VIEW_PAN }],
            "scroll": self.view.scroll,
            "zoom": self.view.zoom
        })
    }

    pub fn view_rotate_reset(&mut self) -> serde_json::Value {
        self.view.rotation_deg = 0.0;
        self.view_rotation_result("view.rotate.reset")
    }

    pub fn can_execute(&self, command: &Command) -> bool {
        if command.name == crate::autosave::CMD_AUTOSAVE_SET { return true; }
        if let Some(allowed) = crate::colour::convert::can_execute_profile_convert(self, command) { return allowed; }
        if let Some(allowed) = crate::longdoc::books::can_execute(command) {
            return allowed;
        }
        if let Some(allowed) = crate::commands::can_execute(self, command) {
            return allowed;
        }
        if let Some(allowed) = crate::table::commands::can_execute(self, command) {
            return allowed;
        }
        match command.name.as_str() {
            "edit.undo" => self.undo.can_undo() || self.edit.is_some() || self.text_edit.as_ref().is_some_and(|edit| edit.before.is_some()),
            "edit.redo" => self.undo.can_redo() || self.edit.is_some(),
            "edit.batch" => true,
            "commands.list" | "noop" | "command.noop" => true,
            "selection.query" => true,
            "selection.set" => true,
            "selection.marquee" => true,
            "selection.hit" => true,
            "selection.enter" => true,
            "selection.exit" => true,
            "selection.cycle" => true,
            "selection.rect" => true,
            "doc.units.set" => serde_json::from_value::<DocUnitsSetArgs>(command.params.clone()).is_ok(),
            "doc.scale.set" => serde_json::from_value::<DocScaleSetArgs>(command.params.clone()).is_ok_and(|args| args.drawing_scale[0] > 0.0 && args.drawing_scale[1] > 0.0),
            "selection.save" => self.has_document && self.document.pixel_selection.as_ref().is_some_and(|sel| !sel.is_empty()),
            "selection.load" => self.has_document,
            "transform.flip" => true,
            "transform.origin.set" => !self.selection.ids.is_empty(),
            "transform.origin.reset" => self.custom_transform_origin(&self.selection.ids).is_some(),
            "transform.rotate_90" => true,
            "snap.prefs.query" => true,
            "snap.prefs.set" => true,
            "transform.selection_box" => true,
            "transform.set" => true,
            "transform.query" => true,
            "object.restore" => true,
            "object.insert" => true,
            "object.create" => true,
            "object.update" | "object.delete" => command.params.get("id").and_then(|v| v.as_u64()).map_or(false, |id| self.document.get_object(ObjectId(id)).is_some()),
            "guides.add" => true,
            "guides.update" => true,
            "guides.delete" => true,
            "guides.list" => true,
            "guides.find" => true,
            "guides.show" => true,
            "guides.insert" => true,
            "guides.remove" => true,
            "guides.restore" => true,
            "ui.key.send" => true,
            "view.rotate.set" => command.params.get("degrees").and_then(|v| v.as_f64()).map_or(false, |d| d.is_finite()),
            "view.rotate.reset" => true,
            "view.pan" => parse_view_delta(&command.params).is_ok(),
            crate::view::VIEW_ZOOM_FIT | crate::view::VIEW_ZOOM_TO_WIDTH | crate::view::VIEW_ZOOM_TO_DOCUMENT => {
                self.has_document && crate::view::parse_view_size(&command.params).is_ok()
            }
            crate::view::VIEW_ZOOM_TO_SELECTION => {
                self.has_document && crate::view::parse_view_size(&command.params).is_ok()
                    && (!self.selection.ids.is_empty() || self.document.pixel_selection.as_ref().is_some_and(|mask| {
                        !mask.is_empty() && crate::selection::PixelSurface::for_page(&self.document, mask.page_id)
                            .is_some_and(|surface| surface.matches(&self.document, mask))
                    }))
            }
            "view.zoom.set" => parse_view_zoom_set(&command.params).is_ok(),
            "view.zoom.in" | "view.zoom.out" => parse_view_zoom_step(&command.params, self.view.zoom, command.name == "view.zoom.in").is_ok(),
            "app.about" | "app.version" | "app.status" => command_params_empty(&command.params),
            _ => false,
        }
    }

    pub fn app_status(&self) -> serde_json::Value {
        serde_json::json!({
            "name": APP_NAME,
            "version": APP_VERSION,
            "doc_version": self.generation,
            "has_document": true,
            "can_undo": self.undo.can_undo(),
            "can_redo": self.undo.can_redo(),
            "active_tool": self.active_tool
        })
    }

    pub fn execute_batch(&mut self, commands: Vec<Command>) -> Result<Vec<ChangeName>, CommandError> {
        for cmd in &commands {
            // Effect edits on an effect added earlier in this batch are checked when they apply.
            let ok = match crate::commands::layer_style::batch_preflight(self, cmd) {
                Some(ok) => ok,
                None => preflight(self, cmd).is_ok(),
            };
            if cmd.name == crate::table::styles::DEFAULT_SAVE || !ok {
                return Err(CommandError { code: "BATCH_PREFLIGHT_FAILED".to_string(), message_key: "error.batch_preflight_failed".to_string() });
            }
        }

        let rollback = BatchRollback::capture(self);
        let before = rollback.document.clone();
        let mut changes = Vec::new();

        for cmd in commands {
            let applied = match cmd.name.as_str() {
                "edit.undo" => match self.undo.undo() {
                    Some(undo_cmd) => self.apply_command(&undo_cmd),
                    None => Err(CommandError::invalid_params()),
                },
                "edit.redo" => match self.undo.redo() {
                    Some(redo_cmd) => self.apply_command(&redo_cmd),
                    None => Err(CommandError::invalid_params()),
                },
                _ => self.apply_command(&cmd),
            };
            if let Err(e) = applied {
                rollback.restore(self);
                return Err(e);
            }
            changes.push(ChangeName { name: cmd.name.clone() });
        }

        if !changes.is_empty() {
            self.commit_replacement("edit.batch", &before, &rollback.selection.ids);
        }

        Ok(changes)
    }

    pub fn apply_job_result(&mut self, pin: &crate::job::JobPin, preview: crate::job::JobPreview) -> CommandResult {
        if pin.source_generation != self.generation {
            return Err(CommandError::new("STALE"));
        }
        if let Some(target) = pin.target {
            if self.document.get_object(target).is_none() {
                return Err(CommandError::not_found());
            }
        }
        let before = self.document.clone();
        let before_selection = self.selection.ids.clone();
        for object in preview.objects {
            match self.document.objects.iter_mut().find(|existing| existing.id == object.id) {
                Some(existing) => *existing = object,
                None => self.document.objects.push(object),
            }
        }
        self.commit_replacement("job.apply", &before, &before_selection);
        Ok(serde_json::json!({ "doc_version": self.generation, "job": pin.job_id.0 }))
    }

    pub fn commit_replacement(&mut self, name: &str, before: &crate::document::Document, before_selection: &[ObjectId]) {
        let restore = |document: &crate::document::Document, selection: &[ObjectId]| Command {
            name: crate::commands::feature_restore::FEATURE_RESTORE.to_string(),
            params: serde_json::json!({ "document": document, "selection": selection }),
        };
        let change = crate::undo::Change {
            name: name.to_string(),
            redo: restore(&self.document, &self.selection.ids),
            undo: restore(before, before_selection),
        };
        self.undo.push(change);
        self.document.raise_id_high_water();
        self.generation += 1;
        self.document.version += 1;
    }

    fn apply_command(&mut self, command: &Command) -> Result<(), CommandError> {
        if command.name == VIEW_ZOOM_SET {
            let args = parse_view_zoom_set(&command.params)?;
            self.view_zoom_set(args);
            return Ok(());
        }
        if let Some(result) = crate::colour::convert::apply_profile_convert(self, command) {
            return result;
        }
        if let Some(result) = crate::commands::apply(self, command) {
            return result;
        }
        if command.name == "edit.batch" {
            let commands: Vec<Command> = serde_json::from_value(command.params.get("commands").cloned().unwrap_or(serde_json::Value::Null))
                .map_err(|_| CommandError::invalid_params())?;
            let rollback = BatchRollback::capture(self);
            for cmd in &commands {
                if let Err(e) = self.apply_command(cmd) {
                    rollback.restore(self);
                    return Err(e);
                }
            }
            return Ok(());
        }
        match command.name.as_str() {
            "transform.flip" => {
                let args: TransformFlipArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                execute_transform_flip(&mut self.document, args)?;
            },
            "transform.rotate_90" => {
                let args: TransformRotate90Args = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                execute_transform_rotate_90(&mut self.document, args)?;
            },
            "object.create" => {
                let args: ObjectCreateArgs = serde_json::from_value(crate::paint::normalize_fill_param(&command.params)?)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                execute_object_create(&mut self.document, args)?;
            },
            "object.update" => {
                let args: ObjectUpdateArgs = serde_json::from_value(crate::paint::normalize_fill_param(&command.params)?)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                if crate::commands::symbols::object_update_is_linked(&self.document, &args) {
                    crate::commands::symbols::apply_object_update(&mut self.document, args, self.symbol_sync)?;
                } else {
                    execute_object_update(&mut self.document, args)?;
                }
            },
            "object.delete" => {
                let args: ObjectDeleteArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                execute_object_delete(&mut self.document, args)?;
            },
            "selection.set" => {
                let args: SelectionSetArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                args.apply(&mut self.selection);
            },
            "selection.marquee" => {
                let args: SelectionMarqueeArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let mode = args.mode.unwrap_or(MarqueeMode::Touch);
                let op = args.op.unwrap_or(SelectOp::Replace);
                self.selection = selection_marquee_with_flags(&self.document, args.rect, mode, op, &self.selection, &self.view.canvas_flags);
            },
            "selection.hit" => {
                let args: SelectionHitArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let cycle = args.cycle.unwrap_or(false);
                self.selection = selection_hit(&self.document, args.point, cycle, &self.selection);
            },
            "selection.enter" => {
                self.selection = selection_enter(&self.document, &self.selection);
            },
            "selection.exit" => {
                self.selection = selection_exit(&self.document, &self.selection);
            },
            "selection.cycle" => {
                let args: SelectionCycleArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                self.selection = selection_cycle(&self.document, args.direction, &self.selection);
            },
            "selection.rect" => {
                apply_selection_rect(&mut self.document, &command.params)?;
            },
            "doc.units.set" => {
                let args: DocUnitsSetArgs = serde_json::from_value(command.params.clone()).map_err(|_| invalid_params_error())?;
                execute_doc_units_set(&mut self.document, args)?;
            },
            "doc.scale.set" => {
                let args: DocScaleSetArgs = serde_json::from_value(command.params.clone()).map_err(|_| invalid_params_error())?;
                execute_doc_scale_set(&mut self.document, args)?;
            },
            PIXEL_SELECTION_RESTORE => {
                let last = command.params.get("last_deselected").map(|value| {
                    serde_json::from_value::<Option<PixelSelection>>(value.clone()).map_err(|_| invalid_params_error())
                }).transpose()?;
                apply_pixel_selection_restore(&mut self.document, &command.params)?;
                if let Some(last) = last { self.last_deselected = last; }
            },
            "selection.save" => {
                apply_selection_save(&mut self.document, &command.params)?;
            },
            "selection.load" => {
                apply_selection_load(&mut self.document, &command.params)?;
            },
            "transform.selection_box" => {
                let args: TransformSelectionBoxArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                let ids = resolve_target_ids(&self.document, &args.ids, &self.selection)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                execute_transform_selection_box(&mut self.document, &ids, &args)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
            },
            "transform.set" => {
                let args: TransformSetArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                let ids = resolve_target_ids(&self.document, &args.ids, &self.selection)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                apply_transform_set(&mut self.document, &ids, &args, command.params["content_bounds"].as_bool().unwrap_or(false))
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
            },
            "object.restore" => {
                let args: ObjectRestoreArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                execute_object_restore(&mut self.document, &args)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
            },
            "object.insert" => {
                let args: ObjectInsertArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                for obj in args.objects {
                    if obj.id.0 == 0 || self.document.get_object(obj.id).is_some() {
                        return Err(CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() });
                    }
                    self.document.create_object(obj);
                }
            },
            "guides.insert" => {
                let args: GuideStateArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                self.guides.insert(args.guide).map_err(guide_error)?;
            },
            "guides.remove" => {
                let args: GuidesDeleteArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                self.guides.remove(GuideId(args.id)).map_err(guide_error)?;
            },
            "guides.restore" => {
                let args: GuideStateArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                self.guides.restore(args.guide).map_err(guide_error)?;
            },
            "view.rotate.set" => {
                let degrees = command.params.get("degrees").and_then(|v| v.as_f64()).ok_or_else(invalid_params_error)? as f32;
                self.view_rotate_set(degrees)?;
            },
            _ => {
                return Err(CommandError { code: "NOT_IMPLEMENTED".to_string(), message_key: "error.not_implemented".to_string() });
            }
        }
        Ok(())
    }

    pub fn run(&mut self, command: &Command) -> Result<serde_json::Value, CommandError> {
        if self.edit.is_some() && crate::edit_session::ends_edit(&command.name) {
            self.end_edit(crate::edit_session::EditEnd::Commit)?;
        }
        let spec = self.catalog.resolve(&command.name);
        if spec.is_some_and(|spec| !spec.implemented) {
            return Err(CommandError::not_implemented());
        }
        let track_masters = !self.document.master_pages.is_empty()
            && spec.is_some_and(|spec| spec.mutating && spec.undoable)
            && !command.name.starts_with("master.");
        let command = &crate::commands::ruler_units::normalize(self, command)?;
        let master_before = track_masters.then(|| {
            (self.document.clone(), self.selection.ids.clone(), self.undo.position(), self.undo.applied_steps())
        });
        let generation_before = self.generation;
        self.change_log.start(generation_before);
        let version_before = self.document.version;
        let geometry_before = self.document.geometry_rev;
        let macro_context = self.macro_recorder.step_context(self);
        self.macro_recorder.enter();
        let result = self.run_dispatch(command);
        self.macro_recorder.exit();
        if result.is_ok() {
            if self.has_document {
                self.document.raise_id_high_water();
            }
            if self.generation > generation_before {
                let inherited = master_before.as_ref().is_some_and(|(before, _, position, steps)| {
                    self.undo.position() != *position && self.undo.applied_steps() <= *steps + 1
                        && crate::commands::masters::inherit_changes(before, &mut self.document, command)
                });
                crate::commands::masters::lock_positions(&mut self.document);
                if inherited && let Some((before, selection, _, _)) = master_before {
                    let undo = crate::commands::document_change::restore(&before, &selection);
                    let redo = crate::commands::document_change::restore(&self.document, &self.selection.ids);
                    self.undo.replace_last_commands(undo, redo);
                }
                if self.document.version != version_before
                    || self.document.geometry_rev != geometry_before
                    || matches!(command.name.as_str(), "edit.undo" | "edit.redo")
                {
                    crate::commands::brush::refresh_blends(&mut self.document);
                    crate::clip_mask::reconcile_mask_view(self);
                }
                crate::commands::caption::follow(self);
                self.change_log.record(self.generation, &command.name);
            }
            if self.macro_recorder.is_recording() {
                self.macro_recorder.tap_step(&self.catalog, command, macro_context, Some(&self.document));
            }
        }
        result
    }

    fn run_dispatch(&mut self, command: &Command) -> Result<serde_json::Value, CommandError> {
        crate::table::selection::reconcile(self);
        if !crate::text_cmd::is_text_edit_command(&command.name)
            && self.catalog.resolve(&command.name).map(|spec| spec.mutating).unwrap_or(true)
        {
            crate::text_cmd::commit_text_edit(self);
        }
        if let Some(result) = autosave_commands::run(self, command) { return result; }
        if let Some(result) = crate::colour::convert::run_profile_convert(self, command) { return result; }
        if let Some(result) = crate::longdoc::books::run(self, command) { return result; }
        if let Some(target) = crate::view::FitTarget::for_command(&command.name) {
            let view_size = crate::view::parse_view_size(&command.params)?;
            return self.view_fit_target(view_size, target);
        }
        if command.name == VIEW_ZOOM_SET {
            let args = parse_view_zoom_set(&command.params)?;
            return Ok(self.view_zoom_set(args));
        }
        if command.name == "view.zoom.in" || command.name == "view.zoom.out" {
            let args = parse_view_zoom_step(&command.params, self.view.zoom, command.name == "view.zoom.in")?;
            return Ok(self.view_zoom_set(args));
        }
        if let Some(result) = crate::commands::run(self, command) {
            return result;
        }
        if command.name == "selection.save" || command.name == "selection.load" {
            return run_selection_io(self, command);
        }
        if let Err(e) = self.catalog.assert_named(&command.name) {
            return Err(CommandError { message_key: format!("error.{}", e.to_lowercase()), code: e });
        }

        match command.name.as_str() {
            "edit.undo" => {
                if !self.undo.can_undo() {
                    return Err(CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() });
                }
                let undo_cmd = self.undo.undo().unwrap();
                self.apply_command(&undo_cmd)?;
                self.generation += 1;
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "edit.undo" }]
                }))
            },
            "edit.redo" => {
                if !self.undo.can_redo() {
                    return Err(CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() });
                }
                let redo_cmd = self.undo.redo().unwrap();
                self.apply_command(&redo_cmd)?;
                self.generation += 1;
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "edit.redo" }]
                }))
            },
            "edit.batch" => {
                let commands = command.params.get("commands")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                if commands.is_empty() {
                    return Err(CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() });
                }

                let mut cmds = Vec::new();
                for sub_cmd in commands {
                    let cmd: Command = serde_json::from_value(sub_cmd.clone())
                        .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                    cmds.push(cmd);
                }

                let changes = self.execute_batch(cmds)?;

                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": changes.iter().map(|c| serde_json::json!({ "name": c.name })).collect::<Vec<_>>()
                }))
            },
            "app.about" => {
                check_no_params(&command.params)?;
                Ok(app_about())
            },
            "app.version" => {
                check_no_params(&command.params)?;
                Ok(app_version())
            },
            "app.status" => {
                check_no_params(&command.params)?;
                Ok(self.app_status())
            },
            "commands.list" => {
                let specs = self.catalog.all();
                Ok(serde_json::json!({
                    "commands": specs
                }))
            },
            "object.create" => {
                let args: ObjectCreateArgs = serde_json::from_value(crate::paint::normalize_fill_param(&command.params)?)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let id = execute_object_create(&mut self.document, args)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let undo_cmd = Command {
                    name: "object.delete".to_string(),
                    params: serde_json::json!({ "id": id.0 }),
                };
                let created = self.document.get_object(id).cloned();
                let redo = Command {
                    name: "object.insert".to_string(),
                    params: serde_json::to_value(ObjectInsertArgs { objects: created.into_iter().collect() }).unwrap_or_default(),
                };

                self.undo.push(crate::undo::Change {
                    name: "object.create".to_string(),
                    redo,
                    undo: undo_cmd,
                });

                self.generation += 1;
                self.document.version += 1;

                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "object.create", "id": id.0 }]
                }))
            },
            "object.update" => {
                let args: ObjectUpdateArgs = serde_json::from_value(crate::paint::normalize_fill_param(&command.params)?)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let id = ObjectId(args.id);
                let old_obj = self.document.get_object(id)
                    .ok_or_else(|| CommandError { code: "NOT_FOUND".to_string(), message_key: "error.not_found".to_string() })?
                    .clone();

                use crate::commands::symbols;
                if symbols::object_update_is_linked(&self.document, &args) {
                    let mut document = self.document.clone();
                    let kept_override = symbols::apply_object_update(&mut document, args, self.symbol_sync)?;
                    return crate::commands::document_change::commit(
                        self, &command.name, document, self.selection.ids.clone(),
                        serde_json::json!({
                            "changes": [{"name": "object.update", "id": id.0}],
                            "warnings": symbols::warnings(kept_override)
                        }),
                    );
                }

                execute_object_update(&mut self.document, args.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let undo_cmd = Command {
                    name: "object.restore".to_string(),
                    params: serde_json::to_value(ObjectRestoreArgs { objects: vec![old_obj] }).unwrap_or_default(),
                };

                self.undo.push(crate::undo::Change {
                    name: "object.update".to_string(),
                    redo: command.clone(),
                    undo: undo_cmd,
                });

                self.generation += 1;
                self.document.version += 1;

                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "object.update", "id": args.id }]
                }))
            },
            "object.delete" => {
                let args: ObjectDeleteArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let id = ObjectId(args.id);
                if self.document.get_object(id).is_none() {
                    return Err(CommandError { code: "NOT_FOUND".to_string(), message_key: "error.not_found".to_string() });
                }
                let before = crate::commands::arrange::snapshot(self);
                execute_object_delete(&mut self.document, args.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                self.selection.ids.retain(|selected| self.document.get_object(*selected).is_some());
                crate::commands::arrange::commit_snapshot(self, "object.delete", before, serde_json::json!({ "changes": [{ "name": "object.delete", "id": args.id }] }))
            },
            "selection.query" => {
                let ids: Vec<u64> = self.selection.ids.iter().map(|id| id.0).collect();
                let mut reply = pixel_selection_summary(&self.document);
                if let Some(map) = reply.as_object_mut() {
                    map.insert("ids".to_string(), serde_json::json!(ids));
                    map.insert("key".to_string(), serde_json::json!(self.selection.key().map(|id| id.0)));
                }
                Ok(reply)
            },
            "doc.units.set" | "doc.scale.set" => {
                let undo_cmd = if command.name == "doc.units.set" {
                    let args: DocUnitsSetArgs = serde_json::from_value(command.params.clone()).map_err(|_| invalid_params_error())?;
                    let old = Command { name: "doc.units.set".to_string(), params: serde_json::json!({ "units": self.document.units }) };
                    execute_doc_units_set(&mut self.document, args)?;
                    old
                } else {
                    let args: DocScaleSetArgs = serde_json::from_value(command.params.clone()).map_err(|_| invalid_params_error())?;
                    let old = Command { name: "doc.scale.set".to_string(), params: serde_json::json!({ "drawing_scale": self.document.drawing_scale }) };
                    execute_doc_scale_set(&mut self.document, args)?;
                    old
                };
                self.undo.push(crate::undo::Change { name: command.name.clone(), redo: command.clone(), undo: undo_cmd });
                self.generation += 1;
                Ok(serde_json::json!({ "doc_version": self.generation, "changes": [{ "name": command.name }], "units": self.document.units, "drawing_scale": self.document.drawing_scale }))
            },
            "selection.rect" => {
                let before = self.document.pixel_selection.clone();
                apply_selection_rect(&mut self.document, &command.params)?;

                let undo_cmd = Command {
                    name: PIXEL_SELECTION_RESTORE.to_string(),
                    params: serde_json::json!({ "selection": before }),
                };

                self.undo.push(crate::undo::Change {
                    name: "selection.rect".to_string(),
                    redo: command.clone(),
                    undo: undo_cmd,
                });

                self.generation += 1;

                let mut reply = pixel_selection_summary(&self.document);
                if let Some(map) = reply.as_object_mut() {
                    map.insert("doc_version".to_string(), serde_json::json!(self.generation));
                    map.insert("changes".to_string(), serde_json::json!([{ "name": "selection.rect" }]));
                }
                Ok(reply)
            },
            "selection.set" => {
                let args: SelectionSetArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let old_selection = self.selection.clone();

                args.apply(&mut self.selection);

                let undo_cmd = Command {
                    name: "selection.set".to_string(),
                    params: serde_json::json!({
                        "ids": old_selection.ids.iter().map(|id| id.0).collect::<Vec<u64>>(),
                        "key_id": old_selection.explicit_key().map(|id| id.0),
                        "op": SelectOp::Replace
                    }),
                };

                self.undo.push(crate::undo::Change {
                    name: "selection.set".to_string(),
                    redo: command.clone(),
                    undo: undo_cmd,
                });

                self.generation += 1;

                let result_ids: Vec<u64> = self.selection.ids.iter().map(|id| id.0).collect();
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "selection.set" }],
                    "ids": result_ids
                }))
            },
            "selection.marquee" => {
                let args: SelectionMarqueeArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                if args.rect[2] <= 0.0 || args.rect[3] <= 0.0 {
                    return Err(CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() });
                }

                let mode = args.mode.unwrap_or(MarqueeMode::Touch);
                let op = args.op.unwrap_or(SelectOp::Replace);

                let old_selection = self.selection.clone();

                self.selection = selection_marquee_with_flags(&self.document, args.rect, mode, op, &old_selection, &self.view.canvas_flags);

                // Redo restores the result even if the view mode changed since the drag.
                let restore = |selection: &Selection| Command::with_params("selection.set", serde_json::json!({
                    "ids": selection.ids,
                    "op": SelectOp::Replace,
                    "key_id": selection.explicit_key().map(|id| id.0),
                }));
                self.undo.push(crate::undo::Change {
                    name: "selection.marquee".to_string(),
                    redo: restore(&self.selection),
                    undo: restore(&old_selection),
                });

                self.generation += 1;

                let result_ids: Vec<u64> = self.selection.ids.iter().map(|id| id.0).collect();
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "selection.marquee" }],
                    "ids": result_ids
                }))
            },
            "selection.hit" => {
                let args: SelectionHitArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let cycle = args.cycle.unwrap_or(false);

                let old_selection = self.selection.clone();

                self.selection = selection_hit(&self.document, args.point, cycle, &old_selection);

                let undo_cmd = Command {
                    name: "selection.set".to_string(),
                    params: serde_json::json!({
                        "ids": old_selection.ids.iter().map(|id| id.0).collect::<Vec<u64>>(),
                        "op": SelectOp::Replace
                    }),
                };

                self.undo.push(crate::undo::Change {
                    name: "selection.hit".to_string(),
                    redo: command.clone(),
                    undo: undo_cmd,
                });

                self.generation += 1;

                let result_ids: Vec<u64> = self.selection.ids.iter().map(|id| id.0).collect();
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "selection.hit" }],
                    "ids": result_ids
                }))
            },
            "selection.enter" => {
                let old_selection = self.selection.clone();

                self.selection = selection_enter(&self.document, &old_selection);

                let undo_cmd = Command {
                    name: "selection.set".to_string(),
                    params: serde_json::json!({
                        "ids": old_selection.ids.iter().map(|id| id.0).collect::<Vec<u64>>(),
                        "op": SelectOp::Replace
                    }),
                };

                self.undo.push(crate::undo::Change {
                    name: "selection.enter".to_string(),
                    redo: command.clone(),
                    undo: undo_cmd,
                });

                self.generation += 1;

                let result_ids: Vec<u64> = self.selection.ids.iter().map(|id| id.0).collect();
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "selection.enter" }],
                    "ids": result_ids
                }))
            },
            "selection.exit" => {
                let old_selection = self.selection.clone();

                self.selection = selection_exit(&self.document, &old_selection);

                let undo_cmd = Command {
                    name: "selection.set".to_string(),
                    params: serde_json::json!({
                        "ids": old_selection.ids.iter().map(|id| id.0).collect::<Vec<u64>>(),
                        "op": SelectOp::Replace
                    }),
                };

                self.undo.push(crate::undo::Change {
                    name: "selection.exit".to_string(),
                    redo: command.clone(),
                    undo: undo_cmd,
                });

                self.generation += 1;

                let result_ids: Vec<u64> = self.selection.ids.iter().map(|id| id.0).collect();
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "selection.exit" }],
                    "ids": result_ids
                }))
            },
            "selection.cycle" => {
                let args: SelectionCycleArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let old_selection = self.selection.clone();

                self.selection = selection_cycle(&self.document, args.direction, &old_selection);

                let undo_cmd = Command {
                    name: "selection.set".to_string(),
                    params: serde_json::json!({
                        "ids": old_selection.ids.iter().map(|id| id.0).collect::<Vec<u64>>(),
                        "op": SelectOp::Replace
                    }),
                };

                self.undo.push(crate::undo::Change {
                    name: "selection.cycle".to_string(),
                    redo: command.clone(),
                    undo: undo_cmd,
                });

                self.generation += 1;

                let result_ids: Vec<u64> = self.selection.ids.iter().map(|id| id.0).collect();
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "selection.cycle" }],
                    "ids": result_ids
                }))
            },
            "transform.origin.set" | "transform.origin.reset" => self.update_transform_origin(command),
            "transform.flip" => {
                let mut args: TransformFlipArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError::invalid_params())?;
                let ids = resolve_target_ids(&self.document, &args.ids, &self.selection)?;
                args.origin = args.origin.or_else(|| self.transform_origin_for(&ids));
                let old_objects = self.snapshot_objects(&transform_target_ids(&self.document, &ids));
                args.ids = Some(ids.iter().map(|id| id.0).collect());
                let redo = Command::with_params(&command.name, serde_json::to_value(&args).unwrap_or_default());
                execute_transform_flip(&mut self.document, args)?;
                self.push_restore_undo(&command.name, redo, old_objects);
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": command.name }]
                }))
            },
            "transform.rotate_90" => {
                let mut args: TransformRotate90Args = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError::invalid_params())?;
                let ids = resolve_target_ids(&self.document, &args.ids, &self.selection)?;
                args.origin = args.origin.or_else(|| self.transform_origin_for(&ids));
                let old_objects = self.snapshot_objects(&transform_target_ids(&self.document, &ids));
                args.ids = Some(ids.iter().map(|id| id.0).collect());
                let redo = Command::with_params(&command.name, serde_json::to_value(&args).unwrap_or_default());
                execute_transform_rotate_90(&mut self.document, args)?;
                self.push_restore_undo(&command.name, redo, old_objects);
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": command.name }]
                }))
            },
            "snap.prefs.query" => {
                let prefs = self.snap_prefs.query();
                serde_json::to_value(prefs)
                    .map_err(|_| CommandError { code: "INTERNAL_ERROR".to_string(), message_key: "error.internal_error".to_string() })
            },
            "snap.prefs.set" => {
                let args: SnapPrefsSetArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                if !args.is_valid() { return Err(CommandError::invalid_params()); }

                self.snap_prefs.set(args)
                    .map_err(|_| CommandError { code: "INTERNAL_ERROR".to_string(), message_key: "error.internal_error".to_string() })?;

                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "snap.prefs.set" }]
                }))
            },
            "ui.key.send" => {
                let args: KeySendArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;

                let tool = args.tool.unwrap_or_else(|| self.active_tool.clone());
                let canonical = crate::context_toolbar::canonical_tool(&tool).unwrap_or("");

                if tool == "move" || canonical == "tool.move" {
                    return Ok(serde_json::json!({
                        "doc_version": self.generation,
                        "changes": []
                    }));
                }

                if tool == "pen" || canonical == "tool.pen" || canonical.starts_with("tool.pen.") {
                    return Err(CommandError { code: "NOT_IMPLEMENTED".to_string(), message_key: "error.not_implemented".to_string() });
                }

                let has_size = matches!(tool.as_str(), "brush" | "eraser" | "shape")
                    || canonical == "tool.paint_brush"
                    || canonical.starts_with("tool.paint.")
                    || canonical == "tool.eraser"
                    || canonical.starts_with("tool.eraser.")
                    || canonical == "tool.shapes"
                    || canonical.starts_with("tool.shape.");

                if !has_size {
                    return Ok(serde_json::json!({
                        "doc_version": self.generation,
                        "changes": []
                    }));
                }

                let key = args.key.as_str();
                
                if key == "]" || key == "[" {
                    let delta = if key == "]" { 1.0 } else { -1.0 };
                    
                    let mut updated_ids = Vec::new();
                    for id in &self.selection.ids {
                        if let Some(obj) = self.document.find_object_mut(id.0) {
                            obj.size[0] += delta;
                            obj.size[1] += delta;
                            if obj.size[0] < 1.0 { obj.size[0] = 1.0; }
                            if obj.size[1] < 1.0 { obj.size[1] = 1.0; }
                            updated_ids.push(id.0);
                        }
                    }

                    if !updated_ids.is_empty() {
                        self.generation += 1;
                        self.document.version += 1;
                        
                        let changes: Vec<serde_json::Value> = updated_ids.iter().map(|id| {
                            serde_json::json!({ "name": "object.update", "id": id })
                        }).collect();

                        return Ok(serde_json::json!({
                            "doc_version": self.generation,
                            "changes": changes
                        }));
                    }
                }

                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": []
                }))
            },
            "transform.selection_box" => {
                let mut args: TransformSelectionBoxArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                let ids = resolve_target_ids(&self.document, &args.ids, &self.selection)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                let affected = transform_target_ids(&self.document, &ids);
                let old_objects = self.snapshot_objects(&affected);
                execute_transform_selection_box(&mut self.document, &ids, &args)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                if !ids.is_empty() && ids == self.last_duplicate_ids {
                    let roots: Vec<_> = old_objects.iter().filter(|object| ids.contains(&object.id)).cloned().collect();
                    let centre = crate::clipboard::box_centre(&roots);
                    self.last_duplicate_steps.push(TransformSelectionBoxArgs { ids: None, origin: [args.origin[0] - centre[0], args.origin[1] - centre[1]], ..args.clone() });
                }
                args.ids = Some(ids.iter().map(|id| id.0).collect());
                let redo = Command {
                    name: "transform.selection_box".to_string(),
                    params: serde_json::to_value(&args).unwrap_or_default(),
                };
                self.push_restore_undo("transform.selection_box", redo, old_objects);
                let changed: Vec<u64> = affected.iter().map(|id| id.0).collect();
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "transform.selection_box", "ids": changed }]
                }))
            },
            "transform.set" => {
                let mut args: TransformSetArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                let ids = resolve_target_ids(&self.document, &args.ids, &self.selection)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                let affected = transform_target_ids(&self.document, &ids);
                let old_objects = self.snapshot_objects(&affected);
                execute_transform_set(&mut self.document, &ids, &args)
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                if let Some(lock) = args.aspect_locked {
                    self.transform_aspect_locked = lock;
                }
                if !ids.is_empty() && ids == self.last_duplicate_ids {
                    if let Some((old, new)) = ids.first().and_then(|id| old_objects.iter().find(|old| old.id == *id))
                        .and_then(|old| self.document.get_object(old.id).map(|new| (old, new))) {
                        let origin = [old.pos[0] + old.size[0].abs() * 0.5, old.pos[1] + old.size[1].abs() * 0.5];
                        let centre = [new.pos[0] + new.size[0].abs() * 0.5, new.pos[1] + new.size[1].abs() * 0.5];
                        let scale = [if old.size[0] == 0.0 { 1.0 } else { new.size[0] / old.size[0] }, if old.size[1] == 0.0 { 1.0 } else { new.size[1] / old.size[1] }];
                        self.last_duplicate_steps.push(TransformSelectionBoxArgs { ids: None, origin: [0.0, 0.0], translate: [centre[0] - origin[0], centre[1] - origin[1]], scale, rotation_deg: new.rotation_deg - old.rotation_deg, shear_deg: new.shear_deg - old.shear_deg, ..Default::default() });
                    }
                }
                args.ids = Some(ids.iter().map(|id| id.0).collect());
                let redo = Command {
                    name: "transform.set".to_string(),
                    params: {
                        let mut params = serde_json::to_value(&args).unwrap_or_default();
                        params["content_bounds"] = true.into();
                        params
                    },
                };
                self.push_restore_undo("transform.set", redo, old_objects);
                let changed: Vec<u64> = affected.iter().map(|id| id.0).collect();
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "transform.set", "ids": changed }]
                }))
            },
            "transform.query" => {
                let args: TransformQueryArgs = serde_json::from_value(command.params.clone())
                    .unwrap_or(TransformQueryArgs { ids: None, anchor: None });
                let ids: Vec<ObjectId> = match &args.ids {
                    Some(list) => list.iter().map(|id| ObjectId(*id)).collect(),
                    None => self.selection.ids.clone(),
                };
                let mut result = transform_query(&self.document, &ids, args.anchor.unwrap_or(Anchor::TopLeft), self.transform_aspect_locked);
                if let Some(point) = self.transform_origin_for(&ids) {
                    result["origin"] = serde_json::json!(point);
                }
                Ok(result)
            },
            "guides.add" => {
                let args: GuidesAddArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                if args.page_id.is_some_and(|page| crate::guides::page_bounds(&self.document, page).is_none()) {
                    return Err(CommandError::not_found());
                }
                let id = self.guides.add_on_page(args.kind, args.position, args.name, args.page_id).map_err(guide_error)?;
                let guide = self.guides.get(id).cloned()
                    .ok_or_else(|| CommandError { code: "NOT_FOUND".to_string(), message_key: "error.not_found".to_string() })?;
                self.push_guide_undo(
                    "guides.add",
                    Command { name: "guides.insert".to_string(), params: serde_json::to_value(GuideStateArgs { guide }).unwrap_or_default() },
                    Command { name: "guides.remove".to_string(), params: serde_json::json!({ "id": id.0 }) },
                );
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "guides.add", "id": id.0 }],
                    "id": id.0
                }))
            },
            "guides.update" => {
                let args: GuidesUpdateArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                let id = GuideId(args.id);
                let old = self.guides.update(id, args.kind, args.position, args.name.clone(), args.locked).map_err(guide_error)?;
                let new = self.guides.get(id).cloned()
                    .ok_or_else(|| CommandError { code: "NOT_FOUND".to_string(), message_key: "error.not_found".to_string() })?;
                self.push_guide_undo(
                    "guides.update",
                    Command { name: "guides.restore".to_string(), params: serde_json::to_value(GuideStateArgs { guide: new }).unwrap_or_default() },
                    Command { name: "guides.restore".to_string(), params: serde_json::to_value(GuideStateArgs { guide: old }).unwrap_or_default() },
                );
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "guides.update", "id": args.id }]
                }))
            },
            "guides.delete" => {
                let args: GuidesDeleteArgs = serde_json::from_value(command.params.clone())
                    .map_err(|_| CommandError { code: "INVALID_PARAMS".to_string(), message_key: "error.invalid_params".to_string() })?;
                let old = self.guides.delete(GuideId(args.id)).map_err(guide_error)?;
                self.push_guide_undo(
                    "guides.delete",
                    Command { name: "guides.remove".to_string(), params: serde_json::json!({ "id": args.id }) },
                    Command { name: "guides.insert".to_string(), params: serde_json::to_value(GuideStateArgs { guide: old }).unwrap_or_default() },
                );
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "guides.delete", "id": args.id }]
                }))
            },
            "guides.find" => {
                let args: GuidesFindArgs = serde_json::from_value(command.params.clone()).map_err(|_| CommandError::invalid_params())?;
                if args.id.is_none() && args.page_id.is_none() && args.name.as_deref().map_or(true, |n| n.trim().is_empty()) {
                    return Err(CommandError::invalid_params());
                }
                let found = self.guides.find_in_scope(args.id, args.name.as_deref(), args.page_id);
                Ok(serde_json::json!({ "count": found.len(), "guides": found }))
            },
            "guides.list" => {                let mut value = serde_json::to_value(&self.guides)
                    .map_err(|_| CommandError { code: "INTERNAL_ERROR".to_string(), message_key: "error.internal_error".to_string() })?;
                value["guides"] = value["items"].clone();
                Ok(value)
            },
            "guides.show" => {
                let args: GuidesShowArgs = serde_json::from_value(command.params.clone()).unwrap_or_default();
                self.guides.set_show(args.show, args.show_columns, args.locked);
                Ok(serde_json::json!({
                    "doc_version": self.generation,
                    "changes": [{ "name": "guides.show" }],
                    "show": self.guides.show,
                    "show_columns": self.guides.show_columns,
                    "locked": self.guides.locked
                }))
            },
            "view.rotate.set" => {
                let degrees = command.params.get("degrees")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(invalid_params_error)? as f32;
                self.view_rotate_set(degrees)
            },
            "view.rotate.reset" => {
                Ok(self.view_rotate_reset())
            },
            "view.pan" => {
                let delta = parse_view_delta(&command.params)?;
                Ok(self.view_pan(delta))
            },
            "noop" | "command.noop" => Ok(serde_json::json!({})),
            "clipboard.query" => {
                let payload = crate::clipboard::clipboard_query(&self.clipboard, &crate::clipboard::OsClipboard::new());
                serde_json::to_value(payload).map_err(|_| CommandError { code: "INTERNAL_ERROR".to_string(), message_key: "error.internal.serialization".to_string() })
            },
            _ => {
                if let Some(result) = crate::table::commands::run(self, command) {
                    return result;
                }
                if self.catalog.resolve(&command.name).is_some() {
                    return Err(CommandError::not_implemented());
                }
                Err(CommandError { code: "UNKNOWN_COMMAND".to_string(), message_key: "error.unknown_command".to_string() })
            }
        }
    }
}


impl Default for Session {
    fn default() -> Self {
        let mut s = Session::new(Document::default());
        s.has_document = false;
        s
    }
}

impl CommandBus for Session {
    fn run(&mut self, command: Command) -> CommandResult {
        Session::run(self, &command)
    }
}

impl Session {
    pub fn actual_zoom(&self) -> f32 {
        let units = self.document.units;
        let Some(dpi) = self.display_dpi.filter(|dpi| dpi.is_finite() && *dpi > 0.0) else {
            return crate::view::actual_zoom(units);
        };
        if units == crate::document::Units::Px {
            return dpi / self.document.dpi.max(1.0);
        }
        crate::units::to_px(1.0, units, dpi).unwrap_or_else(|| crate::view::actual_zoom(units))
    }

    pub fn view_zoom_set(&mut self, args: ViewZoomSetArgs) -> serde_json::Value {
        let pivot = args.around.or_else(|| args.view_size.map(|size| [size[0] * 0.5, size[1] * 0.5]));
        match pivot {
            Some(pivot) => crate::view::zoom_about(&mut self.view, args.zoom, pivot),
            None => self.view.zoom = crate::view::clamp_zoom(args.zoom),
        }
        serde_json::json!({
            "doc_version": self.generation,
            "changes": [{ "name": VIEW_ZOOM_SET }],
            "zoom": self.view.zoom,
            "scroll": self.view.scroll
        })
    }

    pub fn view_zoom_fit(&mut self, view_size: [f32; 2]) -> Result<serde_json::Value, CommandError> {
        self.view_fit_target(view_size, crate::view::FitTarget::Current)
    }

    fn view_fit_target(
        &mut self,
        view_size: [f32; 2],
        target: crate::view::FitTarget,
    ) -> Result<serde_json::Value, CommandError> {
        use crate::view::{FitTarget, center_region, clamp_zoom, fit_zoom, rotated_bounds};

        if !self.has_document {
            return Err(CommandError::no_document());
        }
        let bounds = target.bounds(self, view_size).ok_or_else(|| {
            if target == FitTarget::Selection { CommandError::new("NOTHING_SELECTED") }
            else { CommandError::no_document() }
        })?;
        let size = [bounds[2], bounds[3]];
        let rotation_deg = self.view.rotation_deg;
        let rotated = rotated_bounds(size, rotation_deg);
        self.view.zoom = match target {
            FitTarget::Current => {
                // Leave room for the visible outline stroke at the viewport edge.
                let available = if target.includes_bleed(&self.view) {
                    view_size.map(|side| (side - 4.0).max(side * 0.5))
                } else {
                    view_size
                };
                fit_zoom(size, available, rotation_deg)
            }
            FitTarget::Width => clamp_zoom(view_size[0] / rotated[0].max(f32::EPSILON)),
            FitTarget::Selection | FitTarget::Document => {
                let available = view_size.map(|side| (side - 24.0).max(side * 0.5));
                clamp_zoom((available[0] / rotated[0].max(f32::EPSILON))
                    .min(available[1] / rotated[1].max(f32::EPSILON)))
            }
        };
        center_region(&mut self.view, bounds, view_size);
        Ok(serde_json::json!({
            "doc_version": self.generation,
            "changes": [{ "name": target.command() }],
            "zoom": self.view.zoom,
            "scroll": self.view.scroll,
            "rotation_deg": rotation_deg,
            "bounds": rotated
        }))
    }
}

pub use crate::commands::nudge::{SelectionNudgeArgs, NUDGE_SHIFT_MULTIPLIER, NUDGE_STEP_PX};
