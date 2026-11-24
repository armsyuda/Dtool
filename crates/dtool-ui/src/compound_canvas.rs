use dtool_core::boolean::{COMPOUND_EDIT, CompoundSpec, compound_operand_outline, preview_compound_move};
use dtool_core::geometry::{apply_matrix, invert_matrix, object_matrix};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::{Command, Object, ObjectId, ObjectKind, Session, doc_to_view, view_to_doc};
use egui::{Color32, Pos2, Rect, Response, Stroke, Ui};

#[derive(Clone, Copy, PartialEq, Eq)]
struct OperandTarget {
    compound: ObjectId,
    operand: ObjectId,
}

fn key(owner: ContextId) -> egui::Id {
    egui::Id::new(("compound.operand", owner))
}

fn chosen(ui: &Ui, owner: ContextId) -> Option<OperandTarget> {
    ui.data(|data| data.get_temp(key(owner)))
}

fn choose(ui: &Ui, owner: ContextId, target: Option<OperandTarget>) {
    ui.data_mut(|data| {
        if let Some(target) = target { data.insert_temp(key(owner), target); }
        else { data.remove::<OperandTarget>(key(owner)); }
    });
}

fn selected_compound(session: &Session) -> Option<(&Object, &CompoundSpec)> {
    let [id] = session.selection.ids.as_slice() else { return None };
    let object = session.document.get_object(*id)?;
    match &object.kind {
        ObjectKind::Compound(spec) if !object.locked => Some((object, spec)),
        _ => None,
    }
}

pub fn show_operands(ui: &mut Ui, workspace: &Workspace) {
    let Some((owner, (object, spec))) = workspace.active().zip(workspace.session().and_then(selected_compound)) else { return };
    let current = chosen(ui, owner);
    egui::CollapsingHeader::new(dtool_strings::text("compound.operand"))
        .id_salt((owner, object.id))
        .show(ui, |ui| {
            for source in &spec.operands {
                let target = OperandTarget { compound: object.id, operand: source.id };
                let selected = current == Some(target);
                let name = crate::panels::layers::display_name(source.id.0, &source.name);
                if ui.add_enabled(!source.locked, egui::Button::selectable(selected, name)).clicked() {
                    choose(ui, owner, (!selected).then_some(target));
                }
            }
        });
}

struct Drag {
    screen_start: Pos2,
    start: [f32; 2],
    original: [f32; 2],
    target: OperandTarget,
    owner: ContextId,
    generation: u64,
}

#[derive(Default)]
pub struct CanvasEditor {
    drag: Option<Drag>,
}

pub struct CanvasUpdate {
    pub active: bool,
    pub draft: Option<Object>,
    pub command: Option<Command>,
}

impl CanvasEditor {
    pub fn update(&mut self, ui: &Ui, response: &Response, session: &Session, owner: ContextId, input: bool) -> CanvasUpdate {
        let mut result = CanvasUpdate { active: false, draft: None, command: None };
        let Some(target) = chosen(ui, owner) else {
            self.drag = None;
            return result;
        };
        let Some((compound, spec)) = selected_compound(session).filter(|(object, _)| object.id == target.compound) else {
            choose(ui, owner, None);
            self.drag = None;
            return result;
        };
        let Some(source) = spec.operands.iter().find(|operand| operand.id == target.operand && !operand.locked) else {
            choose(ui, owner, None);
            self.drag = None;
            return result;
        };
        if input && (ui.input(|state| state.key_pressed(egui::Key::Escape))
            || crate::canvas_input::tool_kind(&session.active_tool) != crate::canvas_input::ToolKind::Move)
        {
            choose(ui, owner, None);
            self.drag = None;
            result.active = true;
            return result;
        }
        result.active = true;
        if self.drag.as_ref().is_some_and(|drag| drag.owner != owner || drag.target != target || drag.generation != session.generation) || !input {
            self.drag = None;
        }
        if !input { return result; }

        let source_point = |screen: Pos2| {
            let view = screen - response.rect.min;
            let doc = view_to_doc(&session.view, [view.x, view.y]);
            let doc = dtool_core::commands::warp::unwarp_point(&session.document, compound.id, doc);
            let inverse = invert_matrix(&object_matrix(compound))?;
            let raw = apply_matrix(&inverse, doc);
            Some([raw[0] - compound.pos[0] + spec.origin[0], raw[1] - compound.pos[1] + spec.origin[1]])
        };
        let contours = outline(session, compound, spec, source, response.rect);
        if response.hovered() && response.hover_pos().is_some_and(|point| inside(&contours, point)) {
            ui.ctx().set_cursor_icon(if self.drag.is_some() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }
        if response.clicked_by(egui::PointerButton::Primary)
            && response.interact_pointer_pos().is_some_and(|point| !inside(&contours, point))
        {
            choose(ui, owner, None);
            return result;
        }
        let (pressed, released) = crate::input::primary_button_positions(ui);
        if let Some(origin) = pressed
            && response.contains_pointer()
            && response.rect.contains(origin)
            && inside(&contours, origin)
            && let Some(start) = source_point(origin)
        {
            self.drag = Some(Drag { screen_start: origin, start, original: source.pos, target, owner, generation: session.generation });
        }
        let (held, pointer) = ui.input(|state| (state.pointer.primary_down(), state.pointer.latest_pos()));
        if !held && released.is_none() { self.drag = None; }
        let pos = self.drag.as_ref().and_then(|drag| {
            let pointer = released.or(pointer)?;
            if pointer.distance(drag.screen_start) < 3.0 { return None; }
            let now = source_point(pointer)?;
            let pos = [drag.original[0] + now[0] - drag.start[0], drag.original[1] + now[1] - drag.start[1]];
            (pos != drag.original).then_some(pos)
        });
        if released.is_some() {
            self.drag = None;
            if let Some(pos) = pos {
                result.command = Some(Command::with_params(COMPOUND_EDIT, serde_json::json!({
                    "id": target.compound, "operands": [{ "id": target.operand, "pos": pos }]
                })));
            }
        } else if let Some(pos) = pos {
            result.draft = preview_compound_move(compound, source.id, pos);
        }
        result
    }

    pub fn paint(&self, ui: &Ui, rect: Rect, session: &Session, owner: ContextId, draft: Option<&Object>) {
        let Some(target) = chosen(ui, owner) else { return };
        let Some(compound) = draft.or_else(|| session.document.get_object(target.compound)) else { return };
        let ObjectKind::Compound(spec) = &compound.kind else { return };
        let Some(source) = spec.operands.iter().find(|operand| operand.id == target.operand) else { return };
        let painter = ui.painter_at(rect);
        for points in outline(session, compound, spec, source, rect) {
            painter.add(egui::Shape::closed_line(points, Stroke::new(1.25, Color32::from_rgb(245, 154, 46))));
        }
    }
}

fn outline(session: &Session, compound: &Object, spec: &CompoundSpec, source: &Object, rect: Rect) -> Vec<Vec<Pos2>> {
    let Some(path) = compound_operand_outline(&session.document, compound, spec, source) else { return Vec::new() };
    dtool_core::outline::flatten_subpaths(&path, dtool_core::outline::OUTLINE_TOLERANCE)
        .into_iter().filter(|(points, closed)| *closed && points.len() >= 3)
        .map(|(points, _)| points.into_iter().map(|p| {
            let [x, y] = doc_to_view(&session.view, p.map(|axis| axis as f32));
            rect.min + egui::vec2(x, y)
        }).collect()).collect()
}

fn inside(contours: &[Vec<Pos2>], point: Pos2) -> bool {
    contours.iter().flat_map(|ring| ring.iter().zip(ring.iter().cycle().skip(1))).fold(false, |inside, (a, b)| {
        inside ^ ((a.y > point.y) != (b.y > point.y)
            && point.x < a.x + (b.x - a.x) * (point.y - a.y) / (b.y - a.y))
    })
}
