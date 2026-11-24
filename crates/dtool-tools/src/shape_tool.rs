use dtool_core::shape::{CornerRecipe, ShapeKind};
use dtool_core::{Command, Object};
use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix};

mod recipe_handles;

#[derive(Debug, Clone)]
pub struct ShapeTool { pub kind: ShapeKind }

impl ShapeTool {
    pub fn create(&self, pos:[f32;2], size:[f32;2]) -> Command {
        let mut command = Command::new("shape.add");
        let params = &mut command.params;
        params["kind"] = shape_kind_key(self.kind).into();
        params["pos"] = pos.to_vec().into();
        params["size"] = size.to_vec().into();
        if self.kind == ShapeKind::RoundedRectangle {
            params["corner"]["radii"] = vec![0.0_f64, 0.0, 0.0, 0.0].into();
            params["corner"]["absolute"] = false.into();
            params["corner"]["lock"] = true.into();
        }
        if self.kind == ShapeKind::Star {
            params["star"]["inner_radius"] = 50.0_f64.into();
            params["star"]["outer_radius"] = 100.0_f64.into();
            params["star"]["number_of_points"] = 5_i64.into();
            params["star"]["curve_left"] = 0.0_f64.into();
            params["star"]["curve_right"] = 0.0_f64.into();
            params["star"]["legacy"] = false.into();
        }
        if self.kind == ShapeKind::Triangle {
            params["triangle"]["points"] = dtool_core::commands::shape::DEFAULT_TRIANGLE_POINTS.to_vec().into();
        }
        if self.kind == ShapeKind::Arrow {
            params["arrow"]["head_width"] = 50.0_f64.into();
            params["arrow"]["head_length"] = 50.0_f64.into();
            params["arrow"]["shaft_width"] = 25.0_f64.into();
            params["arrow"]["shaft_length"] = 50.0_f64.into();
            params["arrow"]["tail_width"] = 25.0_f64.into();
        }
        if self.kind == ShapeKind::CalloutEllipse {
            params["callout_ellipse"]["radius_x"] = 50.0_f64.into();
            params["callout_ellipse"]["radius_y"] = 30.0_f64.into();
            params["callout_ellipse"]["pointer_angle"] = 45.0_f64.into();
            params["callout_ellipse"]["pointer_length"] = 20.0_f64.into();
            params["callout_ellipse"]["pointer_width"] = 10.0_f64.into();
        }
        if self.kind == ShapeKind::CalloutRectangle {
            params["callout_rectangle"]["corner_radius"] = 5.0_f64.into();
            params["callout_rectangle"]["absolute_radii"] = false.into();
            params["callout_rectangle"]["lock_radii"] = true.into();
            params["callout_rectangle"]["pointer_angle"] = 45.0_f64.into();
            params["callout_rectangle"]["pointer_length"] = 20.0_f64.into();
            params["callout_rectangle"]["pointer_width"] = 10.0_f64.into();
        }
        if self.kind == ShapeKind::Donut {
            params["donut"]["inner_radius"] = 50.0_f64.into();
            params["donut"]["outer_radius"] = 100.0_f64.into();
        }
        if self.kind == ShapeKind::Pie {
            params["pie"]["start_angle"] = 0.0_f64.into();
            params["pie"]["end_angle"] = 90.0_f64.into();
            params["pie"]["inner_radius"] = 0.0_f64.into();
            params["pie"]["outer_radius"] = 100.0_f64.into();
        }
        if self.kind == ShapeKind::Heart {
            params["heart"]["indent"] = 50.0_f64.into();
            params["heart"]["pointiness"] = 50.0_f64.into();
        }
        if self.kind == ShapeKind::Cog {
            params["cog"]["number_of_teeth"] = 12_i64.into();
            params["cog"]["tooth_depth"] = 10.0_f64.into();
            params["cog"]["tooth_width"] = 50.0_f64.into();
            params["cog"]["inner_radius"] = 40.0_f64.into();
            params["cog"]["outer_radius"] = 50.0_f64.into();
        }
        if self.kind == ShapeKind::Tear {
            params["tear"]["indent"] = 50.0_f64.into();
            params["tear"]["pointiness"] = 50.0_f64.into();
        }
        if self.kind == ShapeKind::Crescent {
            params["crescent"]["indent"] = 50.0_f64.into();
        }
        if self.kind == ShapeKind::Cloud {
            params["cloud"]["indent"] = 50.0_f64.into();
            params["cloud"]["pointiness"] = 50.0_f64.into();
        }
        if self.kind == ShapeKind::Segment {
            params["segment"]["indent"] = 50.0_f64.into();
            params["segment"]["pointiness"] = 50.0_f64.into();
        }
        if self.kind == ShapeKind::Cat {
            params["cat"]["indent"] = 50.0_f64.into();
            params["cat"]["pointiness"] = 50.0_f64.into();
        }
        if self.kind == ShapeKind::SquareStar {
            params["square_star"]["number_of_sides"] = 4_i64.into();
            params["square_star"]["cutout"] = 50.0_f64.into();
        }
        if self.kind == ShapeKind::DoubleStar {
            params["double_star"]["inner_radius"] = 50.0_f64.into();
            params["double_star"]["point_radius"] = 100.0_f64.into();
            params["double_star"]["number_of_points"] = 5_i64.into();
        }
        if self.kind == ShapeKind::Diamond {
            params["diamond"]["indent"] = 50.0_f64.into();
        }
        if self.kind == ShapeKind::Trapezoid {
            params["trapezoid"]["indent"] = 50.0_f64.into();
        }
        if self.kind == ShapeKind::Polygon {
            params["polygon"]["number_of_sides"] = 6_i64.into();
            params["polygon"]["curve"] = 0.0_f64.into();
            params["polygon"]["smooth_points"] = false.into();
        }
        if self.kind == ShapeKind::Spiral {
            params["spiral"]["style"] = "linear".into();
            params["spiral"]["turns"] = 3.0_f64.into();
            params["spiral"]["part_turn"] = 1.0_f64.into();
            params["spiral"]["segment_angle"] = 90.0_f64.into();
            params["spiral"]["divisions"] = 4_i64.into();
            params["spiral"]["decay_per_turn"] = 1.0_f64.into();
            params["spiral"]["decay_per_segment"] = 1.0_f64.into();
            params["spiral"]["inner_radius"] = 0.0_f64.into();
            params["spiral"]["choke"] = 0.0_f64.into();
            params["spiral"]["cap_choke"] = 0.0_f64.into();
            params["spiral"]["bias"] = 0.0_f64.into();
            params["spiral"]["cusped"] = false.into();
            params["spiral"]["flip"] = false.into();
        }
        command
    }

    pub fn create_with_modifiers(&self, start:[f32;2], end:[f32;2], shift: bool, ctrl: bool) -> Command {
        let mut pos = start;
        let mut size = [end[0] - start[0], end[1] - start[1]];

        if shift {
            let max_dim = size[0].abs().max(size[1].abs());
            size[0] = max_dim * size[0].signum();
            size[1] = max_dim * size[1].signum();
        }

        if ctrl {
            pos[0] = start[0] - size[0] / 2.0;
            pos[1] = start[1] - size[1] / 2.0;
        }

        self.create(pos, size)
    }

    pub fn set_size(&self,id:u64,size:[f32;2])->Command {
        let mut command = Command::new("shape.params.set");
        command.params["id"] = id.into();
        command.params["kind"] = shape_kind_key(self.kind).into();
        command.params["size"] = size.to_vec().into();
        command
    }

    pub fn convert_to_curves(&self, id: u64) -> Command {
        let mut command = Command::new("shape.convert_to_curves");
        command.params["id"] = id.into();
        command
    }
}

pub fn shape_kind_key(kind: ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Rectangle => "rectangle",
        ShapeKind::Triangle => "triangle",
        ShapeKind::Ellipse => "ellipse",
        ShapeKind::RoundedRectangle => "rounded_rectangle",
        ShapeKind::Star => "star",
        ShapeKind::Arrow => "arrow",
        ShapeKind::CalloutEllipse => "callout_ellipse",
        ShapeKind::CalloutRectangle => "callout_rectangle",
        ShapeKind::Donut => "donut",
        ShapeKind::Pie => "pie",
        ShapeKind::Heart => "heart",
        ShapeKind::Cog => "cog",
        ShapeKind::Tear => "tear",
        ShapeKind::Crescent => "crescent",
        ShapeKind::Cloud => "cloud",
        ShapeKind::Segment => "segment",
        ShapeKind::Cat => "cat",
        ShapeKind::SquareStar => "square_star",
        ShapeKind::DoubleStar => "double_star",
        ShapeKind::Diamond => "diamond",
        ShapeKind::Trapezoid => "trapezoid",
        ShapeKind::Polygon => "polygon",
        ShapeKind::Spiral => "spiral",
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeParamKind {
    Size,
    TriangleApex,
    Corner(u8),
    Recipe(&'static str),
}

pub fn is_p0(kind: ShapeKind) -> bool {
    matches!(kind, ShapeKind::Rectangle | ShapeKind::Triangle | ShapeKind::Ellipse | ShapeKind::RoundedRectangle)
}

fn corner_px(corner: &CornerRecipe, size: [f32; 2], index: usize) -> f32 {
    let short = size[0].abs().min(size[1].abs());
    let raw = corner.radii.get(index).copied().unwrap_or(0.0);
    let value = if corner.absolute { raw } else { short * raw.clamp(0.0, 50.0) / 100.0 };
    value.clamp(0.0, short * 0.5)
}

pub fn param_handles(object: &Object) -> Vec<(ShapeParamKind, [f32; 2])> {
    let Some(recipe) = object.shape.as_ref() else { return Vec::new() };
    if object.locked || dtool_core::commands::qr::payload(object).is_some() {
        return Vec::new();
    }
    if !is_p0(recipe.kind) {
        let [x, y] = object.pos;
        let [w, h] = object.size.map(f32::abs);
        if [w, h].into_iter().any(|v| !v.is_finite() || v <= 0.0) {
            return Vec::new();
        }
        let matrix = object_matrix(object);
        return recipe_handles::controls(recipe, [w, h]).unwrap_or_default().into_iter().map(|control| {
            let [u, v] = control.position();
            (ShapeParamKind::Recipe(control.field), apply_matrix(&matrix, [x + w * u, y + h * v]))
        }).collect();
    }
    let [x, y] = object.pos;
    let [w, h] = object.size;
    match recipe.kind {
        ShapeKind::Rectangle | ShapeKind::Ellipse => vec![(ShapeParamKind::Size, [x + w, y + h])],
        ShapeKind::Triangle => {
            let t = recipe.triangle.as_ref().map(|tri| tri.points[0].clamp(0.0, 1.0)).unwrap_or(0.5);
            vec![(ShapeParamKind::TriangleApex, [x + t * w, y])]
        }
        ShapeKind::RoundedRectangle => {
            let locked = recipe.corner.as_ref().is_none_or(|c| c.lock);
            let radii = [0, 1, 2, 3].map(|i| recipe.corner.as_ref().map(|c| corner_px(c, object.size, i)).unwrap_or(0.0));
            if locked {
                vec![(ShapeParamKind::Corner(0), [x + radii[0], y])]
            } else {
                vec![
                    (ShapeParamKind::Corner(0), [x + radii[0], y]),
                    (ShapeParamKind::Corner(1), [x + w - radii[1], y]),
                    (ShapeParamKind::Corner(2), [x + w - radii[2], y + h]),
                    (ShapeParamKind::Corner(3), [x + radii[3], y + h]),
                ]
            }
        }
        _ => Vec::new(),
    }
}

pub fn hit_param_handle(object: &Object, point: [f32; 2], radius: f32) -> Option<ShapeParamKind> {
    let mut best = None;
    let mut best_d = radius;
    for (kind, at) in param_handles(object) {
        let d = (at[0] - point[0]).hypot(at[1] - point[1]);
        if d <= best_d {
            best_d = d;
            best = Some(kind);
        }
    }
    best
}

fn radius_value(corner: &CornerRecipe, size: [f32; 2], px: f32) -> f32 {
    let short = size[0].abs().min(size[1].abs()).max(0.0);
    let px = px.clamp(0.0, short * 0.5);
    if corner.absolute {
        px
    } else if short <= 0.0 {
        0.0
    } else {
        (px / short * 100.0).clamp(0.0, 50.0)
    }
}

pub fn apply_param_drag(object: &Object, kind: ShapeParamKind, point: [f32; 2]) -> Option<Command> {
    if object.locked {
        return None;
    }
    if let ShapeParamKind::Recipe(field) = kind {
        let (key, value) = recipe_drag_value(object, field, point)?;
        let mut command = Command::new("shape.params.set");
        command.params["id"] = object.id.0.into();
        command.params["kind"] = key.into();
        command.params[key][field] = value.into();
        return Some(command);
    }
    let recipe = object.shape.as_ref()?;
    if !is_p0(recipe.kind) {
        return None;
    }
    let id = object.id.0;
    let [x, y] = object.pos;
    let [w, _h] = object.size;
    let mut command = Command::new("shape.params.set");
    command.params["id"] = id.into();
    command.params["kind"] = shape_kind_key(recipe.kind).into();
    match kind {
        ShapeParamKind::Size => {
            command.params["size"] = vec![(point[0] - x).max(0.001), (point[1] - y).max(0.001)].into();
        }
        ShapeParamKind::TriangleApex => {
            let mut points = recipe.triangle.as_ref().map(|t| t.points).unwrap_or(dtool_core::commands::shape::DEFAULT_TRIANGLE_POINTS);
            points[0] = if w.abs() <= 1e-6 { 0.0 } else { ((point[0] - x) / w).clamp(0.0, 1.0) };
            command.params["triangle"]["points"] = points.to_vec().into();
        }
        ShapeParamKind::Corner(index) => {
            let mut corner = recipe.corner.clone().unwrap_or(CornerRecipe { radii: [0.0; 4], absolute: false, lock: true });
            let px = match index {
                1 | 2 => x + w - point[0],
                _ => point[0] - x,
            };
            let value = radius_value(&corner, object.size, px);
            if corner.lock {
                corner.radii = [value; 4];
            } else if let Some(slot) = corner.radii.get_mut(index as usize) {
                *slot = value;
            }
            command.params["corner"]["radii"] = corner.radii.to_vec().into();
            command.params["corner"]["absolute"] = corner.absolute.into();
            command.params["corner"]["lock"] = corner.lock.into();
        }
        ShapeParamKind::Recipe(_) => unreachable!(),
    }
    Some(command)
}

fn recipe_drag_value(object: &Object, field: &str, point: [f32; 2]) -> Option<(&'static str, f32)> {
    let recipe = object.shape.as_ref()?;
    if object.locked || dtool_core::commands::qr::payload(object).is_some() {
        return None;
    }
    let [w, h] = object.size.map(f32::abs);
    if [w, h].into_iter().any(|v| !v.is_finite() || v <= 0.0) {
        return None;
    }
    let [x, y] = apply_matrix(&invert_matrix(&object_matrix(object))?, point);
    let local = [(x - object.pos[0]) / w, (y - object.pos[1]) / h];
    let control = recipe_handles::controls(recipe, [w, h])?.into_iter().find(|control| control.field == field)?;
    Some((shape_kind_key(recipe.kind), control.value_at(local)?))
}

pub fn preview_recipe_drag(object: &Object, kind: ShapeParamKind, point: [f32; 2]) -> Option<Object> {
    let ShapeParamKind::Recipe(field) = kind else { return None };
    let (key, value) = recipe_drag_value(object, field, point)?;
    let mut preview = object.clone();
    preview.shape = Some(dtool_core::commands::shape::preview_recipe_field(object.shape.as_ref()?, key, field, value)?);
    Some(preview)
}

pub fn selected_parametric_shape(session: &dtool_core::Session) -> Option<&Object> {
    let [id] = session.selection.ids.as_slice() else { return None };
    let object = session.document.get_object(*id)?;
    (object.shape.is_some() && !object.locked && dtool_core::commands::qr::payload(object).is_none()).then_some(object)
}
