use crate::boolean::{self, FillRule};
use crate::curve::Curve;
use crate::{Command, CommandError, Object, ObjectId, ObjectKind, Session};
use kurbo::BezPath;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const TOOL: &str = "tool.contour";
pub const SET: &str = "contour.set";
pub const QUERY: &str = "contour.query";
pub const BAKE: &str = "appearance.bake";

pub const MAX_RADIUS: f64 = 100_000.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContourJoin {
    #[default]
    Round,
    Miter,
    Bevel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContourCap {
    #[default]
    None,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct ContourSpec {
    pub radius: f64,
    pub join: ContourJoin,
    pub miter: f64,
    pub cap: ContourCap,
}

impl Default for ContourSpec {
    fn default() -> Self {
        Self { radius: 0.0, join: ContourJoin::Round, miter: 4.0, cap: ContourCap::None }
    }
}

impl ContourSpec {
    pub fn is_valid(self) -> bool {
        self.radius.is_finite() && self.radius.abs() <= MAX_RADIUS
            && self.miter.is_finite() && (1.0..=100.0).contains(&self.miter)
    }

    pub(crate) fn outline(self, path: &BezPath, rule: FillRule, tolerance: f64) -> Result<BezPath, CommandError> {
        if self.radius == 0.0 { return Ok(path.clone()); }
        if !self.is_valid() { return Err(CommandError::invalid_params()); }
        boolean::contour_path(path, rule, self, tolerance)
    }
}

pub fn supports(object: &Object) -> bool {
    matches!(object.kind, ObjectKind::Shape | ObjectKind::Curve(_))
        && crate::commands::qr::payload(object).is_none()
}

fn target<'a>(session: &'a Session, params: &Value) -> Result<&'a Object, CommandError> {
    if !session.has_document { return Err(CommandError::no_document()); }
    let id = if let Some(value) = params.get("id") {
        ObjectId(value.as_u64().ok_or_else(CommandError::invalid_params)?)
    } else {
        let [id] = session.selection.ids.as_slice() else { return Err(CommandError::invalid_params()); };
        *id
    };
    let object = session.document.get_object(id).ok_or_else(CommandError::not_found)?;
    if !supports(object) { return Err(CommandError::new("UNSUPPORTED_SHAPE")); }
    Ok(object)
}

pub fn selected(session: &Session) -> Option<&Object> {
    target(session, &Value::Null).ok()
}

fn writable<'a>(session: &'a Session, params: &Value) -> Result<&'a Object, CommandError> {
    let object = target(session, params)?;
    if session.document.is_object_locked(object) { return Err(CommandError::new("LOCKED")); }
    Ok(object)
}

#[derive(Deserialize)]
struct Patch {
    radius: Option<f64>,
    join: Option<ContourJoin>,
    miter: Option<f64>,
    cap: Option<ContourCap>,
}

fn patched(object: &Object, params: &Value) -> Result<ContourSpec, CommandError> {
    let patch: Patch = serde_json::from_value(params.clone()).map_err(|_| CommandError::invalid_params())?;
    let mut spec = object.contour.unwrap_or_default();
    if let Some(radius) = patch.radius { spec.radius = radius; }
    if let Some(join) = patch.join { spec.join = join; }
    if let Some(miter) = patch.miter { spec.miter = miter; }
    if let Some(cap) = patch.cap { spec.cap = cap; }
    if !spec.is_valid() { return Err(CommandError::invalid_params()); }
    Ok(spec)
}

pub fn set_command(id: ObjectId, spec: ContourSpec) -> Command {
    Command::with_params(SET, json!({"id": id, "radius": spec.radius, "join": spec.join, "miter": spec.miter, "cap": spec.cap}))
}

pub fn can_bake(object: &Object) -> bool {
    object.has_live_contour() || super::curve_nodes::corner::has_corners(object)
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        SET => Some(writable(session, &command.params).and_then(|object| patched(object, &command.params)).is_ok()),
        BAKE => Some(writable(session, &command.params).is_ok_and(can_bake)),
        QUERY => Some(target(session, &command.params).is_ok()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<dtool_protocol::CommandResult> {
    match command.name.as_str() {
        QUERY => Some(target(session, &command.params).map(|object| {
            let bounds = crate::outline::object_outline_world(object).filter(|path| !path.elements().is_empty())
                .map(|path| crate::geometry::bez_path_bounds(&path));
            json!({"id": object.id, "contour": object.contour.unwrap_or_default(), "bounds": bounds})
        })),
        SET | BAKE => Some(edit(session, command)),
        _ => None,
    }
}

fn edit(session: &mut Session, command: &Command) -> dtool_protocol::CommandResult {
    let old = writable(session, &command.params)?.clone();
    let mut next = old.clone();
    if command.name == SET {
        let spec = patched(&old, &command.params)?;
        let base = crate::outline::base_object_outline(&old, boolean::DEFAULT_TOLERANCE)
            .ok_or_else(|| CommandError::new("UNSUPPORTED_SHAPE"))?;
        spec.outline(&base, old.fill_rule, boolean::DEFAULT_TOLERANCE)?;
        next.contour = (spec != ContourSpec::default()).then_some(spec);
    } else {
        if !can_bake(&old) { return Err(CommandError::invalid_params()); }
        let base = crate::outline::base_object_outline(&old, boolean::DEFAULT_TOLERANCE)
            .ok_or_else(|| CommandError::new("UNSUPPORTED_SHAPE"))?;
        let path = old.contour.unwrap_or_default().outline(&base, old.fill_rule, boolean::DEFAULT_TOLERANCE)?;
        if path.elements().is_empty() { return Err(CommandError::new("BOOLEAN_EMPTY")); }
        next.kind = ObjectKind::Curve(Curve::from_kurbo(&path).with_source(old.id.0));
        next.contour = None;
        next.shape = None;
    }
    if command.name == SET && old.contour == next.contour {
        return Ok(json!({"id":old.id,"doc_version":session.generation,"changes":[]}));
    }
    if command.name == BAKE && old.contour.is_none()
        && let Some(result) = super::symbols::curves::apply_edit(session, &old, &next, BAKE, json!({"id":old.id}))
    {
        return result;
    }
    let redo = Command::with_params("object.restore", json!({"objects":[next]}));
    *session.document.find_object_mut(old.id.0).ok_or_else(CommandError::not_found)? = next;
    let id = old.id;
    session.push_restore_undo(&command.name, redo, vec![old]);
    Ok(json!({"id":id,"doc_version":session.generation,"changes":[{"name":command.name,"id":id}]}))
}

