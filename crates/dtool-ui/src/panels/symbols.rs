use dtool_core::commands::symbols;
use dtool_core::Session;
use dtool_core::workspace::{ContextId, Workspace};
use dtool_protocol::{Command, CommandError};
use dtool_strings::text;
use serde_json::json;

pub const PANEL_KEY: &str = "symbols";
pub const WINDOW_PANEL: &str = "window.vector.symbols";

#[derive(Clone, Copy)]
pub(crate) struct SymbolDrag {
    context: ContextId,
    definition: u64,
    start: egui::Pos2,
}

pub(crate) fn canvas_drop(
    ui: &egui::Ui, response: &egui::Response, context: ContextId, session: &mut Session,
) -> Option<CommandError> {
    let payload = egui::DragAndDrop::payload::<SymbolDrag>(ui.ctx())?;
    if payload.context != context {
        return None;
    }
    let definition = session.document.symbols.definitions.iter().find(|entry| entry.id == payload.definition)?;
    let size = definition.size();
    let (release, pointer) = ui.input(|input| {
        let release = input.events.iter().rev().find_map(|event| match event {
            egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, .. } => Some(*pos),
            _ => None,
        });
        (release, release.or_else(|| input.pointer.interact_pos()))
    });
    let pointer = pointer?;
    let rect = response.rect;
    if payload.start.distance(pointer) < 6.0 || !response.interact_rect.contains(pointer)
        || ui.ctx().layer_id_at(pointer) != Some(response.layer_id)
    {
        return None;
    }
    let center = dtool_core::view_to_doc(&session.view, [pointer.x - rect.min.x, pointer.y - rect.min.y]);
    if release.is_some() && egui::DragAndDrop::take_payload::<SymbolDrag>(ui.ctx()).is_some() {
        let command = Command::with_params("symbol.place", json!({"definition": payload.definition, "center": center}));
        response.request_focus();
        ui.ctx().request_repaint();
        return session.run(&command).err();
    }
    let corners = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|[x, y]| {
        let point = [center[0] + x * size[0] * 0.5, center[1] + y * size[1] * 0.5];
        let [x, y] = dtool_core::doc_to_view(&session.view, point);
        rect.min + egui::vec2(x, y)
    });
    let selection = ui.visuals().selection;
    ui.painter_at(rect).add(egui::Shape::convex_polygon(
        corners.to_vec(), selection.bg_fill.gamma_multiply(0.18), selection.stroke,
    ));
    ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
    None
}

#[derive(Default)]
pub struct SymbolsPanel {
    name: String,
    fill: String,
    error: Option<String>,
}

impl SymbolsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        let Some(context) = workspace.active() else { return };
        let Some(session) = workspace.session_mut() else { return };
        let link = symbols::selection_link(session);
        let selected = session.selection.ids.first().and_then(|id| session.document.get_object(*id));
        let has_story = selected.is_some_and(|object| object.story_id().is_some());
        let has_curve = selected.is_some_and(|object| object.curve().is_some());
        let create = Command::with_params("symbol.create", json!({"name": self.name}));
        let can_create = symbols::can_execute(session, &create) == Some(true);
        let mut action = None;
        let mut sync = session.symbol_sync;
        if ui.checkbox(&mut sync, text("symbols.sync")).changed() {
            action = Some(("symbol.sync", json!({"enabled": sync})));
        }

        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.name).hint_text(text("symbols.name")).desired_width(105.0));
            if ui.add_enabled(can_create, egui::Button::new(text("symbols.create"))).clicked() {
                action = Some(("symbol.create", json!({"name": self.name})));
            }
        });

        egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
            if session.document.symbols.definitions.is_empty() {
                ui.weak(text("symbols.empty"));
            }
            for definition in &session.document.symbols.definitions {
                ui.horizontal(|ui| {
                    if dtool_design::button(ui, text("symbols.place")).clicked() {
                        action = Some(("symbol.place", json!({"definition": definition.id})));
                    }
                    let label = ui.add(egui::Label::new(&definition.name).truncate().sense(egui::Sense::drag()))
                        .on_hover_text(text("symbols.drag_to_canvas"))
                        .on_hover_cursor(egui::CursorIcon::Grab);
                    let press = ui.input(|input| input.events.iter().find_map(|event| match event {
                        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. }
                            if label.interact_rect.contains(*pos) => Some(*pos),
                        _ => None,
                    }));
                    if let Some(start) = press && ui.ctx().layer_id_at(start) == Some(label.layer_id) {
                        egui::DragAndDrop::set_payload(ui.ctx(), SymbolDrag { context, definition: definition.id, start });
                    }
                });
            }
        });

        if let Some((instance, definition, node)) = link {
            ui.separator();
            if symbols::has_structure_override(session, instance)
                && dtool_design::button(ui, text("symbols.revert_structure")).clicked()
            {
                action = Some(("symbol.revert", json!({"instance": instance, "property": "structure"})));
            }
            if dtool_design::button(ui, text("symbols.detach")).clicked() {
                action = Some(("symbol.detach", json!({"instance": instance})));
            }
            if let Some(node) = node {
                ui.add(egui::TextEdit::singleline(&mut self.fill).hint_text("#RRGGBB").desired_width(110.0));
                ui.vertical(|ui| {
                    let set = json!({"fill": self.fill.trim()});
                    if dtool_design::button(ui, text("symbols.shared_fill")).clicked() {
                        action = Some(("symbol.edit", json!({"definition": definition, "node": node, "set": set})));
                    }
                    if dtool_design::button(ui, text("symbols.local_fill")).clicked() {
                        action = Some(("symbol.override", json!({"instance": instance, "node": node, "set": set})));
                    }
                    if dtool_design::button(ui, text("symbols.revert_fill")).clicked() {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node})));
                    }
                    if dtool_design::button(ui, text("symbols.revert_stroke")).clicked() {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node, "property": "stroke"})));
                    }
                    if has_story && dtool_design::button(ui, text("symbols.revert_text")).clicked() {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node, "property": "text"})));
                    }
                    if symbols::has_corner_override(session, instance, node)
                        && dtool_design::button(ui, text("symbols.revert_corner")).clicked()
                    {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node, "property": "corner"})));
                    }
                    if symbols::constraints::has_override(session, instance, node)
                        && dtool_design::button(ui, text("symbols.revert_constraint")).clicked()
                    {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node, "property": "constraint"})));
                    }
                    if has_curve && dtool_design::button(ui, text("symbols.revert_curve")).clicked() {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node, "property": "curve"})));
                    }
                });
            }
        }

        if let Some((name, params)) = action {
            self.error = session.run(&Command::with_params(name, params)).err().map(|error| error.message_key);
        }
        if let Some(error) = &self.error {
            ui.label(text(error));
        }
    }
}
