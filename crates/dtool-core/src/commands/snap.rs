use crate::command::Session;
use crate::snap_candidates::{smart_guides, snap_candidates, CandidateKind, SmartGuideKind, SnapDocument};
use dtool_protocol::{Command, CommandError};
use serde::Deserialize;
use serde_json::{json, Value};

pub const SNAP_CANDIDATES: &str = "snap.candidates";
pub const SNAP_SMART_GUIDE_FEEDBACK: &str = "snap.smart_guide.feedback";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidatesArgs {
    position: [f32; 2],
    #[serde(default)]
    moving_owner: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
enum FeedbackKind { Horizontal, Vertical }

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FeedbackArgs {
    kind: FeedbackKind,
    position: f32,
    #[serde(default)]
    cross: Option<f32>,
    #[serde(default)]
    moving_owner: Option<u64>,
}

fn parse<T: for<'de> Deserialize<'de>>(params: &Value) -> Result<T, CommandError> {
    serde_json::from_value(params.clone()).map_err(|_| CommandError::invalid_params())
}

fn candidates_args(params: &Value) -> Result<CandidatesArgs, CommandError> {
    let args: CandidatesArgs = parse(params)?;
    if args.position.iter().all(|v| v.is_finite()) { Ok(args) } else { Err(CommandError::invalid_params()) }
}

fn feedback_args(params: &Value) -> Result<FeedbackArgs, CommandError> {
    let args: FeedbackArgs = parse(params)?;
    if args.position.is_finite() && args.cross.is_none_or(f32::is_finite) { Ok(args) } else { Err(CommandError::invalid_params()) }
}

fn kind_key(kind: CandidateKind) -> &'static str {
    match kind {
        CandidateKind::Guide => "guide",
        CandidateKind::Grid => "grid",
        CandidateKind::Page => "page",
        CandidateKind::ObjectBounds => "object_bounds",
        CandidateKind::ObjectGeometry => "object_geometry",
        CandidateKind::Pixel => "pixel",
    }
}

fn guide_key(kind: SmartGuideKind) -> &'static str {
    match kind { SmartGuideKind::Horizontal => "Horizontal", SmartGuideKind::Vertical => "Vertical" }
}

pub fn can_execute(session: &Session, command: &Command) -> Option<bool> {
    match command.name.as_str() {
        SNAP_CANDIDATES => Some(session.has_document && candidates_args(&command.params).is_ok()),
        SNAP_SMART_GUIDE_FEEDBACK => Some(session.has_document && feedback_args(&command.params).is_ok()),
        _ => None,
    }
}

pub fn run(session: &mut Session, command: &Command) -> Option<Result<Value, CommandError>> {
    if !matches!(command.name.as_str(), SNAP_CANDIDATES | SNAP_SMART_GUIDE_FEEDBACK) { return None; }
    Some((|| {
        if !session.has_document { return Err(CommandError::no_document()); }
        let doc = SnapDocument { document: &session.document, guides: &session.guides, grid: &session.document.grid };
        let prefs = &session.snap_prefs.query();
        if command.name == SNAP_CANDIDATES {
            let args = candidates_args(&command.params)?;
            let candidates: Vec<Value> = snap_candidates(&doc, &session.view, prefs, args.position).iter()
                .map(|c| json!({ "point": c.point, "distance": c.distance, "kind": kind_key(c.kind), "owner": c.owner })).collect();
            let guides: Vec<Value> = smart_guides(&doc, &session.view, prefs, args.position, args.moving_owner).iter()
                .map(|g| json!({ "kind": guide_key(g.kind), "position": g.position, "source_owner": g.source_owner, "target_owner": g.target_owner })).collect();
            return Ok(json!({ "candidates": candidates, "smart_guides": guides, "doc_version": session.generation }));
        }
        let args = feedback_args(&command.params)?;
        let cross = args.cross.unwrap_or(0.0);
        let (point, wanted) = match args.kind {
            FeedbackKind::Vertical => ([args.position, cross], SmartGuideKind::Vertical),
            FeedbackKind::Horizontal => ([cross, args.position], SmartGuideKind::Horizontal),
        };
        let overlay: Vec<Value> = smart_guides(&doc, &session.view, prefs, point, args.moving_owner).iter()
            .filter(|g| g.kind == wanted)
            .map(|g| json!({ "kind": guide_key(g.kind), "position": g.position, "source_owner": g.source_owner, "target_owner": g.target_owner })).collect();
        Ok(json!({ "overlay": overlay, "persisted": false, "doc_version": session.generation }))
    })())
}
