use crate::command::Session;
use crate::object::{Object, ObjectId, ObjectKind};
use crate::selection::Selection;
use crate::warp::{apply_preset, identity_mesh, is_offered_preset, mark_mesh_edited, mesh_is_valid, preset_default_value, preset_slug, preset_value_is_valid, reset_mesh, warp_path, MeshPoint, PathData, WarpGroup, WarpMesh, WarpPreset, OFFERED_PRESETS, PRESET_VALUE_LIMIT};
use dtool_protocol::{Command, CommandError, CommandResult};
use kurbo::{BezPath, Rect};
use serde_json::{json, Map, Value};

pub const GROUP_CREATE: &str = "warp.group.create";
pub const GROUP_QUERY: &str = "warp.group.query";
pub const GROUP_UPDATE: &str = "warp.group.update";
pub const MESH_QUERY: &str = "warp.mesh.query";
pub const RESTORE: &str = "warp.group.restore";
pub const MESH_EDIT: &str = "warp.mesh.edit";
pub const CONVERT_TO_CURVES: &str = "warp.convert_to_curves";
pub const STORE_KEY: &str = "dtool.warp_groups";
pub const DEFAULT_MESH_COLS: u32 = 4;
pub const DEFAULT_MESH_ROWS: u32 = 4;
pub const VALUE_LIMIT: f64 = PRESET_VALUE_LIMIT;
pub const INVALID_PRESET: &str = "INVALID_PRESET";
pub const INVALID_VALUE: &str = "INVALID_VALUE";
pub const WARP_PRESETS: [WarpPreset; 15] = OFFERED_PRESETS;

pub use crate::warp::parse_preset;


pub fn preset_name(preset: &WarpPreset) -> &'static str {
    preset_slug(preset)
}

pub fn default_value(preset: &WarpPreset) -> f64 {
    preset_default_value(preset)
}


fn parse_value(params: &Value, fallback: f64) -> Result<f64, CommandError> {
    match params.get("value") {
        None | Some(Value::Null) => Ok(fallback),
        Some(v) => v.as_f64().filter(|v| preset_value_is_valid(*v)).ok_or_else(|| CommandError::new(INVALID_VALUE)),
    }
}

fn parse_preset_param(params: &Value) -> Result<Option<WarpPreset>, CommandError> {
    match params.get("preset") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) => parse_preset(name).map(Some).ok_or_else(|| CommandError::new(INVALID_PRESET)),
        Some(_) => Err(CommandError::new(INVALID_PRESET)),
    }
}

pub fn mesh_to_json(mesh: &WarpMesh) -> Value {
    let mut value = json!({
        "cols": mesh.cols,
        "rows": mesh.rows,
        "points": mesh.points.iter().map(|p| json!({"id": p.id, "pos": p.pos, "handles": p.handles})).collect::<Vec<_>>(),
    });
    if !mesh.column_knots.is_empty() {
        value["column_knots"] = json!(mesh.column_knots);
    }
    if !mesh.row_knots.is_empty() {
        value["row_knots"] = json!(mesh.row_knots);
    }
    value
}

fn pair(value: &Value) -> Option<[f64; 2]> {
    let a = value.as_array()?;
    let x = a.first()?.as_f64()?;
    let y = a.get(1)?.as_f64()?;
    (a.len() == 2 && x.is_finite() && y.is_finite()).then_some([x, y])
}

pub fn mesh_from_json(value: &Value) -> Option<WarpMesh> {
    let cols = u32::try_from(value.get("cols")?.as_u64()?).ok()?;
    let rows = u32::try_from(value.get("rows")?.as_u64()?).ok()?;
    let mut points = Vec::new();
    for item in value.get("points")?.as_array()? {
        let handles = item.get("handles")?.as_array()?;
        if handles.len() != 4 {
            return None;
        }
        points.push(MeshPoint {
            id: item.get("id")?.as_u64()?,
            pos: pair(item.get("pos")?)?,
            handles: [pair(&handles[0])?, pair(&handles[1])?, pair(&handles[2])?, pair(&handles[3])?],
        });
    }
    let knots = |name| match value.get(name) {
        Some(knots) => knots.as_array()?.iter().map(Value::as_f64).collect(),
        None => Some(Vec::new()),
    };
    let mesh = WarpMesh { cols, rows, points, column_knots: knots("column_knots")?, row_knots: knots("row_knots")? };
    mesh_is_valid(&mesh).then_some(mesh)
}

pub fn bounds_to_json(bounds: Rect) -> Value {
    json!([bounds.x0, bounds.y0, bounds.x1, bounds.y1])
}

pub fn bounds_from_json(value: &Value) -> Option<Rect> {
    let a = value.as_array()?;
    if a.len() != 4 {
        return None;
    }
    let v: Vec<f64> = a.iter().filter_map(Value::as_f64).filter(|v| v.is_finite()).collect();
    (v.len() == 4).then(|| Rect::new(v[0], v[1], v[2], v[3]))
}

pub fn group_to_json(group: &WarpGroup, bounds: Rect) -> Value {
    json!({
        "id": group.id.0,
        "preset": preset_name(&group.preset),
        "value": group.value,
        "muted": group.muted,
        "children": group.children.iter().map(|c| c.0).collect::<Vec<_>>(),
        "bounds": bounds_to_json(bounds),
        "mesh": mesh_to_json(&group.mesh),
    })
}

pub fn group_from_json(value: &Value) -> Option<(WarpGroup, Rect)> {
    let group = WarpGroup {
        id: ObjectId(value.get("id")?.as_u64()?),
        preset: parse_preset(value.get("preset")?.as_str()?)?,
        value: value.get("value")?.as_f64().filter(|v| v.is_finite())?,
        mesh: mesh_from_json(value.get("mesh")?)?,
        muted: value.get("muted").and_then(Value::as_bool).unwrap_or(false),
        children: value
            .get("children")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_u64).map(ObjectId).collect())
            .unwrap_or_default(),
    };
    let bounds = bounds_from_json(value.get("bounds")?)?;
    Some((group, bounds))
}

fn store(doc: &crate::document::Document) -> Map<String, Value> {
    doc.export_defaults.get(STORE_KEY).and_then(Value::as_object).cloned().unwrap_or_default()
}

pub fn warp_group(session: &Session, id: ObjectId) -> Option<(WarpGroup, Rect)> {
    let entry = session.document.export_defaults.get(STORE_KEY)?.get(id.0.to_string())?;
    let (mut group, bounds) = group_from_json(entry)?;
    session.document.objects.iter().find(|o| o.id == id && kind_is_warp_group(&o.kind))?;
    group.children = session.document.objects.iter().filter(|o| o.parent_id == Some(id)).map(|o| o.id).collect();
    Some((group, bounds))
}

pub fn warp_groups(session: &Session) -> Vec<(WarpGroup, Rect)> {
    let mut ids: Vec<u64> = store(&session.document).keys().filter_map(|k| k.parse().ok()).collect();
    ids.sort_unstable();
    ids.into_iter().filter_map(|id| warp_group(session, ObjectId(id))).collect()
}

pub fn is_warp_group(session: &Session, id: ObjectId) -> bool {
    warp_group(session, id).is_some()
}

pub fn live_warp_points(doc: &crate::Document, object_id: ObjectId, points: &[[f32; 2]]) -> Option<Vec<[f32; 2]>> {
    let warps = LiveWarp::for_object(doc, object_id)?;
    Some(points.iter().map(|point| warps.point(point.map(f64::from)).map(|value| value as f32)).collect())
}

/// Recover layout coordinates from a pointer over a warped object.
pub fn unwarp_point(doc: &crate::Document, object_id: ObjectId, point: [f32; 2]) -> [f32; 2] {
    let Some(warp) = LiveWarp::for_object(doc, object_id) else { return point };
    let target = point.map(f64::from);
    let mut estimate = target;
    let h = 0.01;
    for _ in 0..12 {
        let centre = warp.point(estimate);
        let [ex, ey] = [centre[0] - target[0], centre[1] - target[1]];
        if ex.hypot(ey) <= h {
            return estimate.map(|value| value as f32);
        }
        let [x, y] = estimate;
        let dx = warp.point([x + h, y]);
        let dy = warp.point([x, y + h]);
        let [a, b] = [(dx[0] - centre[0]) / h, (dx[1] - centre[1]) / h];
        let [c, d] = [(dy[0] - centre[0]) / h, (dy[1] - centre[1]) / h];
        let det = a * d - b * c;
        if !det.is_finite() || det.abs() < 1e-8 { break; }
        estimate = [x - (d * ex - c * ey) / det, y - (a * ey - b * ex) / det];
    }
    point
}

/// Deform every contour in a world-space outline, including the closing edges.
pub fn live_warp_outline(doc: &crate::Document, object_id: ObjectId, outline: &BezPath) -> Option<BezPath> {
    Some(LiveWarp::for_object(doc, object_id)?.outline(outline))
}

pub(crate) struct LiveWarp(Vec<(WarpGroup, Rect)>);

pub(crate) fn ancestor_warps(doc: &crate::Document, object: &Object) -> Vec<(WarpGroup, Rect)> {
    let Some(groups) = doc.export_defaults.get(STORE_KEY).and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut warps = Vec::new();
    let mut parent = object.parent_id;
    for _ in 0..doc.objects.len() {
        let Some(id) = parent else { return warps };
        let Some(owner) = doc.get_object(id) else { return Vec::new() };
        if !object.baked_warps.contains(&id)
            && let Some((group, bounds)) = groups.get(&id.0.to_string()).and_then(group_from_json)
        {
            warps.push((group, bounds));
        }
        parent = owner.parent_id;
    }
    Vec::new()
}

impl LiveWarp {
    pub(crate) fn for_object(doc: &crate::Document, object_id: ObjectId) -> Option<Self> {
        let object = doc.get_object(object_id)?;
        let mut warps = ancestor_warps(doc, object);
        warps.retain(|(group, _)| !group.muted);
        (!warps.is_empty()).then_some(Self(warps))
    }

    pub(crate) fn point(&self, mut point: [f64; 2]) -> [f64; 2] {
        for (group, bounds) in &self.0 {
            point = crate::warp::evaluate(&group.mesh, *bounds, point);
        }
        point
    }

    pub(crate) fn outline(&self, outline: &BezPath) -> BezPath {
        let step = self.0.iter().map(|(_, bounds)| {
            bounds.width().abs().min(bounds.height().abs()) / 32.0
        }).fold(f64::INFINITY, f64::min).max(0.25);
        let map = |point| {
            let [x, y] = self.point(point);
            (x, y)
        };
        let mut result = BezPath::new();
        for (points, closed) in crate::outline::flatten_subpaths(outline, crate::outline::OUTLINE_TOLERANCE) {
            result.move_to(map(points[0]));
            let end = points.iter().copied().skip(1).chain(closed.then_some(points[0]));
            for ([x, y], [right, bottom]) in points.iter().copied().zip(end) {
                let [dx, dy] = [right - x, bottom - y];
                let parts = (dx.hypot(dy) / step).ceil().clamp(1.0, 128.0) as u32;
                for index in 1..=parts {
                    let t = f64::from(index) / f64::from(parts);
                    result.line_to(map([x + dx * t, y + dy * t]));
                }
            }
            if closed {
                result.close_path();
            }
        }
        result
    }
}

fn put_group(doc: &mut crate::document::Document, group: &WarpGroup, bounds: Rect) {
    let mut map = store(doc);
    map.insert(group.id.0.to_string(), group_to_json(group, bounds));
    if !doc.export_defaults.is_object() {
        doc.export_defaults = json!({});
    }
    doc.export_defaults[STORE_KEY] = Value::Object(map);
}


pub fn warp_group_kind() -> ObjectKind {
    serde_json::from_value(json!("WarpGroup")).unwrap_or(ObjectKind::Group)
}

pub fn kind_is_warp_group(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Group) || std::mem::discriminant(kind) == std::mem::discriminant(&warp_group_kind())
}

pub fn migrate_sidecar_kinds(doc: &mut crate::document::Document) {
    let kind = warp_group_kind();
    if matches!(kind, ObjectKind::Group) {
        return;
    }
    let ids: Vec<u64> = store(doc).keys().filter_map(|k| k.parse().ok()).collect();
    for id in ids {
        let object_id = ObjectId(id);
        let Some(object) = doc.objects.iter_mut().find(|object| object.id == object_id) else {
            continue;
        };
        if matches!(object.kind, ObjectKind::Group) {
            object.kind = kind.clone();
        }
    }
}

pub fn hydrate_sidecar(doc: &mut crate::document::Document) {
    let Some(raw) = doc.export_defaults.get(STORE_KEY).cloned() else {
        return;
    };
    let Some(obj) = raw.as_object() else {
        return;
    };
    let mut next = Map::new();
    let mut changed = false;
    for (key, value) in obj {
        if let Some(name) = value.get("preset").and_then(Value::as_str) {
            if let Some(preset) = parse_preset(name) {
                let slug = preset_name(&preset);
                if name != slug {
                    let mut entry = value.clone();
                    entry["preset"] = json!(slug);
                    next.insert(key.clone(), entry);
                    changed = true;
                    continue;
                }
            }
        }
        next.insert(key.clone(), value.clone());
    }
    if changed {
        if !doc.export_defaults.is_object() {
            doc.export_defaults = json!({});
        }
        doc.export_defaults[STORE_KEY] = Value::Object(next);
    }
    migrate_sidecar_kinds(doc);
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        GROUP_CREATE => Some(session.has_document && plan_create(session, &command.params).is_ok()),
        GROUP_UPDATE => Some(session.has_document && plan_update(session, &command.params).is_ok()),
        GROUP_QUERY | MESH_QUERY => Some(session.has_document),
        MESH_EDIT => Some(session.has_document && plan_mesh_edit(session, &command.params).is_ok()),
        CONVERT_TO_CURVES => Some(session.has_document && plan_bake(session, &command.params).is_ok()),
        RESTORE => Some(false),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        GROUP_CREATE => Some(create(session, &command.params)),
        GROUP_UPDATE => Some(update(session, &command.params)),
        GROUP_QUERY => Some(query(session, &command.params)),
        MESH_QUERY => Some(mesh_query(session, &command.params)),
        MESH_EDIT => Some(mesh_edit(session, &command.params)),
        CONVERT_TO_CURVES => Some(bake(session, &command.params)),
        RESTORE => Some(Err(CommandError::invalid_params())),
        _ => None,
    }
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if command.name != RESTORE {
        return None;
    }
    Some(restore(session, &command.params))
}

fn snapshot(session: &Session) -> Value {
    json!({
        "objects": session.document.objects,
        "selection": session.selection,
        "next_object_id": session.document.next_object_id,
        "warp": store(&session.document),
    })
}

fn restore(session: &mut Session, params: &Value) -> Result<(), CommandError> {
    let objects: Vec<Object> = serde_json::from_value(params["objects"].clone()).map_err(|_| CommandError::invalid_params())?;
    let selection: Selection = serde_json::from_value(params["selection"].clone()).map_err(|_| CommandError::invalid_params())?;
    let next = params["next_object_id"].as_u64().ok_or_else(CommandError::invalid_params)?;
    let warp = params["warp"].as_object().cloned().ok_or_else(CommandError::invalid_params)?;
    let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    session.document.objects = objects;
    session.document.next_object_id = next;
    session.selection = selection;
    if !session.document.export_defaults.is_object() {
        session.document.export_defaults = json!({});
    }
    session.document.export_defaults[STORE_KEY] = Value::Object(warp);
    session.document.version = version;
    Ok(())
}

struct CreatePlan {
    preset: WarpPreset,
    value: f64,
    nodes: Vec<ObjectId>,
    parent: Option<ObjectId>,
    insert_at: usize,
    bounds: Rect,
}

fn node_ids(session: &Session, params: &Value) -> Result<Vec<ObjectId>, CommandError> {
    let ids: Vec<ObjectId> = match params.get("nodes") {
        Some(Value::Array(items)) => {
            let mut out = Vec::new();
            for item in items {
                out.push(ObjectId(item.as_u64().ok_or_else(CommandError::invalid_params)?));
            }
            out
        }
        Some(_) => return Err(CommandError::invalid_params()),
        None => session.selection.ids.clone(),
    };
    let mut unique: Vec<ObjectId> = Vec::new();
    for id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    if unique.is_empty() {
        return Err(CommandError::new("NOTHING_SELECTED"));
    }
    Ok(unique)
}

fn has_ancestor_in(objects: &[Object], id: ObjectId, set: &[ObjectId]) -> bool {
    let mut parent = objects.iter().find(|o| o.id == id).and_then(|o| o.parent_id);
    for _ in 0..objects.len() {
        let Some(p) = parent else {
            return false;
        };
        if set.contains(&p) {
            return true;
        }
        parent = objects.iter().find(|o| o.id == p).and_then(|o| o.parent_id);
    }
    false
}

fn plan_create(session: &Session, params: &Value) -> Result<CreatePlan, CommandError> {
    let preset = parse_preset_param(params)?.unwrap_or(WarpPreset::Mesh);
    if !is_offered_preset(preset) {
        return Err(CommandError::new(INVALID_PRESET));
    }
    let value = parse_value(params, default_value(&preset))?;
    let requested = node_ids(session, params)?;
    let objects = &session.document.objects;
    for id in &requested {
        if !objects.iter().any(|o| o.id == *id) {
            return Err(CommandError::new("NOT_FOUND"));
        }
    }
    let nodes: Vec<ObjectId> = requested.iter().copied().filter(|id| !has_ancestor_in(objects, *id, &requested)).collect();
    let top = nodes
        .iter()
        .filter_map(|id| objects.iter().position(|o| o.id == *id))
        .max()
        .ok_or_else(|| CommandError::new("NOT_FOUND"))?;
    let parent = objects[top].parent_id;
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for object in objects.iter().filter(|o| nodes.contains(&o.id)) {
        let ax = f64::from(object.pos[0]);
        let ay = f64::from(object.pos[1]);
        let bx = ax + f64::from(object.size[0]);
        let by = ay + f64::from(object.size[1]);
        x0 = x0.min(ax.min(bx));
        y0 = y0.min(ay.min(by));
        x1 = x1.max(ax.max(bx));
        y1 = y1.max(ay.max(by));
    }
    let bounds = Rect::new(x0, y0, x1, y1);
    if ![bounds.x0, bounds.y0, bounds.x1, bounds.y1].iter().all(|v| v.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    Ok(CreatePlan { preset, value, nodes, parent, insert_at: top + 1, bounds })
}

fn create(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let plan = plan_create(session, params)?;
    let undo = Command::with_params(RESTORE, snapshot(session));
    let maximum = session.document.objects.iter().map(|o| o.id.0).max().unwrap_or(0);
    let id = session.document.next_object_id.max(maximum.checked_add(1).ok_or_else(CommandError::invalid_params)?);
    let next = id.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let generation = session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let group_id = ObjectId(id);
    let group_object = Object {
        id: group_id,
        name: params.get("name").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| dtool_strings::text("menu.vector.new_warp_group").to_owned()),
        pos: [plan.bounds.x0 as f32, plan.bounds.y0 as f32],
        size: [plan.bounds.width() as f32, plan.bounds.height() as f32],
        parent_id: plan.parent,
        kind: warp_group_kind(),
        ..Object::default()
    };
    let mut objects = session.document.objects.clone();
    for object in objects.iter_mut().filter(|o| plan.nodes.contains(&o.id)) {
        object.parent_id = Some(group_id);
    }
    objects.insert(plan.insert_at.min(objects.len()), group_object);
    let mut group = WarpGroup {
        id: group_id,
        preset: WarpPreset::Mesh,
        value: 0.0,
        mesh: identity_mesh(DEFAULT_MESH_COLS, DEFAULT_MESH_ROWS, plan.bounds),
        muted: false,
        children: plan.nodes.clone(),
    };
    apply_preset(&mut group, plan.preset.clone(), plan.value, plan.bounds);
    session.document.objects = objects;
    session.document.next_object_id = next;
    session.selection = Selection::from_ids(vec![group_id]);
    put_group(&mut session.document, &group, plan.bounds);
    session.generation = generation;
    session.document.version = version;
    let redo = Command::with_params(RESTORE, snapshot(session));
    session.undo.push(crate::undo::Change { name: GROUP_CREATE.to_string(), undo, redo });
    let mut out = group_to_json(&group, plan.bounds);
    out["group_id"] = json!(id);
    out["doc_version"] = json!(generation);
    Ok(out)
}

struct UpdatePlan {
    group: WarpGroup,
    bounds: Rect,
    changed: bool,
}

fn plan_update(session: &Session, params: &Value) -> Result<UpdatePlan, CommandError> {
    let preset = parse_preset_param(params)?;
    let reset = match params.get("reset") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(CommandError::invalid_params()),
    };
    let has_value = params.get("value").is_some_and(|v| !v.is_null());
    let muted = match params.get("muted") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(flag)) => Some(*flag),
        Some(_) => return Err(CommandError::invalid_params()),
    };
    if preset.is_none() && !has_value && !reset && muted.is_none() {
        return Err(CommandError::invalid_params());
    }
    let id = target_group(session, params)?.ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    let (current, bounds) = warp_group(session, id).ok_or_else(|| CommandError::new("NOT_FOUND"))?;
    let next_preset = preset.unwrap_or_else(|| current.preset.clone());
    let fallback = if next_preset == current.preset { current.value } else { default_value(&next_preset) };
    let value = parse_value(params, fallback)?;
    let mut group = current.clone();
    if reset {
        reset_mesh(&mut group, bounds);
    }
    apply_preset(&mut group, next_preset, value, bounds);
    if let Some(flag) = muted {
        group.muted = flag;
    }
    let changed = group_to_json(&group, bounds) != group_to_json(&current, bounds);
    Ok(UpdatePlan { group, bounds, changed })
}

fn update(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let plan = plan_update(session, params)?;
    if !plan.changed {
        let mut out = group_to_json(&plan.group, plan.bounds);
        out["group_id"] = json!(plan.group.id.0);
        out["changed"] = json!(false);
        out["doc_version"] = json!(session.generation);
        return Ok(out);
    }
    let undo = Command::with_params(RESTORE, snapshot(session));
    let generation = session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    put_group(&mut session.document, &plan.group, plan.bounds);
    session.generation = generation;
    session.document.version = version;
    let redo = Command::with_params(RESTORE, snapshot(session));
    session.undo.push(crate::undo::Change { name: GROUP_UPDATE.to_string(), undo, redo });
    let mut out = group_to_json(&plan.group, plan.bounds);
    out["group_id"] = json!(plan.group.id.0);
    out["changed"] = json!(true);
    out["doc_version"] = json!(generation);
    Ok(out)
}

fn point2(value: &Value) -> Result<[f64; 2], CommandError> {
    pair(value).filter(|p| p[0].is_finite() && p[1].is_finite()).ok_or_else(CommandError::invalid_params)
}

fn next_mesh_id(mesh: &WarpMesh) -> u64 {
    mesh.points.iter().map(|point| point.id).max().unwrap_or(0).saturating_add(1)
}

fn grid_index(mesh: &WarpMesh, id: u64) -> Result<(usize, usize), CommandError> {
    let cols = mesh.cols as usize;
    let index = mesh.points.iter().position(|point| point.id == id).ok_or_else(|| CommandError::new("NOT_FOUND"))?;
    Ok((index / cols, index % cols))
}

fn insert_column(mesh: &mut WarpMesh, at: usize) -> Result<(), CommandError> {
    use crate::warp::{edge_control, HANDLE_DOWN, HANDLE_LEFT, HANDLE_RIGHT, HANDLE_UP};
    use kurbo::{CubicBez, ParamCurve, Point};

    let cols = mesh.cols as usize;
    let rows = mesh.rows as usize;
    if at == 0 || at >= cols {
        return Err(CommandError::invalid_params());
    }
    let point = |[x, y]: [f64; 2]| Point::new(x, y);
    let coords = |p: Point| [p.x, p.y];
    let mut columns = Vec::with_capacity(rows);
    for row in 0..rows {
        let i = row * cols + at;
        let left = &mesh.points[i - 1];
        let right = &mesh.points[i];
        let edge = CubicBez::new(point(left.pos), point(edge_control(left, HANDLE_RIGHT, right)),
            point(edge_control(right, HANDLE_LEFT, left)), point(right.pos));
        columns.push((edge.subsegment(0.0..0.5), edge.subsegment(0.5..1.0)));
    }

    let mut next_id = next_mesh_id(mesh);
    let mut inserted = Vec::with_capacity(rows);
    for (left, right) in &columns {
        let pos = coords(left.p3);
        let mut handles = [pos; 4];
        handles[HANDLE_LEFT] = coords(left.p2);
        handles[HANDLE_RIGHT] = coords(right.p1);
        inserted.push(MeshPoint { id: next_id, pos, handles });
        next_id = next_id.saturating_add(1);
    }
    // The new interior edge is the old patch at s = 1/2. The Coons correction
    // to the average of the side curves is linear along that edge.
    for row in 0..rows - 1 {
        let i = row * cols + at;
        let [tl, tr, bl, br] = [i - 1, i, i + cols - 1, i + cols].map(|index| &mesh.points[index]);
        let top = point(inserted[row].pos) - point(tl.pos).midpoint(point(tr.pos));
        let bottom = point(inserted[row + 1].pos) - point(bl.pos).midpoint(point(br.pos));
        let down = point(edge_control(tl, HANDLE_DOWN, bl)).midpoint(point(edge_control(tr, HANDLE_DOWN, br)));
        let up = point(edge_control(bl, HANDLE_UP, tl)).midpoint(point(edge_control(br, HANDLE_UP, tr)));
        inserted[row].handles[HANDLE_DOWN] = coords(down + top.lerp(bottom, 1.0 / 3.0));
        inserted[row + 1].handles[HANDLE_UP] = coords(up + top.lerp(bottom, 2.0 / 3.0));
    }

    let mut points = Vec::with_capacity(rows * (cols + 1));
    for (row, ((left, right), junction)) in columns.into_iter().zip(inserted).enumerate() {
        let start = row * cols;
        points.extend_from_slice(&mesh.points[start..start + at]);
        points.last_mut().unwrap().handles[HANDLE_RIGHT] = coords(left.p1);
        points.push(junction);
        let next = points.len();
        points.extend_from_slice(&mesh.points[start + at..start + cols]);
        points[next].handles[HANDLE_LEFT] = coords(right.p2);
    }
    if mesh.column_knots.is_empty() {
        mesh.column_knots = (0..cols).map(|col| col as f64 / (cols - 1) as f64).collect();
    }
    let knot = (mesh.column_knots[at - 1] + mesh.column_knots[at]) * 0.5;
    mesh.column_knots.insert(at, knot);
    mesh.cols += 1;
    mesh.points = points;
    Ok(())
}

fn transpose_mesh(mesh: &mut WarpMesh) {
    use crate::warp::{HANDLE_DOWN, HANDLE_LEFT, HANDLE_RIGHT, HANDLE_UP};
    let cols = mesh.cols as usize;
    let rows = mesh.rows as usize;
    let mut points = Vec::with_capacity(cols * rows);
    for col in 0..cols {
        for row in 0..rows {
            let mut point = mesh.points[row * cols + col].clone();
            point.handles.swap(HANDLE_LEFT, HANDLE_UP);
            point.handles.swap(HANDLE_RIGHT, HANDLE_DOWN);
            points.push(point);
        }
    }
    mesh.points = points;
    std::mem::swap(&mut mesh.cols, &mut mesh.rows);
    std::mem::swap(&mut mesh.column_knots, &mut mesh.row_knots);
}

fn insert_row(mesh: &mut WarpMesh, at: usize) -> Result<(), CommandError> {
    transpose_mesh(mesh);
    let result = insert_column(mesh, at);
    transpose_mesh(mesh);
    result
}

fn append_column(mesh: &mut WarpMesh) -> Result<(), CommandError> {
    let cols = mesh.cols as usize;
    let rows = mesh.rows as usize;
    if cols < 2 {
        return Err(CommandError::invalid_params());
    }
    let mut next_id = next_mesh_id(mesh);
    let mut points = Vec::with_capacity(rows * (cols + 1));
    for row in 0..rows {
        let end = (row + 1) * cols;
        points.extend_from_slice(&mesh.points[row * cols..end]);
        let left = mesh.points[end - 1].pos;
        let prev = mesh.points[end - 2].pos;
        let pos = [left[0] * 2.0 - prev[0], left[1] * 2.0 - prev[1]];
        points.push(MeshPoint { id: next_id, pos, handles: [pos; 4] });
        next_id = next_id.saturating_add(1);
    }
    if !mesh.column_knots.is_empty() {
        let end = 2.0 - mesh.column_knots[cols - 2];
        for knot in &mut mesh.column_knots {
            *knot /= end;
        }
        mesh.column_knots.push(1.0);
    }
    mesh.cols += 1;
    mesh.points = points;
    Ok(())
}

fn add_junction(mesh: &mut WarpMesh, a: u64, b: u64) -> Result<u64, CommandError> {
    let (row_a, col_a) = grid_index(mesh, a)?;
    let (row_b, col_b) = grid_index(mesh, b)?;
    if row_a == row_b && (col_a as isize - col_b as isize).abs() == 1 {
        let at = col_a.max(col_b);
        insert_column(mesh, at)?;
        let cols = mesh.cols as usize;
        return Ok(mesh.points[row_a * cols + at].id);
    }
    if col_a == col_b && (row_a as isize - row_b as isize).abs() == 1 {
        let at = row_a.max(row_b);
        insert_row(mesh, at)?;
        let cols = mesh.cols as usize;
        return Ok(mesh.points[at * cols + col_a].id);
    }
    Err(CommandError::invalid_params())
}

fn add_patch(mesh: &mut WarpMesh) -> Result<(), CommandError> {
    append_column(mesh)
}

fn plan_mesh_edit(session: &Session, params: &Value) -> Result<(WarpGroup, Rect, usize), CommandError> {
    let ops = params.get("ops").and_then(Value::as_array).filter(|ops| !ops.is_empty()).ok_or_else(CommandError::invalid_params)?;
    let id = target_group(session, params)?.ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    let (mut group, bounds) = warp_group(session, id).ok_or_else(|| CommandError::new("NOT_FOUND"))?;
    for op in ops {
        if let Some(edge) = op.get("add_junction") {
            let a = edge.get("a").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?;
            let b = edge.get("b").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?;
            add_junction(&mut group.mesh, a, b)?;
            continue;
        }
        if op.get("add_patch").and_then(Value::as_bool) == Some(true) {
            add_patch(&mut group.mesh)?;
            continue;
        }
        let point = op.get("point").and_then(Value::as_u64).ok_or_else(CommandError::invalid_params)?;
        let target = group.mesh.points.iter_mut().find(|p| p.id == point).ok_or_else(|| CommandError::new("NOT_FOUND"))?;
        let mut touched = false;
        if let Some(pos) = op.get("pos") {
            target.pos = point2(pos)?;
            touched = true;
        }
        if let Some(handles) = op.get("handles") {
            let list = handles.as_array().filter(|list| list.len() == 4).ok_or_else(CommandError::invalid_params)?;
            for (slot, value) in target.handles.iter_mut().zip(list) {
                *slot = point2(value)?;
            }
            touched = true;
        }
        if !touched {
            return Err(CommandError::invalid_params());
        }
    }
    if !mesh_is_valid(&group.mesh) {
        return Err(CommandError::invalid_params());
    }
    mark_mesh_edited(&mut group);
    Ok((group, bounds, ops.len()))
}

const MESH_DRAG_KEY: &str = "dtool.warp_mesh_drag";

fn mesh_edit(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    if params.get("cancel_drag").and_then(Value::as_bool) == Some(true) {
        if let Some(saved) = session.document.export_defaults.get(MESH_DRAG_KEY).cloned() {
            let version = session.document.version;
            restore(session, &saved)?;
            session.document.version = version;
            if let Some(object) = session.document.export_defaults.as_object_mut() {
                object.remove(MESH_DRAG_KEY);
            }
        }
        return Ok(json!({ "ok": true, "cancelled": true, "doc_version": session.generation }));
    }
    let commit = params.get("commit").and_then(Value::as_bool).unwrap_or(true);
    if !commit && session.document.export_defaults.get(MESH_DRAG_KEY).is_none() {
        let snap = snapshot(session);
        if !session.document.export_defaults.is_object() {
            session.document.export_defaults = json!({});
        }
        session.document.export_defaults.as_object_mut().unwrap().insert(MESH_DRAG_KEY.to_string(), snap);
    }
    let (group, bounds, applied) = plan_mesh_edit(session, params)?;
    if !commit {
        put_group(&mut session.document, &group, bounds);
        let mut out = group_to_json(&group, bounds);
        out["group_id"] = json!(group.id.0);
        out["applied_ops"] = json!(applied);
        out["committed"] = json!(false);
        out["doc_version"] = json!(session.generation);
        return Ok(out);
    }
    let undo_state = session.document.export_defaults.get(MESH_DRAG_KEY).cloned().unwrap_or_else(|| snapshot(session));
    if let Some(object) = session.document.export_defaults.as_object_mut() {
        object.remove(MESH_DRAG_KEY);
    }
    let undo = Command::with_params(RESTORE, undo_state);
    let generation = session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    put_group(&mut session.document, &group, bounds);
    session.generation = generation;
    session.document.version = version;
    let redo = Command::with_params(RESTORE, snapshot(session));
    session.undo.push(crate::undo::Change { name: MESH_EDIT.to_string(), undo, redo });
    let mut out = group_to_json(&group, bounds);
    out["group_id"] = json!(group.id.0);
    out["applied_ops"] = json!(applied);
    out["doc_version"] = json!(generation);
    Ok(out)
}

struct BakePlan {
    group: ObjectId,
    baked: Vec<Object>,
    skipped: Vec<u64>,
}

fn plan_bake(session: &Session, params: &Value) -> Result<BakePlan, CommandError> {
    let id = target_group(session, params)?.ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    let (group, bounds) = warp_group(session, id).ok_or_else(|| CommandError::new("NOT_FOUND"))?;
    let mut baked = Vec::new();
    let mut skipped = Vec::new();
    for child in &group.children {
        let Some(object) = session.document.get_object(*child) else {
            continue;
        };
        let Some(path) = crate::outline::object_outline_world(object) else {
            skipped.push(child.0);
            continue;
        };
        if object.baked_warps.contains(&id) {
            let mut next = object.clone();
            next.baked_warps.retain(|&warp| warp != id);
            baked.push(next);
            continue;
        }
        let contours: Vec<(Vec<[f64; 2]>, bool)> = crate::outline::flatten_subpaths(&path, 0.25)
            .into_iter()
            .map(|(points, closed)| if group.muted { (points, closed) } else { (warp_path(&group.mesh, bounds, &PathData { points }).points, closed) })
            .collect();
        let all: Vec<[f64; 2]> = contours.iter().flat_map(|(points, _)| points.iter().copied()).collect();
        if all.is_empty() || all.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) {
            skipped.push(child.0);
            continue;
        }
        let x0 = all.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        let y0 = all.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        let x1 = all.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        let y1 = all.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
        let mut built = contours.into_iter().map(|(points, closed)| crate::curve::Contour {
            points: points.iter().enumerate().map(|(i, p)| crate::curve::PathPoint::sharp(crate::curve::PointId(i as u64 + 1), [p[0] - x0, p[1] - y0])).collect(),
            closed,
        });
        let contour = built.next().unwrap_or_default();
        let holes: Vec<crate::curve::Contour> = built.collect();
        let mut next = object.clone();
        next.kind = ObjectKind::Curve(crate::curve::Curve::from_contour_and_holes(contour, holes).with_source(child.0));
        next.shape = None;
        next.pos = [x0 as f32, y0 as f32];
        next.size = [(x1 - x0) as f32, (y1 - y0) as f32];
        next.rotation_deg = 0.0;
        next.shear_deg = 0.0;
        baked.push(next);
    }
    if baked.is_empty() {
        return Err(CommandError::new("NOTHING_TO_CONVERT"));
    }
    Ok(BakePlan { group: id, baked, skipped })
}

fn drop_group(doc: &mut crate::document::Document, id: ObjectId) {
    let mut map = store(doc);
    map.remove(&id.0.to_string());
    if !doc.export_defaults.is_object() {
        doc.export_defaults = json!({});
    }
    doc.export_defaults[STORE_KEY] = Value::Object(map);
}

fn bake(session: &mut Session, params: &Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let plan = plan_bake(session, params)?;
    let undo = Command::with_params(RESTORE, snapshot(session));
    let generation = session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let converted: Vec<u64> = plan.baked.iter().map(|o| o.id.0).collect();
    for next in plan.baked {
        if let Some(slot) = session.document.objects.iter_mut().find(|o| o.id == next.id) {
            *slot = next;
        }
    }
    drop_group(&mut session.document, plan.group);
    session.generation = generation;
    session.document.version = version;
    let redo = Command::with_params(RESTORE, snapshot(session));
    session.undo.push(crate::undo::Change { name: CONVERT_TO_CURVES.to_string(), undo, redo });
    Ok(json!({ "group_id": plan.group.0, "converted": converted, "skipped": plan.skipped, "doc_version": generation }))
}

#[cfg(test)]
mod bake_contracts {
    use super::*;
    use crate::document::{Document, Units};

    #[test]
    fn bake_warps_children_into_curves_and_drops_warp() {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let id = session.document.create_object(Object { pos: [0.0, 0.0], size: [100.0, 100.0], ..Object::default() });
        let created = session.run(&Command::with_params(GROUP_CREATE, json!({ "nodes": [id.0] }))).unwrap();
        let group = ObjectId(created["group_id"].as_u64().unwrap());
        let mesh = session.run(&Command::new(MESH_QUERY)).unwrap();
        let corner = mesh["mesh"]["points"][0]["id"].as_u64().unwrap();
        session.run(&Command::with_params(MESH_EDIT, json!({ "ops": [{ "point": corner, "pos": [-20.0, -20.0] }] }))).unwrap();
        let reply = session.run(&Command::with_params(CONVERT_TO_CURVES, json!({ "group": group.0 }))).unwrap();
        assert_eq!(reply["converted"], json!([id.0]));
        let object = session.document.get_object(id).unwrap();
        assert!(object.is_curve());
        assert!(object.pos[0] < -1.0 && object.pos[1] < -1.0);
        assert!(!is_warp_group(&session, group));
        assert!(session.document.get_object(group).is_some());
        session.run(&Command::new("edit.undo")).unwrap();
        assert!(is_warp_group(&session, group));
        assert!(matches!(session.document.get_object(id).unwrap().kind, ObjectKind::Shape));
    }
}

#[cfg(test)]
mod mesh_edit_contracts {
    use super::*;
    use crate::document::{Document, Units};

    #[test]
    fn mesh_edit_moves_point_in_one_undo_step() {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let id = session.document.create_object(Object { pos: [10.0, 10.0], size: [100.0, 50.0], ..Object::default() });
        session.run(&Command::with_params(GROUP_CREATE, json!({ "nodes": [id.0] }))).unwrap();
        let before = session.run(&Command::new(MESH_QUERY)).unwrap();
        let point = before["mesh"]["points"][0]["id"].as_u64().unwrap();
        let generation = session.generation;
        let reply = session.run(&Command::with_params(MESH_EDIT, json!({ "ops": [{ "point": point, "pos": [1.0, 2.0] }] }))).unwrap();
        assert_eq!(reply["applied_ops"], json!(1));
        assert_eq!(session.generation, generation + 1);
        let after = session.run(&Command::new(MESH_QUERY)).unwrap();
        assert_eq!(after["mesh"]["points"][0]["pos"], json!([1.0, 2.0]));
        assert_eq!(after["preset"], json!("mesh"));
        assert!(session.run(&Command::with_params(MESH_EDIT, json!({ "ops": [{ "point": 9999, "pos": [0.0, 0.0] }] }))).is_err());
        session.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(session.run(&Command::new(MESH_QUERY)).unwrap()["mesh"], before["mesh"]);
    }

    fn warped_session() -> (Session, u64, serde_json::Value) {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let id = session.document.create_object(Object { pos: [10.0, 10.0], size: [100.0, 50.0], ..Object::default() });
        session.run(&Command::with_params(GROUP_CREATE, json!({ "nodes": [id.0] }))).unwrap();
        let before = session.run(&Command::new(MESH_QUERY)).unwrap();
        let point = before["mesh"]["points"][0]["id"].as_u64().unwrap();
        (session, point, before)
    }

    #[test]
    fn mesh_edit_commit_after_live_drag_undoes_to_pre_drag() {
        let (mut session, point, before) = warped_session();
        session.run(&Command::with_params(MESH_EDIT, json!({ "ops": [{ "point": point, "pos": [1.0, 2.0] }], "commit": false }))).unwrap();
        session.run(&Command::with_params(MESH_EDIT, json!({ "ops": [{ "point": point, "pos": [3.0, 4.0] }], "commit": false }))).unwrap();
        assert_eq!(session.run(&Command::new(MESH_QUERY)).unwrap()["mesh"]["points"][0]["pos"], json!([3.0, 4.0]));
        session.run(&Command::with_params(MESH_EDIT, json!({ "ops": [{ "point": point, "pos": [3.0, 4.0] }], "commit": true }))).unwrap();
        assert_eq!(session.run(&Command::new(MESH_QUERY)).unwrap()["mesh"]["points"][0]["pos"], json!([3.0, 4.0]));
        session.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(session.run(&Command::new(MESH_QUERY)).unwrap()["mesh"], before["mesh"]);
    }

    #[test]
    fn mesh_edit_cancel_drag_keeps_document_version() {
        let (mut session, point, before) = warped_session();
        let version = session.document.version;
        let generation = session.generation;
        session.run(&Command::with_params(MESH_EDIT, json!({ "ops": [{ "point": point, "pos": [5.0, 6.0] }], "commit": false }))).unwrap();
        assert_eq!(session.run(&Command::new(MESH_QUERY)).unwrap()["mesh"]["points"][0]["pos"], json!([5.0, 6.0]));
        session.run(&Command::with_params(MESH_EDIT, json!({ "cancel_drag": true }))).unwrap();
        assert_eq!(session.run(&Command::new(MESH_QUERY)).unwrap()["mesh"], before["mesh"]);
        assert_eq!(session.document.version, version);
        assert_eq!(session.generation, generation);
    }
}

fn target_group(session: &Session, params: &Value) -> Result<Option<ObjectId>, CommandError> {
    match params.get("group").or_else(|| params.get("id")) {
        Some(Value::Null) | None => Ok(session.selection.ids.iter().copied().find(|id| is_warp_group(session, *id))),
        Some(v) => Ok(Some(ObjectId(v.as_u64().ok_or_else(CommandError::invalid_params)?))),
    }
}

fn query(session: &Session, params: &Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let explicit = params.get("group").or_else(|| params.get("id")).is_some_and(|v| !v.is_null());
    let groups: Vec<Value> = warp_groups(session).iter().map(|(g, b)| group_to_json(g, *b)).collect();
    let selected = target_group(session, params)?;
    let current = match selected {
        Some(id) => match warp_group(session, id) {
            Some((g, b)) => group_to_json(&g, b),
            None if explicit => return Err(CommandError::new("NOT_FOUND")),
            None => Value::Null,
        },
        None => Value::Null,
    };
    Ok(json!({
        "group": current,
        "groups": groups,
        "presets": WARP_PRESETS.iter().map(preset_name).collect::<Vec<_>>(),
    }))
}

fn mesh_query(session: &Session, params: &Value) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let id = target_group(session, params)?.ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    let (group, bounds) = warp_group(session, id).ok_or_else(|| CommandError::new("NOT_FOUND"))?;
    Ok(json!({
        "group": id.0,
        "bounds": bounds_to_json(bounds),
        "preset": preset_name(&group.preset),
        "value": group.value,
        "mesh": mesh_to_json(&group.mesh),
        "muted": group.muted,
    }))
}

#[cfg(test)]
mod preset_offer_contracts {
    use super::*;
    use crate::document::{Document, Units};
    use crate::warp::{is_offered_preset, is_pending_preset, preset_display_name, PENDING_PRESETS};

    #[test]
    fn query_lists_fifteen_offered_preset_slugs() {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let reply = query(&session, &json!({})).unwrap();
        let presets = reply["presets"].as_array().unwrap();
        assert_eq!(presets.len(), 15);
        assert_eq!(presets[0], json!("mesh"));
        assert_eq!(presets[3], json!("arc_vertical"));
        assert_eq!(presets[7], json!("fish_eye"));
        assert_eq!(presets[14], json!("rise"));
        assert!(presets.iter().all(|v| v != "wave" && v != "pinch" && v != "twirl" && v != "spiral"));
        for preset in PENDING_PRESETS {
            assert!(is_pending_preset(preset));
            assert!(!is_offered_preset(preset));
            let _ = preset_display_name(&preset);
        }
    }

    #[test]
    fn create_rejects_pending_preset_but_loads_legacy_sidecar() {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let id = session.document.create_object(Object { pos: [0.0, 0.0], size: [100.0, 80.0], ..Object::default() });
        let err = create(&mut session, &json!({"nodes": [id.0], "preset": "wave"})).unwrap_err();
        assert_eq!(err.code, "INVALID_PRESET");
        let ok = create(&mut session, &json!({"nodes": [id.0], "preset": "Flag"})).unwrap();
        assert_eq!(ok["preset"], json!("flag"));
        let mesh = mesh_from_json(&ok["mesh"]).unwrap();
        assert!(mesh_is_valid(&mesh));
    }

    #[test]
    fn hydrate_normalizes_legacy_preset_names() {
        let mut session = Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1));
        let id = session.document.create_object(Object { pos: [0.0, 0.0], size: [40.0, 40.0], kind: ObjectKind::Group, ..Object::default() });
        let bounds = kurbo::Rect::new(0.0, 0.0, 40.0, 40.0);
        let mesh = identity_mesh(4, 4, bounds);
        session.document.export_defaults = json!({
            "dtool.warp_groups": {
                "1": {
                    "id": id.0,
                    "preset": "Fish Eye",
                    "value": 25.0,
                    "muted": false,
                    "children": [],
                    "bounds": [0.0, 0.0, 40.0, 40.0],
                    "mesh": mesh_to_json(&mesh)
                }
            }
        });
        let version = session.document.version;
        hydrate_sidecar(&mut session.document);
        assert_eq!(session.document.export_defaults["dtool.warp_groups"][id.0.to_string()]["preset"], json!("fish_eye"));
        hydrate_sidecar(&mut session.document);
        assert_eq!(session.document.export_defaults["dtool.warp_groups"][id.0.to_string()]["preset"], json!("fish_eye"));
        let _ = version;
        let loaded = warp_group(&session, id).unwrap();
        assert_eq!(loaded.0.preset, WarpPreset::FishEye);
    }
}

#[cfg(test)]
mod v26b_fix_contracts {
    use super::*;
    use crate::document::{Document, Story, Units};
    use crate::object::ObjectKind;
    use crate::StoryId;

    fn session() -> Session {
        Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1))
    }

    #[test]
    fn add_junction_is_one_undo_step() {
        let mut session = session();
        let id = session.document.create_object(Object { pos: [0.0, 0.0], size: [100.0, 50.0], ..Object::default() });
        session.run(&Command::with_params(GROUP_CREATE, json!({ "nodes": [id.0] }))).unwrap();
        let before = session.run(&Command::new(MESH_QUERY)).unwrap();
        let a = before["mesh"]["points"][0]["id"].as_u64().unwrap();
        let b = before["mesh"]["points"][1]["id"].as_u64().unwrap();
        let cols = before["mesh"]["cols"].as_u64().unwrap();
        let generation = session.generation;
        let reply = session.run(&Command::with_params(MESH_EDIT, json!({ "ops": [{ "add_junction": { "a": a, "b": b } }] }))).unwrap();
        assert_eq!(reply["applied_ops"], json!(1));
        assert_eq!(session.generation, generation + 1);
        let after = session.run(&Command::new(MESH_QUERY)).unwrap();
        assert_eq!(after["mesh"]["cols"].as_u64().unwrap(), cols + 1);
        let ids: Vec<u64> = after["mesh"]["points"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
        assert!(ids.contains(&a) && ids.contains(&b));
        session.run(&Command::new("edit.undo")).unwrap();
        let restored = session.run(&Command::new(MESH_QUERY)).unwrap();
        assert_eq!(restored["mesh"]["cols"], before["mesh"]["cols"]);
    }

    #[test]
    fn muted_bake_leaves_child_paths_unwarped() {
        let mut session = session();
        let id = session.document.create_object(Object { pos: [0.0, 0.0], size: [100.0, 100.0], ..Object::default() });
        let created = session.run(&Command::with_params(GROUP_CREATE, json!({ "nodes": [id.0] }))).unwrap();
        let group = ObjectId(created["group_id"].as_u64().unwrap());
        let mesh = session.run(&Command::new(MESH_QUERY)).unwrap();
        let corner = mesh["mesh"]["points"][0]["id"].as_u64().unwrap();
        session.run(&Command::with_params(MESH_EDIT, json!({ "ops": [{ "point": corner, "pos": [-40.0, -40.0] }] }))).unwrap();
        session.run(&Command::with_params(GROUP_UPDATE, json!({ "muted": true }))).unwrap();
        let before = session.document.get_object(id).unwrap().clone();
        session.run(&Command::with_params(CONVERT_TO_CURVES, json!({ "group": group.0 }))).unwrap();
        let baked = session.document.get_object(id).unwrap();
        assert!(before.pos[0] - baked.pos[0] < 1.0);
        assert!(baked.curve().is_some() || baked.size[0] > 0.0);
        assert!(!is_warp_group(&session, group));
    }

    #[test]
    fn create_warp_over_frame_text_keeps_kind_and_story() {
        let mut session = session();
        let story = StoryId(1);
        session.document.stories.push(Story { id: story, text: "hello".into(), ranges: Vec::new(), specials: Vec::new(), paras: Vec::new() });
        let id = session.document.create_object(Object {
            pos: [10.0, 10.0],
            size: [120.0, 40.0],
            kind: ObjectKind::FrameText { story },
            frame: Some(crate::FrameProps::from_rect([120.0, 40.0])),
            ..Object::default()
        });
        session.run(&Command::with_params(GROUP_CREATE, json!({ "nodes": [id.0] }))).unwrap();
        let object = session.document.get_object(id).unwrap();
        match object.kind {
            ObjectKind::FrameText { story: kept } => assert_eq!(kept, story),
            _ => panic!("kind changed"),
        }
    }
}

#[cfg(test)]
mod kind_aware_contracts {
    use super::*;
    use crate::document::{Document, Units};

    fn new_session() -> Session {
        Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1))
    }

    #[test]
    fn create_and_hydrate_are_kind_aware() {
        let mut session = new_session();
        let child = session.document.create_object(Object { pos: [0.0, 0.0], size: [80.0, 40.0], ..Object::default() });
        let created = create(&mut session, &json!({ "nodes": [child.0] })).unwrap();
        let group_id = ObjectId(created["group_id"].as_u64().unwrap());
        let object = session.document.get_object(group_id).unwrap();
        assert!(kind_is_warp_group(&object.kind));
        assert_eq!(format!("{:?}", object.kind), format!("{:?}", warp_group_kind()));
        let child_object = session.document.get_object(child).unwrap();
        match child_object.kind {
            ObjectKind::Shape => {}
            _ => panic!("create baked or changed the child kind"),
        }

        let mut sidecar = new_session();
        let id = sidecar.document.create_object(Object { pos: [0.0, 0.0], size: [40.0, 40.0], kind: ObjectKind::Group, ..Object::default() });
        let bounds = kurbo::Rect::new(0.0, 0.0, 40.0, 40.0);
        let mesh = identity_mesh(4, 4, bounds);
        sidecar.document.export_defaults = json!({
            STORE_KEY: {
                id.0.to_string(): {
                    "id": id.0,
                    "preset": "mesh",
                    "value": 0.0,
                    "muted": false,
                    "children": [],
                    "bounds": [0.0, 0.0, 40.0, 40.0],
                    "mesh": mesh_to_json(&mesh)
                }
            }
        });
        hydrate_sidecar(&mut sidecar.document);
        let after_kind = format!("{:?}", sidecar.document.get_object(id).unwrap().kind);
        assert!(kind_is_warp_group(&sidecar.document.get_object(id).unwrap().kind));
        assert_eq!(after_kind, format!("{:?}", warp_group_kind()));
        let again_id = sidecar.document.get_object(id).unwrap().id;
        hydrate_sidecar(&mut sidecar.document);
        let again = sidecar.document.get_object(id).unwrap();
        assert_eq!(again.id, again_id);
        assert_eq!(format!("{:?}", again.kind), after_kind);
        let loaded = warp_group(&sidecar, id).unwrap();
        let loaded = warp_group(&sidecar, id).unwrap();
        assert_eq!(loaded.0.mesh.cols, mesh.cols);
        assert_eq!(loaded.0.preset, WarpPreset::Mesh);
    }
}
