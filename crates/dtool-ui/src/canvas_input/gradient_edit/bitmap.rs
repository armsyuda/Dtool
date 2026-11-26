use super::{Role, screen, to_local, to_world};
use dtool_core::paint::{BitmapPaint, Paint};
use dtool_core::{Command, Object, ObjectId, Session};
use egui::{Pos2, Rect, Stroke, Ui};

type Point = [f64; 2];

struct Bitmap<'a> {
    object: &'a Object,
    role: Role,
    paint: &'a BitmapPaint,
    size: Point,
}

impl<'a> Bitmap<'a> {
    fn selected(session: &'a Session) -> Option<Self> {
        let (object, role) = Role::selected_bitmap_target(session, &session.active_tool)?;
        let Paint::Bitmap(paint) = role.paint(object)? else { return None };
        let resource = session.document.image_resource_by_ref(&paint.resource_id)?;
        let size = resource.pixel_size.map(f64::from);
        (size[0] > 0.0 && size[1] > 0.0).then_some(Self { object, role, paint, size })
    }

    fn points(&self, matrix: [f64; 6]) -> [Point; 3] {
        let [a, b, c, d, x, y] = matrix;
        [[x, y], [x + a * self.size[0], y + b * self.size[0]], [x + c * self.size[1], y + d * self.size[1]]]
    }

    fn hit(&self, session: &Session, canvas: Rect, pointer: Pos2) -> Option<(usize, Point)> {
        self.points(self.paint.matrix).into_iter().enumerate().filter_map(|(index, point)| {
            let distance = pointer.distance(screen(canvas, &session.view, to_world(self.object, point)));
            (distance <= 8.0).then_some((index, point, distance))
        }).min_by(|a, b| a.2.total_cmp(&b.2)).map(|(index, point, _)| (index, point))
    }
}

#[derive(Debug, Clone)]
pub(super) struct Edit {
    id: ObjectId,
    role: Role,
    resource: String,
    handle: usize,
    grab: Point,
    original: [f64; 6],
    preview: [f64; 6],
}

impl Edit {
    pub(super) fn hovering(session: &Session, canvas: Rect, at: Pos2) -> bool {
        Bitmap::selected(session).is_some_and(|bitmap| bitmap.hit(session, canvas, at).is_some())
    }

    pub(super) fn begin(session: &Session, canvas: Rect, at: Pos2, doc: [f32; 2]) -> Option<Self> {
        let bitmap = Bitmap::selected(session)?;
        let (handle, point) = bitmap.hit(session, canvas, at)?;
        let local = to_local(bitmap.object, doc)?;
        Some(Self {
            id: bitmap.object.id,
            role: bitmap.role,
            resource: bitmap.paint.resource_id.clone(),
            handle,
            grab: [point[0] - local[0], point[1] - local[1]],
            original: bitmap.paint.matrix,
            preview: bitmap.paint.matrix,
        })
    }

    fn matches(&self, bitmap: &Bitmap<'_>) -> bool {
        self.id == bitmap.object.id && self.role == bitmap.role && self.resource == bitmap.paint.resource_id
            && self.original == bitmap.paint.matrix
    }

    pub(super) fn motion(&mut self, session: &Session, doc: [f32; 2], release: bool) -> Option<Command> {
        let bitmap = Bitmap::selected(session).filter(|bitmap| self.matches(bitmap))?;
        let local = to_local(bitmap.object, doc)?;
        let target: Point = std::array::from_fn(|i| local[i] + self.grab[i]);
        let [a, b, c, d, x, y] = self.original;
        if self.handle == 0 {
            self.preview = [a, b, c, d, target[0], target[1]];
        } else {
            let points = bitmap.points(self.original);
            let [px, py] = points[self.handle];
            let [u, v] = [px - x, py - y];
            let [dx, dy] = [target[0] - x, target[1] - y];
            let length_squared = u * u + v * v;
            if length_squared > 1e-10 && dx.hypot(dy) >= 0.1 {
                let scale_cos = (dx * u + dy * v) / length_squared;
                let scale_sin = (dy * u - dx * v) / length_squared;
                let next = [a * scale_cos - b * scale_sin, a * scale_sin + b * scale_cos,
                    c * scale_cos - d * scale_sin, c * scale_sin + d * scale_cos, x, y];
                if next.iter().all(|value| value.is_finite()) && (next[0] * next[3] - next[1] * next[2]).abs() > 1e-12 {
                    self.preview = next;
                }
            }
        }
        let changed = self.preview.iter().zip(self.original).any(|(value, initial)| (value - initial).abs() > 1e-8);
        (release && changed).then(|| bitmap.role.paint_command(bitmap.object, Paint::Bitmap(BitmapPaint {
            matrix: self.preview, ..bitmap.paint.clone()
        })))
    }

    pub(super) fn paint(edit: Option<&Self>, ui: &Ui, canvas: Rect, session: &Session, stroke: Stroke) -> bool {
        let Some(bitmap) = Bitmap::selected(session) else { return false };
        let matrix = edit.filter(|edit| edit.matches(&bitmap)).map_or(bitmap.paint.matrix, |edit| edit.preview);
        let [origin, x, y] = bitmap.points(matrix).map(|point| screen(canvas, &session.view, to_world(bitmap.object, point)));
        let painter = ui.painter_at(canvas);
        painter.line_segment([origin, x], stroke);
        painter.line_segment([origin, y], stroke);
        for point in [origin, x, y] {
            painter.circle(point, 4.5, ui.visuals().panel_fill, stroke);
        }
        true
    }
}
