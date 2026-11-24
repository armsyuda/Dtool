use dtool_core::paint::Colour;
use dtool_strings::text;
use egui::ecolor::{Hsva, HsvaGamma};
use egui::{Color32, Mesh, Pos2, Rect, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, Debug)]
enum Field {
    Saturation,
    Hue,
}

#[derive(Clone, Debug, Default)]
pub struct Picker {
    hue: f32,
    active: Option<Field>,
}

impl Picker {
    pub fn cancel(&mut self) {
        self.active = None;
    }

    pub fn show_wheel(&mut self, ui: &mut Ui, colour: Colour, maximum: f32) -> Option<Colour> {
        let mut hsv = HsvaGamma::from(super::picker_hsva(colour));
        if hsv.s == 0.0 || hsv.v == 0.0 { hsv.h = self.hue; }
        let width = picker_width(ui);
        let diameter = maximum.min(width);
        let (row, _) = ui.allocate_exact_size(Vec2::new(width, diameter), Sense::hover());
        let rect = Rect::from_center_size(row.center(), Vec2::splat(diameter));
        let response = ui.interact(rect, ui.make_persistent_id("colour.wheel"), Sense::click_and_drag());
        let centre = rect.center();
        let radius = diameter * 0.5 - 2.0;
        if radius <= 4.0 { return None; }
        let inner = radius * 0.78;
        let direction = |h: f32| Vec2::angled(h * std::f32::consts::TAU);
        let triangle = [0.0, 1.0 / 3.0, 2.0 / 3.0].map(|angle| centre + direction(angle) * (inner - 3.0));
        let (pressed, released) = crate::input::primary_button_positions(ui);
        if let Some(point) = pressed.filter(|point| picker_hit(ui, &response, *point))
        {
            let distance = point.distance(centre);
            self.active = if (inner..=radius).contains(&distance) {
                Some(Field::Hue)
            } else if triangle_weights(triangle, point).iter().all(|weight| *weight >= 0.0) {
                Some(Field::Saturation)
            } else {
                None
            };
        }
        let pointer = released.or_else(|| ui.input(|input| input.pointer.latest_pos()));
        if let (Some(active), Some(point)) = (self.active, pointer) {
            match active {
                Field::Hue => {
                    let offset = point - centre;
                    hsv.h = (offset.y.atan2(offset.x) / std::f32::consts::TAU).rem_euclid(1.0);
                }
                Field::Saturation => {
                    let point = constrain_to_triangle(triangle, point);
                    let [hue, white, _] = triangle_weights(triangle, point);
                    hsv.v = (hue + white).clamp(0.0, 1.0);
                    if hsv.v > 1e-6 { hsv.s = (hue / hsv.v).clamp(0.0, 1.0); }
                }
            }
        }
        self.hue = hsv.h;
        let mut mesh = Mesh::default();
        const SEGMENTS: u32 = 60;
        for segment in 0..=SEGMENTS {
            let hue = segment as f32 / SEGMENTS as f32;
            let offset = direction(hue);
            let color = hue_colour(hue);
            mesh.colored_vertex(centre + offset * radius, color);
            mesh.colored_vertex(centre + offset * inner, color);
            if segment > 0 {
                let index = segment * 2;
                mesh.add_triangle(index - 2, index, index - 1);
                mesh.add_triangle(index - 1, index, index + 1);
            }
        }
        let index = mesh.vertices.len() as u32;
        for (point, color) in triangle.into_iter().zip([hue_colour(hsv.h), Color32::WHITE, Color32::BLACK]) {
            mesh.colored_vertex(point, color);
        }
        mesh.add_triangle(index, index + 1, index + 2);
        ui.painter().add(mesh);
        let [hue, white, black] = triangle.map(|p| p.to_vec2());
        let value_pos = (hue * hsv.s * hsv.v + white * (1.0 - hsv.s) * hsv.v + black * (1.0 - hsv.v)).to_pos2();
        for marker in [centre + direction(hsv.h) * ((radius + inner) * 0.5), value_pos] {
            ui.painter().circle_stroke(marker, 4.0, Stroke::new(3.0, Color32::BLACK));
            ui.painter().circle_stroke(marker, 4.0, Stroke::new(1.4, Color32::WHITE));
        }
        self.finish(ui, hsv, colour, released.is_some())
    }

    pub fn show(&mut self, ui: &mut Ui, colour: Colour, field_height: f32) -> Option<Colour> {
        let mut hsv = HsvaGamma::from(super::picker_hsva(colour));
        if hsv.s == 0.0 || hsv.v == 0.0 {
            hsv.h = self.hue;
        }
        let width = picker_width(ui);
        let (strip, hue) = ui.allocate_exact_size(Vec2::new(width, 12.0), Sense::click_and_drag());
        let (field, saturation) = ui.allocate_exact_size(Vec2::new(width, field_height), Sense::click_and_drag());
        let saturation = saturation.on_hover_text(text("colour.panel.saturation_value"));
        let hue = hue.on_hover_text(text("colour.panel.hue"));
        let (pressed, released) = crate::input::primary_button_positions(ui);
        if let Some(point) = pressed {
            for (control, response) in [(Field::Saturation, &saturation), (Field::Hue, &hue)] {
                if picker_hit(ui, response, point) {
                    self.active = Some(control);
                }
            }
        }
        let point = released.or_else(|| ui.input(|input| input.pointer.latest_pos()));
        if let (Some(active), Some(point)) = (self.active, point) {
            match active {
                Field::Saturation => {
                    hsv.s = ((point.x - field.left()) / field.width()).clamp(0.0, 1.0);
                    hsv.v = 1.0 - ((point.y - field.top()) / field.height()).clamp(0.0, 1.0);
                }
                Field::Hue => hsv.h = ((point.x - strip.left()) / strip.width()).clamp(0.0, 1.0),
            }
        }
        self.hue = hsv.h;
        paint_field(ui, field, hsv);
        paint_hue(ui, strip, hsv.h);
        self.finish(ui, hsv, colour, released.is_some())
    }

    fn finish(&mut self, ui: &Ui, hsv: HsvaGamma, colour: Colour, released: bool) -> Option<Colour> {
        let committed = released && self.active.is_some();
        if !ui.input(|input| input.pointer.primary_down()) {
            self.cancel();
        }
        let [r, g, b, _] = Hsva::from(hsv).to_srgba_unmultiplied();
        let result = Colour::new(r, g, b, colour.a);
        (committed && result != colour).then_some(result)
    }
}

fn picker_width(ui: &Ui) -> f32 {
    ui.available_width().min(ui.clip_rect().right() - ui.cursor().min.x).max(1.0)
}

fn picker_hit(ui: &Ui, response: &egui::Response, press: Pos2) -> bool {
    // Use the original press position even if the pointer is released outside the field in the same frame.
    ui.is_enabled() && response.interact_rect.contains(press)
        && ui.ctx().layer_id_at(press) == Some(response.layer_id)
}

fn triangle_weights([a, b, c]: [Pos2; 3], point: Pos2) -> [f32; 3] {
    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
    let denominator = cross(b - a, c - a);
    let second = cross(point - a, c - a) / denominator;
    let third = cross(b - a, point - a) / denominator;
    [1.0 - second - third, second, third]
}

fn constrain_to_triangle(triangle: [Pos2; 3], point: Pos2) -> Pos2 {
    if triangle_weights(triangle, point).iter().all(|weight| *weight >= 0.0) {
        return point;
    }
    let [a, b, c] = triangle;
    [(a, b), (b, c), (c, a)].into_iter().map(|(start, end)| {
        let edge = end - start;
        start + edge * ((point - start).dot(edge) / edge.length_sq()).clamp(0.0, 1.0)
    }).min_by(|a, b| a.distance_sq(point).total_cmp(&b.distance_sq(point))).unwrap_or(point)
}

fn hue_colour(hue: f32) -> Color32 {
    HsvaGamma {
        h: hue,
        s: 1.0,
        v: 1.0,
        a: 1.0,
    }
    .into()
}

pub(super) fn gradient(mesh: &mut Mesh, rect: Rect, corners: [Color32; 4]) {
    let start = mesh.vertices.len() as u32;
    for (pos, colour) in [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
    .into_iter()
    .zip(corners)
    {
        mesh.colored_vertex(pos, colour);
    }
    mesh.add_triangle(start, start + 1, start + 2);
    mesh.add_triangle(start, start + 2, start + 3);
}

fn paint_field(ui: &Ui, rect: Rect, hsv: HsvaGamma) {
    let mut mesh = Mesh::default();
    let hue = hue_colour(hsv.h);
    gradient(&mut mesh, rect, [Color32::WHITE, hue, hue, Color32::WHITE]);
    gradient(
        &mut mesh,
        rect,
        [
            Color32::TRANSPARENT,
            Color32::TRANSPARENT,
            Color32::BLACK,
            Color32::BLACK,
        ],
    );
    let painter = ui.painter();
    painter.add(mesh);
    let pos = rect.lerp_inside(Vec2::new(hsv.s, 1.0 - hsv.v));
    painter.circle_stroke(pos, 4.0, Stroke::new(3.0, Color32::BLACK));
    painter.circle_stroke(pos, 4.0, Stroke::new(1.4, Color32::WHITE));
}

fn paint_hue(ui: &Ui, rect: Rect, hue: f32) {
    let mut mesh = Mesh::default();
    for segment in 0..6 {
        let left = segment as f32 / 6.0;
        let right = (segment + 1) as f32 / 6.0;
        let start = hue_colour(left);
        let end = hue_colour(right);
        let part = Rect::from_min_max(
            rect.lerp_inside(Vec2::new(left, 0.0)),
            rect.lerp_inside(Vec2::new(right, 1.0)),
        );
        gradient(&mut mesh, part, [start, end, end, start]);
    }
    let painter = ui.painter();
    painter.add(mesh);
    let inset = rect.width().min(10.0) * 0.5;
    let marker = egui::pos2(egui::lerp(rect.shrink2(Vec2::new(inset, 0.0)).x_range(), hue), rect.center().y);
    painter.circle_stroke(marker, 4.0, Stroke::new(3.0, Color32::BLACK));
    painter.circle_stroke(marker, 4.0, Stroke::new(1.4, Color32::WHITE));
}
