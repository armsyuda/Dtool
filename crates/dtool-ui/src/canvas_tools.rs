use dtool_core::command::Session;
use dtool_core::object::ObjectId;
use dtool_core::paint::{Colour, GradientGeom, GradientKind, GradientStop, Paint};
use dtool_tools::vector_crop::VectorCropTool;
use dtool_core::snap_candidates::{snap_candidates, CandidateKind, SnapDocument};
use dtool_core::scene::Scene;
use dtool_core::scene_hit::SelectionQuads;
use dtool_protocol::{Command, CommandError};
use dtool_core::prefs::Prefs;
use dtool_tools::move_tool::{MoveTool, NudgeDirection};
use serde_json::{json, Value};

pub fn nudge_distances() -> [f32; 2] {
    let prefs = Prefs::load(&Prefs::default_path());
    let fallback = Prefs::default();
    let pick = |value: f32, default: f32| if value.is_finite() && value > 0.0 { value } else { default };
    [pick(prefs.nudge_distance, fallback.nudge_distance), pick(prefs.modifier_nudge_distance, fallback.modifier_nudge_distance)]
}

pub fn arrow_key(input: &egui::InputState, tool: &MoveTool) -> Option<(NudgeDirection, bool)> {
    let (direction, shift) = crate::shell::nudge_key(input)?;
    tool.on_arrow(direction, shift)?;
    Some((direction, shift))
}

pub fn arrow_delta(direction: NudgeDirection, shift: bool, distances: [f32; 2]) -> [f32; 2] {
    let step = if shift { distances[1] } else { distances[0] };
    match direction {
        NudgeDirection::Left => [-step, 0.0],
        NudgeDirection::Right => [step, 0.0],
        NudgeDirection::Up => [0.0, -step],
        NudgeDirection::Down => [0.0, step],
    }
}

pub fn nudge_command(delta: [f32; 2]) -> Command {
    Command::with_params("selection.nudge", json!({ "dx": delta[0], "dy": delta[1] }))
}

pub fn nudge(ctx: &egui::Context, tool: &MoveTool, session: &mut Session, distances: impl FnOnce() -> [f32; 2]) -> Option<Result<Value, CommandError>> {
    if session.selection.ids.is_empty() || session.text_edit.is_some() || ctx.memory(|m| m.focused().is_some()) {
        return None;
    }
    let (direction, shift) = ctx.input(|i| arrow_key(i, tool))?;
    Some(session.run(&nudge_command(arrow_delta(direction, shift, distances()))))
}

pub const SNAP_MAX_CANDIDATES: u32 = 64;

#[derive(Clone, Copy)]
pub(crate) struct AxisSnap {
    pub delta: f32,
    pub point: [f32; 2],
    pub kind: CandidateKind,
}

pub fn snap_offset(session: &Session, points: &[[f32; 2]], exclude: &[ObjectId]) -> Option<([f32; 2], [f32; 2])> {
    let best = axis_snaps(session, points, exclude);
    let mark = best[0].or(best[1])?.point;
    Some((best.map(|snap| snap.map_or(0.0, |snap| snap.delta)), mark))
}

pub(crate) fn axis_snaps(session: &Session, points: &[[f32; 2]], exclude: &[ObjectId]) -> [Option<AxisSnap>; 2] {
    let current = &session.snap_prefs.query();
    if !current.enabled {
        return [None; 2];
    }
    let mut prefs = current.clone();
    prefs.max_candidates = prefs.max_candidates.max(SNAP_MAX_CANDIDATES);
    let doc = SnapDocument { document: &session.document, guides: &session.guides, grid: &session.document.grid };
    let mut best: [Option<AxisSnap>; 2] = [None, None];
    for point in points {
        for candidate in snap_candidates(&doc, &session.view, &prefs, *point) {
            if matches!(candidate.kind, CandidateKind::ObjectBounds | CandidateKind::ObjectGeometry)
                && candidate.owner.is_some_and(|owner| exclude.iter().any(|id| id.0 == owner)) {
                continue;
            }
            for axis in 0..2 {
                let delta = candidate.point[axis] - point[axis];
                if delta.abs() > 1e-6 && best[axis].is_none_or(|b| delta.abs() < b.delta.abs()) {
                    best[axis] = Some(AxisSnap { delta, point: candidate.point, kind: candidate.kind });
                }
            }
        }
    }
    best
}

pub fn snap_point(session: &Session, point: [f32; 2], exclude: &[ObjectId]) -> ([f32; 2], Option<[f32; 2]>) {
    match snap_offset(session, &[point], exclude) {
        Some((offset, mark)) => ([point[0] + offset[0], point[1] + offset[1]], Some(mark)),
        None => (point, None),
    }
}

pub const GUIDE_EPSILON: f32 = 1e-3;
pub const ROTATE_STEP_DEG: f32 = 15.0;
pub const ROTATE_SNAP_DEG: f32 = 3.0;

pub fn handle_fraction(handle: dtool_tools::move_tool::Handle) -> [f32; 2] {
    use dtool_tools::move_tool::Handle;
    match handle {
        Handle::TopLeft => [0.0, 0.0],
        Handle::Top | Handle::Rotate | Handle::ShearTop => [0.5, 0.0],
        Handle::TopRight => [1.0, 0.0],
        Handle::Left => [0.0, 0.5],
        Handle::Right => [1.0, 0.5],
        Handle::BottomLeft => [0.0, 1.0],
        Handle::Bottom | Handle::ShearBottom => [0.5, 1.0],
        Handle::BottomRight | Handle::FrameTextScale => [1.0, 1.0],
    }
}

pub fn snap_rotation(origin: [f32; 2], start: [f32; 2], point: [f32; 2]) -> Option<[f32; 2]> {
    let radius = (point[0] - origin[0]).hypot(point[1] - origin[1]);
    let start_radius = (start[0] - origin[0]).hypot(start[1] - origin[1]);
    if radius <= f32::EPSILON || start_radius <= f32::EPSILON {
        return None;
    }
    let a0 = (start[1] - origin[1]).atan2(start[0] - origin[0]);
    let a1 = (point[1] - origin[1]).atan2(point[0] - origin[0]);
    let deg = (a1 - a0).to_degrees();
    let target = (deg / ROTATE_STEP_DEG).round() * ROTATE_STEP_DEG;
    let diff = (target - deg).abs();
    if diff > ROTATE_SNAP_DEG || diff < 1e-4 {
        return None;
    }
    let angle = a0 + target.to_radians();
    Some([origin[0] + radius * angle.cos(), origin[1] + radius * angle.sin()])
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuideLine {
    pub from: [f32; 2],
    pub to: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GapGuide {
    pub line: GuideLine,
    pub matched: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct GapSnap {
    pub delta: f32,
    spans: [[f32; 2]; 2],
    cross: f32,
}

fn gap_line(axis: usize, span: [f32; 2], cross: f32) -> GuideLine {
    match axis {
        0 => GuideLine { from: [span[0], cross], to: [span[1], cross] },
        _ => GuideLine { from: [cross, span[0]], to: [cross, span[1]] },
    }
}

fn end(rect: [f32; 4], axis: usize) -> f32 { rect[axis] + rect[axis + 2] }

fn shares_lane(a: [f32; 4], b: [f32; 4], cross: usize) -> bool {
    a[cross].max(b[cross]) < end(a, cross).min(end(b, cross))
}

fn gap_neighbours(boxes: &[[f32; 4]], rect: [f32; 4], axis: usize) -> [Option<[f32; 4]>; 2] {
    let cross = 1 - axis;
    let mut before: Option<[f32; 4]> = None;
    let mut after: Option<[f32; 4]> = None;
    for &other in boxes.iter().filter(|&&other| shares_lane(rect, other, cross)) {
        if end(other, axis) <= rect[axis] + GUIDE_EPSILON
            && before.is_none_or(|old| end(other, axis) > end(old, axis))
        {
            before = Some(other);
        }
        if other[axis] >= end(rect, axis) - GUIDE_EPSILON
            && after.is_none_or(|old| other[axis] < old[axis])
        {
            after = Some(other);
        }
    }
    [before, after]
}

pub(crate) fn gap_snap(boxes: &[[f32; 4]], rect: [f32; 4], axis: usize, tolerance: f32) -> Option<GapSnap> {
    let cross = 1 - axis;
    let start = rect[axis];
    let finish = end(rect, axis);
    let [before, after] = gap_neighbours(boxes, rect, axis);
    let mut best: Option<GapSnap> = None;
    let mut offer = |delta: f32, spans: [[f32; 2]; 2], neighbours: [[f32; 4]; 2]| {
        if delta.abs() > tolerance || spans.iter().any(|[a, b]| b - a <= GUIDE_EPSILON)
            || best.is_some_and(|old| old.delta.abs() <= delta.abs())
        {
            return;
        }
        let min = neighbours.iter().fold(rect[cross], |min, item| min.max(item[cross]));
        let max = neighbours.iter().fold(end(rect, cross), |max, &item| max.min(end(item, cross)));
        if min < max {
            best = Some(GapSnap { delta, spans, cross: (min + max) * 0.5 });
        }
    };
    if let (Some(left), Some(right)) = (before, after) {
        let l = end(left, axis);
        let r = right[axis];
        let delta = ((r - finish) - (start - l)) * 0.5;
        offer(delta, [[l, start + delta], [finish + delta, r]], [left, right]);
    }
    if let Some(near) = before
        && let Some(far) = gap_neighbours(boxes, near, axis)[0].filter(|&far| shares_lane(rect, far, cross))
    {
        let previous = [end(far, axis), near[axis]];
        let l = end(near, axis);
        let delta = previous[1] - previous[0] + l - start;
        if after.is_none_or(|right| finish + delta <= right[axis] + GUIDE_EPSILON) {
            offer(delta, [previous, [l, start + delta]], [far, near]);
        }
    }
    if let Some(near) = after
        && let Some(far) = gap_neighbours(boxes, near, axis)[1].filter(|&far| shares_lane(rect, far, cross))
    {
        let following = [end(near, axis), far[axis]];
        let r = near[axis];
        let delta = r - finish - (following[1] - following[0]);
        if before.is_none_or(|left| start + delta >= end(left, axis) - GUIDE_EPSILON) {
            offer(delta, [[finish + delta, r], following], [near, far]);
        }
    }
    best
}

pub(crate) fn gap_guides(boxes: &[[f32; 4]], rect: [f32; 4], show_equal: bool) -> Vec<GapGuide> {
    let mut guides = Vec::new();
    for axis in 0..2 {
        if show_equal && let Some(gap) = gap_snap(boxes, rect, axis, GUIDE_EPSILON) {
            guides.extend(gap.spans.map(|span| GapGuide { line: gap_line(axis, span, gap.cross), matched: true }));
            continue;
        }
        let cross = 1 - axis;
        let [before, after] = gap_neighbours(boxes, rect, axis);
        for (item, is_before) in [(before, true), (after, false)] {
            let Some(other) = item else { continue };
            let span = if is_before { [end(other, axis), rect[axis]] } else { [end(rect, axis), other[axis]] };
            if span[1] - span[0] > GUIDE_EPSILON {
                let cross = (rect[cross].max(other[cross]) + end(rect, cross).min(end(other, cross))) * 0.5;
                guides.push(GapGuide { line: gap_line(axis, span, cross), matched: false });
            }
        }
    }
    guides
}

pub(crate) fn matched_size(boxes: &[[f32; 4]], rect: [f32; 4], axis: usize, size: f32, tolerance: f32) -> Option<[f32; 4]> {
    let cross = 1 - axis;
    let distance = |other: &[f32; 4]| (other[cross] + other[cross + 2] * 0.5
        - rect[cross] - rect[cross + 2] * 0.5).abs();
    boxes.iter().copied()
        .filter(|other| other[axis + 2] > GUIDE_EPSILON && (other[axis + 2] - size).abs() <= tolerance)
        .min_by(|a, b| (a[axis + 2] - size).abs().total_cmp(&(b[axis + 2] - size).abs())
            .then_with(|| distance(a).total_cmp(&distance(b))))
}

pub(crate) fn size_guides(rect: [f32; 4], other: [f32; 4], axis: usize) -> [GapGuide; 2] {
    [rect, other].map(|b| GapGuide {
        line: gap_line(axis, [b[axis], end(b, axis)], b[1 - axis]),
        matched: true,
    })
}

pub(crate) fn guides_enabled(prefs: &dtool_core::snap_prefs::SnapPrefs) -> bool {
    prefs.enabled && prefs.dynamic_guides && prefs.object_snapping && prefs.snap_to_object_bounds
}

fn under(parents: &std::collections::HashMap<ObjectId, ObjectId>, id: ObjectId, exclude: &[ObjectId]) -> bool {
    let mut current = Some(id);
    for _ in 0..64 {
        let Some(c) = current else { return false };
        if exclude.contains(&c) {
            return true;
        }
        current = parents.get(&c).copied();
    }
    false
}

pub fn object_boxes(session: &Session, scene: &Scene, exclude: &[ObjectId]) -> Vec<[f32; 4]> {
    let prefs = &session.snap_prefs.query();
    let doc = &session.document;
    let quads = SelectionQuads::new(scene, doc);
    let mut parents = std::collections::HashMap::new();
    if !exclude.is_empty() {
        for object in doc.objects() {
            if let Some(parent) = object.parent_id {
                parents.entry(object.id).or_insert(parent);
            }
        }
    }
    doc.objects()
        .iter()
        .filter(|o| o.pickable && o.kind != dtool_core::ObjectKind::Artboard && !o.is_layer_folder()
            && !(prefs.only_visible_objects && o.opacity <= 0.0) && !under(&parents, o.id, exclude))
        .filter_map(|o| {
            if o.rotation_deg == 0.0 && o.shear_deg == 0.0
                && let Some(quad) = quads.visible_quad(o)
            {
                let [x0, y0, x1, y1] = dtool_core::scene::contours_bounds(&[quad.to_vec()])?;
                Some([x0, y0, x1 - x0, y1 - y0])
            } else {
                quad_box(&dtool_core::geometry::object_world_quad(o))
            }
        })
        .collect()
}

fn quad_box(quad: &[[f32; 2]; 4]) -> Option<[f32; 4]> {
    let xs = quad.iter().map(|p| p[0]);
    let ys = quad.iter().map(|p| p[1]);
    let (x0, x1) = xs.fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
    let (y0, y1) = ys.fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)));
    (x0.is_finite() && y0.is_finite() && x1 >= x0 && y1 >= y0).then_some([x0, y0, x1 - x0, y1 - y0])
}

fn stops(rect: [f32; 4], axis: usize, midpoints: bool) -> impl Iterator<Item = f32> {
    [rect[axis], end(rect, axis), rect[axis] + rect[axis + 2] * 0.5]
        .into_iter().take(2 + usize::from(midpoints))
}

pub fn alignment_guides(session: &Session, scene: &Scene, rect: [f32; 4], exclude: &[ObjectId]) -> Vec<GuideLine> {
    let prefs = &session.snap_prefs.query();
    if !guides_enabled(prefs) {
        return Vec::new();
    }
    box_alignment_guides(&object_boxes(session, scene, exclude), rect, prefs.include_bbox_midpoints)
}

pub(crate) fn box_alignment_guides(boxes: &[[f32; 4]], rect: [f32; 4], midpoints: bool) -> Vec<GuideLine> {
    let mut lines: Vec<(usize, f32, [f32; 2])> = Vec::new();
    for &other in boxes {
        for axis in 0..2 {
            let across = 1 - axis;
            for a in stops(rect, axis, midpoints) {
                for b in stops(other, axis, midpoints) {
                    if (a - b).abs() > GUIDE_EPSILON {
                        continue;
                    }
                    let span = [rect[across].min(other[across]), (rect[across] + rect[across + 2]).max(other[across] + other[across + 2])];
                    match lines.iter_mut().find(|(k, p, _)| *k == axis && (*p - b).abs() <= GUIDE_EPSILON) {
                        Some((_, _, extent)) => *extent = [extent[0].min(span[0]), extent[1].max(span[1])],
                        None => lines.push((axis, b, span)),
                    }
                }
            }
        }
    }
    lines
        .into_iter()
        .map(|(axis, position, extent)| match axis {
            0 => GuideLine { from: [position, extent[0]], to: [position, extent[1]] },
            _ => GuideLine { from: [extent[0], position], to: [extent[1], position] },
        })
        .collect()
}

pub fn guide_offset(session: &Session, scene: &Scene, rect: [f32; 4], exclude: &[ObjectId]) -> [Option<f32>; 2] {
    let prefs = &session.snap_prefs.query();
    if !guides_enabled(prefs) {
        return [None, None];
    }
    let tolerance = prefs.screen_tolerance as f32 / session.view.zoom.max(1e-4);
    box_guide_offset(&object_boxes(session, scene, exclude), rect, prefs.include_bbox_midpoints, tolerance)
}

pub(crate) fn box_guide_offset(boxes: &[[f32; 4]], rect: [f32; 4], midpoints: bool, tolerance: f32) -> [Option<f32>; 2] {
    let mut best: [Option<f32>; 2] = [None, None];
    for &other in boxes {
        for axis in 0..2 {
            for a in stops(rect, axis, midpoints) {
                for b in stops(other, axis, midpoints) {
                    let d = b - a;
                    if d.abs() <= tolerance && best[axis].is_none_or(|x| d.abs() < x.abs()) {
                        best[axis] = Some(d);
                    }
                }
            }
        }
    }
    best
}

pub fn merge_offsets(snap: Option<([f32; 2], [f32; 2])>, guide: [Option<f32>; 2], rect: [f32; 4]) -> Option<([f32; 2], [f32; 2])> {
    let mut offset = [0.0f32; 2];
    let mut from_guide = false;
    for axis in 0..2 {
        let s = snap.map(|(o, _)| o[axis]).filter(|v| *v != 0.0);
        offset[axis] = match (s, guide[axis]) {
            (Some(s), Some(g)) if g.abs() < s.abs() => {
                from_guide = true;
                g
            }
            (Some(s), _) => s,
            (None, Some(g)) => {
                from_guide = true;
                g
            }
            (None, None) => 0.0,
        };
    }
    if offset == [0.0, 0.0] {
        return None;
    }
    let mark = match snap {
        Some((_, mark)) if !from_guide => mark,
        _ => [rect[0] + offset[0], rect[1] + offset[1]],
    };
    Some((offset, mark))
}

pub fn box_points(rect: [f32; 4], delta: [f32; 2]) -> [[f32; 2]; 5] {
    let x = rect[0] + delta[0];
    let y = rect[1] + delta[1];
    [[x, y], [x + rect[2], y], [x, y + rect[3]], [x + rect[2], y + rect[3]], [x + rect[2] * 0.5, y + rect[3] * 0.5]]
}

pub fn shear_grips(bounds: [f32; 4], zoom: f32) -> [[f32; 2]; 2] {
    let r = crate::canvas_input::HANDLE_PX / zoom.max(1e-4);
    dtool_tools::move_tool::shear_handle_points(bounds, r)
}

pub fn diamond(centre: egui::Pos2, half: f32) -> Vec<egui::Pos2> {
    vec![centre + egui::vec2(0.0, -half), centre + egui::vec2(half, 0.0), centre + egui::vec2(0.0, half), centre + egui::vec2(-half, 0.0)]
}

pub fn gradient_kind(active_tool: &str) -> Option<GradientKind> {
    dtool_core::commands::context_toolbar::gradient::parse_kind(active_tool.rsplit('.').next()?)
}

fn drag_rect(start: [f32; 2], end: [f32; 2]) -> [f32; 4] {
    [start[0].min(end[0]), start[1].min(end[1]), (end[0] - start[0]).abs(), (end[1] - start[1]).abs()]
}

pub fn crop_command(session: &Session, target: ObjectId, start: [f32; 2], end: [f32; 2]) -> Option<Command> {
    let object = session.document.get_object(target)?;
    let r = drag_rect(start, end);
    if r[2] < crate::canvas_input::MIN_DRAG_DOC || r[3] < crate::canvas_input::MIN_DRAG_DOC {
        return None;
    }
    let mut tool = VectorCropTool::new();
    tool.set_crop(target.0, [r[0] - object.pos[0], r[1] - object.pos[1], r[2], r[3]]);
    tool.command()
}

pub fn clear_crop_command(session: &Session, target: ObjectId) -> Option<Command> {
    session.document.get_object(target)?.vector_crop.as_ref()?;
    let mut tool = VectorCropTool::new();
    tool.set_crop(target.0, [0.0, 0.0, 1.0, 1.0]);
    tool.clear_crop();
    tool.command()
}

pub fn gradient_command_for_tool(
    session: &Session, target: ObjectId, kind: Option<GradientKind>, transparency: bool, start: [f32; 2], end: [f32; 2],
) -> Option<Command> {
    use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix};
    use dtool_core::commands::context_toolbar::gradient::{Target, TYPE};
    let object = session.document.get_object(target).filter(|object| !object.locked)?;
    if (end[0] - start[0]).hypot(end[1] - start[1]) < crate::canvas_input::MIN_DRAG_DOC {
        return None;
    }
    let inverse = invert_matrix(&object_matrix(object))?;
    let local = |point| {
        let [x, y] = apply_matrix(&inverse, point);
        [f64::from(x - object.pos[0]), f64::from(y - object.pos[1])]
    };
    let tool = if transparency { "tool.transparency" } else { &session.active_tool };
    let role = Target::for_object(session, tool, object)
        .or_else(|| (!tool.starts_with("tool.gradient") && !transparency).then_some(Target::Fill))?;
    if match role {
        Target::Stroke => object.stroke.width <= 0.0,
        Target::Appearance(slot) => slot.stroke(object).is_some_and(|stroke| stroke.width <= 0.0),
        _ => false,
    } {
        return None;
    }
    let choice = session.tool_options.choice(tool, TYPE);
    let chosen = choice.as_deref().and_then(gradient_kind);
    let (kind, stops, previous) = if let Some((current, stops, geom)) = role.gradient(object) {
        (current, stops.to_vec(), *geom)
    } else {
        let stops = if transparency {
            vec![
                GradientStop { offset: 0.0, colour: Colour::new(0, 0, 0, 255) },
                GradientStop { offset: 1.0, colour: Colour::new(0, 0, 0, 0) },
            ]
        } else {
            let colour = match role.paint(object)? { Paint::Solid { colour } => *colour, _ => Colour::new(0, 0, 0, 255) };
            vec![
                GradientStop { offset: 0.0, colour },
                GradientStop { offset: 1.0, colour: Colour::new(255, 255, 255, 255) },
            ]
        };
        (chosen.or(kind).unwrap_or(GradientKind::Linear), stops, GradientGeom::default())
    };
    let geometry = GradientGeom { start: local(start), end: local(end), ..previous };
    Some(role.command(object, kind, stops, geometry))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtool_core::document::Units;
    use dtool_core::object::{Object, ObjectId};
    use dtool_core::Document;

    pub(super) fn session() -> (Session, ObjectId, ObjectId) {
        let mut doc = Document::new([400.0, 400.0], Units::Px, 72.0, 1);
        let red = dtool_core::paint::Paint::Solid { colour: dtool_core::paint::Colour::new(255, 0, 0, 255) };
        let a = doc.create_object(Object { pos: [10.0, 10.0], size: [50.0, 50.0], fill: red.clone(), ..Object::default() });
        let b = doc.create_object(Object { pos: [200.0, 200.0], size: [40.0, 40.0], fill: red, ..Object::default() });
        let mut session = Session::new(doc);
        session.has_document = true;
        (session, a, b)
    }

    pub(super) fn key_ctx(key: egui::Key, shift: bool) -> egui::Context {
        let modifiers = egui::Modifiers { shift, ..Default::default() };
        let ctx = egui::Context::default();
        let events = vec![egui::Event::ModifiersChanged(modifiers), egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }];
        ctx.begin_pass(egui::RawInput { events, ..Default::default() });
        ctx
    }

    #[test]
    fn arrow_nudge_one_undo_per_press() {
        let (mut s, a, _) = session();
        let tool = MoveTool::default();
        let prefs = || [2.0, 20.0];
        assert!(nudge(&key_ctx(egui::Key::ArrowRight, false), &tool, &mut s, prefs).is_none());
        s.run(&Command::with_params("selection.set", json!({ "ids": [a.0], "op": "Replace" }))).unwrap();
        assert!(nudge(&key_ctx(egui::Key::ArrowRight, false), &tool, &mut s, prefs).unwrap().is_ok());
        assert_eq!(s.document.get_object(a).unwrap().pos, [12.0, 10.0]);
        assert!(nudge(&key_ctx(egui::Key::ArrowDown, true), &tool, &mut s, prefs).unwrap().is_ok());
        assert_eq!(s.document.get_object(a).unwrap().pos, [12.0, 30.0]);
        let mut busy = MoveTool::default();
        busy.dragging = true;
        assert!(nudge(&key_ctx(egui::Key::ArrowLeft, false), &busy, &mut s, prefs).is_none());
        assert_eq!(nudge_command(arrow_delta(NudgeDirection::Up, false, [1.0, 10.0])).params, json!({ "dx": 0.0, "dy": -1.0 }));
        let d = nudge_distances();
        assert!(d.iter().all(|v| v.is_finite() && *v > 0.0));
        s.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(s.document.get_object(a).unwrap().pos, [12.0, 10.0]);
        s.run(&Command::new("edit.undo")).unwrap();
        assert_eq!(s.document.get_object(a).unwrap().pos, [10.0, 10.0]);
    }

    #[test]
    fn snap_point_to_object_edge() {
        let (mut s, a, _) = session();
        s.snap_prefs.replace(dtool_core::snap_prefs::SnapPrefs::default());
        assert_eq!(snap_point(&s, [62.0, 100.0], &[]).0, [60.0, 100.0]);
        assert_eq!(snap_point(&s, [62.0, 100.0], &[a]).0, [62.0, 100.0]);
        let mut off = dtool_core::snap_prefs::SnapPrefs::default();
        off.enabled = false;
        s.snap_prefs.replace(off);
        assert_eq!(snap_point(&s, [62.0, 100.0], &[]), ([62.0, 100.0], None));
    }

    #[test]
    fn move_drag_snaps_box_edge() {
        let (mut s, _, b) = session();
        s.snap_prefs.replace(dtool_core::snap_prefs::SnapPrefs::default());
        let scene = dtool_core::scene::estimate_scene(&s.document, &dtool_core::CanvasFlags::default());
        let mods = crate::canvas_input::Mods { shift: false, ctrl: false, alt: false };
        let mut input = crate::canvas_input::CanvasInput::new();
        input.press_move(&scene, &mut s, [210.0, 210.0], mods);
        let targets = crate::canvas_input::DragSnapTargets::new(&s, &scene);
        let p = input.snap_drag(&targets, [72.0, 210.0], mods);
        assert_eq!(p, [70.0, 210.0]);
        assert!(input.snap_mark.is_some());
        input.drag_move(p, mods);
        input.update_guides(&targets, p);
        assert_eq!(input.guide_lines, vec![GuideLine { from: [60.0, 10.0], to: [60.0, 240.0] }]);
        assert!(alignment_guides(&s, &scene, [61.0, 200.0, 40.0, 40.0], &[b]).is_empty());
        let centred = alignment_guides(&s, &scene, [10.0, 200.0, 50.0, 40.0], &[b]);
        assert!(centred.contains(&GuideLine { from: [35.0, 10.0], to: [35.0, 240.0] }), "{centred:?}");
        input.release_move(&scene, &mut s, p, mods);
        assert_eq!(s.document.get_object(b).unwrap().pos, [60.0, 200.0]);
    }

    #[test]
    fn guides_use_rotated_bounds_and_prefs() {
        let (mut s, a, b) = session();
        s.snap_prefs.replace(dtool_core::snap_prefs::SnapPrefs::default());
        s.document.objects.iter_mut().find(|o| o.id == a).unwrap().rotation_deg = 45.0;
        let scene = dtool_core::scene::estimate_scene(&s.document, &dtool_core::CanvasFlags::default());
        let bb = object_boxes(&s, &scene, &[b]).into_iter().next().unwrap();
        assert!(bb[2] > 60.0, "{bb:?}");
        let right = bb[0] + bb[2];
        let lines = alignment_guides(&s, &scene, [right, 200.0, 40.0, 40.0], &[b]);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].from[0], right);
        assert!((lines[0].from[1] - bb[1]).abs() < 1e-3 && lines[0].to[1] == 240.0);
        assert!(alignment_guides(&s, &scene, [60.0, 200.0, 40.0, 40.0], &[b]).is_empty());
        let offset = guide_offset(&s, &scene, [right + 3.0, 200.0, 40.0, 40.0], &[b]);
        assert!(offset[0].is_some_and(|d| (d + 3.0).abs() < 1e-3), "{offset:?}");
        assert!(alignment_guides(&s, &scene, [right, 200.0, 40.0, 40.0], &[a, b]).is_empty());
        let mut off = dtool_core::snap_prefs::SnapPrefs::default();
        off.dynamic_guides = false;
        s.snap_prefs.replace(off.clone());
        assert!(alignment_guides(&s, &scene, [right, 200.0, 40.0, 40.0], &[b]).is_empty());
        assert_eq!(guide_offset(&s, &scene, [right + 3.0, 200.0, 40.0, 40.0], &[b]), [None, None]);
        off.dynamic_guides = true;
        off.object_snapping = false;
        s.snap_prefs.replace(off);
        assert!(alignment_guides(&s, &scene, [right, 200.0, 40.0, 40.0], &[b]).is_empty());
        assert_eq!(merge_offsets(Some(([2.0, 0.0], [1.0, 1.0])), [Some(-1.0), Some(0.5)], [0.0, 0.0, 1.0, 1.0]), Some(([-1.0, 0.5], [-1.0, 0.5])));
        assert_eq!(merge_offsets(Some(([1.0, 0.0], [5.0, 5.0])), [Some(2.0), None], [0.0, 0.0, 1.0, 1.0]), Some(([1.0, 0.0], [5.0, 5.0])));
        assert_eq!(merge_offsets(None, [None, None], [0.0, 0.0, 1.0, 1.0]), None);
    }

    #[test]
    fn resize_and_rotate_handles_snap() {
        let (mut s, _, b) = session();
        s.snap_prefs.replace(dtool_core::snap_prefs::SnapPrefs::default());
        s.run(&Command::with_params("selection.set", json!({ "ids": [b.0], "op": "Replace" }))).unwrap();
        let scene = dtool_core::scene::estimate_scene(&s.document, &dtool_core::CanvasFlags::default());
        let mods = crate::canvas_input::Mods { shift: false, ctrl: false, alt: false };
        let mut input = crate::canvas_input::CanvasInput::new();
        input.press_move(&scene, &mut s, [200.0, 220.0], mods);
        assert_eq!(input.move_tool.gesture, Some(dtool_tools::move_tool::GestureKind::Scale(dtool_tools::move_tool::Handle::Left)));
        let targets = crate::canvas_input::DragSnapTargets::new(&s, &scene);
        assert_eq!(input.snap_drag(&targets, [62.0, 223.0], mods), [60.0, 223.0]);
        input.cancel();
        let origin = [100.0, 100.0];
        let snapped = snap_rotation(origin, [110.0, 100.0], [110.0, 102.5]).unwrap();
        let expected = [100.0 + 10.0_f32.hypot(2.5) * 15.0_f32.to_radians().cos(), 100.0 + 10.0_f32.hypot(2.5) * 15.0_f32.to_radians().sin()];
        assert!((snapped[0] - expected[0]).abs() < 1e-3 && (snapped[1] - expected[1]).abs() < 1e-3, "{snapped:?}");
        assert!(snap_rotation(origin, [110.0, 100.0], [110.0, 102.0]).is_none());
        assert!(snap_rotation(origin, [110.0, 100.0], [100.0, 100.0]).is_none());
        let back = snap_rotation(origin, [110.0, 100.0], [110.0, 100.2]).unwrap();
        assert!((back[1] - 100.0).abs() < 1e-3);
        s.run(&Command::with_params("selection.set", json!({ "ids": [b.0], "op": "Replace" }))).unwrap();
        input.press_move(&scene, &mut s, [220.0, 200.0 - input.move_tool.rotate_offset], mods);
        assert_eq!(input.move_tool.gesture, Some(dtool_tools::move_tool::GestureKind::Rotate));
        let press = input.press_at.unwrap();
        let raw = [press[0] + 10.0, press[1]];
        let targets = crate::canvas_input::DragSnapTargets::new(&s, &scene);
        let p = input.snap_drag(&targets, raw, mods);
        assert!(input.snap_mark.is_some() && p != raw, "{p:?}");
        assert_eq!(input.snap_drag(&targets, raw, crate::canvas_input::Mods { shift: true, ..mods }), raw);
    }

    #[test]
    fn double_click_enters_group_and_escape_exits() {
        let (mut s, a, _) = session();
        let fill = dtool_core::paint::Paint::Solid { colour: dtool_core::paint::Colour::new(0, 0, 255, 255) };
        let g = s.document.create_object(Object { kind: dtool_core::object::ObjectKind::Group, pos: [100.0, 100.0], size: [90.0, 90.0], ..Object::default() });
        let c1 = s.document.create_object(Object { pos: [100.0, 100.0], size: [40.0, 40.0], fill: fill.clone(), parent_id: Some(g), ..Object::default() });
        let c2 = s.document.create_object(Object { pos: [150.0, 150.0], size: [40.0, 40.0], fill, parent_id: Some(g), ..Object::default() });
        let scene = dtool_core::scene::estimate_scene(&s.document, &dtool_core::CanvasFlags::default());
        let mods = crate::canvas_input::Mods { shift: false, ctrl: false, alt: false };
        let mut input = crate::canvas_input::CanvasInput::new();
        input.click_move(&scene, &mut s, [110.0, 110.0], mods);
        assert_eq!(s.selection.ids, vec![g]);
        assert!(input.enter_group(&scene, &mut s, [110.0, 110.0]));
        assert_eq!((input.scope, s.selection.ids.clone()), (Some(g), vec![c1]));
        input.click_move(&scene, &mut s, [160.0, 160.0], mods);
        assert_eq!((input.scope, s.selection.ids.clone()), (Some(g), vec![c2]));
        assert!(input.exit_group(&mut s));
        assert_eq!((input.scope, s.selection.ids.clone()), (None, vec![g]));
        assert!(input.enter_group(&scene, &mut s, [160.0, 160.0]));
        input.click_move(&scene, &mut s, [20.0, 20.0], mods);
        assert_eq!((input.scope, s.selection.ids.clone()), (None, vec![a]));
    }

    #[test]
    fn shear_grips_hit_shear_handles() {
        use dtool_tools::move_tool::Handle;
        for zoom in [0.5_f32, 1.0, 3.0] {
            let bounds = [10.0, 20.0, 200.0, 80.0];
            let mut tool = MoveTool::default();
            tool.handle_radius = crate::canvas_input::HANDLE_PX / zoom;
            tool.rotate_offset = crate::canvas_input::ROTATE_PX / zoom;
            let [top, bottom] = shear_grips(bounds, zoom);
            assert_eq!(tool.hit_handle(bounds, top), Some(Handle::ShearTop));
            assert_eq!(tool.hit_handle(bounds, bottom), Some(Handle::ShearBottom));
        }
    }

    #[test]
    fn crop_and_gradient_drags() {
        use crate::canvas_input::{tool_kind, CanvasInput, Drag, Mods, ToolKind};
        assert_eq!(tool_kind("tool.vector_crop"), ToolKind::VectorCrop);
        assert_eq!(tool_kind("tool.gradient.radial"), ToolKind::Gradient(Some(GradientKind::Radial)));
        assert_eq!(tool_kind("tool.gradient"), ToolKind::Gradient(None));
        let (mut s, a, _) = session();
        let mods = Mods { shift: false, ctrl: false, alt: false };
        let scene = dtool_core::scene::estimate_scene(&s.document, &dtool_core::CanvasFlags::default());
        let mut input = CanvasInput::new();
        assert!(input.tool_drag(ToolKind::Crop, &scene, &s, [20.0, 20.0]).is_none());
        let drag = input.tool_drag(ToolKind::Gradient(None), &scene, &s, [12.0, 12.0]);
        assert!(matches!(drag, Some(Drag::Gradient { target, .. }) if target == a));
        input.drag = drag;
        assert!(input.finish_drag(&mut s, [62.0, 12.0], mods), "{:?}", input.last_error);
        match &s.document.get_object(a).unwrap().fill {
            Paint::Gradient { kind, stops, geom } => {
                assert_eq!(*kind, GradientKind::Linear);
                assert_eq!((geom.start, geom.end), ([2.0, 2.0], [52.0, 2.0]));
                assert_eq!(stops[0].colour, Colour::new(255, 0, 0, 255));
            }
            other => panic!("{other:?}"),
        }
        s.run(&Command::new("edit.undo")).unwrap();
        assert!(matches!(s.document.get_object(a).unwrap().fill, Paint::Solid { .. }));
        s.run(&Command::with_params("selection.set", json!({ "ids": [a.0], "op": "Replace" }))).unwrap();
        input.drag = input.tool_drag(ToolKind::Crop, &scene, &s, [20.0, 20.0]);
        assert!(input.finish_drag(&mut s, [40.0, 50.0], mods), "{:?}", input.last_error);
        assert_eq!(s.document.get_object(a).unwrap().vector_crop.as_ref().map(|c| c.rect), Some([10.0, 10.0, 20.0, 30.0]));
        let clear = clear_crop_command(&s, a).unwrap();
        s.run(&clear).unwrap();
        assert!(s.document.get_object(a).unwrap().vector_crop.is_none());
        s.run(&Command::new("edit.undo")).unwrap();
        s.run(&Command::new("edit.undo")).unwrap();
        assert!(s.document.get_object(a).unwrap().vector_crop.is_none());
        assert!(clear_crop_command(&s, a).is_none());
    }
}
