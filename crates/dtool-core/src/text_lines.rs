use crate::document::{Document, Story};
use crate::object::{ObjectId, ObjectKind};
use crate::paint::{Colour, Paint};
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    pub object: ObjectId,
    pub origin: [f32; 2],
    pub text: String,
    pub size: f32,
    pub width: f32,
    pub para_end: bool,
    pub paint: Paint,
}

pub type TextLines = fn(&Document, ObjectId) -> Option<Vec<TextLine>>;

static TEXT_LINES: OnceLock<TextLines> = OnceLock::new();

static FONT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn font_generation() -> u64 {
    FONT_GENERATION.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn publish_font_generation(generation: u64) {
    FONT_GENERATION.fetch_max(generation, std::sync::atomic::Ordering::SeqCst);
}

pub fn register_text_lines(lines: TextLines) -> bool {
    TEXT_LINES.set(lines).is_ok() || TEXT_LINES.get().is_some()
}

pub fn text_lines_ready() -> bool {
    TEXT_LINES.get().is_some()
}

pub type Overset = fn(&Document, ObjectId) -> Option<Option<(usize, usize)>>;

static OVERSET: OnceLock<Overset> = OnceLock::new();

pub fn register_overset(overset: Overset) -> bool {
    OVERSET.set(overset).is_ok() || OVERSET.get().is_some()
}

pub fn overset(doc: &Document, id: ObjectId) -> Option<Option<(usize, usize)>> {
    let hook = OVERSET.get()?;
    let object = doc.get_object(id)?;
    match object.kind {
        ObjectKind::ArtText { .. } => Some(None),
        ObjectKind::FrameText { .. } | ObjectKind::PathText(_) => hook(doc, id),
        _ => None,
    }
}

pub type TextOutlines = fn(&Document, ObjectId) -> Option<Vec<Vec<[f32; 2]>>>;

static TEXT_OUTLINES: OnceLock<TextOutlines> = OnceLock::new();

pub fn register_text_outlines(outlines: TextOutlines) -> bool {
    TEXT_OUTLINES.set(outlines).is_ok() || TEXT_OUTLINES.get().is_some()
}

pub fn text_outlines(doc: &Document, id: ObjectId) -> Option<Vec<Vec<[f32; 2]>>> {
    TEXT_OUTLINES.get().and_then(|hook| hook(doc, id)).filter(|contours| contours.iter().any(|contour| contour.len() > 2))
}

pub struct TextInk {
    pub paint: Paint,
    pub contours: Vec<Vec<[f32; 2]>>,
}

impl TextInk {
    pub fn append(ink: &mut Vec<Self>, paint: &Paint, contour: Vec<[f32; 2]>) {
        if let Some(previous) = ink.last_mut().filter(|run| &run.paint == paint) {
            previous.contours.push(contour);
        } else {
            ink.push(Self { paint: paint.clone(), contours: vec![contour] });
        }
    }
}

pub type TextInkProvider = fn(&Document, ObjectId) -> Option<Vec<TextInk>>;
static TEXT_INK: OnceLock<TextInkProvider> = OnceLock::new();

pub fn register_text_ink(provider: TextInkProvider) -> bool {
    TEXT_INK.set(provider).is_ok() || TEXT_INK.get().is_some()
}

pub fn text_ink(doc: &Document, id: ObjectId) -> Option<Vec<TextInk>> {
    TEXT_INK.get().and_then(|provider| provider(doc, id))
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextDecoration {
    pub rect: [f32; 4],
    pub paint: Paint,
    pub under_text: bool,
}

pub type TextDecorations = fn(&Document, ObjectId) -> Option<Vec<TextDecoration>>;

static TEXT_DECORATIONS: OnceLock<TextDecorations> = OnceLock::new();

pub fn register_text_decorations(decorations: TextDecorations) -> bool {
    TEXT_DECORATIONS.set(decorations).is_ok() || TEXT_DECORATIONS.get().is_some()
}

pub fn text_decorations(doc: &Document, id: ObjectId) -> Vec<TextDecoration> {
    TEXT_DECORATIONS.get().and_then(|hook| hook(doc, id)).unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
pub struct BreakMark {
    pub kind: crate::document::BreakKind,
    pub pos: usize,
    pub at: [f32; 2],
    pub size: f32,
}

pub type TextBreakMarks = fn(&Document, ObjectId) -> Option<Vec<BreakMark>>;

static TEXT_BREAK_MARKS: OnceLock<TextBreakMarks> = OnceLock::new();

pub fn register_text_break_marks(marks: TextBreakMarks) -> bool {
    TEXT_BREAK_MARKS.set(marks).is_ok() || TEXT_BREAK_MARKS.get().is_some()
}

pub fn text_break_marks(doc: &Document, id: ObjectId) -> Vec<BreakMark> {
    TEXT_BREAK_MARKS.get().and_then(|hook| hook(doc, id)).unwrap_or_default()
}

pub const TEXT_MARKS_QUERY: &str = "text.marks.query";

pub fn marks_query(doc: &Document, params: &serde_json::Value) -> dtool_protocol::CommandResult {
    let id = params.get("id").and_then(serde_json::Value::as_u64).filter(|id| *id > 0).map(ObjectId).ok_or_else(dtool_protocol::CommandError::invalid_params)?;
    let object = doc.get_object(id).ok_or_else(dtool_protocol::CommandError::not_found)?;
    let text = matches!(object.kind, ObjectKind::ArtText { .. } | ObjectKind::FrameText { .. } | ObjectKind::PathText(_));
    let mut marks = text_break_marks(doc, id);
    marks.sort_by_key(|mark| mark.pos);
    let marks: Vec<serde_json::Value> = marks.iter().map(|mark| serde_json::json!({ "kind": mark.kind, "offset": mark.pos, "page_pos": mark.at, "size": mark.size })).collect();
    Ok(serde_json::json!({ "id": id.0, "text": text, "marks": marks }))
}

pub fn can_execute(session: &crate::command::Session, command: &dtool_protocol::Command) -> Option<bool> {
    (command.name == TEXT_MARKS_QUERY).then_some(session.has_document)
}

pub fn run(session: &mut crate::command::Session, command: &dtool_protocol::Command) -> Option<dtool_protocol::CommandResult> {
    (command.name == TEXT_MARKS_QUERY).then(|| if session.has_document { marks_query(&session.document, &command.params) } else { Err(dtool_protocol::CommandError::no_document()) })
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaceRun {
    pub start: usize,
    pub end: usize,
    pub family: String,
    pub style: String,
    pub weight: u16,
    pub italic: bool,
    pub size: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineFace {
    pub family: String,
    pub style: String,
    pub runs: Vec<FaceRun>,
}

impl LineFace {
    pub fn from_runs(runs: Vec<FaceRun>) -> Self {
        let main = runs.iter().max_by_key(|run| run.end.saturating_sub(run.start)).cloned().unwrap_or_default();
        Self { family: main.family, style: main.style, runs }
    }

    pub fn face_at(&self, index: usize) -> (&str, &str) {
        self.runs.iter().find(|run| run.start <= index && index < run.end).map_or((self.family.as_str(), self.style.as_str()), |run| (run.family.as_str(), run.style.as_str()))
    }

    pub fn mixed(&self) -> bool {
        self.runs.iter().any(|run| run.family != self.family || run.style != self.style)
    }
}

pub type TextFaces = fn(&Document, ObjectId) -> Option<Vec<LineFace>>;

static TEXT_FACES: OnceLock<TextFaces> = OnceLock::new();

pub fn register_text_faces(faces: TextFaces) -> bool {
    TEXT_FACES.set(faces).is_ok() || TEXT_FACES.get().is_some()
}

pub fn text_faces(doc: &Document, id: ObjectId) -> Vec<LineFace> {
    if let Some(faces) = TEXT_FACES.get().and_then(|hook| hook(doc, id)) {
        return faces;
    }
    let Some(story) = doc.get_object(id).and_then(|object| object.story_id()).and_then(|story| doc.stories.iter().find(|item| item.id == story)) else { return Vec::new() };
    let face = story_face(story);
    text_lines(doc, id).iter().map(|line| {
        let count = line.text.chars().count();
        LineFace::from_runs(vec![FaceRun { start: 0, end: count, size: line.size, ..face.clone() }])
    }).collect()
}

pub fn story_face(story: &Story) -> FaceRun {
    let range = story.ranges.iter().find(|range| range.get("frame").is_none() && range.get("family").is_some()).or_else(|| story.ranges.first());
    let text = |key: &str| range.and_then(|range| range.get(key)).and_then(|value| value.as_str()).unwrap_or_default().to_string();
    let weight = range.and_then(|range| range.get("weight")).and_then(|value| value.as_u64()).map_or(400, |value| value.min(u16::MAX as u64) as u16);
    let italic = range.and_then(|range| range.get("italic")).and_then(|value| value.as_bool()).unwrap_or(false);
    FaceRun { family: text("family"), style: text("style"), weight, italic, ..FaceRun::default() }
}

pub type TextOutline = ([f32; 2], [f32; 2], crate::curve::Curve);

pub fn text_outline_curve(doc: &Document, id: ObjectId) -> Result<TextOutline, dtool_protocol::CommandError> {
    let hook = TEXT_OUTLINES.get().copied();
    text_outline_from(doc, id, |doc, id| hook.map(|hook| hook(doc, id).unwrap_or_default()))
}

pub fn text_outline_from(doc: &Document, id: ObjectId, outlines: impl FnOnce(&Document, ObjectId) -> Option<Vec<Vec<[f32; 2]>>>) -> Result<TextOutline, dtool_protocol::CommandError> {
    use dtool_protocol::CommandError;
    let object = doc.get_object(id).ok_or_else(CommandError::not_found)?;
    let story = match &object.kind {
        ObjectKind::ArtText { story } | ObjectKind::FrameText { story } => *story,
        ObjectKind::PathText(data) => data.story,
        _ => return Err(CommandError::invalid_params()),
    };
    let text = doc.stories.iter().find(|s| s.id == story).map(|s| s.text.as_str()).unwrap_or("");
    if text.chars().all(char::is_whitespace) {
        return Err(CommandError::new("TEXT_EMPTY"));
    }
    let polygons = outlines(doc, id).ok_or_else(|| CommandError::new("TEXT_OUTLINES_UNAVAILABLE"))?;
    let mut path = kurbo::BezPath::new();
    for polygon in polygons.iter().filter(|p| p.len() >= 3 && p.iter().all(|v| v[0].is_finite() && v[1].is_finite())) {
        path.move_to((f64::from(polygon[0][0]), f64::from(polygon[0][1])));
        for point in &polygon[1..] {
            path.line_to((f64::from(point[0]), f64::from(point[1])));
        }
        path.close_path();
    }
    if path.elements().is_empty() {
        return Err(CommandError::new("FONT_OUTLINE_MISSING"));
    }
    let curve = crate::curve::Curve::from_kurbo(&path);
    let bounds = std::iter::once(&curve.contour).chain(curve.holes.iter()).map(|c| c.bounds()).fold([f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY], |a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]);
    if bounds.iter().any(|v| !v.is_finite()) {
        return Err(CommandError::new("FONT_OUTLINE_MISSING"));
    }
    let pos = [bounds[0], bounds[1]];
    let size = [(bounds[2] - bounds[0]).max(1.0e-3), (bounds[3] - bounds[1]).max(1.0e-3)];
    let delta = [-f64::from(pos[0]), -f64::from(pos[1])];
    let local = |c: &crate::curve::Contour| crate::commands::curve_edit::offset_contour(c, delta);
    Ok((pos, size, crate::curve::Curve::from_contour_and_holes(local(&curve.contour), curve.holes.iter().map(local).collect()).with_source(id.0)))
}

pub fn story_paint(story: &Story) -> Paint {
    story
        .ranges
        .iter()
        .find_map(|r| r.get("fill").and_then(|v| serde_json::from_value::<Paint>(v.clone()).ok()))
        .filter(|p| !matches!(p, Paint::None))
        .unwrap_or(Paint::Solid { colour: Colour::new(0, 0, 0, 255) })
}

pub fn is_text(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Text(_) | ObjectKind::ArtText { .. } | ObjectKind::FrameText { .. } | ObjectKind::PathText(_))
}


fn path_text_origin(doc: &Document, object: &crate::Object, data: &crate::object::PathTextData) -> Option<[f32; 2]> {
    let path = doc.get_object(data.path)?;
    let (mut points, closed) = crate::outline::path_text_layout_contours(object, path).into_iter().next()?;
    if closed { points.push(points[0]); }
    let start = data.flows.first().map(|flow| {
        if flow.start_pct.abs() <= 1.0 { flow.start_pct } else { flow.start_pct / 100.0 }
    }).unwrap_or(0.0);
    crate::scene::along(&points, start).map(|point| crate::geometry::apply_matrix(&crate::geometry::text_matrix(object), point))
}

pub fn estimate_text_lines(doc: &Document, id: ObjectId) -> Vec<TextLine> {
    let Some(object) = doc.get_object(id) else { return Vec::new() };
    let default_size = crate::text_cmd::DEFAULT_SIZE_PT;
    let units_per_point = crate::units::units_per_point(doc.units, doc.dpi);
    let (text, size, paint, wrap) = match &object.kind {
        ObjectKind::Text(s) => (s.clone(), default_size * units_per_point, Paint::Solid { colour: Colour::new(0, 0, 0, 255) }, false),
        ObjectKind::ArtText { story } | ObjectKind::FrameText { story } => {
            let Some(item) = doc.stories.iter().find(|s| s.id == *story) else { return Vec::new() };
            let size = crate::text_cmd::size_pt_of(item, default_size) * units_per_point;
            let frame = matches!(object.kind, ObjectKind::FrameText { .. });
            let text = if frame {
                let ranges = crate::longdoc::pages::flow_ranges(doc, *story);
                match ranges.iter().find(|r| r.frame == id) {
                    Some(r) => item.text.chars().skip(r.start).take(r.end.saturating_sub(r.start)).collect(),
                    None => String::new(),
                }
            } else {
                item.text.clone()
            };
            (text, size, story_paint(item), frame)
        }
        ObjectKind::PathText(data) => {
            let Some(item) = doc.stories.iter().find(|s| s.id == data.story) else { return Vec::new() };
            let size = crate::text_cmd::size_pt_of(item, default_size) * units_per_point;
            let origin = path_text_origin(doc, object, data).unwrap_or(object.pos);
            let advance = (size * 0.5).max(0.1);
            return vec![TextLine {
                object: id,
                origin,
                text: item.text.clone(),
                size,
                width: item.text.chars().count() as f32 * advance,
                para_end: false,
                paint: if object.fill == Paint::None { story_paint(item) } else { object.fill.clone() },
            }];
        }
        _ => return Vec::new(),
    };
    let advance = (size * 0.5).max(0.1);
    let max_chars = if wrap { ((object.size[0] / advance).floor() as usize).max(1) } else { usize::MAX };
    let mut out = Vec::new();
    let mut row = 0usize;
    let paras: Vec<&str> = text.split('\n').collect();
    for (pi, para) in paras.iter().enumerate() {
        let chars: Vec<char> = para.chars().collect();
        let mut start = 0usize;
        loop {
            let end = (start + max_chars).min(chars.len());
            let baseline = object.pos[1] + size * 0.8 + row as f32 * size * 1.2;
            if wrap && baseline > object.pos[1] + object.size[1] {
                return out;
            }
            let piece: String = chars[start..end].iter().collect();
            out.push(TextLine {
                object: id,
                origin: [object.pos[0], baseline],
                width: piece.chars().count() as f32 * advance,
                text: piece,
                size,
                para_end: end == chars.len() && pi + 1 < paras.len(),
                paint: paint.clone(),
            });
            row += 1;
            if end >= chars.len() {
                break;
            }
            start = end;
        }
    }
    out
}

pub fn text_lines(doc: &Document, id: ObjectId) -> Vec<TextLine> {
    TEXT_LINES
        .get()
        .and_then(|lines| lines(doc, id))
        .unwrap_or_else(|| estimate_text_lines(doc, id))
}
