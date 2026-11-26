use crate::boolean::{boolean, boolean_operands, BoolOp, FillRule, DEFAULT_TOLERANCE};
use crate::command::Session;
use crate::curve::{Contour, Curve, PathPoint, PointId};
use crate::object::{Object, ObjectId, ObjectKind};
use crate::path_brush::Texture;
use crate::paint::{Cap, Colour, Join, Paint, StrokeStyle, validate_stroke};
use dtool_protocol::{Command, CommandError, CommandResult};
use kurbo::BezPath;

const SYMMETRY_KEY: &str = "dtool.symmetry";
const TIP_KEY: &str = "dtool.vector_brush_tip";
const BLENDS_KEY: &str = "dtool.blends";
const TIPS_KEY: &str = "dtool.path_brushes";
pub(crate) const VECTOR_TOOL: &str = "tool.paint.vector_brush";
pub(crate) const BRUSH_NUMBERS: [&str; 2] = ["brush.width", "brush.opacity"];

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub name: String,
    /// The stored width is in points regardless of the document unit.
    pub stroke: StrokeStyle,
    pub opacity: f32,
    pub texture: Option<Texture>,
}

pub const APPLY_SHARED_PRESET: &str = "brush.vector.preset.apply_shared";

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SharedPreset {
    pub preset: Preset,
    pub image: Option<crate::resources::ImageResource>,
}

impl SharedPreset {
    pub fn capture(session: &Session, name: &str) -> Result<Self, CommandError> {
        let preset = current_preset(session, valid_preset_name(name)?);
        let image = preset.texture.as_ref().map(|texture| {
            crate::path_brush::snapshot_resource(&session.document, &texture.resource_id)
                .ok_or_else(CommandError::invalid_params)
        }).transpose()?;
        Ok(Self { preset, image })
    }
}

pub fn presets(doc: &crate::Document) -> impl Iterator<Item = Preset> + '_ {
    doc.export_defaults.get(TIPS_KEY).and_then(serde_json::Value::as_array)
        .into_iter().flatten().filter_map(|value| serde_json::from_value(value.clone()).ok())
}

pub fn active_preset_name(doc: &crate::Document) -> Option<&str> {
    doc.export_defaults.get(TIP_KEY)?.get("preset_name")?.as_str()
}

pub fn active_preset_is_shared(doc: &crate::Document) -> bool {
    doc.export_defaults.get(TIP_KEY).and_then(|tip| tip.get("shared")).and_then(serde_json::Value::as_bool) == Some(true)
}
mod live_blend;
pub use live_blend::{blend_for_object, linked_blend_path, refresh_blends, BlendInfo, BlendLayout, BLEND_SPACING, DEFAULT_BLEND_STEPS, MAX_BLEND_STEPS};
pub(crate) use live_blend::refresh_saved_blend_appearance;

fn ours(command: &Command) -> bool {
    matches!(
        command.name.as_str(),
        "brush.vector.stroke"
            | "brush.vector.tip.apply"
            | "brush.vector.tip.query"
            | "brush.vector.preset.save"
            | "brush.vector.preset.apply"
            | APPLY_SHARED_PRESET
            | "brush.vector.preset.remove"
            | "blob.paint"
            | "vector.blob.erase"
            | "symmetry.set"
            | "blend.create"
            | "blend.update"
            | "blend.expand"
            | "blend.query"
            | "flood.vector"
    )
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    if !ours(command) {
        return None;
    }
    Some(session.has_document)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    if !ours(command) {
        return None;
    }
    Some(execute(session, command))
}

pub fn apply(session: &mut Session, command: &Command) -> Option<Result<(), CommandError>> {
    if !ours(command) {
        return None;
    }
    Some(execute(session, command).map(|_| ()))
}

fn execute(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::new("NO_DOCUMENT"));
    }
    match command.name.as_str() {
        "brush.vector.tip.query" => tip_query(session),
        "symmetry.set" => with_undo(session, command, symmetry_set),
        "brush.vector.tip.apply" => with_undo(session, command, tip_apply),
        "brush.vector.preset.save" => with_undo(session, command, preset_save),
        "brush.vector.preset.apply" => with_undo(session, command, preset_apply),
        APPLY_SHARED_PRESET => with_undo(session, command, preset_apply_shared),
        "brush.vector.preset.remove" => with_undo(session, command, preset_remove),
        "brush.vector.stroke" => with_undo(session, command, vector_stroke),
        "blob.paint" | "vector.blob.erase" => with_undo(session, command, blob_paint),
        "blend.create" => with_undo(session, command, blend_create),
        "blend.update" => with_undo(session, command, blend_update),
        "blend.expand" => with_undo(session, command, blend_expand),
        "blend.query" => Ok(serde_json::json!({"blends":live_blend::entries(&session.document)})),
        "flood.vector" => with_undo(session, command, flood_vector),
        _ => Err(CommandError::invalid_params()),
    }
}

fn with_undo(
    session: &mut Session,
    command: &Command,
    op: fn(&mut Session, &Command) -> CommandResult,
) -> CommandResult {
    let before = session.document.clone();
    let before_selection = session.selection.ids.clone();
    let brush_options = |session: &Session| BRUSH_NUMBERS.map(|id| session.tool_options.explicit_number(VECTOR_TOOL, id));
    let before_options = matches!(command.name.as_str(), "brush.vector.preset.apply" | APPLY_SHARED_PRESET)
        .then(|| brush_options(session));
    let value = match op(session, command) {
        Ok(value) => value,
        Err(error) => {
            if command.name == APPLY_SHARED_PRESET {
                session.document = before;
            }
            return Err(error);
        }
    };
    let after = session.document.clone();
    let mut undo = serde_json::json!({"document": before});
    let mut redo = serde_json::json!({"document": after});
    if before_selection != session.selection.ids {
        undo["selection"] = serde_json::json!(before_selection);
        redo["selection"] = serde_json::json!(session.selection.ids);
    }
    if let Some(before) = before_options {
        undo["vector_brush_options"] = serde_json::json!(before);
        redo["vector_brush_options"] = serde_json::json!(brush_options(session));
    }
    session.undo.push(crate::undo::Change {
        name: command.name.clone(),
        undo: Command::with_params("core.feature.restore", undo),
        redo: Command::with_params("core.feature.restore", redo),
    });
    session.generation += 1;
    session.document.version += 1;
    Ok(value)
}

fn sidecar_arr<'a>(defaults: &'a mut serde_json::Value, key: &str) -> &'a mut Vec<serde_json::Value> {
    if !defaults.is_object() {
        *defaults = serde_json::json!({});
    }
    let map = defaults.as_object_mut().unwrap();
    if !map.get(key).map(|v| v.is_array()).unwrap_or(false) {
        map.insert(key.to_string(), serde_json::json!([]));
    }
    map.get_mut(key).unwrap().as_array_mut().unwrap()
}

fn parse_points(params: &serde_json::Value) -> Result<Vec<[f64; 2]>, CommandError> {
    let raw = params
        .get("points")
        .and_then(|v| v.as_array())
        .ok_or_else(CommandError::invalid_params)?;
    let mut points = Vec::new();
    for item in raw {
        let pair = item.as_array().ok_or_else(CommandError::invalid_params)?;
        if pair.len() != 2 {
            return Err(CommandError::invalid_params());
        }
        let x = pair[0].as_f64().ok_or_else(CommandError::invalid_params)?;
        let y = pair[1].as_f64().ok_or_else(CommandError::invalid_params)?;
        if !x.is_finite() || !y.is_finite() {
            return Err(CommandError::invalid_params());
        }
        points.push([x, y]);
    }
    if points.len() < 2 {
        return Err(CommandError::invalid_params());
    }
    Ok(points)
}

fn parse_centre(value: &serde_json::Value) -> Result<[f64; 2], CommandError> {
    let pair = value.as_array().ok_or_else(CommandError::invalid_params)?;
    if pair.len() != 2 {
        return Err(CommandError::invalid_params());
    }
    let x = pair[0].as_f64().ok_or_else(CommandError::invalid_params)?;
    let y = pair[1].as_f64().ok_or_else(CommandError::invalid_params)?;
    if !x.is_finite() || !y.is_finite() {
        return Err(CommandError::invalid_params());
    }
    Ok([x, y])
}

fn parse_paint(value: &serde_json::Value) -> Result<Paint, CommandError> {
    serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())
}

fn parse_stroke(value: &serde_json::Value) -> Result<StrokeStyle, CommandError> {
    let stroke = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
    validate_stroke(&stroke).then_some(stroke).ok_or_else(CommandError::invalid_params)
}

fn current_tip(session: &Session) -> serde_json::Value {
    session
        .document
        .export_defaults
        .get(TIP_KEY)
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}))
}

fn parse_texture(value: &serde_json::Value) -> Result<Option<Texture>, CommandError> {
    serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())
}

fn validate_texture(session: &Session, texture: Option<Texture>) -> Result<Option<Texture>, CommandError> {
    if let Some(texture) = &texture
        && crate::path_brush::source_pixels(&session.document, &texture.resource_id).is_none()
    {
        return Err(CommandError::invalid_params());
    }
    Ok(texture)
}

pub fn vector_brush_settings(session: &Session) -> (StrokeStyle, f32, Option<Texture>) {
    let mut style = applied_tip_style(session);
    let [width, opacity] = brush_control_values(session).map(|value| value as f32);
    let opacity = opacity / 100.0;
    style.width = width * crate::units::units_per_point(session.document.units, session.document.dpi);
    (style, opacity, current_texture(session))
}

pub(crate) fn brush_control_values(session: &Session) -> [f64; 2] {
    let tip = session.document.export_defaults.get(TIP_KEY);
    let width = tip.and_then(|value| value["width_pt"].as_f64()).filter(|value| (0.1..=1000.0).contains(value));
    let opacity = tip.and_then(|value| value["opacity"].as_f64()).filter(|value| (0.0..=1.0).contains(value));
    let [width_id, opacity_id] = BRUSH_NUMBERS;
    [
        session.tool_options.explicit_number(VECTOR_TOOL, width_id).or(width).unwrap_or(6.0),
        session.tool_options.explicit_number(VECTOR_TOOL, opacity_id).or(opacity.map(|value| value * 100.0)).unwrap_or(100.0),
    ]
}

pub fn current_texture(session: &Session) -> Option<Texture> {
    parse_texture(current_tip(session).get("texture")?).ok().flatten()
}

fn applied_tip_style(session: &Session) -> StrokeStyle {
    if let Some(stroke) = current_tip(session).get("stroke").and_then(|value| parse_stroke(value).ok()) {
        return stroke;
    }
    let defaults = super::defaults::current();
    let paint = session.selection.ids.first().and_then(|id| session.document.get_object(*id))
        .map(|object| &object.stroke.paint).filter(|paint| **paint != Paint::None).cloned()
        .unwrap_or(defaults.stroke.paint);
    let paint = if paint == Paint::None { Paint::Solid { colour: Colour::new(0, 0, 0, 255) } } else { paint };
    StrokeStyle {
        paint,
        width: 6.0 * crate::units::units_per_point(session.document.units, session.document.dpi),
        cap: Cap::Round,
        join: Join::Round,
        ..Default::default()
    }
}

fn contour_from_points(points: &[[f64; 2]], closed: bool, fitted: bool) -> Contour {
    let pts = if fitted && points.len() >= 2 {
        Contour::smart_handles(points)
    } else {
        points
            .iter()
            .enumerate()
            .map(|(i, pos)| PathPoint::sharp(PointId((i as u64) + 1), *pos))
            .collect()
    };
    Contour { points: pts, closed }
}

fn bounds_of(points: &[[f64; 2]]) -> ([f32; 2], [f32; 2]) {
    let mut min_x = points[0][0];
    let mut min_y = points[0][1];
    let mut max_x = min_x;
    let mut max_y = min_y;
    for p in points {
        min_x = min_x.min(p[0]);
        min_y = min_y.min(p[1]);
        max_x = max_x.max(p[0]);
        max_y = max_y.max(p[1]);
    }
    ([min_x as f32, min_y as f32], [(max_x - min_x) as f32, (max_y - min_y) as f32])
}

fn rotate_point(point: [f64; 2], centre: [f64; 2], angle: f64) -> [f64; 2] {
    let dx = point[0] - centre[0];
    let dy = point[1] - centre[1];
    let cos = angle.cos();
    let sin = angle.sin();
    [centre[0] + dx * cos - dy * sin, centre[1] + dx * sin + dy * cos]
}

fn reflect_point(point: [f64; 2], centre: [f64; 2], axis: f64) -> [f64; 2] {
    let rotated = rotate_point(point, centre, -axis);
    let flipped = [rotated[0], centre[1] * 2.0 - rotated[1]];
    rotate_point(flipped, centre, axis)
}

fn symmetry_copies(points: &[[f64; 2]], spec: &serde_json::Value) -> Vec<Vec<[f64; 2]>> {
    let count = spec.get("count").and_then(|v| v.as_u64()).unwrap_or(1).max(1) as usize;
    if count <= 1 {
        return vec![points.to_vec()];
    }
    let centre = spec
        .get("centre")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            if arr.len() == 2 {
                Some([arr[0].as_f64()?, arr[1].as_f64()?])
            } else {
                None
            }
        })
        .unwrap_or([0.0, 0.0]);
    let mirror = spec.get("mirror").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut copies = Vec::with_capacity(count);
    for i in 0..count {
        let angle = (i as f64) * std::f64::consts::TAU / (count as f64);
        let mut copy: Vec<[f64; 2]> = points.iter().map(|p| rotate_point(*p, centre, angle)).collect();
        if mirror && i % 2 == 1 {
            copy = copy.iter().map(|p| reflect_point(*p, centre, angle)).collect();
        }
        copies.push(copy);
    }
    copies
}

fn make_curve_object(points: &[[f64; 2]], closed: bool, fitted: bool, fill: Paint, stroke: StrokeStyle, opacity: f32) -> Object {
    let contour = contour_from_points(points, closed, fitted);
    let (pos, size) = bounds_of(points);
    let mut object = Object::default();
    object.pos = pos;
    object.size = size;
    object.fill = fill;
    object.stroke = stroke;
    object.opacity = opacity;
    object.kind = ObjectKind::Curve(Curve {
        contour,
        holes: Vec::new(),
        source_object: None,
    });
    object
}

fn commit_curve(
    session: &mut Session,
    points: &[[f64; 2]],
    closed: bool,
    fitted: bool,
    fill: Paint,
    stroke: StrokeStyle,
    opacity: f32,
) -> ObjectId {
    let object = make_curve_object(points, closed, fitted, fill, stroke, opacity);
    session.document.create_object(object)
}

fn symmetry_set(session: &mut Session, command: &Command) -> CommandResult {
    let count = command
        .params
        .get("count")
        .and_then(|v| v.as_u64())
        .ok_or_else(CommandError::invalid_params)?;
    if count < 1 || count > 32 {
        return Err(CommandError::invalid_params());
    }
    let centre = match command.params.get("centre") {
        Some(value) => parse_centre(value)?,
        None => [0.0, 0.0],
    };
    let mirror = command.params.get("mirror").and_then(|v| v.as_bool()).unwrap_or(false);
    let lock = command.params.get("lock").and_then(|v| v.as_bool()).unwrap_or(false);
    let spec = serde_json::json!({
        "count": count,
        "centre": centre,
        "mirror": mirror,
        "lock": lock,
    });
    if !session.document.export_defaults.is_object() {
        session.document.export_defaults = serde_json::json!({});
    }
    session
        .document
        .export_defaults
        .as_object_mut()
        .unwrap()
        .insert(SYMMETRY_KEY.to_string(), spec.clone());
    Ok(spec)
}

fn tip_query(session: &Session) -> CommandResult {
    Ok(serde_json::json!({
        "tip": current_tip(session),
        "brushes": session.document.export_defaults.get(TIPS_KEY).cloned().unwrap_or_else(|| serde_json::json!([])),
    }))
}

fn tip_apply(session: &mut Session, command: &Command) -> CommandResult {
    let mut tip = command
        .params
        .get("tip")
        .cloned()
        .ok_or_else(CommandError::invalid_params)?;
    if !tip.is_object() {
        return Err(CommandError::invalid_params());
    }
    let stroke = tip.get("stroke").map(parse_stroke).transpose()?;
    let texture = validate_texture(session, tip.get("texture").map(parse_texture).transpose()?.flatten())?;
    let changes_texture = tip.get("texture").is_some() || stroke.is_some();
    let fallback_stroke = applied_tip_style(session);
    let previous = current_tip(session);
    for key in ["width_pt", "opacity"] {
        if tip.get(key).is_none() && let Some(value) = previous.get(key) {
            tip[key] = value.clone();
        }
    }
    if !session.document.export_defaults.is_object() {
        session.document.export_defaults = serde_json::json!({});
    }
    session
        .document
        .export_defaults
        .as_object_mut()
        .unwrap()
        .insert(TIP_KEY.to_string(), tip.clone());
    let mut applied = Vec::new();
    if changes_texture {
        for &id in &session.selection.ids {
            if let Some(object) = session.document.find_object_mut(id.0)
                && object.is_curve() && !object.locked
            {
                if let Some(stroke) = &stroke {
                    object.stroke = stroke.clone();
                } else if texture.is_some() && (object.stroke.width <= 0.0 || object.stroke.paint == Paint::None) {
                    object.stroke = fallback_stroke.clone();
                }
                object.path_brush = texture.clone();
                applied.push(id.0);
            }
        }
    }
    Ok(serde_json::json!({"tip": tip, "applied": applied}))
}

fn preset_name(command: &Command) -> Result<&str, CommandError> {
    valid_preset_name(command.params["name"].as_str().ok_or_else(CommandError::invalid_params)?)
}

fn valid_preset_name(name: &str) -> Result<&str, CommandError> {
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= 80 && !name.chars().any(char::is_control))
        .then_some(name).ok_or_else(CommandError::invalid_params)
}

fn current_preset(session: &Session, name: &str) -> Preset {
    let (mut stroke, opacity, texture) = vector_brush_settings(session);
    stroke.width = brush_control_values(session)[0] as f32;
    Preset { name: name.to_string(), stroke, opacity, texture }
}

fn preset_save(session: &mut Session, command: &Command) -> CommandResult {
    let name = preset_name(command)?;
    let mut preset = current_preset(session, name);
    preset.texture = validate_texture(session, preset.texture)?;
    let rows = sidecar_arr(&mut session.document.export_defaults, TIPS_KEY);
    if let Some(row) = rows.iter_mut().find(|row| row["name"] == name) {
        if command.params["replace"] != true {
            return Err(CommandError::invalid_params());
        }
        *row = serde_json::json!(preset);
    } else {
        rows.push(serde_json::json!(preset));
    }
    Ok(serde_json::json!({"preset": preset}))
}

fn preset_apply(session: &mut Session, command: &Command) -> CommandResult {
    let name = preset_name(command)?;
    let preset = presets(&session.document).find(|preset| preset.name == name).ok_or_else(CommandError::not_found)?;
    apply_preset(session, preset, false)
}

fn preset_apply_shared(session: &mut Session, command: &Command) -> CommandResult {
    let SharedPreset { mut preset, image } = serde_json::from_value(command.params["preset"].clone())
        .map_err(|_| CommandError::invalid_params())?;
    preset.name = valid_preset_name(&preset.name)?.to_owned();
    match (&mut preset.texture, image) {
        (Some(texture), Some(image)) if image.embedded && image.path.is_none() && image.data.is_some() => {
            texture.resource_id = crate::path_brush::import_snapshot(&mut session.document, &image).0.to_string();
        }
        (None, None) => {}
        _ => return Err(CommandError::invalid_params()),
    }
    apply_preset(session, preset, true)
}

fn apply_preset(session: &mut Session, preset: Preset, shared: bool) -> CommandResult {
    let width_pt = preset.stroke.width;
    if !(0.1..=1000.0).contains(&width_pt) || !(0.0..=1.0).contains(&preset.opacity) {
        return Err(CommandError::invalid_params());
    }
    let mut stroke = preset.stroke;
    stroke.width *= crate::units::units_per_point(session.document.units, session.document.dpi);
    let change = Command::with_params("brush.vector.tip.apply", serde_json::json!({"tip": {
        "preset_name": preset.name, "shared": shared, "stroke": stroke, "texture": preset.texture, "width_pt": width_pt, "opacity": preset.opacity
    }}));
    let result = tip_apply(session, &change)?;
    session.tool_options.reset_tool(VECTOR_TOOL);
    for id in result["applied"].as_array().into_iter().flatten().filter_map(serde_json::Value::as_u64) {
        if let Some(object) = session.document.find_object_mut(id) {
            object.opacity = preset.opacity;
        }
    }
    Ok(result)
}

fn preset_remove(session: &mut Session, command: &Command) -> CommandResult {
    let name = preset_name(command)?;
    let rows = session.document.export_defaults.get_mut(TIPS_KEY).and_then(serde_json::Value::as_array_mut)
        .ok_or_else(CommandError::not_found)?;
    let index = rows.iter().position(|row| row["name"] == name).ok_or_else(CommandError::not_found)?;
    rows.remove(index);
    if !active_preset_is_shared(&session.document) && active_preset_name(&session.document) == Some(name)
        && let Some(tip) = session.document.export_defaults.get_mut(TIP_KEY).and_then(serde_json::Value::as_object_mut)
    {
        tip.remove("preset_name");
    }
    Ok(serde_json::json!({"name": name}))
}

fn vector_stroke(session: &mut Session, command: &Command) -> CommandResult {
    let points = parse_points(&command.params)?;
    let preset_options = active_preset_name(&session.document).map(|_| vector_brush_settings(session));
    let stroke = command
        .params
        .get("stroke")
        .map(parse_stroke)
        .transpose()?
        .unwrap_or_else(|| preset_options.as_ref().map(|(stroke, _, _)| stroke.clone())
            .unwrap_or_else(|| applied_tip_style(session)));
    let opacity = command
        .params
        .get("opacity")
        .and_then(|v| v.as_f64())
        .map(|value| value as f32)
        .unwrap_or_else(|| preset_options.as_ref().map_or(1.0, |(_, opacity, _)| *opacity));
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err(CommandError::invalid_params());
    }
    let tip = current_tip(session);
    let texture = validate_texture(session, command.params.get("texture").or_else(|| tip.get("texture"))
        .map(parse_texture).transpose()?.flatten())?;
    let spec = session
        .document
        .export_defaults
        .get(SYMMETRY_KEY)
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"count": 1}));
    let copies = symmetry_copies(&points, &spec).into_iter().map(|copy| {
        let contour = contour_from_points(&copy, false, true);
        let (pos, size, local) = super::curve_edit::placement(&contour)?;
        let mut object = Object {
            pos,
            size,
            stroke: stroke.clone(),
            path_brush: texture.clone(),
            opacity,
            kind: ObjectKind::Curve(Curve::from_contour_and_holes(local, Vec::new())),
            ..Object::default()
        };
        let start = copy[0].map(|coordinate| coordinate as f32);
        crate::artboard::edit::assign_new_object(&session.document, &mut object, start);
        Ok(object)
    }).collect::<Result<Vec<_>, CommandError>>()?;
    let ids: Vec<_> = copies.into_iter().map(|object| session.document.create_object(object).0).collect();
    if command.params["select"] == true {
        session.selection.set(ids.iter().copied().map(ObjectId).collect(), crate::selection::SelectOp::Replace);
    }
    Ok(serde_json::json!({"ids": ids, "copies": ids.len()}))
}

fn blob_width(command: &Command) -> f64 {
    command
        .params
        .get("width")
        .and_then(|v| v.as_f64())
        .or_else(|| {
            command
                .params
                .get("stroke")
                .and_then(|v| v.get("width"))
                .and_then(|v| v.as_f64())
        })
        .unwrap_or(8.0)
        .max(0.5)
}

fn thicken(points: &[[f64; 2]], width: f64) -> Vec<[f64; 2]> {
    let r = width / 2.0;
    if points.len() < 2 {
        return points.to_vec();
    }
    let mut left = Vec::new();
    let mut right = Vec::new();
    for window in points.windows(2) {
        let dx = window[1][0] - window[0][0];
        let dy = window[1][1] - window[0][1];
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let nx = -dy / len * r;
        let ny = dx / len * r;
        if left.is_empty() {
            left.push([window[0][0] + nx, window[0][1] + ny]);
            right.push([window[0][0] - nx, window[0][1] - ny]);
        }
        left.push([window[1][0] + nx, window[1][1] + ny]);
        right.push([window[1][0] - nx, window[1][1] - ny]);
    }
    left.extend(right.into_iter().rev());
    left
}

fn point_in_bounds(point: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let (pos, size) = bounds_of(poly);
    let min_x = pos[0] as f64;
    let min_y = pos[1] as f64;
    let max_x = min_x + size[0] as f64;
    let max_y = min_y + size[1] as f64;
    point[0] >= min_x && point[0] <= max_x && point[1] >= min_y && point[1] <= max_y
}

fn point_in_poly(point: [f64; 2], poly: &[[f64; 2]]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let (x, y) = (point[0], point[1]);
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = (poly[i][0], poly[i][1]);
        let (xj, yj) = (poly[j][0], poly[j][1]);
        if (yi > y) != (yj > y) {
            let denom = yj - yi;
            if denom != 0.0 {
                let xint = (xj - xi) * (y - yi) / denom + xi;
                if x < xint {
                    inside = !inside;
                }
            }
        }
        j = i;
    }
    inside || point_in_bounds(point, poly)
}

fn bounds_overlap(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    let (ap, asz) = bounds_of(a);
    let (bp, bsz) = bounds_of(b);
    let ax2 = ap[0] + asz[0];
    let ay2 = ap[1] + asz[1];
    let bx2 = bp[0] + bsz[0];
    let by2 = bp[1] + bsz[1];
    ap[0] <= bx2 && bp[0] <= ax2 && ap[1] <= by2 && bp[1] <= ay2
}

fn ring_to_path(ring: &[[f64; 2]]) -> BezPath {
    let mut path = BezPath::new();
    if let Some(first) = ring.first() {
        path.move_to(kurbo::Point::new(first[0], first[1]));
        for point in ring.iter().skip(1) {
            path.line_to(kurbo::Point::new(point[0], point[1]));
        }
        path.close_path();
    }
    path
}

fn curve_from_ring(ring: &[[f64; 2]]) -> Option<Curve> {
    if ring.len() < 3 {
        return None;
    }
    let mut next_id = 1u64;
    let points = ring
        .iter()
        .map(|pos| {
            let point = PathPoint::sharp(PointId(next_id), *pos);
            next_id += 1;
            point
        })
        .collect();
    Some(Curve {
        contour: Contour { points, closed: true },
        holes: Vec::new(),
        source_object: None,
    })
}

fn rings_overlap(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    boolean(
        &[ring_to_path(a)],
        &[ring_to_path(b)],
        BoolOp::Intersect,
        FillRule::NonZero,
        DEFAULT_TOLERANCE,
    )
    .map(|curves| !curves.is_empty())
    .unwrap_or(false)
}

fn union_rings(rings: &[Vec<[f64; 2]>]) -> Option<Curve> {
    if rings.is_empty() {
        return None;
    }
    if rings.len() == 1 {
        return curve_from_ring(&rings[0]);
    }
    let operands: Vec<Vec<BezPath>> = rings.iter().map(|ring| vec![ring_to_path(ring)]).collect();
    boolean_operands(&operands, BoolOp::Add, FillRule::NonZero, DEFAULT_TOLERANCE)
        .ok()
        .and_then(|mut curves| curves.pop())
}

fn commit_blob_curve(session: &mut Session, curve: Curve, fill: Paint) -> ObjectId {
    let points: Vec<[f64; 2]> = curve.contour.points.iter().map(|point| point.pos).collect();
    let (pos, size) = if points.is_empty() {
        ([0.0, 0.0], [0.0, 0.0])
    } else {
        bounds_of(&points)
    };
    let mut object = Object::default();
    object.pos = pos;
    object.size = size;
    object.fill = fill;
    object.stroke = StrokeStyle::default();
    object.opacity = 1.0;
    object.kind = ObjectKind::Curve(curve);
    session.document.create_object(object)
}

fn contour_points(curve: &Curve) -> Vec<[f64; 2]> {
    curve.contour.points.iter().map(|p| p.pos).collect()
}

fn same_fill(a: &Paint, b: &Paint) -> bool {
    a == b
}

fn erase_requested(command: &Command) -> bool {
    command.name == "vector.blob.erase" || command.params.get("erase").and_then(serde_json::Value::as_bool) == Some(true)
}

fn selected_target_ids(session: &Session, params: &serde_json::Value) -> Result<Vec<ObjectId>, CommandError> {
    if let Some(value) = params.get("target").or_else(|| params.get("selection")) {
        if let Some(id) = value.as_u64() {
            return Ok(vec![ObjectId(id)]);
        }
        let list = value.as_array().ok_or_else(CommandError::invalid_params)?;
        let mut ids = Vec::new();
        for item in list {
            let id = item
                .as_u64()
                .or_else(|| item.get("id").and_then(serde_json::Value::as_u64))
                .ok_or_else(CommandError::invalid_params)?;
            ids.push(ObjectId(id));
        }
        return Ok(ids);
    }
    Ok(session.selection.ids.clone())
}

fn contour_to_path(contour: &Contour) -> BezPath {
    ring_to_path(&contour.points.iter().map(|point| point.pos).collect::<Vec<_>>())
}

fn curve_to_paths(curve: &Curve) -> Vec<BezPath> {
    let mut paths = vec![contour_to_path(&curve.contour)];
    for hole in &curve.holes {
        paths.push(contour_to_path(hole));
    }
    paths
}

fn ring_centroid(ring: &[[f64; 2]]) -> Option<[f64; 2]> {
    if ring.is_empty() {
        return None;
    }
    let n = ring.len() as f64;
    Some([ring.iter().map(|p| p[0]).sum::<f64>() / n, ring.iter().map(|p| p[1]).sum::<f64>() / n])
}

fn winding_contains(point: [f64; 2], ring: &[[f64; 2]]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let (x, y) = (point[0], point[1]);
    let mut inside = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (xi, yi) = (ring[i][0], ring[i][1]);
        let (xj, yj) = (ring[j][0], ring[j][1]);
        if (yi > y) != (yj > y) {
            let denom = yj - yi;
            if denom != 0.0 {
                let xint = (xj - xi) * (y - yi) / denom + xi;
                if x < xint {
                    inside = !inside;
                }
            }
        }
        j = i;
    }
    inside
}

fn split_disjoint_pieces(curve: Curve) -> Vec<Curve> {
    if curve.holes.is_empty() {
        return vec![curve];
    }
    let outer = contour_points(&curve);
    let mut holes = Vec::new();
    let mut pieces = Vec::new();
    for hole in curve.holes {
        let pts: Vec<[f64; 2]> = hole.points.iter().map(|point| point.pos).collect();
        let inside = ring_centroid(&pts).is_some_and(|point| winding_contains(point, &outer));
        if inside {
            holes.push(hole);
        } else if pts.len() >= 3 {
            pieces.push(Curve {
                contour: hole,
                holes: Vec::new(),
                source_object: None,
            });
        }
    }
    let mut out = vec![Curve {
        contour: curve.contour,
        holes,
        source_object: curve.source_object,
    }];
    out.extend(pieces);
    out
}

fn is_filled_curve(object: &Object) -> bool {
    object.curve().is_some_and(|curve| curve.contour.closed && curve.contour.points.len() >= 3)
}

fn apply_curve(object: &mut Object, curve: Curve) {
    let points = contour_points(&curve);
    let (pos, size) = if points.is_empty() {
        ([0.0, 0.0], [0.0, 0.0])
    } else {
        bounds_of(&points)
    };
    object.pos = pos;
    object.size = size;
    object.kind = ObjectKind::Curve(curve);
}

fn blob_erase(session: &mut Session, command: &Command) -> CommandResult {
    let spine = parse_points(&command.params)?;
    let width = blob_width(command);
    let cutter = thicken(&spine, width);
    if cutter.len() < 3 {
        return Err(CommandError::invalid_params());
    }
    let ids = selected_target_ids(session, &command.params)?;
    if session.document.objects.iter().any(|object| ids.contains(&object.id) && object.locked && is_filled_curve(object)) {
        return Err(CommandError::invalid_params());
    }
    let targets: Vec<ObjectId> = session
        .document
        .objects
        .iter()
        .filter(|object| ids.contains(&object.id) && is_filled_curve(object))
        .map(|object| object.id)
        .collect();
    if targets.is_empty() {
        return Err(CommandError::invalid_params());
    }
    let cutter_paths = vec![ring_to_path(&cutter)];
    let mut updates: Vec<(ObjectId, Vec<Curve>)> = Vec::new();
    for id in &targets {
        let object = session.document.objects.iter().find(|object| object.id == *id).unwrap();
        let curve = object.curve().unwrap();
        let remainders = boolean(
            &curve_to_paths(curve),
            &cutter_paths,
            BoolOp::Subtract,
            FillRule::NonZero,
            DEFAULT_TOLERANCE,
        )?;
        let pieces: Vec<Curve> = remainders
            .into_iter()
            .flat_map(split_disjoint_pieces)
            .filter(|curve| curve.contour.points.len() >= 3)
            .collect();
        updates.push((*id, pieces));
    }
    let mut kept = Vec::new();
    let mut created = Vec::new();
    let mut removed = Vec::new();
    for (id, pieces) in updates {
        if pieces.is_empty() {
            let _ = session.document.delete_object(id);
            removed.push(id.0);
            continue;
        }
        let mut extra = pieces.into_iter();
        let first = extra.next().unwrap();
        if let Some(object) = session.document.objects.iter_mut().find(|object| object.id == id) {
            apply_curve(object, first);
        }
        kept.push(id.0);
        let template = session.document.objects.iter().find(|object| object.id == id).cloned();
        if let Some(template) = template {
            for piece in extra {
                let mut object = template.clone();
                object.id = ObjectId(0);
                apply_curve(&mut object, piece);
                created.push(session.document.create_object(object).0);
            }
        }
    }
    session.selection.ids.retain(|id| session.document.get_object(*id).is_some());
    Ok(serde_json::json!({
        "erased": true,
        "ids": kept,
        "created": created,
        "removed": removed,
    }))
}

fn blob_paint(session: &mut Session, command: &Command) -> CommandResult {
    if erase_requested(command) {
        return blob_erase(session, command);
    }
    let spine = parse_points(&command.params)?;
    let width = blob_width(command);
    let outline = thicken(&spine, width);
    let fill = command
        .params
        .get("fill")
        .map(parse_paint)
        .transpose()?
        .unwrap_or_else(Paint::default);
    let mut candidates: Vec<(ObjectId, Vec<[f64; 2]>)> = Vec::new();
    for object in &session.document.objects {
        if let Some(curve) = object.curve() {
            let pts = contour_points(curve);
            if pts.len() >= 3 && same_fill(&object.fill, &fill) && bounds_overlap(&outline, &pts) {
                candidates.push((object.id, pts));
            }
        }
    }
    let mut merge_ids: Vec<ObjectId> = Vec::new();
    let mut rings: Vec<Vec<[f64; 2]>> = vec![outline.clone()];
    let mut remaining = candidates;
    let mut progressed = true;
    while progressed {
        progressed = false;
        let mut leftover = Vec::new();
        for (id, pts) in remaining {
            if rings.iter().any(|ring| rings_overlap(ring, &pts)) {
                merge_ids.push(id);
                rings.push(pts);
                progressed = true;
            } else {
                leftover.push((id, pts));
            }
        }
        remaining = leftover;
    }
    let id = if let Some(curve) = union_rings(&rings) {
        session.document.objects.retain(|o| !merge_ids.contains(&o.id));
        commit_blob_curve(session, curve, fill)
    } else {
        commit_curve(session, &outline, true, false, fill, StrokeStyle::default(), 1.0)
    };
    Ok(serde_json::json!({"id": id.0, "merged": merge_ids.iter().map(|i| i.0).collect::<Vec<_>>()}))
}

fn object_id_param(params: &serde_json::Value, key: &str) -> Result<ObjectId, CommandError> {
    let value = params.get(key).ok_or_else(CommandError::invalid_params)?;
    let id = value
        .as_u64()
        .or_else(|| value.get("id").and_then(|v| v.as_u64()))
        .ok_or_else(CommandError::invalid_params)?;
    Ok(ObjectId(id))
}

fn next_blend_id(doc: &crate::document::Document) -> Result<u64, CommandError> {
    live_blend::entries(doc)
        .iter()
        .filter_map(|v| v.get("id").and_then(|id| id.as_u64()))
        .max()
        .unwrap_or(0)
        .checked_add(1).ok_or_else(CommandError::invalid_params)
}

fn blend_layout(params: &serde_json::Value, existing: Option<&serde_json::Value>) -> Result<BlendLayout, CommandError> {
    let mut layout = existing.map(BlendLayout::from_entry).unwrap_or_default();
    if let Some(spacing) = params.get("spacing") {
        layout.spacing = serde_json::from_value(spacing.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(spine) = params.get("spine") {
        layout.spine = serde_json::from_value(spine.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if let Some(path) = params.get("path_id") {
        layout.path_id = serde_json::from_value(path.clone()).map_err(|_| CommandError::invalid_params())?;
    }
    if !layout.is_valid() {
        return Err(CommandError::invalid_params());
    }
    Ok(layout)
}

fn blend_steps(params: &serde_json::Value, default: u64) -> Result<u32, CommandError> {
    let value = params.get("steps").map_or(Some(default), serde_json::Value::as_u64)
        .filter(|v| (1..=u64::from(MAX_BLEND_STEPS)).contains(v))
        .ok_or_else(CommandError::invalid_params)?;
    Ok(value as u32)
}

fn validate_blend_path(session: &Session, from: ObjectId, to: ObjectId, layout: BlendLayout, params: &serde_json::Value) -> Result<(), CommandError> {
    if params.get("path_id").is_some() && let Some(id) = layout.path_id
        && linked_blend_path(&session.document, from, to, id).is_none()
    {
        return Err(CommandError::invalid_params());
    }
    Ok(())
}

fn respawn_legacy_blend(session: &mut Session, from: ObjectId, to: ObjectId, steps: u32) -> Result<Vec<u64>, CommandError> {
    let from = session.document.get_object(from).cloned().ok_or_else(CommandError::not_found)?;
    let to = session.document.get_object(to).cloned().ok_or_else(CommandError::not_found)?;
    Ok((1..=steps).map(|step| {
        let t = step as f32 / (steps + 1) as f32;
        let lerp = |a, b| a + (b - a) * t;
        let mut object = from.clone();
        object.id = ObjectId(0);
        object.name = format!("Blend {step}");
        object.pos = std::array::from_fn(|i| lerp(from.pos[i], to.pos[i]));
        object.size = std::array::from_fn(|i| lerp(from.size[i], to.size[i]));
        object.opacity = lerp(from.opacity, to.opacity);
        session.document.create_object(object).0
    }).collect())
}

fn remove_ids(session: &mut Session, ids: &[u64]) {
    session
        .document
        .objects
        .retain(|o| !ids.contains(&o.id.0));
}

fn blend_create(session: &mut Session, command: &Command) -> CommandResult {
    let from = object_id_param(&command.params, "from")?;
    let to = object_id_param(&command.params, "to")?;
    if from == to || [from, to].iter().any(|id| live_blend::is_generated(&session.document, *id)) {
        return Err(CommandError::invalid_params());
    }
    let steps = blend_steps(&command.params, u64::from(DEFAULT_BLEND_STEPS))?;
    let id = next_blend_id(&session.document)?;
    let layout = blend_layout(&command.params, None)?;
    validate_blend_path(session, from, to, layout, &command.params)?;
    let objects = live_blend::objects(&session.document, from, to, steps, layout).ok_or_else(CommandError::invalid_params)?;
    let generated: Vec<_> = objects.into_iter().map(|object| session.document.create_object(object).0).collect();
    let blends = sidecar_arr(&mut session.document.export_defaults, BLENDS_KEY);
    let entry = serde_json::json!({
        "id": id,
        "live": true,
        "layout": layout,
        "from": from.0,
        "to": to.0,
        "steps": steps,
        "generated": generated,
    });
    blends.push(entry.clone());
    Ok(entry)
}

fn blend_update(session: &mut Session, command: &Command) -> CommandResult {
    let id = object_id_param(&command.params, "id")?;
    let existing = session
        .document
        .export_defaults
        .get(BLENDS_KEY)
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|b| b.get("id").and_then(|v| v.as_u64()) == Some(id.0)))
        .cloned()
        .ok_or_else(|| CommandError::new("NOT_FOUND"))?;
    let from = ObjectId(existing.get("from").and_then(|v| v.as_u64()).ok_or_else(CommandError::invalid_params)?);
    let to = ObjectId(existing.get("to").and_then(|v| v.as_u64()).ok_or_else(CommandError::invalid_params)?);
    let steps = blend_steps(&command.params, existing.get("steps").and_then(|v| v.as_u64()).unwrap_or(u64::from(DEFAULT_BLEND_STEPS)))?;
    let live = existing["live"].as_bool() == Some(true);
    let old: Vec<u64> = existing
        .get("generated")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();
    let layout = blend_layout(&command.params, Some(&existing))?;
    if !live && ["spacing", "spine", "path_id"].iter().any(|key| command.params.get(*key).is_some()) {
        return Err(CommandError::invalid_params());
    }
    validate_blend_path(session, from, to, layout, &command.params)?;
    let generated = if live {
        let objects = live_blend::objects(&session.document, from, to, steps, layout).ok_or_else(CommandError::invalid_params)?;
        let mut ids = Vec::with_capacity(objects.len());
        for (index, mut object) in objects.into_iter().enumerate() {
            if let Some(slot) = old.get(index).and_then(|id| session.document.find_object_mut(*id)) {
                object.id = slot.id;
                ids.push(slot.id.0);
                *slot = object;
            } else {
                ids.push(session.document.create_object(object).0);
            }
        }
        remove_ids(session, &old[old.len().min(ids.len())..]);
        ids
    } else {
        let generated = respawn_legacy_blend(session, from, to, steps)?;
        remove_ids(session, &old);
        generated
    };
    let blends = sidecar_arr(&mut session.document.export_defaults, BLENDS_KEY);
    if let Some(entry) = blends.iter_mut().find(|b| b.get("id").and_then(|v| v.as_u64()) == Some(id.0)) {
        *entry = serde_json::json!({
            "id": id.0,
            "live": live,
            "layout": layout,
            "from": from.0,
            "to": to.0,
            "steps": steps,
            "generated": generated,
        });
        return Ok(entry.clone());
    }
    Err(CommandError::new("NOT_FOUND"))
}

fn blend_expand(session: &mut Session, command: &Command) -> CommandResult {
    let id = object_id_param(&command.params, "id")?;
    if !live_blend::entries(&session.document).iter().any(|entry| entry["id"].as_u64() == Some(id.0)) {
        return Err(CommandError::not_found());
    }
    refresh_blends(&mut session.document);
    let blends = sidecar_arr(&mut session.document.export_defaults, BLENDS_KEY);
    let pos = blends
        .iter()
        .position(|b| b.get("id").and_then(|v| v.as_u64()) == Some(id.0))
        .ok_or_else(|| CommandError::new("NOT_FOUND"))?;
    let entry = blends.remove(pos);
    if let Some(ids) = entry.get("generated").and_then(|value| value.as_array()).filter(|_| entry["live"].as_bool() == Some(true)) {
        for id in ids.iter().filter_map(|value| value.as_u64()) {
            if let Some(object) = session.document.find_object_mut(id) { object.locked = false; }
        }
    }
    Ok(serde_json::json!({"id": id.0, "expanded": entry.get("generated").cloned().unwrap_or_else(|| serde_json::json!([]))}))
}


fn parse_at(params: &serde_json::Value) -> Result<[f64; 2], CommandError> {
    if let Some(arr) = params.get("at").and_then(|v| v.as_array()) {
        if arr.len() == 2 {
            let x = arr[0].as_f64().ok_or_else(CommandError::invalid_params)?;
            let y = arr[1].as_f64().ok_or_else(CommandError::invalid_params)?;
            if x.is_finite() && y.is_finite() {
                return Ok([x, y]);
            }
        }
    }
    if let Some(obj) = params.get("at").and_then(|v| v.as_object()) {
        let x = obj.get("x").and_then(|v| v.as_f64()).ok_or_else(CommandError::invalid_params)?;
        let y = obj.get("y").and_then(|v| v.as_f64()).ok_or_else(CommandError::invalid_params)?;
        if x.is_finite() && y.is_finite() {
            return Ok([x, y]);
        }
    }
    Err(CommandError::invalid_params())
}

fn contour_area(points: &[[f64; 2]]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        area += (points[j][0] + points[i][0]) * (points[j][1] - points[i][1]);
        j = i;
    }
    area.abs() * 0.5
}

fn flood_vector(session: &mut Session, command: &Command) -> CommandResult {
    let at = parse_at(&command.params)?;
    let fit = command
        .params
        .get("fit")
        .and_then(|v| v.as_str())
        .unwrap_or("min")
        .to_ascii_lowercase();
    let mode = command
        .params
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("add")
        .to_ascii_lowercase();
    let fill = command
        .params
        .get("fill")
        .map(parse_paint)
        .transpose()?
        .unwrap_or_else(Paint::default);
    let mut candidates: Vec<(ObjectId, Vec<[f64; 2]>, f64)> = Vec::new();
    for object in &session.document.objects {
        let Some(curve) = object.curve() else { continue };
        if !curve.contour.closed {
            continue;
        }
        let pts = contour_points(curve);
        if pts.len() >= 3 && point_in_poly(at, &pts) {
            candidates.push((object.id, pts, contour_area(&contour_points(curve))));
        }
    }
    if candidates.is_empty() {
        return Err(CommandError::new("NOT_FOUND"));
    }
    candidates.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
    let chosen = if fit.contains("max") {
        candidates.last().unwrap().clone()
    } else {
        candidates.first().unwrap().clone()
    };
    if mode.contains("smart") || mode.contains("refill") {
        if let Some(existing) = session.document.objects.iter_mut().find(|o| o.id == chosen.0) {
            existing.fill = fill;
            return Ok(serde_json::json!({"id": chosen.0.0, "mode": "smart_refill"}));
        }
    }
    if mode.contains("knock") {
        if let Some(existing) = session.document.objects.iter_mut().find(|o| o.id == chosen.0) {
            if let Some(curve) = existing.curve_mut() {
                curve.holes.push(contour_from_points(&chosen.1, true, false));
                return Ok(serde_json::json!({"id": chosen.0.0, "mode": "knockout"}));
            }
        }
    }
    let id = commit_curve(session, &chosen.1, true, false, fill, StrokeStyle::default(), 1.0);
    Ok(serde_json::json!({"id": id.0, "mode": "add", "source": chosen.0.0}))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, Units};

    fn session() -> Session {
        Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1))
    }

    fn cmd(name: &str, params: serde_json::Value) -> Command {
        Command::with_params(name, params)
    }

    #[test]
    fn vector_stroke_creates_fitted_curve_and_is_undoable() {
        let mut session = session();
        let result = run(
            &mut session,
            &cmd(
                "brush.vector.stroke",
                serde_json::json!({"points": [[0.0, 0.0], [40.0, 10.0], [80.0, 0.0]]}),
            ),
        )
        .unwrap()
        .unwrap();
        let ids = result["ids"].as_array().unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(session.document.objects.len(), 1);
        assert!(session.document.objects[0].is_curve());
        assert!(!session.document.objects[0].curve().unwrap().contour.closed);
        assert_eq!(session.undo.applied_steps(), 1);
        let undo_cmd = session.undo.undo().expect("undo command");
        crate::commands::feature_restore::run(&mut session, &undo_cmd)
            .unwrap()
            .unwrap();
        assert_eq!(session.document.objects.len(), 0);
    }

    #[test]
    fn symmetry_repeats_stroke_in_one_commit() {
        let mut session = session();
        run(
            &mut session,
            &cmd(
                "symmetry.set",
                serde_json::json!({"count": 4, "centre": [0.0, 0.0], "mirror": true, "lock": true}),
            ),
        )
        .unwrap()
        .unwrap();
        let before_steps = session.undo.applied_steps();
        let result = run(
            &mut session,
            &cmd(
                "brush.vector.stroke",
                serde_json::json!({"points": [[10.0, 0.0], [20.0, 0.0]]}),
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result["copies"], 4);
        assert_eq!(session.document.objects.len(), 4);
        assert_eq!(session.undo.applied_steps(), before_steps + 1);
    }

    #[test]
    fn blob_paint_merges_overlapping_same_fill() {
        let mut session = session();
        run(
            &mut session,
            &cmd(
                "blob.paint",
                serde_json::json!({"points": [[0.0, 0.0], [20.0, 0.0]], "width": 8.0}),
            ),
        )
        .unwrap()
        .unwrap();
        run(
            &mut session,
            &cmd(
                "blob.paint",
                serde_json::json!({"points": [[10.0, 0.0], [30.0, 0.0]], "width": 8.0}),
            ),
        )
        .unwrap()
        .unwrap();
        let blobs: Vec<_> = session
            .document
            .objects
            .iter()
            .filter(|o| o.is_curve() && o.curve().unwrap().contour.closed)
            .collect();
        assert_eq!(blobs.len(), 1);
    }

    fn closed_blob_rings(session: &Session) -> Vec<Vec<[f64; 2]>> {
        session
            .document
            .objects
            .iter()
            .filter(|object| object.is_curve() && object.curve().unwrap().contour.closed)
            .map(|object| contour_points(object.curve().unwrap()))
            .collect()
    }

    fn strict_point_in_ring(point: [f64; 2], ring: &[[f64; 2]]) -> bool {
        if ring.len() < 3 {
            return false;
        }
        let (x, y) = (point[0], point[1]);
        let mut inside = false;
        let mut j = ring.len() - 1;
        for i in 0..ring.len() {
            let (xi, yi) = (ring[i][0], ring[i][1]);
            let (xj, yj) = (ring[j][0], ring[j][1]);
            if (yi > y) != (yj > y) {
                let denom = yj - yi;
                if denom != 0.0 {
                    let xint = (xj - xi) * (y - yi) / denom + xi;
                    if x < xint {
                        inside = !inside;
                    }
                }
            }
            j = i;
        }
        inside
    }

    #[test]
    fn blob_paint_keeps_disjoint_blobs_separate() {
        let mut session = session();
        run(
            &mut session,
            &cmd(
                "blob.paint",
                serde_json::json!({"points": [[0.0, 0.0], [20.0, 0.0]], "width": 8.0}),
            ),
        )
        .unwrap()
        .unwrap();
        run(
            &mut session,
            &cmd(
                "blob.paint",
                serde_json::json!({"points": [[100.0, 0.0], [120.0, 0.0]], "width": 8.0}),
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(closed_blob_rings(&session).len(), 2);
    }

    #[test]
    fn blob_paint_preserves_concave_l_and_union() {
        let mut session = session();
        run(
            &mut session,
            &cmd(
                "blob.paint",
                serde_json::json!({"points": [[0.0, 0.0], [0.0, 40.0]], "width": 10.0}),
            ),
        )
        .unwrap()
        .unwrap();
        run(
            &mut session,
            &cmd(
                "blob.paint",
                serde_json::json!({"points": [[0.0, 40.0], [40.0, 40.0]], "width": 10.0}),
            ),
        )
        .unwrap()
        .unwrap();
        let rings = closed_blob_rings(&session);
        assert_eq!(rings.len(), 1);
        let ring = &rings[0];
        assert!(strict_point_in_ring([0.0, 20.0], ring));
        assert!(strict_point_in_ring([20.0, 40.0], ring));
        assert!(!strict_point_in_ring([25.0, 10.0], ring));
    }

    #[test]
    fn blend_create_update_and_expand() {
        let mut session = session();
        let mut a = Object::default();
        a.pos = [0.0, 0.0];
        a.size = [10.0, 10.0];
        let mut b = Object::default();
        b.pos = [100.0, 0.0];
        b.size = [10.0, 10.0];
        let from = session.document.create_object(a);
        let to = session.document.create_object(b);
        let created = run(
            &mut session,
            &cmd("blend.create", serde_json::json!({"from": from.0, "to": to.0, "steps": 3})),
        )
        .unwrap()
        .unwrap();
        assert_eq!(created["steps"], 3);
        assert_eq!(created["generated"].as_array().unwrap().len(), 3);
        assert_eq!(session.document.objects.len(), 5);
        let bid = created["id"].as_u64().unwrap();
        let updated = run(
            &mut session,
            &cmd("blend.update", serde_json::json!({"id": bid, "steps": 5})),
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated["generated"].as_array().unwrap().len(), 5);
        assert_eq!(session.document.objects.len(), 7);
        run(&mut session, &cmd("blend.expand", serde_json::json!({"id": bid})))
            .unwrap()
            .unwrap();
        assert_eq!(session.document.objects.len(), 7);
        assert!(session
            .document
            .export_defaults
            .get(BLENDS_KEY)
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true));
    }

    #[test]
    fn tip_apply_sets_current_vector_brush_tip() {
        let mut session = session();
        let id = commit_curve(
            &mut session,
            &[[0.0, 0.0], [10.0, 0.0]],
            false,
            false,
            Paint::None,
            StrokeStyle::default(),
            1.0,
        );
        session.selection.ids = vec![id];
        run(
            &mut session,
            &cmd("brush.vector.tip.apply", serde_json::json!({"tip": {"id": "round"}})),
        )
        .unwrap()
        .unwrap();
        let queried = run(&mut session, &cmd("brush.vector.tip.query", serde_json::json!({})))
            .unwrap()
            .unwrap();
        assert_eq!(queried["tip"]["id"], "round");
    }

    #[test]
    fn flood_vector_fills_closed_curve_region() {
        let mut session = session();
        commit_curve(
            &mut session,
            &[[0.0, 0.0], [40.0, 0.0], [40.0, 40.0], [0.0, 40.0]],
            true,
            false,
            Paint::None,
            StrokeStyle::default(),
            1.0,
        );
        let result = run(
            &mut session,
            &cmd("flood.vector", serde_json::json!({"at": [20.0, 20.0], "fit": "min"})),
        )
        .unwrap()
        .unwrap();
        assert!(result.get("id").is_some());
        let filled = session
            .document
            .objects
            .iter()
            .filter(|o| o.is_curve() && o.curve().unwrap().contour.closed)
            .count();
        assert_eq!(filled, 2);
    }

    #[test]
    fn symmetry_rejects_count_out_of_range() {
        let mut session = session();
        let err = run(
            &mut session,
            &cmd("symmetry.set", serde_json::json!({"count": 0})),
        )
        .unwrap();
        assert!(err.is_err());
        let err = run(
            &mut session,
            &cmd("symmetry.set", serde_json::json!({"count": 33})),
        )
        .unwrap();
        assert!(err.is_err());
    }

    fn solid_fill() -> Paint {
        Paint::Solid { colour: crate::paint::Colour::new(200, 40, 40, 255) }
    }

    fn add_rect(session: &mut Session, origin: [f64; 2], size: [f64; 2]) -> ObjectId {
        commit_curve(
            session,
            &[
                origin,
                [origin[0] + size[0], origin[1]],
                [origin[0] + size[0], origin[1] + size[1]],
                [origin[0], origin[1] + size[1]],
            ],
            true,
            false,
            solid_fill(),
            StrokeStyle::default(),
            1.0,
        )
    }

    fn closed_by_id(session: &Session, id: ObjectId) -> Option<Vec<[f64; 2]>> {
        session
            .document
            .get_object(id)
            .and_then(Object::curve)
            .filter(|curve| curve.contour.closed)
            .map(contour_points)
    }

    #[test]
    fn blob_erase_subtracts_from_selected_and_leaves_unselected() {
        let mut session = session();
        let selected = add_rect(&mut session, [0.0, 0.0], [80.0, 40.0]);
        let unselected = add_rect(&mut session, [200.0, 0.0], [80.0, 40.0]);
        let before = closed_by_id(&session, unselected).unwrap();
        session.selection.ids = vec![selected];
        run(
            &mut session,
            &cmd(
                "blob.paint",
                serde_json::json!({"erase": true, "points": [[40.0, -10.0], [40.0, 50.0]], "width": 12.0}),
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(closed_by_id(&session, unselected).unwrap(), before);
        let selected_ring = closed_by_id(&session, selected).unwrap();
        assert!(!strict_point_in_ring([40.0, 20.0], &selected_ring));
        assert!(session.document.get_object(unselected).is_some());
    }

    #[test]
    fn blob_erase_splits_into_multiple_pieces() {
        let mut session = session();
        let id = add_rect(&mut session, [0.0, 0.0], [80.0, 40.0]);
        session.selection.ids = vec![id];
        let result = run(
            &mut session,
            &cmd(
                "blob.paint",
                serde_json::json!({"erase": true, "points": [[40.0, -10.0], [40.0, 50.0]], "width": 12.0}),
            ),
        )
        .unwrap()
        .unwrap();
        let rings = closed_blob_rings(&session);
        assert_eq!(rings.len(), 2);
        assert_eq!(result["ids"].as_array().unwrap().len(), 1);
        assert_eq!(result["ids"][0], id.0);
        assert_eq!(result["created"].as_array().unwrap().len(), 1);
        assert!(session.document.get_object(id).is_some());
        let left = rings.iter().any(|ring| strict_point_in_ring([10.0, 20.0], ring));
        let right = rings.iter().any(|ring| strict_point_in_ring([70.0, 20.0], ring));
        let mid = rings.iter().any(|ring| strict_point_in_ring([40.0, 20.0], ring));
        assert!(left && right);
        assert!(!mid);
    }

    #[test]
    fn blob_erase_full_coverage_removes_curve() {
        let mut session = session();
        let id = add_rect(&mut session, [0.0, 0.0], [40.0, 40.0]);
        let other = add_rect(&mut session, [200.0, 0.0], [40.0, 40.0]);
        session.selection.ids = vec![id];
        let result = run(
            &mut session,
            &cmd(
                "blob.paint",
                serde_json::json!({"erase": true, "points": [[0.0, 20.0], [40.0, 20.0]], "width": 50.0}),
            ),
        )
        .unwrap()
        .unwrap();
        assert!(result["removed"].as_array().unwrap().iter().any(|v| v == id.0));
        assert!(session.document.get_object(id).is_none());
        assert!(session.document.get_object(other).is_some());
        assert_eq!(closed_blob_rings(&session).len(), 1);
    }

    #[test]
    fn blob_erase_is_one_undo_step_and_skips_pixel_layers() {
        let mut session = session();
        let id = add_rect(&mut session, [0.0, 0.0], [80.0, 40.0]);
        session.selection.ids = vec![id];
        let pixels = session.document.pixel_layers.entries.len();
        let before = session.undo.applied_steps();
        let objects = session.document.objects.clone();
        run(
            &mut session,
            &cmd(
                "vector.blob.erase",
                serde_json::json!({"points": [[40.0, -10.0], [40.0, 50.0]], "width": 12.0}),
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(session.undo.applied_steps(), before + 1);
        assert_eq!(session.document.pixel_layers.entries.len(), pixels);
        let undo_cmd = session.undo.undo().expect("undo command");
        crate::commands::feature_restore::run(&mut session, &undo_cmd)
            .unwrap()
            .unwrap();
        assert_eq!(session.document.objects.len(), objects.len());
        assert_eq!(closed_by_id(&session, id).unwrap().len(), contour_points(objects.iter().find(|o| o.id == id).unwrap().curve().unwrap()).len());
        assert!(strict_point_in_ring([40.0, 20.0], &closed_by_id(&session, id).unwrap()));
        assert_eq!(session.document.pixel_layers.entries.len(), pixels);
    }

    #[test]
    fn blob_erase_rejects_empty_selection() {
        let mut session = session();
        add_rect(&mut session, [0.0, 0.0], [40.0, 40.0]);
        let count = session.document.objects.len();
        let steps = session.undo.applied_steps();
        let err = run(
            &mut session,
            &cmd(
                "blob.paint",
                serde_json::json!({"erase": true, "points": [[10.0, 0.0], [10.0, 40.0]], "width": 8.0}),
            ),
        )
        .unwrap();
        assert!(err.is_err());
        assert_eq!(session.document.objects.len(), count);
        assert_eq!(session.undo.applied_steps(), steps);
    }
}
