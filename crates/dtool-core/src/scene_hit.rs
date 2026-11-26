use crate::object::{ObjectId, ObjectKind};
use crate::scene::{contours_bounds, inside, Prim, Scene};
use crate::Document;

pub const HIT_TOLERANCE_PX: f32 = 4.0;

pub fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx * dx + dy * dy;
    let t = if len <= f32::EPSILON { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len).clamp(0.0, 1.0) };
    let (x, y) = (a[0] + dx * t, a[1] + dy * t);
    ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt()
}

pub fn near_polyline(points: &[[f32; 2]], closed: bool, p: [f32; 2], reach: f32) -> bool {
    let n = points.len();
    if n == 1 {
        return segment_distance(p, points[0], points[0]) <= reach;
    }
    let count = if closed { n } else { n.saturating_sub(1) };
    (0..count).any(|i| segment_distance(p, points[i], points[(i + 1) % n]) <= reach)
}

pub(crate) fn in_rect(rect: [f32; 4], p: [f32; 2]) -> bool {
    p[0] >= rect[0] && p[0] <= rect[0] + rect[2] && p[1] >= rect[1] && p[1] <= rect[1] + rect[3]
}

pub fn prim_object(prim: &Prim) -> Option<ObjectId> {
    match prim {
        Prim::Fill { object, .. } | Prim::Appearance { object, .. } | Prim::Stroke { object, .. } | Prim::Image { object, .. } | Prim::Text { object, .. } | Prim::Label { object, .. } => Some(*object),
        _ => None,
    }
}

pub fn prim_hit(scene: &Scene, prim: &Prim, p: [f32; 2], tolerance: f32) -> bool {
    match prim {
        Prim::Fill { contours, .. } => inside(contours, p[0], p[1]) || contours.iter().any(|c| near_polyline(c, true, p, tolerance)),
        Prim::Appearance { inks, .. } => inks.iter().any(|ink| {
            inside(&ink.contours, p[0], p[1]) || ink.contours.iter().any(|c| near_polyline(c, true, p, tolerance))
        }),
        Prim::Stroke { points, closed, width, .. } => near_polyline(points, *closed, p, (width * 0.5).max(tolerance)),
        Prim::Image { quad, .. } => inside(&[quad.to_vec()], p[0], p[1]),
        Prim::Text { object, .. } => scene.quad(*object).is_some_and(|q| inside(&[q.to_vec()], p[0], p[1])),
        Prim::Label { at, size, .. } => segment_distance(p, *at, *at) <= size.max(tolerance),
        _ => false,
    }
}

pub fn frame_image_hit(doc: &Document, id: ObjectId, quad: &[[f32; 2]; 4], p: [f32; 2]) -> Option<bool> {
    let geometry = crate::commands::picture_frame::frame_geometry(doc, doc.get_object(id)?)?;
    let polygon: Vec<[f32; 2]> = geometry.image_polygon(quad).into_iter().map(|(point, _)| point).collect();
    Some(polygon.len() >= 3 && inside(&[polygon], p[0], p[1]))
}

pub fn pick_target(doc: &Document, id: ObjectId) -> Option<ObjectId> {
    pick_target_in(doc, id, None)
}

pub fn pick_target_in(doc: &Document, id: ObjectId, scope: Option<ObjectId>) -> Option<ObjectId> {
    let object = doc.get_object(id)?;
    if !canvas_candidate(doc, object) {
        return None;
    }
    let mut target = id;
    let mut parent_id = object.parent_id;
    for _ in 0..doc.objects.len() {
        let Some(parent) = parent_id.filter(|id| Some(*id) != scope).and_then(|id| doc.get_object(id)) else { break };
        if !parent.is_group() || crate::artboard::is_artboard(parent) {
            break;
        }
        if !parent.is_layer_folder() {
            target = parent.id;
        }
        parent_id = parent.parent_id;
    }
    Some(target)
}

pub(crate) fn canvas_candidate(doc: &Document, object: &crate::object::Object) -> bool {
    if object.locked || !object.pickable || object.is_layer_folder() || crate::artboard::is_artboard(object) {
        return false;
    }
    let mut parent_id = object.parent_id;
    for _ in 0..doc.objects.len() {
        let Some(parent) = parent_id.and_then(|id| doc.get_object(id)) else { return true };
        if parent.is_layer_folder() && (parent.locked || !parent.pickable) {
            return false;
        }
        parent_id = parent.parent_id;
    }
    parent_id.is_none()
}

/// Returns the selection parent after skipping organizational folders.
pub fn pick_parent(doc: &Document, id: ObjectId) -> Option<ObjectId> {
    let mut parent_id = doc.get_object(id)?.parent_id;
    for _ in 0..doc.objects.len() {
        let parent = doc.get_object(parent_id?)?;
        if !parent.is_layer_folder() {
            return Some(parent.id);
        }
        parent_id = parent.parent_id;
    }
    None
}

pub fn hit_test(scene: &Scene, doc: &Document, p: [f32; 2], tolerance: f32) -> Option<ObjectId> {
    hit_test_in(scene, doc, p, tolerance, None)
}

pub fn hit_test_in(scene: &Scene, doc: &Document, p: [f32; 2], tolerance: f32, scope: Option<ObjectId>) -> Option<ObjectId> {
    hit_test_scoped(scene, doc, p, tolerance, scope, true)
}

pub fn hit_test_in_view(
    scene: &Scene, session: &crate::Session, p: [f32; 2], tolerance: f32, scope: Option<ObjectId>,
) -> Option<ObjectId> {
    use crate::ViewMode;
    let clip_artboards = !matches!(session.view.canvas_flags.view_mode, ViewMode::Outline | ViewMode::Wireframe);
    hit_test_scoped(scene, &session.document, p, tolerance, scope, clip_artboards)
}

fn hit_test_scoped(
    scene: &Scene, doc: &Document, p: [f32; 2], tolerance: f32, scope: Option<ObjectId>, clip_artboards: bool,
) -> Option<ObjectId> {
    let outside_artboard = |object| clip_artboards && crate::artboard::owner(doc, object)
        .is_some_and(|board| !crate::artboard::contains(board, p));
    let mut clips = Vec::with_capacity(scene.prims.len());
    let mut stack = Vec::new();
    for prim in &scene.prims {
        if let Prim::Clip { rect, .. } = prim {
            if let Some(rect) = rect {
                stack.push(stack.last().copied().unwrap_or(false) || !in_rect(*rect, p));
            } else {
                stack.pop();
            }
        }
        clips.push(stack.last().copied().unwrap_or(false));
    }
    for (prim, clipped) in scene.prims.iter().zip(clips).rev() {
        let Some(object) = prim_object(prim) else { continue };
        if clipped || outside_artboard(object) {
            continue;
        }
        let hit = match prim {
            Prim::Text { .. } if crate::scene::has_live_text_warp(doc, object) => false,
            Prim::Image { object, quad, .. } => {
                let brush = doc.get_object(*object).and_then(|source| crate::path_brush::Ink::new(doc, source));
                if let Some(brush) = brush {
                    brush.hit(p, tolerance)
                } else {
                    frame_image_hit(doc, *object, quad, p).unwrap_or_else(|| prim_hit(scene, prim, p, tolerance))
                }
            }
            _ => prim_hit(scene, prim, p, tolerance),
        };
        if hit && doc.get_object(object).is_some_and(|source| crate::scene::vector_crop_contains(source, p)) {
            if let Some(target) = pick_target_in(doc, object, scope) {
                return Some(target);
            }
        }
    }
    scene.quads.iter().rev().find_map(|(id, quad)| {
        let object = doc.get_object(*id)?;
        if !crate::scene::vector_crop_contains(object, p) {
            return None;
        }
        if matches!(object.kind, ObjectKind::PictureFrame) {
            use crate::commands::picture_frame::{contains_point, frame_image_size};
            let frame = doc.picture_frames.iter().find(|frame| frame.object == *id)?;
            if frame_image_size(doc, object, frame).is_none() && contains_point(doc, object, p) {
                return pick_target_in(doc, *id, scope);
            }
            return None;
        }
        let text = matches!(object.kind, ObjectKind::FrameText { .. } | ObjectKind::ArtText { .. });
        if !text || outside_artboard(*id) {
            return None;
        }
        let [x, y] = crate::commands::warp::unwarp_point(doc, *id, p);
        inside(&[quad.to_vec()], x, y).then(|| pick_target_in(doc, *id, scope)).flatten()
    })
}

/// Canvas handles follow symbol contents and the visible geometry of vector masks.
pub fn selection_quad(scene: &Scene, doc: &Document, id: ObjectId) -> Option<crate::scene::Quad> {
    doc.get_object(id).and_then(|root| {
        let matrix = crate::geometry::object_matrix(root);
        let bounds = if scene.quad(id).is_some() && matches!(root.kind, ObjectKind::ArtText { .. }) {
            let inverse = crate::geometry::invert_matrix(&matrix)?;
            let mut contours = crate::text_lines::text_outlines(doc, id)?;
            for point in contours.iter_mut().flatten() {
                *point = crate::geometry::apply_matrix(&inverse, *point);
            }
            let [l, t, r, b] = contours_bounds(&contours)?;
            (r > l && b > t).then_some([l, t, r - l, b - t])
        } else if matches!(root.kind, ObjectKind::Shape | ObjectKind::Curve(_))
            && doc.objects().iter().any(|object| object.parent_id == Some(id) && object.clip_role == crate::object::ClipRole::Mask)
        {
            visible_masked_bounds(scene, id, &matrix)
        } else {
            group_content_bounds(doc, root, Some(scene))
        }?;
        Some(crate::geometry::rect_quad(bounds, &matrix))
    })
        .or_else(|| scene.quad(id))
}

fn visible_masked_bounds(scene: &Scene, id: ObjectId, matrix: &[f32; 6]) -> Option<[f32; 4]> {
    let inverse = crate::geometry::invert_matrix(matrix)?;
    let mut points = Vec::new();
    for prim in &scene.prims {
        match prim {
            Prim::Fill { object, contours, .. } if *object == id => {
                points.extend(contours.iter().flatten().map(|&point| crate::geometry::apply_matrix(&inverse, point)));
            }
            Prim::Appearance { object, inks, .. } if *object == id => {
                points.extend(inks.iter().flat_map(|ink| ink.contours.iter().flatten())
                    .map(|&point| crate::geometry::apply_matrix(&inverse, point)));
            }
            _ => {}
        }
    }
    let [left, top, right, bottom] = contours_bounds(&[points])?;
    (right > left && bottom > top).then_some([left, top, right - left, bottom - top])
}

pub(crate) fn group_content_bounds(doc: &Document, root: &crate::Object, scene: Option<&Scene>) -> Option<[f32; 4]> {
    if !root.is_group() {
        return None;
    }
    let symbol = doc.symbols.instances.iter().any(|entry| entry.root == root.id);
    let applied_master = root.is_layer_folder()
        && doc.master_pages.inherited_content().any(|binding| binding.group == Some(root.id));
    let uses_content_bounds = symbol || applied_master;
    if !uses_content_bounds && root.is_layer_folder() {
        return None;
    }
    let subtree = doc.subtree_ids(root.id);
    if !uses_content_bounds && doc.layer_masks.iter().any(|mask| subtree.contains(&mask.object)) {
        return None;
    }
    let mut members = std::collections::HashSet::new();
    let mut has_hidden_member = !crate::scene::is_drawn(doc, root.id);
    for object in doc.objects.iter().filter(|object| subtree.contains(&object.id)) {
        // Constraints and masks can use the stored frame independently of the children.
        if !uses_content_bounds && (object.vector_crop.is_some() || !object.clip_role.is_none()
            || (object.id != root.id && object.constraint.is_some()))
        {
            return None;
        }
        has_hidden_member |= !object.pickable;
        if !object.is_group() {
            members.insert(object.id);
        }
    }
    let matrix = crate::geometry::object_matrix(root);
    let inverse = crate::geometry::invert_matrix(&matrix)?;
    let local = |quad: crate::scene::Quad| quad.map(|point| crate::geometry::apply_matrix(&inverse, point));
    let points = match scene {
        Some(scene) => scene.quads.iter().filter(|(id, _)| members.contains(id))
            .flat_map(|(_, quad)| local(*quad)).collect(),
        None => doc.objects.iter().filter(|object| members.contains(&object.id) && (!has_hidden_member || crate::scene::is_drawn(doc, object.id)))
            .flat_map(|object| local(crate::geometry::object_world_quad(object))).collect(),
    };
    contours_bounds(&[points]).map(|[x0, y0, x1, y1]| [x0, y0, x1 - x0, y1 - y0])
}

pub fn selection_box(scene: &Scene, doc: &Document, ids: &[ObjectId]) -> Option<[f32; 4]> {
    let mut points: Vec<[f32; 2]> = Vec::new();
    for id in ids {
        if doc.get_object(*id).is_some_and(|object| object.is_layer_folder())
            && !doc.objects.iter().any(|object| object.parent_id == Some(*id))
        {
            continue;
        }
        match selection_quad(scene, doc, *id) {
            Some(q) => points.extend_from_slice(&q),
            None => {
                if let Some(o) = doc.get_object(*id) {
                    points.extend_from_slice(&[o.pos, [o.pos[0] + o.size[0], o.pos[1] + o.size[1]]]);
                }
            }
        }
    }
    let b = contours_bounds(&[points])?;
    Some([b[0], b[1], b[2] - b[0], b[3] - b[1]])
}

/// The basis of a single layer's handles. Multiple layers use document axes.
#[derive(Clone, Copy, Debug, Default)]
pub struct SelectionFrame {
    pub bounds: [f32; 4],
    pub axes: [f32; 2],
    pub centre: [f32; 2],
}

impl SelectionFrame {
    pub fn to_world(self, point: [f32; 2]) -> [f32; 2] {
        self.map(point, false)
    }

    pub fn to_local(self, point: [f32; 2]) -> [f32; 2] {
        self.map(point, true)
    }

    fn map(self, point: [f32; 2], inverse: bool) -> [f32; 2] {
        if self.axes == [0.0, 0.0] {
            return point;
        }
        let [x, y] = self.centre;
        let [rotation, shear] = self.axes;
        let [a, b, c, d] = crate::geometry::object_linear(rotation, shear);
        let matrix = [a, b, c, d, 0.0, 0.0];
        let matrix = if inverse { crate::geometry::invert_matrix(&matrix).unwrap_or(crate::geometry::IDENTITY) } else { matrix };
        let [u, v] = crate::geometry::apply_matrix(&matrix, [point[0] - x, point[1] - y]);
        [u + x, v + y]
    }

    pub fn grip(self, [x, y]: [f32; 2]) -> [f32; 2] {
        let [left, top, width, height] = self.bounds;
        self.to_world([left + width * x, top + height * y])
    }
}

pub fn selection_frame(scene: &Scene, doc: &Document, ids: &[ObjectId]) -> Option<SelectionFrame> {
    if let [id] = ids {
        if let Some(object) = doc.get_object(*id) {
            let axes = [object.rotation_deg, object.shear_deg];
            if axes != [0.0, 0.0] && axes.iter().all(|angle| angle.is_finite()) {
                let mut frame = SelectionFrame { axes, centre: crate::geometry::box_centre(object), ..SelectionFrame::default() };
                let quad = selection_quad(scene, doc, *id).unwrap_or_else(|| crate::scene::object_quad(object, &crate::geometry::object_matrix(object)));
                let points = quad.map(|point| frame.to_local(point)).to_vec();
                let [x0, y0, x1, y1] = contours_bounds(&[points])?;
                frame.bounds = [x0, y0, x1 - x0, y1 - y0];
                return Some(frame);
            }
        }
    }
    Some(SelectionFrame { bounds: selection_box(scene, doc, ids)?, ..SelectionFrame::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::Object;
    use crate::paint::{Colour, Paint};
    use crate::scene::estimate_scene;
    use crate::CanvasFlags;

    #[test]
    fn topmost_fill_wins_and_empty_misses() {
        let mut doc = Document::new([200.0, 200.0], crate::document::Units::Px, 72.0, 1);
        let red = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
        let a = doc.create_object(Object { pos: [0.0, 0.0], size: [100.0, 100.0], fill: red.clone(), ..Object::default() });
        let b = doc.create_object(Object { pos: [50.0, 50.0], size: [100.0, 100.0], fill: red, ..Object::default() });
        let scene = estimate_scene(&doc, &CanvasFlags::default());
        assert_eq!(hit_test(&scene, &doc, [75.0, 75.0], 1.0), Some(b));
        assert_eq!(hit_test(&scene, &doc, [10.0, 10.0], 1.0), Some(a));
        assert_eq!(hit_test(&scene, &doc, [180.0, 10.0], 1.0), None);
        assert_eq!(selection_box(&scene, &doc, &[a, b]), Some([0.0, 0.0, 150.0, 150.0]));
    }
}
