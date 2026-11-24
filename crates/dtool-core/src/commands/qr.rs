use crate::paint::{Colour, Paint};
use crate::{Command, CommandError, CommandResult, Object, ObjectId, ObjectKind, Session};
use kurbo::BezPath;
use qrcode::{Color, QrCode};
use serde_json::{Value, json};

mod content;
pub use content::{QrContent, WifiSecurity};

pub const DEFAULT_PAYLOAD: &str = "https://example.org";

pub fn payload(object: &Object) -> Option<&str> {
    if matches!(object.kind, ObjectKind::Shape) {
        object.qr_payload.as_deref()
    } else {
        None
    }
}

pub fn encode(value: &str) -> Result<QrCode, CommandError> {
    if value.trim().is_empty() {
        return Err(CommandError::invalid_params());
    }
    QrCode::new(value.as_bytes()).map_err(|_| CommandError::invalid_params())
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    matches!(command.name.as_str(), "qr.create" | "qr.update").then_some(session.has_document)
}

pub fn run(session: &mut Session, command: &Command) -> Option<CommandResult> {
    can_execute(session, command).map(|_| execute(session, command))
}

fn execute(session: &mut Session, command: &Command) -> CommandResult {
    if !session.has_document {
        return Err(CommandError::no_document());
    }
    let params = &command.params;
    let (value, content) = match (params.get("payload"), params.get("content")) {
        (Some(raw), None) => (raw.as_str().ok_or_else(CommandError::invalid_params)?.to_owned(), None),
        (None, Some(spec)) => {
            let content: QrContent = serde_json::from_value(spec.clone()).map_err(|_| CommandError::invalid_params())?;
            (content.payload()?, Some(content))
        }
        _ => return Err(CommandError::invalid_params()),
    };
    encode(&value)?;
    let mut document;
    let id;
    if command.name == "qr.create" {
        let pos = pair(params.get("pos"), [0.0, 0.0])?;
        let size = pair(params.get("size"), [100.0, 100.0])?;
        let shape: crate::shape::ShapeRecipe = serde_json::from_value(json!({"kind": "rectangle", "size": size}))
            .map_err(|_| CommandError::invalid_params())?;
        shape.validate()?;
        document = session.document.clone();
        let mut object = Object {
            name: dtool_strings::text("tool.qr").to_string(),
            pos, size, shape: Some(shape), qr_payload: Some(value), qr_content: content,
            fill: Paint::Solid { colour: Colour::new(0, 0, 0, 255) },
            ..Object::default()
        };
        let center = [pos[0] + size[0] * 0.5, pos[1] + size[1] * 0.5];
        crate::artboard::edit::assign_new_object(&document, &mut object, center);
        id = document.create_object(object);
    } else {
        id = ObjectId(params.get("id").and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .ok_or_else(CommandError::invalid_params)?);
        let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
        let previous = payload(object).filter(|_| !object.locked).ok_or_else(CommandError::invalid_params)?;
        if value == previous && content == object.qr_content {
            return Ok(json!({"id": id.0, "doc_version": session.generation, "changes": []}));
        }
        document = session.document.clone();
        let object = document.find_object_mut(id.0).ok_or_else(CommandError::not_found)?;
        object.qr_payload = Some(value);
        object.qr_content = content;
    }
    let selection = if command.name == "qr.create" { vec![id] } else { session.selection.ids.clone() };
    super::document_change::commit(session, &command.name, document, selection, json!({"id": id.0}))
}

fn pair(value: Option<&Value>, default: [f32; 2]) -> Result<[f32; 2], CommandError> {
    let Some(value) = value else { return Ok(default) };
    let pair: [f32; 2] = serde_json::from_value(value.clone()).map_err(|_| CommandError::invalid_params())?;
    pair.iter().all(|coordinate| coordinate.is_finite()).then_some(pair).ok_or_else(CommandError::invalid_params)
}

pub(crate) fn outline(value: &str, [width, height]: [f32; 2]) -> BezPath {
    let mut path = BezPath::new();
    let Ok(code) = encode(value) else { return path };
    let side = width.abs().min(height.abs());
    if !side.is_finite() || side <= 0.0 {
        return path;
    }
    let n = code.width();
    let cell = f64::from(side) / (n + 8) as f64;
    let left = f64::from(width.abs() - side) * 0.5 + 4.0 * cell;
    let top = f64::from(height.abs() - side) * 0.5 + 4.0 * cell;
    let point = |x: usize, y: usize| (left + x as f64 * cell, top + y as f64 * cell);
    let stride = n + 1;
    let mut edges = vec![[false; 4]; stride * stride];
    for y in 0..n {
        for x in 0..n {
            if code[(x, y)] != Color::Dark { continue; }
            let i = y * stride + x;
            edges[i][0] = y == 0 || code[(x, y - 1)] == Color::Light;
            edges[i + 1][1] = x + 1 == n || code[(x + 1, y)] == Color::Light;
            edges[i + stride + 1][2] = y + 1 == n || code[(x, y + 1)] == Color::Light;
            edges[i + stride][3] = x == 0 || code[(x - 1, y)] == Color::Light;
        }
    }
    let offsets = [1, stride as isize, -1, -(stride as isize)];
    for start in 0..edges.len() {
        while let Some(mut direction) = edges[start].iter().position(|edge| *edge) {
            let mut vertex = start;
            path.move_to(point(vertex % stride, vertex / stride));
            loop {
                edges[vertex][direction] = false;
                vertex = vertex.checked_add_signed(offsets[direction]).expect("QR edge stays inside the grid");
                if vertex == start {
                    path.close_path();
                    break;
                }
                let next = [(direction + 1) % 4, direction, (direction + 3) % 4]
                    .into_iter().find(|&edge| edges[vertex][edge])
                    .expect("QR boundary closes");
                if next != direction {
                    path.line_to(point(vertex % stride, vertex / stride));
                }
                direction = next;
            }
        }
    }
    path
}

