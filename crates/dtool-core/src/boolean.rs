use crate::curve::{Contour, Curve, PathPoint, PointId};
use crate::paint::{Cap, Join, StrokeAlign, StrokeStyle};
use dtool_protocol::CommandError;
use i_overlay::core::fill_rule::FillRule as OverlayFillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::overlay::FloatOverlay;
use i_overlay::float::slice::FloatSlice;
use i_overlay::mesh::float::stroke::offset::StrokeOffset;
use i_overlay::mesh::float::style::{LineCap, LineJoin, StrokeStyle as OverlayStroke};
use kurbo::{BezPath, PathEl};
use crate::command::Session;
use crate::commands::arrange::{commit_snapshot, snapshot};
use crate::object::{Object, ObjectId, ObjectKind};
use crate::outline::{object_outline, object_outline_world};
use crate::selection::SelectOp;
use dtool_protocol::{Command, CommandResult};
use serde_json::{json, Value};
use serde::{Deserialize, Serialize};

mod builder_cleanup;
mod knife_open;
use builder_cleanup::BuilderCleanup;

type Ring = Vec<[f64; 2]>;
type Shape = Vec<Ring>;

pub const MAX_BOOLEAN_POINTS: usize = 250_000;
pub const DEFAULT_TOLERANCE: f64 = 0.05;
const ROUND_STEP: f64 = 0.1;
const MIN_AREA: f64 = 1e-12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoolOp {
    Add,
    Subtract,
    Intersect,
    Xor,
    Divide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillRule {
    #[default]
    EvenOdd,
    NonZero,
}

impl FillRule {
    fn overlay(self) -> OverlayFillRule {
        match self {
            FillRule::EvenOdd => OverlayFillRule::EvenOdd,
            FillRule::NonZero => OverlayFillRule::NonZero,
        }
    }
}

fn request_too_large() -> CommandError {
    CommandError::new("REQUEST_TOO_LARGE")
}

fn path_is_finite(path: &BezPath) -> bool {
    path.elements().iter().all(|el| match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => p.x.is_finite() && p.y.is_finite(),
        PathEl::QuadTo(a, b) => [a, b].iter().all(|p| p.x.is_finite() && p.y.is_finite()),
        PathEl::CurveTo(a, b, c) => [a, b, c].iter().all(|p| p.x.is_finite() && p.y.is_finite()),
        PathEl::ClosePath => true,
    })
}

fn signed_area(ring: &[[f64; 2]]) -> f64 {
    let mut area = 0.0;
    for (i, a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        area += a[0] * b[1] - b[0] * a[1];
    }
    area * 0.5
}

fn clean_points(points: Vec<[f64; 2]>) -> Ring {
    let mut out: Ring = Vec::with_capacity(points.len());
    for p in points {
        if out.last() != Some(&p) {
            out.push(p);
        }
    }
    while out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

fn flatten_closed(paths: &[BezPath], tolerance: f64, budget: &mut usize) -> Result<Vec<Ring>, CommandError> {
    let mut rings = Vec::new();
    for path in paths {
        if !path_is_finite(path) {
            return Err(CommandError::invalid_params());
        }
        for (points, closed) in crate::outline::flatten_subpaths(path, tolerance) {
            if !closed {
                return Err(CommandError::invalid_params());
            }
            let ring = clean_points(points);
            if ring.len() < 3 || signed_area(&ring).abs() < MIN_AREA {
                return Err(CommandError::invalid_params());
            }
            *budget += ring.len();
            if *budget > MAX_BOOLEAN_POINTS {
                return Err(request_too_large());
            }
            rings.push(ring);
        }
    }
    if rings.is_empty() {
        return Err(CommandError::invalid_params());
    }
    Ok(rings)
}

fn check_tolerance(tolerance: f64) -> Result<(), CommandError> {
    if tolerance.is_finite() && tolerance > 0.0 {
        Ok(())
    } else {
        Err(CommandError::invalid_params())
    }
}

fn normalize(rings: &[Ring], fill: FillRule) -> Vec<Shape> {
    FloatOverlay::with_subj(&rings.to_vec()).overlay(OverlayRule::Subject, fill.overlay())
}

fn combine(a: &[Shape], b: &[Shape], rule: OverlayRule) -> Vec<Shape> {
    if a.is_empty() {
        return match rule {
            OverlayRule::Union | OverlayRule::Xor => b.to_vec(),
            _ => Vec::new(),
        };
    }
    if b.is_empty() {
        return match rule {
            OverlayRule::Intersect => Vec::new(),
            _ => a.to_vec(),
        };
    }
    FloatOverlay::with_subj_and_clip(&a.to_vec(), &b.to_vec()).overlay(rule, OverlayFillRule::NonZero)
}

fn ring_to_contour(ring: &[[f64; 2]], next_id: &mut u64) -> Contour {
    let points = ring
        .iter()
        .map(|pos| {
            let point = PathPoint::sharp(PointId(*next_id), *pos);
            *next_id += 1;
            point
        })
        .collect();
    Contour { points, closed: true }
}

fn shapes_to_curves(shapes: &[Shape]) -> Vec<Curve> {
    shapes.iter().filter_map(|shape| shapes_to_curve(std::slice::from_ref(shape))).collect()
}

fn shapes_to_curve(shapes: &[Shape]) -> Option<Curve> {
    let rings: Vec<&Ring> = shapes.iter().flat_map(|shape| shape.iter()).filter(|ring| ring.len() >= 3).collect();
    if rings.is_empty() {
        return None;
    }
    let mut outer = 0;
    for (index, ring) in rings.iter().enumerate() {
        if signed_area(ring).abs() > signed_area(rings[outer]).abs() {
            outer = index;
        }
    }
    let mut next_id = 1;
    let contour = ring_to_contour(rings[outer], &mut next_id);
    let holes = rings
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != outer)
        .map(|(_, ring)| ring_to_contour(ring, &mut next_id))
        .collect();
    Some(Curve::from_contour_and_holes(contour, holes))
}

fn divide(operands: &[Vec<Shape>]) -> Vec<Vec<Shape>> {
    let mut pieces: Vec<Vec<Shape>> = Vec::new();
    let mut covered: Vec<Shape> = Vec::new();
    for operand in operands {
        let mut next = Vec::new();
        for piece in &pieces {
            let inside = combine(piece, operand, OverlayRule::Intersect);
            let outside = combine(piece, operand, OverlayRule::Difference);
            if !inside.is_empty() {
                next.push(inside);
            }
            if !outside.is_empty() {
                next.push(outside);
            }
        }
        let fresh = combine(operand, &covered, OverlayRule::Difference);
        if !fresh.is_empty() {
            next.push(fresh);
        }
        covered = combine(&covered, operand, OverlayRule::Union);
        pieces = next;
    }
    pieces
}

pub fn boolean_operands(operands: &[Vec<BezPath>], op: BoolOp, fill: FillRule, tolerance: f64) -> Result<Vec<Curve>, CommandError> {
    check_tolerance(tolerance)?;
    if operands.len() < 2 {
        return Err(CommandError::invalid_params());
    }
    let mut budget = 0;
    let mut normalized = Vec::with_capacity(operands.len());
    for paths in operands {
        let rings = flatten_closed(paths, tolerance, &mut budget)?;
        normalized.push(normalize(&rings, fill));
    }
    if op == BoolOp::Divide {
        return Ok(divide(&normalized)
            .iter()
            .flat_map(|piece| piece.iter().filter_map(|shape| shapes_to_curve(std::slice::from_ref(shape))))
            .collect());
    }
    let rule = match op {
        BoolOp::Add => OverlayRule::Union,
        BoolOp::Subtract => OverlayRule::Difference,
        BoolOp::Intersect => OverlayRule::Intersect,
        BoolOp::Xor => OverlayRule::Xor,
        BoolOp::Divide => OverlayRule::Union,
    };
    let mut result = normalized[0].clone();
    for operand in &normalized[1..] {
        result = combine(&result, operand, rule);
    }
    Ok(shapes_to_curves(&result))
}

pub fn boolean(a: &[BezPath], b: &[BezPath], op: BoolOp, fill: FillRule, tolerance: f64) -> Result<Vec<Curve>, CommandError> {
    boolean_operands(&[a.to_vec(), b.to_vec()], op, fill, tolerance)
}

fn overlay_stroke(style: &StrokeStyle) -> Result<OverlayStroke<[f64; 2]>, CommandError> {
    let width = f64::from(style.width);
    if !width.is_finite() || width <= 0.0 {
        return Err(CommandError::invalid_params());
    }
    if !style.dash.is_empty() || style.align != StrokeAlign::Centre || !style.pressure.is_empty() {
        return Err(CommandError::not_implemented());
    }
    let cap = match style.cap {
        Cap::Butt => LineCap::Butt,
        Cap::Round => LineCap::Round(ROUND_STEP),
        Cap::Square => LineCap::Square,
    };
    let join = match style.join {
        Join::Miter { limit } => {
            let limit = f64::from(limit);
            if !limit.is_finite() || limit < 1.0 {
                return Err(CommandError::invalid_params());
            }
            LineJoin::Miter(2.0 * (1.0 / limit).asin())
        }
        Join::Round => LineJoin::Round(ROUND_STEP),
        Join::Bevel => LineJoin::Bevel,
    };
    Ok(OverlayStroke::new(width).start_cap(cap.clone()).end_cap(cap).line_join(join))
}

pub fn expand_stroke(path: &BezPath, style: &StrokeStyle, tolerance: f64) -> Result<Vec<Curve>, CommandError> {
    check_tolerance(tolerance)?;
    let stroke = overlay_stroke(style)?;
    if !path_is_finite(path) {
        return Err(CommandError::invalid_params());
    }
    let mut budget = 0;
    let mut shapes: Vec<Shape> = Vec::new();
    for (points, closed) in crate::outline::flatten_subpaths(path, tolerance) {
        let ring = clean_points(points);
        if ring.len() < 2 {
            continue;
        }
        budget += ring.len();
        if budget > MAX_BOOLEAN_POINTS {
            return Err(request_too_large());
        }
        shapes.extend(ring.stroke(stroke.clone(), closed));
    }
    if shapes.is_empty() {
        return Err(CommandError::invalid_params());
    }
    let merged = FloatOverlay::with_subj(&shapes).overlay(OverlayRule::Subject, OverlayFillRule::NonZero);
    Ok(shapes_to_curve(&merged).into_iter().collect())
}

pub(crate) fn stroke_outline_contours(path: &BezPath) -> Option<Vec<Vec<[f32; 2]>>> {
    if path.elements().is_empty() { return Some(Vec::new()); }
    let rings = flatten_closed(std::slice::from_ref(path), DEFAULT_TOLERANCE, &mut 0).ok()?;
    Some(normalize(&rings, FillRule::NonZero).into_iter().flatten()
        .map(|ring| ring.into_iter().map(|point| point.map(|coord| coord as f32)).collect())
        .collect())
}

pub const BOOLEAN_ADD: &str = "boolean.add";
pub const BOOLEAN_SUBTRACT: &str = "boolean.subtract";
pub const BOOLEAN_INTERSECT: &str = "boolean.intersect";
pub const BOOLEAN_XOR: &str = "boolean.xor";
pub const BOOLEAN_DIVIDE: &str = "boolean.divide";
pub const COMPOUND_RELEASE: &str = "compound.release";
pub const COMPOUND_EXPAND: &str = "compound.expand";
pub const COMPOUND_EDIT: &str = "compound.edit";
pub const STROKE_EXPAND: &str = "stroke.expand";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompoundSpec {
    pub op: BoolOp,
    #[serde(default)]
    pub fill_rule: FillRule,
    #[serde(default)]
    pub origin: [f32; 2],
    #[serde(default)]
    pub operands: Vec<Object>,
}

pub fn compound_curves(spec: &CompoundSpec, tolerance: f64) -> Result<Vec<Curve>, CommandError> {
    let paths = spec
        .operands
        .iter()
        .map(|operand| object_outline_world(operand).map(|path| vec![path]))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| CommandError::new("UNSUPPORTED_SHAPE"))?;
    boolean_operands(&paths, spec.op, spec.fill_rule, tolerance)
}

pub fn compound_outline(spec: &CompoundSpec) -> Option<BezPath> {
    let mut path = BezPath::new();
    for mut curve in compound_curves(spec, DEFAULT_TOLERANCE).ok()? {
        shift_curve(&mut curve, [f64::from(spec.origin[0]), f64::from(spec.origin[1])]);
        for element in curve.to_kurbo().elements() {
            path.push(*element);
        }
    }
    Some(path)
}

fn op_for(name: &str) -> Option<BoolOp> {
    match name {
        BOOLEAN_ADD => Some(BoolOp::Add),
        BOOLEAN_SUBTRACT => Some(BoolOp::Subtract),
        BOOLEAN_INTERSECT => Some(BoolOp::Intersect),
        BOOLEAN_XOR => Some(BoolOp::Xor),
        BOOLEAN_DIVIDE => Some(BoolOp::Divide),
        _ => None,
    }
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    let name = command.name.as_str();
    if op_for(name).is_some() {
        return Some(plan_boolean(session, &command.params).is_ok());
    }
    match name {
        COMPOUND_RELEASE => Some(compound_target(session, &command.params)
            .is_ok_and(|(_, object, _)| release_supported(&object))),
        COMPOUND_EXPAND | COMPOUND_EDIT => Some(compound_target(session, &command.params).is_ok()),
        SHAPE_BUILDER_APPLY => Some(plan_builder(session, &command.params).is_ok()),
        PATH_KNIFE => Some(plan_knife_request(session, &command.params).is_ok()),
        STROKE_EXPAND => Some(plan_stroke(session, &command.params).is_ok()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    let name = command.name.as_str();
    if let Some(op) = op_for(name) {
        return Some(run_boolean(session, name, op, &command.params));
    }
    match name {
        COMPOUND_RELEASE => Some(run_release(session, &command.params)),
        COMPOUND_EXPAND => Some(run_expand(session, &command.params)),
        COMPOUND_EDIT => Some(run_edit_compound(session, &command.params)),
        STROKE_EXPAND => Some(run_stroke(session, &command.params)),
        SHAPE_BUILDER_APPLY => Some(run_shape_builder(session, &command.params)),
        PATH_KNIFE => Some(run_knife(session, &command.params)),
        _ => None,
    }
}

fn targets(session: &Session, params: &Value) -> Vec<ObjectId> {
    if let Some(id) = params.get("id").and_then(Value::as_u64) {
        return vec![ObjectId(id)];
    }
    let mut ids: Vec<ObjectId> = match params.get("ids").or_else(|| params.get("nodes")).and_then(Value::as_array) {
        Some(list) => list.iter().filter_map(Value::as_u64).map(ObjectId).collect(),
        None => session.selection.ids.clone(),
    };
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| seen.insert(*id));
    ids
}

fn tolerance_of(params: &Value) -> f64 {
    params.get("tolerance").and_then(Value::as_f64).unwrap_or(DEFAULT_TOLERANCE)
}

fn fill_rule_of(params: &Value) -> FillRule {
    params.get("fill_rule").and_then(|value| serde_json::from_value(value.clone()).ok()).unwrap_or_default()
}

fn is_fixed(params: &Value) -> bool {
    params.get("fixed").or_else(|| params.get("alt")).and_then(Value::as_bool).unwrap_or(false)
}

fn degenerate(error: CommandError) -> CommandError {
    if error.code == "INVALID_PARAMS" { CommandError::new("BOOLEAN_DEGENERATE") } else { error }
}

struct OperandPlan {
    ids: Vec<ObjectId>,
    paths: Vec<Vec<BezPath>>,
    bottom_index: usize,
    parent: Option<ObjectId>,
    page_id: Option<crate::document::PageId>,
}

fn plan_operands(session: &Session, params: &Value) -> Result<OperandPlan, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let doc = &session.document;
    let mut indexed = Vec::new();
    for id in targets(session, params) {
        let index = doc.objects.iter().position(|object| object.id == id).ok_or_else(CommandError::not_found)?;
        indexed.push(index);
    }
    if indexed.len() < 2 {
        return Err(CommandError::new("NOTHING_SELECTED"));
    }
    indexed.sort_unstable();
    let first = &doc.objects[indexed[0]];
    let parent = first.parent_id;
    let page = crate::data_merge::layout::object_page_index(doc, first.id);
    let page_id = doc.pages.get(page).map(|page| page.id);
    let mut ids = Vec::new();
    let mut paths = Vec::new();
    for index in &indexed {
        let object = &doc.objects[*index];
        if object.locked {
            return Err(CommandError::new("LOCKED"));
        }
        if object.parent_id != parent || crate::data_merge::layout::object_page_index(doc, object.id) != page {
            return Err(CommandError::invalid_params());
        }
        let path = object_outline_world(object).ok_or_else(|| CommandError::new("UNSUPPORTED_SHAPE"))?;
        ids.push(object.id);
        paths.push(vec![path]);
    }
    Ok(OperandPlan { ids, paths, bottom_index: indexed[0], parent, page_id })
}

fn plan_boolean(session: &Session, params: &Value) -> Result<OperandPlan, CommandError> {
    let plan = plan_operands(session, params)?;
    let doc = &session.document;
    if plan.ids.iter().any(|id| {
        let object = doc.get_object(*id).expect("planned operand");
        !object.clip_role.is_none() || object.vector_crop.is_some()
            || doc.layer_masks.iter().any(|mask| mask.object == *id)
            || doc.objects.iter().any(|child| child.parent_id == Some(*id)
                || matches!(&child.kind, ObjectKind::PathText(data) if data.path == *id))
    }) {
        return Err(CommandError::invalid_params());
    }
    Ok(plan)
}

fn curve_bounds(curves: &[Curve]) -> Option<[f64; 4]> {
    let mut bounds: Option<[f64; 4]> = None;
    for point in curves.iter().flat_map(|curve| curve.contours()).flat_map(|contour| contour.points.iter()) {
        let [x, y] = point.pos;
        bounds = Some(match bounds {
            None => [x, y, x, y],
            Some(b) => [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)],
        });
    }
    bounds
}

fn shift_curve(curve: &mut Curve, by: [f64; 2]) {
    transform_curve(curve, kurbo::Affine::translate((-by[0], -by[1])));
}

fn transform_curve(curve: &mut Curve, affine: kurbo::Affine) {
    let transform = |p: &mut [f64; 2]| {
        let transformed = affine * kurbo::Point::new(p[0], p[1]);
        *p = [transformed.x, transformed.y];
    };
    for contour in std::iter::once(&mut curve.contour).chain(curve.holes.iter_mut()) {
        for point in &mut contour.points {
            transform(&mut point.pos);
            if let Some(handle) = point.handle_in.as_mut() {
                transform(handle);
            }
            if let Some(handle) = point.handle_out.as_mut() {
                transform(handle);
            }
        }
    }
}

fn styled(base: &Object, kind: ObjectKind, bounds: [f64; 4], parent: Option<ObjectId>) -> Object {
    let mut object = base.clone();
    object.id = ObjectId(0);
    object.kind = kind;
    object.pos = [bounds[0] as f32, bounds[1] as f32];
    object.size = [(bounds[2] - bounds[0]) as f32, (bounds[3] - bounds[1]) as f32];
    object.parent_id = parent;
    object.rotation_deg = 0.0;
    object.shear_deg = 0.0;
    object.shape = None;
    object.raster_data = None;
    object.raster_size = None;
    object.frame = None;
    object.vector_crop = None;
    object
}

fn place(session: &mut Session, index: usize, object: Object) -> ObjectId {
    let id = session.document.create_object(object);
    if let Some(at) = session.document.objects.iter().position(|o| o.id == id) {
        let object = session.document.objects.remove(at);
        let index = index.min(session.document.objects.len());
        session.document.objects.insert(index, object);
    }
    id
}

fn run_boolean(session: &mut Session, name: &str, op: BoolOp, params: &Value) -> CommandResult {
    let plan = plan_boolean(session, params)?;
    let fill = fill_rule_of(params);
    let curves = boolean_operands(&plan.paths, op, fill, tolerance_of(params)).map_err(degenerate)?;
    let bounds = curve_bounds(&curves).ok_or_else(|| CommandError::new("BOOLEAN_EMPTY"))?;
    let live = op != BoolOp::Divide && !is_fixed(params);
    let operands: Vec<Object> = plan.ids.iter().filter_map(|id| session.document.get_object(*id).cloned()).collect();
    let mut base = operands[0].clone();
    base.page_id = plan.page_id;
    let before = snapshot(session);
    session.document.objects.retain(|object| !plan.ids.contains(&object.id));
    let mut created = Vec::new();
    if live {
        let origin = [bounds[0] as f32, bounds[1] as f32];
        let spec = CompoundSpec { op, fill_rule: fill, origin, operands };
        created.push(place(session, plan.bottom_index, styled(&base, ObjectKind::Compound(spec), bounds, plan.parent)));
    } else {
        for (offset, mut curve) in curves.into_iter().enumerate() {
            let piece = curve_bounds(std::slice::from_ref(&curve)).unwrap_or(bounds);
            shift_curve(&mut curve, [piece[0], piece[1]]);
            created.push(place(session, plan.bottom_index + offset, styled(&base, ObjectKind::Curve(curve), piece, plan.parent)));
        }
    }
    session.selection.set(created.clone(), SelectOp::Replace);
    let created_ids: Vec<u64> = created.iter().map(|id| id.0).collect();
    let deleted_ids: Vec<u64> = plan.ids.iter().map(|id| id.0).collect();
    commit_snapshot(session, name, before, json!({ "created": created_ids, "deleted": deleted_ids, "live": live, "id": created_ids[0] }))
}

fn compound_target(session: &Session, params: &Value) -> Result<(usize, Object, CompoundSpec), CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let ids = targets(session, params);
    let [id] = ids.as_slice() else { return Err(CommandError::invalid_params()) };
    let index = session.document.objects.iter().position(|object| object.id == *id).ok_or_else(CommandError::not_found)?;
    let object = session.document.objects[index].clone();
    if object.locked {
        return Err(CommandError::new("LOCKED"));
    }
    match &object.kind {
        ObjectKind::Compound(spec) => {
            let spec = spec.clone();
            Ok((index, object, spec))
        }
        _ => Err(CommandError::invalid_params()),
    }
}

fn run_release(session: &mut Session, params: &Value) -> CommandResult {
    let (index, compound, spec) = compound_target(session, params)?;
    if !release_supported(&compound) { return Err(CommandError::invalid_params()); }
    let delta = [compound.pos[0] - spec.origin[0], compound.pos[1] - spec.origin[1]];
    let matrix = crate::geometry::object_matrix(&compound);
    let before = snapshot(session);
    session.document.objects.remove(index);
    let mut released = Vec::new();
    for (offset, mut operand) in spec.operands.into_iter().enumerate() {
        let centre = crate::geometry::box_centre(&operand);
        let [x, y] = crate::geometry::apply_matrix(&matrix, [centre[0] + delta[0], centre[1] + delta[1]]);
        operand.pos = [x - operand.size[0].abs() / 2.0, y - operand.size[1].abs() / 2.0];
        operand.rotation_deg += compound.rotation_deg;
        operand.parent_id = compound.parent_id;
        operand.page_id = compound.page_id;
        released.push(operand.id);
        let at = (index + offset).min(session.document.objects.len());
        session.document.objects.insert(at, operand);
    }
    session.document.raise_id_high_water();
    session.selection.set(released.clone(), SelectOp::Replace);
    let ids: Vec<u64> = released.iter().map(|id| id.0).collect();
    commit_snapshot(session, COMPOUND_RELEASE, before, json!({ "released": ids, "deleted": [compound.id.0] }))
}

fn release_supported(object: &Object) -> bool {
    object.rotation_deg.is_finite() && object.shear_deg == 0.0 && object.size.iter().all(|side| *side >= 0.0)
}

fn run_expand(session: &mut Session, params: &Value) -> CommandResult {
    let (index, compound, spec) = compound_target(session, params)?;
    let mut curves = compound_curves(&spec, tolerance_of(params)).map_err(degenerate)?;
    if curves.is_empty() {
        return Err(CommandError::new("BOOLEAN_EMPTY"));
    }
    let mut curve = curves.remove(0);
    for piece in curves {
        curve.holes.push(piece.contour);
        curve.holes.extend(piece.holes);
    }
    let mut next_point = 0;
    for contour in std::iter::once(&mut curve.contour).chain(&mut curve.holes) {
        for point in &mut contour.points {
            next_point += 1;
            point.id = PointId(next_point);
        }
    }
    shift_curve(&mut curve, [f64::from(spec.origin[0]), f64::from(spec.origin[1])]);
    let before = snapshot(session);
    session.document.objects[index].kind = ObjectKind::Curve(curve);
    commit_snapshot(session, COMPOUND_EXPAND, before, json!({ "id": compound.id.0 }))
}

fn run_edit_compound(session: &mut Session, params: &Value) -> CommandResult {
    #[derive(Deserialize)]
    struct OperandMove { id: ObjectId, pos: [f32; 2] }

    let (index, compound, mut spec) = compound_target(session, params)?;
    let old_spec = spec.clone();
    if let Some(value) = params.get("op") {
        let op = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
        if op == BoolOp::Divide { return Err(CommandError::invalid_params()); }
        spec.op = op;
    }
    if let Some(value) = params.get("operands") {
        let moves: Vec<OperandMove> = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
        let mut seen = std::collections::HashSet::new();
        for movement in moves {
            if !seen.insert(movement.id) || movement.pos.iter().any(|value| !value.is_finite()) {
                return Err(CommandError::invalid_params());
            }
            let operand = spec.operands.iter_mut().find(|object| object.id == movement.id)
                .ok_or_else(CommandError::not_found)?;
            if operand.locked { return Err(CommandError::new("LOCKED")); }
            operand.pos = movement.pos;
        }
    }
    if spec.op == old_spec.op && spec.operands.iter().zip(&old_spec.operands).all(|(a, b)| a.pos == b.pos) {
        return Ok(json!({ "id": compound.id, "changes": [] }));
    }
    let id = compound.id;
    let compound = reshape_compound(compound, spec)?;
    let before = snapshot(session);
    session.document.objects[index] = compound;
    commit_snapshot(session, COMPOUND_EDIT, before, json!({ "id": id }))
}

/// Shape an uncommitted operand move for the canvas without touching history.
pub fn preview_compound_move(object: &Object, operand: ObjectId, pos: [f32; 2]) -> Option<Object> {
    let ObjectKind::Compound(spec) = &object.kind else { return None };
    if object.locked || pos.iter().any(|value| !value.is_finite()) { return None; }
    let mut spec = spec.clone();
    let source = spec.operands.iter_mut().find(|source| source.id == operand && !source.locked)?;
    source.pos = pos;
    reshape_compound(object.clone(), spec).ok()
}

pub fn compound_operand_outline(doc: &crate::Document, compound: &Object, spec: &CompoundSpec, operand: &Object) -> Option<BezPath> {
    let path = object_outline_world(operand)?;
    let [x, y] = compound.pos;
    let [ox, oy] = spec.origin;
    let matrix = kurbo::Affine::new(crate::geometry::object_matrix(compound).map(f64::from));
    let path = matrix * kurbo::Affine::translate((f64::from(x - ox), f64::from(y - oy))) * path;
    Some(crate::commands::warp::live_warp_outline(doc, compound.id, &path).unwrap_or(path))
}

fn reshape_compound(mut compound: Object, mut spec: CompoundSpec) -> Result<Object, CommandError> {
    let curves = compound_curves(&spec, DEFAULT_TOLERANCE).map_err(degenerate)?;
    if let Some([x0, y0, x1, y1]) = curve_bounds(&curves) {
        let extent = [(x1 - x0) as f32, (y1 - y0) as f32];
        let offset = [x0 as f32 - spec.origin[0], y0 as f32 - spec.origin[1]];
        let centre = [compound.pos[0] + offset[0] + extent[0] / 2.0, compound.pos[1] + offset[1] + extent[1] / 2.0];
        let [x, y] = crate::geometry::apply_matrix(&crate::geometry::object_matrix(&compound), centre);
        if [x, y].into_iter().chain(extent).any(|value| !value.is_finite()) { return Err(CommandError::invalid_params()); }
        compound.pos = [x - extent[0] / 2.0, y - extent[1] / 2.0];
        compound.size = [extent[0].copysign(compound.size[0]), extent[1].copysign(compound.size[1])];
        let translate = kurbo::Affine::translate((-f64::from(offset[0]), -f64::from(offset[1]))).as_coeffs();
        for paint in std::iter::once(&mut compound.fill).chain(&mut compound.additional_fills) {
            crate::paint::transform_paint_matrix(paint, translate);
        }
        for stroke in std::iter::once(&mut compound.stroke).chain(&mut compound.additional_strokes) {
            crate::paint::transform_stroke_style_matrix(stroke, translate);
        }
        if let crate::paint::Transparency::Gradient { geom, .. } = &mut compound.transparency {
            *geom = geom.transform(translate);
        }
        if let Some(crop) = &mut compound.vector_crop {
            crop.rect[0] -= offset[0];
            crop.rect[1] -= offset[1];
        }
        spec.origin = [x0 as f32, y0 as f32];
    }
    compound.kind = ObjectKind::Compound(spec);
    Ok(compound)
}

struct StrokeExpansion {
    id: ObjectId,
    outline: Curve,
    texture: Option<crate::path_brush::Ink>,
    baked_warps: Vec<ObjectId>,
    keep_body: bool,
}

fn plan_stroke(session: &Session, params: &Value) -> Result<Vec<StrokeExpansion>, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let ids = targets(session, params);
    if ids.is_empty() {
        return Err(CommandError::new("NOTHING_SELECTED"));
    }
    let mut out = Vec::new();
    for id in ids {
        let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
        if object.locked {
            return Err(CommandError::new("LOCKED"));
        }
        if object.stroke.width <= 0.0 || matches!(object.stroke.paint, crate::paint::Paint::None) {
            return Err(CommandError::invalid_params());
        }
        let mut path = object_outline(object).ok_or_else(|| CommandError::new("UNSUPPORTED_SHAPE"))?;
        let mut style = object.stroke.clone();
        let mut baked_warps = Vec::new();
        let mut keep_body = object.clip_role == crate::object::ClipRole::Mask && object.parent_id.is_some();
        let texture = if object.path_brush.is_some() {
            keep_body |= object.fill != crate::paint::Paint::None || crate::commands::appearance::has_layers(object);
            let transform = kurbo::Affine::new(crate::geometry::object_matrix(object).map(f64::from))
                * kurbo::Affine::translate((f64::from(object.pos[0]), f64::from(object.pos[1])));
            path = transform * path;
            if let Some(warped) = crate::commands::warp::live_warp_outline(&session.document, id, &path) {
                path = warped;
            }
            baked_warps = crate::commands::warp::ancestor_warps(&session.document, object)
                .into_iter().map(|(group, _)| group.id).collect();
            style.join = Join::Round;
            Some(crate::path_brush::Ink::new(&session.document, object)
                .ok_or_else(|| CommandError::new("UNSUPPORTED_SHAPE"))?)
        } else {
            None
        };
        let mut curves = expand_stroke(&path, &style, tolerance_of(params))?;
        if curves.is_empty() {
            return Err(CommandError::new("BOOLEAN_EMPTY"));
        }
        out.push(StrokeExpansion { id, outline: curves.remove(0), texture, baked_warps, keep_body });
    }
    Ok(out)
}

fn run_stroke(session: &mut Session, params: &Value) -> CommandResult {
    let plan = plan_stroke(session, params)?;
    let before = snapshot(session);
    let mut document = session.document.clone();
    let mut ids = Vec::new();
    let mut created_layer = false;
    for StrokeExpansion { id, outline: mut curve, texture, baked_warps, keep_body } in plan {
        let texture = if let Some(ink) = texture {
            let [x, y, right, bottom] = curve_bounds(std::slice::from_ref(&curve)).ok_or_else(CommandError::invalid_params)?;
            let origin = [x as f32, y as f32];
            let fill = ink.expanded_fill(&mut document, origin).ok_or_else(CommandError::invalid_params)?;
            shift_curve(&mut curve, [x, y]);
            Some((origin, [(right - x) as f32, (bottom - y) as f32], fill))
        } else {
            None
        };
        let index = document.objects.iter().position(|object| object.id == id).ok_or_else(CommandError::not_found)?;
        let has_pixel_mask = crate::raster_source::compositing_mask(&document, &document.objects[index]).is_some();
        let source = &mut document.objects[index];
        let under = source.paint_order == crate::paint::PaintOrder::StrokeUnder;
        let mut sibling = keep_body.then(|| Object {
            name: dtool_strings::text("layer.expanded_stroke").to_string(),
            pos: source.pos,
            size: source.size,
            parent_id: source.parent_id,
            baked_warps: source.baked_warps.clone(),
            page_id: source.page_id,
            opacity: source.opacity,
            rotation_deg: source.rotation_deg,
            shear_deg: source.shear_deg,
            pickable: source.pickable,
            vector_crop: source.vector_crop,
            clip_role: source.clip_role,
            fill: source.stroke.paint.clone(),
            ..Object::default()
        });
        let object = sibling.as_mut().unwrap_or(source);
        if let Some((origin, size, mut fill)) = texture {
            use kurbo::Affine;
            let matrix = crate::geometry::object_matrix(object);
            let local_to_world = Affine::new(matrix.map(f64::from))
                * Affine::translate((f64::from(object.pos[0]), f64::from(object.pos[1])));
            if has_pixel_mask && !keep_body || object.vector_crop.is_some() && matrix != crate::geometry::IDENTITY {
                // Convert the outline and bitmap back into the object's frame
                // so its crop or pixel mask keeps the same coordinates.
                let inverse = crate::geometry::invert_matrix(&matrix).ok_or_else(CommandError::invalid_params)?;
                let to_local = Affine::translate((-f64::from(object.pos[0]), -f64::from(object.pos[1])))
                    * Affine::new(inverse.map(f64::from))
                    * Affine::translate((f64::from(origin[0]), f64::from(origin[1])));
                transform_curve(&mut curve, to_local);
                if let crate::paint::Paint::Bitmap(bitmap) = &mut fill {
                    bitmap.matrix = (to_local * Affine::new(bitmap.matrix)).as_coeffs();
                }
            } else {
                if let Some(crop) = object.vector_crop.as_mut() {
                    crop.rect[0] += object.pos[0] - origin[0];
                    crop.rect[1] += object.pos[1] - origin[1];
                }
                if let crate::paint::Transparency::Gradient { geom, .. } = &mut object.transparency {
                    let offset = Affine::translate((-f64::from(origin[0]), -f64::from(origin[1])));
                    *geom = geom.transform((offset * local_to_world).as_coeffs());
                }
                object.pos = origin;
                object.size = size;
                object.rotation_deg = 0.0;
                object.shear_deg = 0.0;
            }
            object.path_brush = None;
            object.baked_warps.extend(baked_warps);
            object.fill = fill;
        } else {
            if !has_pixel_mask && object.rotation_deg == 0.0 && object.shear_deg == 0.0
                && let Some(b) = curve_bounds(std::slice::from_ref(&curve))
            {
                shift_curve(&mut curve, [b[0], b[1]]);
                object.pos = [object.pos[0] + b[0] as f32, object.pos[1] + b[1] as f32];
                object.size = [(b[2] - b[0]) as f32, (b[3] - b[1]) as f32];
            }
            if !keep_body {
                object.fill = object.stroke.paint.clone();
            }
        }
        object.stroke.paint = crate::paint::Paint::None;
        object.stroke.width = 0.0;
        object.kind = ObjectKind::Curve(curve);
        object.shape = None;
        if let Some(mut sibling) = sibling {
            // The live texture paints around the entire base appearance.
            source.path_brush = None;
            source.stroke.paint = crate::paint::Paint::None;
            source.stroke.width = 0.0;
            let masks: Vec<_> = crate::clip_mask::vector_masks(&document, id)
                .into_iter().map(|mask| mask.id).collect();
            let body = &document.objects[index];
            let grouped = has_pixel_mask || !masks.is_empty() || !body.blend_mode.is_normal() || body.blend_ranges.is_some()
                || body.clip_role == crate::object::ClipRole::Mask || !body.effects.is_empty()
                || !matches!(body.transparency, crate::paint::Transparency::None);
            if grouped {
                sibling.parent_id = Some(group_stroke_expansion(&mut document, index, &sibling, &masks));
                sibling.clip_role = crate::object::ClipRole::None;
            }
            let expanded_id = document.create_object(sibling);
            let expanded = document.objects.pop().expect("create_object appends");
            document.objects.insert(index + usize::from(grouped) + usize::from(!under), expanded);
            ids.push(expanded_id.0);
            created_layer = true;
        } else {
            ids.push(id.0);
        }
    }
    session.document = document;
    if created_layer {
        session.selection.set(ids.iter().copied().map(ObjectId).collect(), SelectOp::Replace);
    }
    commit_snapshot(session, STROKE_EXPAND, before, json!({ "expanded": ids, "id": ids[0] }))
}

fn group_stroke_expansion(
    doc: &mut crate::Document, index: usize, expanded: &Object, masks: &[ObjectId],
) -> ObjectId {
    let body = &doc.objects[index];
    let [ax, ay, aw, ah] = crate::geometry::object_world_bounds(body);
    let [bx, by, bw, bh] = crate::geometry::object_world_bounds(expanded);
    let pos = [ax.min(bx), ay.min(by)];
    let has_transparency = !matches!(body.transparency, crate::paint::Transparency::None);
    let body_id = body.id;
    let mask_outline = body.clip_role == crate::object::ClipRole::Mask;
    let has_pixel_mask = crate::raster_source::compositing_mask(doc, body).is_some();
    let keep_frame = has_transparency || has_pixel_mask;
    let group = Object {
        name: body.name.clone(),
        kind: ObjectKind::Group,
        parent_id: body.parent_id,
        page_id: body.page_id,
        pickable: body.pickable,
        clip_role: body.clip_role,
        blend_mode: if !keep_frame && body.blend_ranges.is_none() && body.blend_mode.is_normal() {
            crate::blend::BlendMode::PassThrough
        } else {
            body.blend_mode
        },
        blend_ranges: body.blend_ranges.clone(),
        transparency: body.transparency.clone(),
        effects: body.effects.clone(),
        expanded_stroke_effects: true,
        // A gradient is evaluated in the original local coordinate system,
        // including shear and flips that cannot be stored as gradient handles.
        pos: if keep_frame { body.pos } else { pos },
        size: if keep_frame { body.size } else {
            [(ax + aw).max(bx + bw) - pos[0], (ay + ah).max(by + bh) - pos[1]]
        },
        rotation_deg: if keep_frame { body.rotation_deg } else { 0.0 },
        shear_deg: if keep_frame { body.shear_deg } else { 0.0 },
        ..Object::default()
    };
    let id = doc.create_object(group);
    doc.objects[index].parent_id = Some(id);
    doc.objects[index].clip_role = crate::object::ClipRole::None;
    doc.objects[index].stroke_mask_outline = mask_outline;
    doc.objects[index].blend_mode = crate::blend::BlendMode::Normal;
    doc.objects[index].blend_ranges = None;
    doc.objects[index].transparency = crate::paint::Transparency::None;
    doc.objects[index].effects.clear();
    if has_pixel_mask && let Some(mask) = doc.layer_masks.iter_mut().find(|mask| mask.object == body_id) {
        mask.object = id;
    }
    for mask in doc.objects.iter_mut().filter(|node| masks.contains(&node.id)) {
        mask.parent_id = Some(id);
    }
    let group = doc.objects.pop().expect("create_object appends");
    doc.objects.insert(index, group);
    id
}
pub const SHAPE_BUILDER_APPLY: &str = "shape_builder.apply";
pub const PATH_KNIFE: &str = "path.knife";

fn shape_area(shape: &Shape) -> f64 {
    let mut rings = shape.iter();
    let outer = rings.next().map(|ring| signed_area(ring).abs()).unwrap_or(0.0);
    outer - rings.map(|ring| signed_area(ring).abs()).sum::<f64>()
}

pub(crate) fn closed_outline_metrics(paths: &[BezPath], tolerance: f64) -> Result<(f64, f64, f64), CommandError> {
    let rings = flatten_closed(paths, tolerance, &mut 0)?;
    let shapes = normalize(&rings, FillRule::EvenOdd);
    let mut metrics = (0.0, 0.0, 0.0);
    for shape in &shapes {
        metrics.0 += shape_area(shape);
        for (index, ring) in shape.iter().enumerate() {
            let perimeter: f64 = ring.iter().zip(ring.iter().cycle().skip(1)).map(|(a, b)| {
                (a[0] - b[0]).hypot(a[1] - b[1])
            }).sum();
            if index == 0 { metrics.1 += perimeter; } else { metrics.2 += perimeter; }
        }
    }
    Ok(metrics)
}

fn ring_contains(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    for (i, a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    inside
}

fn shape_contains(shape: &Shape, p: [f64; 2]) -> bool {
    shape.iter().filter(|ring| ring_contains(ring, p)).count() % 2 == 1
}

fn interior_point(shape: &Shape) -> Option<[f64; 2]> {
    let outer = shape.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in outer {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    for steps in [4usize, 16, 64] {
        for i in 0..steps {
            for j in 0..steps {
                let p = [x0 + (x1 - x0) * (i as f64 + 0.5) / steps as f64, y0 + (y1 - y0) * (j as f64 + 0.5) / steps as f64];
                if shape_contains(shape, p) {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn piece_object(style: &Object, shapes: &[Shape], parent: Option<ObjectId>) -> Option<Object> {
    let mut curve = shapes_to_curve(shapes)?;
    let bounds = curve_bounds(std::slice::from_ref(&curve))?;
    shift_curve(&mut curve, [bounds[0], bounds[1]]);
    Some(styled(style, ObjectKind::Curve(curve), bounds, parent))
}

fn curve_piece(source: &Object, path: &BezPath, parent: Option<ObjectId>) -> Result<Object, CommandError> {
    let mut curve = Curve::from_kurbo(path);
    let [x, y, right, bottom] = crate::geometry::bez_path_bounds(path).map(f64::from);
    if curve.is_empty() || ![x, y, right, bottom].into_iter().all(f64::is_finite) {
        return Err(CommandError::invalid_params());
    }
    shift_curve(&mut curve, [x, y]);
    let extent = if curve.contour.closed { 0.0 } else { f64::from(crate::commands::curve_edit::MIN_CURVE_EXTENT) };
    let bounds = [x, y, right.max(x + extent), bottom.max(y + extent)];
    Ok(styled(source, ObjectKind::Curve(curve), bounds, parent))
}

fn curve_in_original_frame(source: &Object, path: BezPath) -> Result<Object, CommandError> {
    let matrix = crate::geometry::object_matrix(source);
    let inverse = crate::geometry::invert_matrix(&matrix).ok_or_else(CommandError::invalid_params)?;
    let [x, y] = source.pos.map(f64::from);
    let local = kurbo::Affine::translate((-x, -y)) * kurbo::Affine::new(inverse.map(f64::from));
    let mut replacement = source.clone();
    replacement.kind = ObjectKind::Curve(Curve::from_kurbo(&(local * path)));
    replacement.shape = None;
    Ok(replacement)
}

fn is_vector_mask_layer(object: &Object) -> bool {
    object.clip_role == crate::object::ClipRole::Mask && object.parent_id.is_some()
}

struct BuilderPlan {
    operands: OperandPlan,
    sources: Vec<Object>,
    source_shapes: Vec<Vec<Shape>>,
    open_paths: Vec<Vec<BezPath>>,
    areas: Vec<Shape>,
    picked: Vec<usize>,
    cleanup: BuilderCleanup,
}

fn builder_areas(source_shapes: &[Vec<Shape>]) -> Vec<Shape> {
    divide(source_shapes).into_iter().flatten().filter(|shape| shape_area(shape) > MIN_AREA).collect()
}

pub fn shape_builder_regions(session: &Session) -> Result<Vec<Shape>, CommandError> {
    let operands = plan_operands(session, &Value::Null)?;
    let (shapes, _) = builder_geometry(&operands, &Value::Null)?;
    Ok(builder_areas(&shapes))
}

fn builder_geometry(operands: &OperandPlan, params: &Value) -> Result<(Vec<Vec<Shape>>, Vec<Vec<BezPath>>), CommandError> {
    let (fill, tolerance) = (fill_rule_of(params), tolerance_of(params));
    check_tolerance(tolerance).map_err(degenerate)?;
    let mut budget = 0;
    let mut shapes = Vec::new();
    let mut open_paths = Vec::new();
    for paths in &operands.paths {
        let mut rings = Vec::new();
        let mut open = Vec::new();
        for path in paths {
            if !path_is_finite(path) {
                return Err(degenerate(CommandError::invalid_params()));
            }
            for subpath in path.elements().split_inclusive(|el| matches!(el, PathEl::ClosePath)) {
                let mut starts = subpath.iter().enumerate().filter_map(|(index, el)| matches!(el, PathEl::MoveTo(_)).then_some(index)).peekable();
                while let Some(start) = starts.next() {
                    let end = starts.peek().copied().unwrap_or(subpath.len());
                    let part = BezPath::from_vec(subpath[start..end].to_vec());
                    if matches!(part.elements().last(), Some(PathEl::ClosePath)) {
                        rings.extend(flatten_closed(&[part], tolerance, &mut budget).map_err(degenerate)?);
                    } else if start + 1 < end {
                        budget += end - start;
                        if budget > MAX_BOOLEAN_POINTS { return Err(request_too_large()); }
                        open.push(part);
                    }
                }
            }
        }
        shapes.push(if rings.is_empty() { Vec::new() } else { normalize(&rings, fill) });
        open_paths.push(open);
    }
    Ok((shapes, open_paths))
}

fn plan_builder(session: &Session, params: &Value) -> Result<BuilderPlan, CommandError> {
    let operands = plan_operands(session, params)?;
    let sources: Vec<Object> = operands.ids.iter().filter_map(|id| session.document.get_object(*id).cloned()).collect();
    let (source_shapes, open_paths) = builder_geometry(&operands, params)?;
    let areas = builder_areas(&source_shapes);
    let mut picked: Vec<usize> = Vec::new();
    if let Some(list) = params.get("areas").and_then(Value::as_array) {
        for value in list {
            let index = value.as_u64().map(|i| i as usize).filter(|i| *i < areas.len()).ok_or_else(|| CommandError::new("BOOLEAN_DEGENERATE"))?;
            if !picked.contains(&index) {
                picked.push(index);
            }
        }
    }
    if let Some(list) = params.get("points").and_then(Value::as_array) {
        for value in list {
            let p: [f64; 2] = serde_json::from_value(value.clone()).map_err(|_| CommandError::new("BOOLEAN_DEGENERATE"))?;
            let index = areas.iter().position(|shape| shape_contains(shape, p)).ok_or_else(|| CommandError::new("BOOLEAN_DEGENERATE"))?;
            if !picked.contains(&index) {
                picked.push(index);
            }
        }
    }
    if picked.is_empty() {
        return Err(CommandError::new("BOOLEAN_DEGENERATE"));
    }
    match params.get("op").and_then(Value::as_str).unwrap_or("add") {
        "add" | "delete" | "create" => {}
        _ => return Err(CommandError::invalid_params()),
    }
    let cleanup = BuilderCleanup::parse(params)?;
    Ok(BuilderPlan { operands, sources, source_shapes, open_paths, areas, picked, cleanup })
}

fn covering_source<'a>(plan: &'a BuilderPlan, area: &Shape) -> Option<&'a Object> {
    let p = interior_point(area)?;
    plan.sources.iter().zip(plan.source_shapes.iter()).rev().find(|(_, shapes)| shapes.iter().any(|shape| shape_contains(shape, p))).map(|(source, _)| source)
}

fn run_shape_builder(session: &mut Session, params: &Value) -> CommandResult {
    let plan = plan_builder(session, params)?;
    let op = params.get("op").and_then(Value::as_str).unwrap_or("add").to_string();
    let first_style = params.get("first_style").and_then(Value::as_bool).unwrap_or(true);
    let parent = plan.operands.parent;
    let mut default_style = Object::default();
    if !first_style {
        crate::commands::defaults::current().apply_in_document(&mut default_style, session.document.dpi);
    }
    let merged_style = if first_style { covering_source(&plan, &plan.areas[plan.picked[0]]).unwrap_or(&default_style).clone() } else { default_style.clone() };
    let mut merged: Vec<Shape> = Vec::new();
    for index in &plan.picked {
        merged = combine(&merged, std::slice::from_ref(&plan.areas[*index]), OverlayRule::Union);
    }
    let mut outputs: Vec<Object> = Vec::new();
    if op != "create" && plan.cleanup != BuilderCleanup::Unused {
        for (index, area) in plan.areas.iter().enumerate() {
            if plan.picked.contains(&index) {
                continue;
            }
            let style = covering_source(&plan, area).unwrap_or(&plan.sources[0]).clone();
            outputs.extend(piece_object(&style, std::slice::from_ref(area), parent));
        }
    }
    if op != "delete" {
        let object = piece_object(&merged_style, &merged, parent).ok_or_else(|| CommandError::new("BOOLEAN_DEGENERATE"))?;
        outputs.push(object);
    }
    let mut deleted: Vec<ObjectId> = Vec::new();
    if op != "create" {
        let mut boundary = builder_cleanup::BuilderBoundary::new(&merged, plan.cleanup);
        for ((source, closed), open) in plan.sources.iter().zip(&plan.source_shapes).zip(&plan.open_paths) {
            let mut changed = false;
            let mut remaining = Vec::new();
            for path in open {
                match boundary.cleanup(path, plan.cleanup)? {
                    Some(pieces) => { changed = true; remaining.extend(pieces); }
                    None => remaining.push(path.clone()),
                }
            }
            if !closed.is_empty() || changed {
                deleted.push(source.id);
                for path in &remaining {
                    outputs.push(curve_piece(source, path, parent)?);
                }
            }
        }
    }
    let before = snapshot(session);
    let mut index = plan.operands.bottom_index;
    if op == "create" {
        index = plan.operands.ids.iter().filter_map(|id| session.document.objects.iter().position(|o| o.id == *id)).max().unwrap_or(index) + 1;
    } else {
        session.document.objects.retain(|object| !deleted.contains(&object.id));
    }
    let mut created = Vec::new();
    for (offset, mut object) in outputs.into_iter().enumerate() {
        object.page_id = plan.operands.page_id;
        created.push(place(session, index + offset, object));
    }
    session.selection.set(created.clone(), SelectOp::Replace);
    let created_ids: Vec<u64> = created.iter().map(|id| id.0).collect();
    let deleted_ids: Vec<u64> = deleted.iter().map(|id| id.0).collect();
    commit_snapshot(session, SHAPE_BUILDER_APPLY, before, json!({ "created": created_ids, "deleted": deleted_ids, "areas": plan.areas.len() }))
}

fn plan_knife(session: &Session, mut cut: Vec<[f64; 2]>, params: &Value) -> Result<Vec<(ObjectId, Vec<Object>)>, CommandError> {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    if cut.len() < 2 || cut.iter().flatten().any(|coordinate| !coordinate.is_finite()) {
        return Err(CommandError::invalid_params());
    }
    if cut.len() > MAX_BOOLEAN_POINTS {
        return Err(request_too_large());
    }
    cut.dedup();
    if cut.len() < 2 {
        return Err(CommandError::new("NOTHING_TO_CUT"));
    }
    let close_curves = params.get("close_curves").and_then(Value::as_bool)
        .or_else(|| session.tool_options.flag("tool.knife", "knife.close_curves")).unwrap_or(true);
    let mut open_cut = knife_open::KnifeCut::new(&cut);
    let tolerance = tolerance_of(params);
    check_tolerance(tolerance)?;
    let mut out = Vec::new();
    for id in targets(session, params) {
        let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
        if object.locked {
            return Err(CommandError::new("LOCKED"));
        }
        let vector_mask = is_vector_mask_layer(object);
        if vector_mask && !close_curves {
            continue;
        }
        let Some(path) = object_outline_world(object).filter(path_is_finite) else { continue };
        let (closed, open) = knife_open::partition(&path);
        let mut budget = 0;
        let closed_regions = if close_curves {
            flatten_closed(std::slice::from_ref(&closed), tolerance, &mut budget).ok()
            .map(|rings| normalize(&rings, FillRule::EvenOdd))
            .unwrap_or_default()
        } else {
            Vec::new()
        };
        let closed_pieces: Vec<_> = if closed_regions.is_empty() {
            Vec::new()
        } else {
            closed_regions.slice_by_as::<i64>(&cut, OverlayFillRule::EvenOdd)
                .into_iter().filter(|shape| shape_area(shape) > MIN_AREA).collect()
        };
        let closed_split = closed_pieces.len() > closed_regions.len();
        let (open_split, open_pieces) = open_cut.split(open)?;
        let outlines = if !close_curves {
            open_cut.open_closed(&closed)?
        } else if closed_regions.len() == 1 && closed_regions[0].len() == 1 && closed_pieces.len() == 2 {
            open_cut.close_straight_cut(&closed)?
        } else {
            None
        };
        if !closed_split && !open_split && outlines.is_none() {
            continue;
        }
        let mut pieces = Vec::new();
        if let Some(outlines) = outlines {
            for outline in &outlines {
                pieces.push(curve_piece(object, outline, object.parent_id)?);
            }
        } else if closed_split {
            for piece in &closed_pieces {
                let object = piece_object(object, std::slice::from_ref(piece), object.parent_id)
                    .ok_or_else(|| CommandError::new("BOOLEAN_DEGENERATE"))?;
                pieces.push(object);
            }
        } else if !closed.is_empty() {
            pieces.push(curve_piece(object, &closed, object.parent_id)?);
        }
        for path in &open_pieces {
            pieces.push(curve_piece(object, path, object.parent_id)?);
        }
        if vector_mask {
            let mut combined = BezPath::new();
            for piece in &pieces {
                let outline = object_outline_world(piece).ok_or_else(CommandError::invalid_params)?;
                combined.extend(outline.elements().iter().copied());
            }
            pieces = vec![curve_in_original_frame(object, combined)?];
        }
        out.push((id, pieces));
    }
    if out.is_empty() {
        return Err(CommandError::new("NOTHING_TO_CUT"));
    }
    Ok(out)
}

fn knife_prepared_cut(session: &Session, params: &Value) -> Result<Vec<[f64; 2]>, CommandError> {
    let cut: Vec<[f64; 2]> = params.get("cut").cloned().and_then(|v| serde_json::from_value(v).ok()).ok_or_else(CommandError::invalid_params)?;
    let auto_close = params
        .get("auto_close")
        .and_then(Value::as_bool)
        .or_else(|| session.tool_options.flag("tool.knife", "knife.auto_close"))
        .unwrap_or(true);
    let smoothness = params
        .get("smoothness")
        .and_then(Value::as_f64)
        .or_else(|| session.tool_options.number("tool.knife", "knife.smoothness"))
        .unwrap_or(0.0);
    let stabiliser = params.get("stabiliser").and_then(Value::as_f64)
        .or_else(|| session.tool_options.number("tool.knife", "knife.stabiliser")).unwrap_or(0.0);
    Ok(crate::context_toolbar::prepare_knife_cut(cut, auto_close, smoothness, stabiliser))
}

fn plan_knife_request(session: &Session, params: &Value) -> Result<Vec<(ObjectId, Vec<Object>)>, CommandError> {
    if let Some(location) = params.get("point") {
        if !session.has_document {
            return Err(CommandError::no_document());
        }
        let [x, y]: [f64; 2] = serde_json::from_value(location.clone()).map_err(|_| CommandError::invalid_params())?;
        let radius = params.get("hit_radius").and_then(Value::as_f64).unwrap_or(4.0);
        if params.get("cut").is_some() || ![x, y, radius].into_iter().all(f64::is_finite) || radius <= 0.0 {
            return Err(CommandError::invalid_params());
        }
        let mut nearest = None;
        for id in targets(session, params) {
            let source = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
            if source.locked { continue; }
            let Some(path) = object_outline_world(source).filter(path_is_finite) else { continue };
            if path.elements().len() > MAX_BOOLEAN_POINTS { return Err(request_too_large()); }
            if let Some((distance, result)) = knife_open::nearest_scissor(&path, kurbo::Point::new(x, y), radius) {
                if nearest.as_ref().is_none_or(|(best, _, _)| distance < *best) {
                    nearest = Some((distance, source, result));
                }
            }
        }
        let (_, source, path) = nearest.ok_or_else(|| CommandError::new("NOTHING_TO_CUT"))?;
        let replacement = if is_vector_mask_layer(source)
            || crate::raster_source::compositing_mask(&session.document, source).is_some() {
            curve_in_original_frame(source, path)?
        } else {
            curve_piece(source, &path, source.parent_id)?
        };
        return Ok(vec![(source.id, vec![replacement])]);
    }
    let cut = knife_prepared_cut(session, params)?;
    plan_knife(session, cut, params)
}

fn run_knife(session: &mut Session, params: &Value) -> CommandResult {
    let plan = plan_knife_request(session, params)?;
    let before = snapshot(session);
    if params.get("point").is_some() {
        let (id, pieces) = plan.into_iter().next().ok_or_else(CommandError::not_found)?;
        let mut replacement = pieces.into_iter().next().ok_or_else(CommandError::not_found)?;
        let object = session.document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
        replacement.id = id;
        *object = replacement;
        session.selection.set(vec![id], SelectOp::Replace);
        return commit_snapshot(session, PATH_KNIFE, before, json!({ "created": [], "deleted": [], "updated": [id.0], "unused": [] }));
    }
    let mut created = Vec::new();
    let mut deleted = Vec::new();
    let mut updated = Vec::new();
    for (id, pieces) in plan.into_iter().rev() {
        let Some(index) = session.document.objects.iter().position(|o| o.id == id) else { continue };
        let source = &session.document.objects[index];
        if is_vector_mask_layer(source) {
            if let Some(replacement) = pieces.into_iter().next() {
                session.document.objects[index] = replacement;
                updated.push(id);
            }
            continue;
        }
        // A vector mask is a child layer. Keep its parent ID so the mask still
        // clips the split pieces as a single object.
        let masked = crate::raster_source::compositing_mask(&session.document, source).is_some()
            || !crate::clip_mask::vector_masks(&session.document, id).is_empty();
        if masked {
            let name = if crate::artboard::is_imported_artboard_name(&source.name) {
                format!("{} ({})", dtool_strings::text("artboard.group"), source.name)
            } else {
                source.name.clone()
            };
            session.document.objects[index] = Object {
                id,
                kind: ObjectKind::Group,
                name,
                pos: source.pos,
                size: source.size,
                rotation_deg: source.rotation_deg,
                shear_deg: source.shear_deg,
                parent_id: source.parent_id,
                page_id: source.page_id,
                pickable: source.pickable,
                clip_role: source.clip_role,
                opacity: source.opacity,
                blend_mode: source.blend_mode.clone(),
                blend_ranges: source.blend_ranges.clone(),
                transparency: source.transparency.clone(),
                ..Object::default()
            };
        } else {
            session.document.objects.remove(index);
            deleted.push(id.0);
        }
        for (offset, mut piece) in pieces.into_iter().enumerate() {
            if masked {
                piece.parent_id = Some(id);
                piece.clip_role = crate::object::ClipRole::None;
                piece.opacity = 1.0;
                piece.blend_mode = crate::blend::BlendMode::Normal;
                piece.blend_ranges = None;
                piece.transparency = crate::paint::Transparency::None;
            }
            created.push(place(session, index + usize::from(masked) + offset, piece));
        }
    }
    session.selection.set(created.iter().chain(&updated).copied().collect(), SelectOp::Replace);
    let created_ids: Vec<u64> = created.iter().map(|id| id.0).collect();
    let updated_ids: Vec<u64> = updated.iter().map(|id| id.0).collect();
    commit_snapshot(session, PATH_KNIFE, before, json!({ "created": created_ids, "deleted": deleted, "updated": updated_ids, "unused": [] }))
}
#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{Rect, Shape as _};

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> BezPath {
        Rect::new(x0, y0, x1, y1).to_path(0.1)
    }

    fn area(curve: &Curve) -> f64 {
        curve.contours().map(|c| signed_area(&c.points.iter().map(|p| p.pos).collect::<Vec<_>>())).sum::<f64>().abs()
    }

    #[test]
    fn subtract_inner_square_keeps_one_curve_with_hole() {
        let out = boolean(&[rect(0.0, 0.0, 10.0, 10.0)], &[rect(3.0, 3.0, 7.0, 7.0)], BoolOp::Subtract, FillRule::EvenOdd, DEFAULT_TOLERANCE).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].hole_count(), 1);
        assert!((area(&out[0]) - 84.0).abs() < 1e-6);
        assert!(out[0].contour.closed && out[0].holes[0].closed);
    }

    #[test]
    fn source_hole_survives_add() {
        let mut ring = rect(0.0, 0.0, 10.0, 10.0);
        ring.extend(rect(3.0, 3.0, 7.0, 7.0).elements().iter().copied());
        let out = boolean(&[ring], &[rect(20.0, 0.0, 25.0, 5.0)], BoolOp::Add, FillRule::EvenOdd, DEFAULT_TOLERANCE).unwrap();
        assert_eq!(out.len(), 2);
        let donut = out.iter().find(|curve| curve.hole_count() == 1).expect("donut");
        let extra = out.iter().find(|curve| curve.hole_count() == 0).expect("added piece");
        assert_eq!(donut.contours().count(), 2);
        assert!((area(donut) - 84.0).abs() < 1e-6);
        assert!((area(extra) - 25.0).abs() < 1e-6);
    }

    #[test]
    fn disjoint_add_returns_two_curves_and_divide_is_n_curves() {
        let a = vec![rect(0.0, 0.0, 4.0, 4.0)];
        let b = vec![rect(10.0, 0.0, 14.0, 4.0)];
        let added = boolean(&a, &b, BoolOp::Add, FillRule::EvenOdd, DEFAULT_TOLERANCE).unwrap();
        assert_eq!(added.len(), 2);
        assert!(added.iter().all(|curve| curve.hole_count() == 0 && curve.contour.closed));
        let total: f64 = added.iter().map(area).sum();
        assert!((total - 32.0).abs() < 1e-6);
        let overlapping = boolean(&[rect(0.0, 0.0, 4.0, 4.0)], &[rect(2.0, 0.0, 6.0, 4.0)], BoolOp::Divide, FillRule::EvenOdd, DEFAULT_TOLERANCE).unwrap();
        assert_eq!(overlapping.len(), 3);
        let total: f64 = overlapping.iter().map(area).sum();
        assert!((total - 24.0).abs() < 1e-6);
    }

    #[test]
    fn disjoint_subtract_returns_two_pieces() {
        let out = boolean(&[rect(0.0, 0.0, 80.0, 40.0)], &[rect(34.0, -10.0, 46.0, 50.0)], BoolOp::Subtract, FillRule::EvenOdd, DEFAULT_TOLERANCE).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|curve| curve.hole_count() == 0 && curve.contour.closed));
        let total: f64 = out.iter().map(area).sum();
        assert!((total - (80.0 * 40.0 - 12.0 * 40.0)).abs() < 1.0);
    }

    #[test]
    fn intersect_and_xor_areas() {
        let a = [rect(0.0, 0.0, 4.0, 4.0)];
        let b = [rect(2.0, 0.0, 6.0, 4.0)];
        let inter = boolean(&a, &b, BoolOp::Intersect, FillRule::EvenOdd, DEFAULT_TOLERANCE).unwrap();
        assert!((area(&inter[0]) - 8.0).abs() < 1e-6);
        let xor = boolean(&a, &b, BoolOp::Xor, FillRule::EvenOdd, DEFAULT_TOLERANCE).unwrap();
        assert_eq!(xor.len(), 2);
        let xor_area: f64 = xor.iter().map(area).sum();
        assert!((xor_area - 16.0).abs() < 1e-6);
        let none = boolean(&a, &[rect(10.0, 10.0, 12.0, 12.0)], BoolOp::Intersect, FillRule::EvenOdd, DEFAULT_TOLERANCE).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn bad_input_is_typed_error() {
        let mut nan = BezPath::new();
        nan.move_to((0.0, 0.0));
        nan.line_to((f64::NAN, 1.0));
        nan.line_to((1.0, 1.0));
        nan.close_path();
        let good = [rect(0.0, 0.0, 1.0, 1.0)];
        assert_eq!(boolean(&[nan], &good, BoolOp::Add, FillRule::EvenOdd, DEFAULT_TOLERANCE).unwrap_err().code, "INVALID_PARAMS");
        let mut open = BezPath::new();
        open.move_to((0.0, 0.0));
        open.line_to((5.0, 0.0));
        open.line_to((5.0, 5.0));
        assert_eq!(boolean(&[open], &good, BoolOp::Add, FillRule::EvenOdd, DEFAULT_TOLERANCE).unwrap_err().code, "INVALID_PARAMS");
        let mut point = BezPath::new();
        point.move_to((1.0, 1.0));
        point.close_path();
        assert!(boolean(&[point], &good, BoolOp::Add, FillRule::EvenOdd, DEFAULT_TOLERANCE).is_err());
        assert!(boolean_operands(&[good.to_vec()], BoolOp::Add, FillRule::EvenOdd, DEFAULT_TOLERANCE).is_err());
        assert!(boolean(&good, &good, BoolOp::Add, FillRule::EvenOdd, 0.0).is_err());
    }

    #[test]
    fn expand_open_line_gives_closed_band() {
        let mut line = BezPath::new();
        line.move_to((0.0, 0.0));
        line.line_to((100.0, 0.0));
        let style = StrokeStyle { width: 10.0, ..StrokeStyle::default() };
        let out = expand_stroke(&line, &style, DEFAULT_TOLERANCE).unwrap();
        assert_eq!(out.len(), 1);
        assert!(out[0].contour.closed);
        let bounds = out[0].bounds();
        assert!((bounds[3] - bounds[1] - 10.0).abs() < 1e-3 || (bounds[3] - 10.0).abs() < 1e-3);
        assert!((area(&out[0]) - 1000.0).abs() < 1e-3);
        let zero = StrokeStyle { width: 0.0, ..StrokeStyle::default() };
        assert_eq!(expand_stroke(&line, &zero, DEFAULT_TOLERANCE).unwrap_err().code, "INVALID_PARAMS");
        let dashed = StrokeStyle { width: 2.0, dash: vec![4.0, 2.0], ..StrokeStyle::default() };
        assert_eq!(expand_stroke(&line, &dashed, DEFAULT_TOLERANCE).unwrap_err().code, "NOT_IMPLEMENTED");
    }
}

#[cfg(test)]
mod command_contracts {
    use super::*;
    use crate::{Scope, Workspace};

    fn ring_area(points: &[[f64; 2]]) -> f64 {
        signed_area(points).abs()
    }

    fn outer_area(object: &Object) -> f64 {
        match &object.kind {
            ObjectKind::Curve(curve) => ring_area(&curve.contour.points.iter().map(|p| p.pos).collect::<Vec<_>>()),
            ObjectKind::Compound(spec) => compound_curves(spec, DEFAULT_TOLERANCE).unwrap().iter().map(|c| ring_area(&c.contour.points.iter().map(|p| p.pos).collect::<Vec<_>>())).sum(),
            _ => 0.0,
        }
    }

    fn setup(rects: &[[f32; 4]]) -> (Workspace, crate::ContextId, Vec<ObjectId>) {
        let mut ws = Workspace::default();
        let id = ws.new_document("a");
        let session = &mut ws.get_mut(id).unwrap().session;
        let ids: Vec<ObjectId> = rects
            .iter()
            .map(|r| session.document.create_object(Object { name: "r".into(), pos: [r[0], r[1]], size: [r[2], r[3]], ..Object::default() }))
            .collect();
        session.selection.set(ids.clone(), SelectOp::Replace);
        (ws, id, ids)
    }

    fn exec(ws: &mut Workspace, id: crate::ContextId, name: &str, params: Value) -> CommandResult {
        ws.run(Scope::of(id), Command { name: name.into(), params }).1
    }

    fn doc(ws: &Workspace, id: crate::ContextId) -> Value {
        let mut value = serde_json::to_value(&ws.get(id).unwrap().session.document).unwrap();
        value.as_object_mut().unwrap().remove("version");
        value.as_object_mut().unwrap().remove("next_object_id");
        value
    }

    #[test]
    fn plain_add_is_live_compound_keeping_operands_and_undoes() {
        let (mut ws, id, ids) = setup(&[[0.0, 0.0, 10.0, 10.0], [5.0, 0.0, 10.0, 10.0]]);
        let before = doc(&ws, id);
        let reply = exec(&mut ws, id, BOOLEAN_ADD, json!({})).unwrap();
        assert_eq!(reply["live"], json!(true));
        let objects = &ws.get(id).unwrap().session.document.objects;
        assert_eq!(objects.len(), 1);
        match &objects[0].kind {
            ObjectKind::Compound(spec) => {
                assert_eq!(spec.op, BoolOp::Add);
                assert_eq!(spec.operands.iter().map(|o| o.id).collect::<Vec<_>>(), ids);
            }
            other => panic!("not a compound: {other:?}"),
        }
        assert!((outer_area(&objects[0]) - 150.0).abs() < 0.5);
        assert!(compound_outline(match &objects[0].kind { ObjectKind::Compound(s) => s, _ => unreachable!() }).is_some());
        let after = doc(&ws, id);
        exec(&mut ws, id, "edit.undo", json!({})).unwrap();
        assert_eq!(doc(&ws, id), before);
        exec(&mut ws, id, "edit.redo", json!({})).unwrap();
        assert_eq!(doc(&ws, id), after);
    }

    #[test]
    fn alt_add_is_one_fixed_curve() {
        let (mut ws, id, _) = setup(&[[0.0, 0.0, 10.0, 10.0], [5.0, 0.0, 10.0, 10.0]]);
        exec(&mut ws, id, BOOLEAN_ADD, json!({ "fixed": true })).unwrap();
        let objects = &ws.get(id).unwrap().session.document.objects;
        assert_eq!(objects.len(), 1);
        assert!(matches!(objects[0].kind, ObjectKind::Curve(_)));
        assert_eq!(objects[0].pos, [0.0, 0.0]);
        assert_eq!(objects[0].size, [15.0, 10.0]);
        assert!((outer_area(&objects[0]) - 150.0).abs() < 0.5);
    }

    #[test]
    fn live_subtract_intersect_xor_and_alt_variants() {
        for (name, area) in [(BOOLEAN_SUBTRACT, 50.0), (BOOLEAN_INTERSECT, 50.0)] {
            for fixed in [false, true] {
                let (mut ws, id, _) = setup(&[[0.0, 0.0, 10.0, 10.0], [5.0, 0.0, 10.0, 10.0]]);
                exec(&mut ws, id, name, json!({ "fixed": fixed })).unwrap();
                let objects = &ws.get(id).unwrap().session.document.objects;
                assert_eq!(objects.len(), 1, "{name} {fixed}");
                assert_eq!(matches!(objects[0].kind, ObjectKind::Compound(_)), !fixed);
                assert!((outer_area(&objects[0]) - area).abs() < 0.5, "{name} {fixed}");
            }
        }
        let (mut ws, id, _) = setup(&[[0.0, 0.0, 10.0, 10.0], [5.0, 0.0, 10.0, 10.0]]);
        let before = doc(&ws, id);
        exec(&mut ws, id, BOOLEAN_XOR, json!({ "fixed": true })).unwrap();
        let objects = &ws.get(id).unwrap().session.document.objects;
        assert_eq!(objects.len(), 2);
        assert!(objects.iter().all(|object| matches!(object.kind, ObjectKind::Curve(_))));
        let total: f64 = objects.iter().map(outer_area).sum();
        assert!((total - 100.0).abs() < 0.5);
        exec(&mut ws, id, "edit.undo", json!({})).unwrap();
        assert_eq!(doc(&ws, id), before);
    }

    #[test]
        fn divide_makes_n_objects_even_without_alt() {
        let (mut ws, id, _) = setup(&[[0.0, 0.0, 10.0, 10.0], [5.0, 0.0, 10.0, 10.0]]);
        let reply = exec(&mut ws, id, BOOLEAN_DIVIDE, json!({})).unwrap();
        assert_eq!(reply["live"], json!(false));
        let session = &ws.get(id).unwrap().session;
        assert_eq!(session.document.objects.len(), 3);
        assert!(session.document.objects.iter().all(|o| matches!(o.kind, ObjectKind::Curve(_))));
        assert_eq!(session.selection.ids.len(), 3);
        let total: f64 = session.document.objects.iter().map(outer_area).sum();
        assert!((total - 150.0).abs() < 0.5);
    }

    #[test]
    fn disjoint_intersect_is_empty_and_changes_nothing() {
        let (mut ws, id, _) = setup(&[[0.0, 0.0, 10.0, 10.0], [50.0, 0.0, 10.0, 10.0]]);
        let before = doc(&ws, id);
        let generation = ws.get(id).unwrap().session.generation;
        let error = exec(&mut ws, id, BOOLEAN_INTERSECT, json!({ "fixed": true })).unwrap_err();
        assert_eq!(error.code, "BOOLEAN_EMPTY");
        assert_eq!(doc(&ws, id), before);
        assert_eq!(ws.get(id).unwrap().session.generation, generation);
        let (mut ws, id, _) = setup(&[[0.0, 0.0, 10.0, 10.0]]);
        assert!(exec(&mut ws, id, BOOLEAN_ADD, json!({})).is_err());
    }

    #[test]
    fn moved_live_compound_moves_result_and_release_restores_operands() {
        let (mut ws, id, ids) = setup(&[[0.0, 0.0, 10.0, 10.0], [5.0, 0.0, 10.0, 10.0]]);
        let compound = exec(&mut ws, id, BOOLEAN_ADD, json!({})).unwrap()["id"].as_u64().map(ObjectId).unwrap();
        ws.get_mut(id).unwrap().session.document.find_object_mut(compound.0).unwrap().pos = [100.0, 20.0];
        exec(&mut ws, id, COMPOUND_RELEASE, json!({ "id": compound.0 })).unwrap();
        let session = &ws.get(id).unwrap().session;
        assert_eq!(session.document.objects.iter().map(|o| o.id).collect::<Vec<_>>(), ids);
        assert_eq!(session.document.get_object(ids[0]).unwrap().pos, [100.0, 20.0]);
        assert_eq!(session.document.get_object(ids[1]).unwrap().pos, [105.0, 20.0]);
        assert_eq!(session.selection.ids, ids);
    }

    #[test]
    fn expand_turns_live_compound_into_one_curve_in_place() {
        let (mut ws, id, _) = setup(&[[0.0, 0.0, 10.0, 10.0], [5.0, 0.0, 10.0, 10.0]]);
        let compound = exec(&mut ws, id, BOOLEAN_ADD, json!({})).unwrap()["id"].as_u64().map(ObjectId).unwrap();
        exec(&mut ws, id, COMPOUND_EXPAND, json!({ "id": compound.0 })).unwrap();
        let objects = &ws.get(id).unwrap().session.document.objects;
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].id, compound);
        assert!(matches!(objects[0].kind, ObjectKind::Curve(_)));
        assert!((outer_area(&objects[0]) - 150.0).abs() < 0.5);
        assert!(exec(&mut ws, id, COMPOUND_RELEASE, json!({ "id": compound.0 })).is_err());
    }

    #[test]
    fn stroke_expand_makes_filled_band_from_stroke() {
        let (mut ws, id, ids) = setup(&[[0.0, 0.0, 100.0, 10.0]]);
        {
            let object = ws.get_mut(id).unwrap().session.document.find_object_mut(ids[0].0).unwrap();
            object.stroke.width = 4.0;
            object.stroke.paint = crate::paint::paint_from_param(&json!("#000000")).unwrap();
        }
        let before = doc(&ws, id);
        exec(&mut ws, id, STROKE_EXPAND, json!({})).unwrap();
        let object = ws.get(id).unwrap().session.document.get_object(ids[0]).unwrap().clone();
        match &object.kind { ObjectKind::Curve(curve) => assert_eq!(curve.hole_count(), 1), other => panic!("{other:?}") }
        assert_eq!(object.stroke.width, 0.0);
        assert!(matches!(object.fill, crate::paint::Paint::Solid { .. }));
        assert_eq!(object.pos, [-2.0, -2.0]);
        exec(&mut ws, id, "edit.undo", json!({})).unwrap();
        assert_eq!(doc(&ws, id), before);
    }
}

#[cfg(test)]
mod builder_contracts {
    use super::*;
    use crate::{Scope, Workspace};

    fn setup(rects: &[[f32; 4]]) -> (Workspace, crate::ContextId, Vec<ObjectId>) {
        let mut ws = Workspace::default();
        let id = ws.new_document("a");
        let session = &mut ws.get_mut(id).unwrap().session;
        let ids: Vec<ObjectId> = rects
            .iter()
            .map(|r| session.document.create_object(Object { name: "r".into(), pos: [r[0], r[1]], size: [r[2], r[3]], ..Object::default() }))
            .collect();
        session.selection.set(ids.clone(), SelectOp::Replace);
        (ws, id, ids)
    }

    fn exec(ws: &mut Workspace, id: crate::ContextId, name: &str, params: Value) -> CommandResult {
        ws.run(Scope::of(id), Command { name: name.into(), params }).1
    }

    fn doc(ws: &Workspace, id: crate::ContextId) -> Value {
        let mut value = serde_json::to_value(&ws.get(id).unwrap().session.document).unwrap();
        value.as_object_mut().unwrap().remove("version");
        value.as_object_mut().unwrap().remove("next_object_id");
        value
    }

    fn areas(ws: &Workspace, id: crate::ContextId) -> Vec<f64> {
        ws.get(id).unwrap().session.document.objects.iter().map(|o| match &o.kind {
            ObjectKind::Curve(curve) => {
                let ring = |c: &crate::curve::Contour| signed_area(&c.points.iter().map(|p| p.pos).collect::<Vec<_>>()).abs();
                ring(&curve.contour) - curve.holes.iter().map(ring).sum::<f64>()
            }
            _ => -1.0,
        }).collect()
    }

    const TWO: [[f32; 4]; 2] = [[0.0, 0.0, 10.0, 10.0], [5.0, 0.0, 10.0, 10.0]];

    #[test]
    fn add_over_all_areas_gives_one_curve() {
        let (mut ws, id, _) = setup(&TWO);
        exec(&mut ws, id, SHAPE_BUILDER_APPLY, json!({ "op": "add", "areas": [0, 1, 2] })).unwrap();
        let a = areas(&ws, id);
        assert_eq!(a.len(), 1);
        assert!((a[0] - 150.0).abs() < 0.5);
    }

    #[test]
    fn add_click_on_overlap_gives_three_curves() {
        let (mut ws, id, _) = setup(&TWO);
        exec(&mut ws, id, SHAPE_BUILDER_APPLY, json!({ "op": "add", "points": [[7.5, 5.0]] })).unwrap();
        let a = areas(&ws, id);
        assert_eq!(a.len(), 3);
        assert!(a.iter().all(|v| (v - 50.0).abs() < 0.5));
    }

    #[test]
    fn delete_overlap_gives_two_curves() {
        let (mut ws, id, _) = setup(&TWO);
        exec(&mut ws, id, SHAPE_BUILDER_APPLY, json!({ "op": "delete", "points": [[7.5, 5.0]] })).unwrap();
        let a = areas(&ws, id);
        assert_eq!(a.len(), 2);
        assert!((a.iter().sum::<f64>() - 100.0).abs() < 0.5);
    }

    #[test]
    fn create_on_middle_keeps_sources_and_adds_one_on_top() {
        let (mut ws, id, ids) = setup(&TWO);
        exec(&mut ws, id, SHAPE_BUILDER_APPLY, json!({ "op": "create", "points": [[7.5, 5.0]] })).unwrap();
        let objects = &ws.get(id).unwrap().session.document.objects;
        assert_eq!(objects.len(), 3);
        assert_eq!(objects[0].id, ids[0]);
        assert_eq!(objects[1].id, ids[1]);
        assert!(matches!(objects[2].kind, ObjectKind::Curve(_)));
        assert!((areas(&ws, id)[2] - 50.0).abs() < 0.5);
    }

    #[test]
    fn cleanup_unused_keeps_only_the_built_curve() {
        let (mut ws, id, _) = setup(&TWO);
        exec(&mut ws, id, SHAPE_BUILDER_APPLY, json!({ "op": "add", "points": [[7.5, 5.0]], "cleanup": "unused" })).unwrap();
        let a = areas(&ws, id);
        assert_eq!(a.len(), 1);
        assert!((a[0] - 50.0).abs() < 0.5);
        let (mut ws, id, _) = setup(&TWO);
        exec(&mut ws, id, SHAPE_BUILDER_APPLY, json!({ "points": [[7.5, 5.0]], "cleanup": "internal" })).unwrap();
        assert_eq!(areas(&ws, id).len(), 3);
    }

    #[test]
    fn first_area_style_on_and_off() {
        for (first_style, expect_red) in [(true, true), (false, false)] {
            let (mut ws, id, ids) = setup(&TWO);
            ws.get_mut(id).unwrap().session.document.find_object_mut(ids[1].0).unwrap().fill = crate::paint::paint_from_param(&json!("#ff0000")).unwrap();
            exec(&mut ws, id, SHAPE_BUILDER_APPLY, json!({ "op": "add", "points": [[7.5, 5.0]], "cleanup": "unused", "first_style": first_style })).unwrap();
            let object = &ws.get(id).unwrap().session.document.objects[0];
            assert_eq!(object.fill == crate::paint::paint_from_param(&json!("#ff0000")).unwrap(), expect_red);
        }
    }

    #[test]
    fn bad_area_or_outside_point_is_degenerate_and_unchanged_then_one_undo() {
        let (mut ws, id, _) = setup(&TWO);
        let before = doc(&ws, id);
        assert_eq!(exec(&mut ws, id, SHAPE_BUILDER_APPLY, json!({ "areas": [9] })).unwrap_err().code, "BOOLEAN_DEGENERATE");
        assert_eq!(exec(&mut ws, id, SHAPE_BUILDER_APPLY, json!({ "points": [[100.0, 100.0]] })).unwrap_err().code, "BOOLEAN_DEGENERATE");
        assert_eq!(doc(&ws, id), before);
        exec(&mut ws, id, SHAPE_BUILDER_APPLY, json!({ "op": "add", "areas": [0, 1, 2] })).unwrap();
        exec(&mut ws, id, "edit.undo", json!({})).unwrap();
        assert_eq!(doc(&ws, id), before);
    }

    #[test]
    fn knife_splits_square_in_two_and_undoes() {
        let (mut ws, id, _) = setup(&[[0.0, 0.0, 100.0, 100.0]]);
        let before = doc(&ws, id);
        exec(&mut ws, id, PATH_KNIFE, json!({ "cut": [[50.0, -10.0], [50.0, 110.0]] })).unwrap();
        let a = areas(&ws, id);
        assert_eq!(a.len(), 2);
        assert!(a.iter().all(|v| (v - 5000.0).abs() < 2.0));
        exec(&mut ws, id, "edit.undo", json!({})).unwrap();
        assert_eq!(doc(&ws, id), before);
    }

    #[test]
    fn knife_that_misses_changes_nothing() {
        let (mut ws, id, _) = setup(&[[0.0, 0.0, 100.0, 100.0]]);
        let before = doc(&ws, id);
        assert_eq!(exec(&mut ws, id, PATH_KNIFE, json!({ "cut": [[200.0, 0.0], [200.0, 100.0]] })).unwrap_err().code, "NOTHING_TO_CUT");
        assert_eq!(exec(&mut ws, id, PATH_KNIFE, json!({ "cut": [[200.0, 0.0]] })).unwrap_err().code, "INVALID_PARAMS");
        assert_eq!(doc(&ws, id), before);
    }
}
