use dtool_core::{Command, Session};
use dtool_core::commands::inpaint;
use egui::{Color32, Pos2, Rect, Response, Stroke, Ui, Vec2};
use serde_json::json;

#[derive(Debug, Clone, Default)]
pub(super) struct Gesture {
    points: Vec<[f32; 2]>,
    pointer: Option<Pos2>,
}

impl Gesture {
    pub fn active(&self) -> bool { !self.points.is_empty() }

    pub fn cancel(&mut self) {
        self.points.clear();
        self.pointer = None;
    }

    pub fn input(&mut self, ui: &Ui, response: &Response, session: &Session, to_doc: impl Fn(Pos2) -> [f32; 2]) -> Option<Command> {
        let can_paint = inpaint::can_paint(session);
        self.pointer = response.hover_pos().filter(|_| can_paint);
        if response.hovered() && !can_paint {
            ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
        }
        let (down, up) = crate::input::primary_button_positions(ui);
        if let Some(down) = down.filter(|pos| {
            can_paint && response.enabled() && response.interact_rect.contains(*pos)
                && ui.ctx().layer_id_at(*pos) == Some(response.layer_id)
                && !egui::Popup::is_any_open(ui.ctx())
        }) {
            self.points.clear();
            self.points.push(to_doc(down));
            response.request_focus();
        }
        if !self.active() { return None; }
        if let Some(pos) = up.or_else(|| ui.input(|input| input.pointer.latest_pos())) {
            let point = to_doc(pos);
            if self.points.last().copied() != Some(point) {
                if self.points.len() < 8192 { self.points.push(point); }
                else if let Some(last) = self.points.last_mut() { *last = point; }
            }
        }
        if up.is_some() {
            return Some(Command::with_params(inpaint::STROKE, json!({ "points": std::mem::take(&mut self.points) })));
        }
        if !ui.input(|input| input.pointer.primary_down()) { self.cancel(); }
        None
    }

    pub fn paint(&self, ui: &Ui, rect: Rect, session: &Session) {
        if session.active_tool != inpaint::TOOL { return; }
        let painter = ui.painter_at(rect);
        let screen = |point| super::screen(rect, &session.view, point);
        let ink = dtool_design::color("error");
        let mask_ink = Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 118);
        let cache_id = ui.id().with("inpaint.mask");
        if let Some((revision, mask, [x, y, w, h], corners)) = inpaint::mask_overlay(session) {
            let cached = ui.data(|data| data.get_temp::<(u64, egui::TextureHandle)>(cache_id));
            let texture = cached.filter(|(tag, _)| *tag == revision).map(|(_, texture)| texture)
                .unwrap_or_else(|| {
                    let limit = ui.input(|input| input.max_texture_side).clamp(1, 2048) as u32;
                    let step = w.max(h).div_ceil(limit).max(1);
                    let size = [w.div_ceil(step) as usize, h.div_ceil(step) as usize];
                    let mut pixels = Vec::with_capacity(size[0] * size[1]);
                    for row in (y..y+h).step_by(step as usize) {
                        for col in (x..x+w).step_by(step as usize) {
                            pixels.push(mask_ink.gamma_multiply(f32::from(mask.get(col, row)) / 255.0));
                        }
                    }
                    let texture = ui.ctx().load_texture("inpaint.mask", egui::ColorImage::new(size, pixels), egui::TextureOptions::LINEAR);
                    ui.data_mut(|data| data.insert_temp(cache_id, (revision, texture.clone())));
                    texture
                });
            let mut mesh = egui::Mesh::with_texture(texture.id());
            for (p, uv) in corners.into_iter().map(screen).zip([Pos2::ZERO, egui::pos2(1.0,0.0), egui::pos2(1.0,1.0), egui::pos2(0.0,1.0)]) {
                mesh.vertices.push(egui::epaint::Vertex { pos: p, uv, color: Color32::WHITE });
            }
            mesh.indices.extend([0, 1, 2, 0, 2, 3]);
            painter.add(egui::Shape::mesh(mesh));
        } else {
            ui.data_mut(|data| data.remove::<(u64, egui::TextureHandle)>(cache_id));
        }
        if let Some(diameter) = inpaint::brush_diameter(session) {
            let size = (diameter * session.view.zoom).max(2.0);
            if self.points.len() == 1 {
                painter.circle_filled(screen(self.points[0]), size * 0.5, mask_ink);
            } else if self.points.len() > 1 {
                painter.add(egui::Shape::line(self.points.iter().copied().map(screen).collect(), Stroke::new(size, mask_ink)));
            }
            if let Some(pos) = self.pointer {
                painter.circle_stroke(pos, size * 0.5, Stroke::new(1.8, Color32::BLACK));
                painter.circle_stroke(pos, size * 0.5, Stroke::new(1.0, Color32::WHITE));
            }
        }
        if let Some((progress, error)) = inpaint::feedback(session) {
            let message = progress.map(|pct| dtool_strings::text("inpaint.progress").replace("{pct}", &pct.to_string()))
                .or_else(|| error.map(|key| dtool_strings::text(key).to_string()));
            if let Some(message) = message {
                let galley = painter.layout_no_wrap(message, egui::FontId::proportional(13.0), ui.visuals().text_color());
                let position = rect.center_bottom() - Vec2::new(galley.size().x * 0.5, galley.size().y + 18.0);
                painter.rect_filled(Rect::from_min_size(position, galley.size()).expand(7.0), 4.0, dtool_design::color("panel"));
                painter.galley(position, galley, ui.visuals().text_color());
            }
        }
    }
}
