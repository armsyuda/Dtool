pub use crate::warp::{homography, PerspectiveSpec, Quad};

use crate::command::Session;
use crate::commands::feature_restore::FEATURE_RESTORE;
use crate::curve::Curve;
use crate::document::Document;
use crate::object::{Object, ObjectKind};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

pub const PREVIEW: &str = "perspective.preview";
pub const APPLY: &str = "perspective.apply";
pub const CANCEL: &str = "perspective.cancel";
const STORE_KEY: &str = "dtool.vector_perspective";

pub fn map_point(matrix: [[f64; 3]; 3], point: [f64; 2]) -> [f64; 2] {
    let x = matrix[0][0] * point[0] + matrix[0][1] * point[1] + matrix[0][2];
    let y = matrix[1][0] * point[0] + matrix[1][1] * point[1] + matrix[1][2];
    let w = matrix[2][0] * point[0] + matrix[2][1] * point[1] + matrix[2][2];
    if w.abs() < 1e-12 {
        [x, y]
    } else {
        [x / w, y / w]
    }
}

fn almost(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
}

fn mid(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}

fn point_in_triangle(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    let s = (a[0] - c[0]) * (p[1] - c[1]) - (a[1] - c[1]) * (p[0] - c[0]);
    let t = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    if (s < 0.0) != (t < 0.0) && s != 0.0 && t != 0.0 {
        return false;
    }
    let d = (c[0] - b[0]) * (p[1] - b[1]) - (c[1] - b[1]) * (p[0] - b[0]);
    d == 0.0 || (d < 0.0) == (s + t <= 0.0)
}

pub fn point_in_quad(point: [f64; 2], quad: &Quad) -> bool {
    point_in_triangle(point, quad[0], quad[1], quad[2]) || point_in_triangle(point, quad[0], quad[2], quad[3])
}

pub fn enforce_seam(spec: &mut PerspectiveSpec) {
    if spec.source.len() < 2 || spec.target.len() < 2 {
        return;
    }
    for i in 0..4 {
        for j in 0..4 {
            if almost(spec.source[0][i], spec.source[1][j]) {
                let shared = mid(spec.target[0][i], spec.target[1][j]);
                spec.target[0][i] = shared;
                spec.target[1][j] = shared;
            }
        }
    }
}

pub fn seam_targets_equal(spec: &PerspectiveSpec) -> bool {
    if spec.source.len() < 2 || spec.target.len() < 2 {
        return true;
    }
    for i in 0..4 {
        for j in 0..4 {
            if almost(spec.source[0][i], spec.source[1][j]) && !almost(spec.target[0][i], spec.target[1][j]) {
                return false;
            }
        }
    }
    true
}

fn choose_plane(spec: &PerspectiveSpec, point: [f64; 2]) -> usize {
    for (index, source) in spec.source.iter().enumerate() {
        if point_in_quad(point, source) {
            return index;
        }
    }
    0
}

pub fn map_spec(spec: &PerspectiveSpec, point: [f64; 2]) -> [f64; 2] {
    if spec.source.is_empty() || spec.target.is_empty() {
        return point;
    }
    let index = choose_plane(spec, point).min(spec.source.len() - 1).min(spec.target.len() - 1);
    map_point(homography(spec.source[index], spec.target[index]), point)
}

fn parse_quad(value: &Value) -> Result<Quad, CommandError> {
    let list = value.as_array().filter(|list| list.len() == 4).ok_or_else(CommandError::invalid_params)?;
    let mut quad = [[0.0; 2]; 4];
    for (index, item) in list.iter().enumerate() {
        let pair = item.as_array().filter(|pair| pair.len() == 2).ok_or_else(CommandError::invalid_params)?;
        quad[index] = [
            pair[0].as_f64().ok_or_else(CommandError::invalid_params)?,
            pair[1].as_f64().ok_or_else(CommandError::invalid_params)?,
        ];
        if !quad[index][0].is_finite() || !quad[index][1].is_finite() {
            return Err(CommandError::invalid_params());
        }
    }
    Ok(quad)
}

pub fn parse_spec(params: &Value) -> Result<PerspectiveSpec, CommandError> {
    let planes = params
        .get("planes")
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .try_into()
        .map_err(|_| CommandError::invalid_params())?;
    if planes != 1 && planes != 2 {
        return Err(CommandError::invalid_params());
    }
    let mut source = Vec::new();
    let mut target = Vec::new();
    if let Some(list) = params.get("source").and_then(Value::as_array) {
        for item in list {
            source.push(parse_quad(item)?);
        }
    }
    if let Some(list) = params.get("target").and_then(Value::as_array) {
        for item in list {
            target.push(parse_quad(item)?);
        }
    }
    if source.is_empty() {
        if let Some(corners) = params.get("corners") {
            source.push([[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]]);
            target.push(parse_quad(corners)?);
        }
    }
    if source.is_empty() || target.len() != source.len() || source.len() != planes as usize {
        return Err(CommandError::invalid_params());
    }
    let mut spec = PerspectiveSpec { planes, source, target };
    enforce_seam(&mut spec);
    Ok(spec)
}

fn clear_store(doc: &mut Document) {
    if let Some(object) = doc.export_defaults.as_object_mut() {
        object.remove(STORE_KEY);
    }
}

fn store_active(doc: &mut Document, snapshot: &Document, spec: &PerspectiveSpec) {
    if !doc.export_defaults.is_object() {
        doc.export_defaults = json!({});
    }
    doc.export_defaults.as_object_mut().unwrap().insert(
        STORE_KEY.to_string(),
        json!({
            "active": true,
            "planes": spec.planes,
            "source": spec.source,
            "target": spec.target,
            "snapshot": snapshot,
        }),
    );
}

fn snapshot_from_store(doc: &Document) -> Option<Document> {
    let value = doc.export_defaults.get(STORE_KEY)?;
    if value.get("active").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    serde_json::from_value(value.get("snapshot")?.clone()).ok()
}

fn is_active(doc: &Document) -> bool {
    doc.export_defaults
        .get(STORE_KEY)
        .and_then(|value| value.get("active"))
        .and_then(Value::as_bool)
        == Some(true)
}

fn map_handle(handle: &mut Option<[f64; 2]>, spec: &PerspectiveSpec) {
    if let Some(point) = handle {
        *point = map_spec(spec, *point);
    }
}

fn map_curve(curve: &mut Curve, spec: &PerspectiveSpec) {
    for point in &mut curve.contour.points {
        point.pos = map_spec(spec, point.pos);
        map_handle(&mut point.handle_in, spec);
        map_handle(&mut point.handle_out, spec);
    }
    for hole in &mut curve.holes {
        for point in &mut hole.points {
            point.pos = map_spec(spec, point.pos);
            map_handle(&mut point.handle_in, spec);
            map_handle(&mut point.handle_out, spec);
        }
    }
}

fn recompute_bounds(object: &mut Object) {
    let pts: Vec<[f64; 2]> = match object.curve() {
        Some(curve) => curve.contour.points.iter().map(|point| point.pos).collect(),
        None => return,
    };
    if pts.is_empty() {
        return;
    }
    let x0 = pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let y0 = pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let x1 = pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    let y1 = pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    object.pos = [x0 as f32, y0 as f32];
    object.size = [(x1 - x0) as f32, (y1 - y0) as f32];
}

fn transform_objects(live: &mut Document, snapshot: &Document, spec: &PerspectiveSpec) {
    live.objects = snapshot.objects.clone();
    for object in &mut live.objects {
        match &mut object.kind {
            ObjectKind::Curve(curve) => {
                map_curve(curve, spec);
                recompute_bounds(object);
            }
            _ => {
                let p0 = [object.pos[0] as f64, object.pos[1] as f64];
                let p1 = [object.pos[0] as f64 + object.size[0] as f64, object.pos[1] as f64];
                let p2 = [object.pos[0] as f64 + object.size[0] as f64, object.pos[1] as f64 + object.size[1] as f64];
                let p3 = [object.pos[0] as f64, object.pos[1] as f64 + object.size[1] as f64];
                let mapped = [map_spec(spec, p0), map_spec(spec, p1), map_spec(spec, p2), map_spec(spec, p3)];
                let x0 = mapped.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
                let y0 = mapped.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
                let x1 = mapped.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
                let y1 = mapped.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
                object.pos = [x0 as f32, y0 as f32];
                object.size = [(x1 - x0) as f32, (y1 - y0) as f32];
            }
        }
    }
}

fn bump(session: &mut Session) -> Result<u64, CommandError> {
    let generation = session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    let version = session.document.version.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    session.generation = generation;
    session.document.version = version;
    Ok(generation)
}

pub fn preview(session: &mut Session, spec: PerspectiveSpec) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let mut spec = spec;
    enforce_seam(&mut spec);
    let snapshot = snapshot_from_store(&session.document).unwrap_or_else(|| {
        let mut copy = session.document.clone();
        clear_store(&mut copy);
        copy
    });
    transform_objects(&mut session.document, &snapshot, &spec);
    store_active(&mut session.document, &snapshot, &spec);
    let generation = bump(session)?;
    Ok(json!({
        "ok": true,
        "planes": spec.planes,
        "seam_equal": seam_targets_equal(&spec),
        "preview": true,
        "doc_version": generation,
    }))
}

pub fn apply(session: &mut Session, spec: Option<PerspectiveSpec>) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let snapshot = if let Some(spec) = spec {
        let mut spec = spec;
        enforce_seam(&mut spec);
        let snapshot = snapshot_from_store(&session.document).unwrap_or_else(|| {
            let mut copy = session.document.clone();
            clear_store(&mut copy);
            copy
        });
        transform_objects(&mut session.document, &snapshot, &spec);
        snapshot
    } else {
        snapshot_from_store(&session.document).ok_or_else(CommandError::invalid_params)?
    };
    clear_store(&mut session.document);
    let generation = bump(session)?;
    let undo = Command::with_params(FEATURE_RESTORE, json!({ "document": snapshot }));
    let redo = Command::with_params(FEATURE_RESTORE, json!({ "document": session.document.clone() }));
    session.undo.push(crate::undo::Change {
        name: APPLY.to_string(),
        undo,
        redo,
    });
    Ok(json!({ "ok": true, "applied": true, "doc_version": generation }))
}

pub fn cancel(session: &mut Session) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    if !is_active(&session.document) {
        return Ok(json!({ "ok": true, "cancelled": false, "doc_version": session.generation }));
    }
    let snapshot = snapshot_from_store(&session.document).ok_or_else(CommandError::invalid_params)?;
    let version = session.document.version.max(snapshot.version) + 1;
    session.document = snapshot;
    clear_store(&mut session.document);
    session.document.version = version;
    session.generation = session.generation.checked_add(1).ok_or_else(CommandError::invalid_params)?;
    Ok(json!({ "ok": true, "cancelled": true, "doc_version": session.generation }))
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    match command.name.as_str() {
        PREVIEW => Some(parse_spec(&command.params).and_then(|spec| preview(session, spec))),
        APPLY => {
            let spec = if command.params.get("source").is_some() || command.params.get("corners").is_some() {
                Some(parse_spec(&command.params).ok()).flatten()
            } else {
                None
            };
            if command.params.get("source").is_some() || command.params.get("corners").is_some() {
                Some(parse_spec(&command.params).and_then(|spec| apply(session, Some(spec))))
            } else {
                Some(apply(session, spec))
            }
        }
        CANCEL => Some(cancel(session)),
        _ => None,
    }
}

pub fn has_pixel_layer(doc: &Document) -> bool {
    !doc.pixel_layers.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::{Contour, PathPoint, PointId};
    use crate::document::Units;

    fn session() -> Session {
        Session::new(Document::new([400.0, 200.0], Units::Px, 72.0, 1))
    }

    fn curve_at(points: &[[f64; 2]]) -> Object {
        let contour = Contour {
            points: points
                .iter()
                .enumerate()
                .map(|(i, pos)| PathPoint::sharp(PointId(i as u64 + 1), *pos))
                .collect(),
            closed: true,
        };
        let mut object = Object::default();
        object.kind = ObjectKind::Curve(Curve {
            contour,
            holes: Vec::new(),
            source_object: None,
        });
        object.pos = [points[0][0] as f32, points[0][1] as f32];
        object.size = [40.0, 20.0];
        object
    }

    fn close(a: [f64; 2], b: [f64; 2]) {
        assert!((a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4, "{a:?} != {b:?}");
    }

    #[test]
    fn one_plane_projective_maps_corners_without_pixel_layer() {
        let mut session = session();
        let id = session.document.create_object(curve_at(&[
            [0.0, 0.0],
            [40.0, 0.0],
            [40.0, 20.0],
            [0.0, 20.0],
        ]));
        assert!(!has_pixel_layer(&session.document));
        let target = [[0.0, 0.0], [40.0, 0.0], [36.0, 20.0], [4.0, 20.0]];
        apply(
            &mut session,
            Some(PerspectiveSpec {
                planes: 1,
                source: vec![[[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]]],
                target: vec![target],
            }),
        )
        .unwrap();
        assert!(!has_pixel_layer(&session.document));
        let curve = session.document.get_object(id).unwrap().curve().unwrap();
        close(curve.contour.points[0].pos, [0.0, 0.0]);
        close(curve.contour.points[1].pos, [40.0, 0.0]);
        close(curve.contour.points[2].pos, [36.0, 20.0]);
        close(curve.contour.points[3].pos, [4.0, 20.0]);
    }

    #[test]
    fn two_plane_seam_endpoints_stay_equal() {
        let mut spec = PerspectiveSpec {
            planes: 2,
            source: vec![
                [[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]],
                [[20.0, 0.0], [40.0, 0.0], [40.0, 20.0], [20.0, 20.0]],
            ],
            target: vec![
                [[0.0, 0.0], [18.0, 1.0], [18.0, 19.0], [0.0, 20.0]],
                [[22.0, 1.0], [40.0, 0.0], [40.0, 20.0], [22.0, 19.0]],
            ],
        };
        enforce_seam(&mut spec);
        assert!(seam_targets_equal(&spec));
        let left = map_spec(&spec, [20.0, 0.0]);
        let right = map_spec(&spec, [20.0, 0.0]);
        close(left, right);
        close(map_spec(&spec, [20.0, 20.0]), map_spec(&spec, [20.0, 20.0]));
        close(spec.target[0][1], spec.target[1][0]);
        close(spec.target[0][2], spec.target[1][3]);
    }

    #[test]
    fn cancel_restores_pre_preview_document() {
        let mut session = session();
        let id = session.document.create_object(curve_at(&[
            [0.0, 0.0],
            [40.0, 0.0],
            [40.0, 20.0],
            [0.0, 20.0],
        ]));
        let before = session.document.objects.clone();
        preview(
            &mut session,
            PerspectiveSpec {
                planes: 1,
                source: vec![[[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]]],
                target: vec![[[0.0, 0.0], [40.0, 0.0], [36.0, 20.0], [4.0, 20.0]]],
            },
        )
        .unwrap();
        let previewed = session.document.get_object(id).unwrap().curve().unwrap().contour.points[2].pos;
        assert!((previewed[0] - 40.0).abs() > 0.5);
        cancel(&mut session).unwrap();
        let after = session.document.get_object(id).unwrap().curve().unwrap().contour.points[2].pos;
        close(after, [40.0, 20.0]);
        assert_eq!(before.len(), session.document.objects.len());
        assert!(!is_active(&session.document));
    }
}
