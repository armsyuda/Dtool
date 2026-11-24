use dtool_core::commands::appearance::{AppearanceSlots, Focus, Kind, PaintSlot, RowBlend, supports_multiple};
use dtool_core::paint::{Colour, Paint, StrokeStyle};
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::{Command, ContextId, Object, ObjectId};
use dtool_strings::text;
use egui::{Color32, Ui};
use serde_json::json;

pub const PANEL_KEY: &str = "appearance";

#[derive(Clone, Copy)]
struct RowDrag {
    context: Option<ContextId>,
    node: ObjectId,
    version: u64,
    position: usize,
    start: egui::Pos2,
}

pub fn show(ui: &mut Ui, workspace: &mut Workspace) {
    let command = workspace.session().and_then(|session| {
        let [id] = session.selection.ids.as_slice() else { return None; };
        let object = session.document.get_object(*id).filter(|object| supports_multiple(object))?;
        let focus = session.appearance_focus.filter(|focus| focus.active(session));
        Some(ui.add_enabled_ui(!object.locked, |ui| controls(ui, object, focus, workspace.active(), session.generation)).inner)
    });
    match command {
        Some(Some(command)) => {
            let open_gradient = command.name == "appearance.focus";
            let result = workspace.run(Scope::active(), command).1;
            if result.is_ok() && open_gradient && let Some(session) = workspace.session_mut() {
                let _ = crate::tools_panel::handle_tool_command(session,
                    &Command::with_params("ui.tool.set", json!({"id": "tool.gradient"})));
            }
            if let Err(error) = result {
                ui.weak(text(&error.message_key));
            }
        }
        Some(None) => {}
        None => { ui.weak(text("appearance.select_vector")); }
    }
}

fn controls(ui: &mut Ui, object: &Object, focus: Option<Focus>, context: Option<ContextId>, version: u64) -> Option<Command> {
    let node = object.id.0;
    let mut command = None;
    ui.horizontal_wrapped(|ui| {
        let default_paint = Paint::Solid { colour: Colour::new(0, 0, 0, 255) };
        if dtool_design::button_with(ui, text("appearance.add_fill"), &dtool_design::ControlState::enabled(true)).clicked() {
            command = Some(Command::with_params("appearance.add_fill", json!({"node": node, "paint": default_paint})));
        }
        if dtool_design::button_with(ui, text("appearance.add_stroke"), &dtool_design::ControlState::enabled(true)).clicked() {
            let stroke = StrokeStyle { paint: default_paint, width: 1.0, ..Default::default() };
            command = Some(Command::with_params("appearance.add_stroke", json!({"node": node, "stroke": stroke})));
        }
    });
    if let Some(focus) = focus {
        let current = object.appearance_blends.get(focus.slot);
        let label = |mode| match mode {
            RowBlend::Standard(mode) => crate::dialogs::blend_options::mode_label(&format!("{mode:?}")),
            RowBlend::Operator(_) => text("appearance.blend.erase").to_owned(),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(text("layers.blend"));
            egui::ComboBox::from_id_salt("appearance.row_blend").width(68.0)
                .selected_text(label(current))
                .show_ui(ui, |ui| {
                    use dtool_core::blend::BlendMode;
                    let standard = BlendMode::ALL.into_iter().filter(|mode| *mode != BlendMode::PassThrough)
                        .map(RowBlend::Standard);
                    for mode in standard.chain(std::iter::once(RowBlend::ERASE)) {
                        if ui.selectable_label(current == mode, label(mode)).clicked() && current != mode {
                            let mut update = focus.slot.focus_command(object.id).params;
                            update["blend"] = json!(mode);
                            command = Some(Command::with_params("appearance.update", update));
                        }
                    }
                });
        });
    }
    ui.separator();
    let list = object.appearance();
    egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
        // The frontmost paint is the first visible row.
        let order = list.render_order(object.paint_order);
        let drag_id = ui.id().with("row_drag");
        let (press, release, pointer, down) = ui.input(|input| {
            let mut press = None;
            let mut release = None;
            for event in &input.events {
                if let egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } = event {
                    if *pressed { press = Some(*pos); } else { release = Some(*pos); }
                }
            }
            (press, release, input.pointer.interact_pos(), input.pointer.primary_down())
        });
        let mut drag = ui.data(|data| data.get_temp::<RowDrag>(drag_id)).filter(|drag| {
            press.is_none() && drag.context == context && drag.node == object.id && drag.version == version
        });
        let mut hit = None;
        let move_row = |from, to| Command::with_params("appearance.move", json!({"node": node, "from": from, "to": to}));
        for (position, &(kind, index)) in order.iter().enumerate().rev() {
            let (key, paint, width) = match kind {
                Kind::Fill => ("fill", &list.fills[index], None),
                Kind::Stroke => ("stroke", &list.strokes[index].paint, Some(list.strokes[index].width)),
            };
            let params = || json!({"node": node, "kind": key, "index": index});
            ui.push_id((node, key, index), |ui| {
                let row = ui.horizontal_wrapped(|ui| {
                    let slot = PaintSlot { kind, index };
                    let mut visible = list.hidden.is_visible(slot);
                    if dtool_design::visibility_toggle(ui, &mut visible)
                        .on_hover_text(text(if visible { "menu.layer.hide" } else { "menu.layer.show" })).changed()
                    {
                        let mut update = params();
                        update["visible"] = json!(visible);
                        command = Some(Command::with_params("appearance.update", update));
                    }
                    let rgba = dtool_core::scene::representative_colour(paint).unwrap_or(Colour::new(0, 0, 0, 0));
                    let mut display = Color32::from_rgba_unmultiplied(rgba.r, rgba.g, rgba.b, rgba.a);
                    if ui.color_edit_button_srgba(&mut display).changed() {
                        let [r, g, b, a] = display.to_srgba_unmultiplied();
                        let mut update = params();
                        update["paint"] = json!(Paint::Solid { colour: Colour::new(r, g, b, a) });
                        command = Some(Command::with_params("appearance.update", update));
                    }
                    let focused = focus.is_some_and(|focus| focus.slot == slot);
                    let label = text(if kind == Kind::Fill { "context.gradient.target.fill" } else { "context.gradient.target.stroke" });
                    let label = ui.add(egui::Button::selectable(focused, label).sense(egui::Sense::click_and_drag()))
                        .on_hover_text(text("appearance.gradient.edit"));
                    if let Some(start) = press.filter(|point| label.interact_rect.contains(*point))
                        && ui.is_enabled() && ui.ctx().layer_id_at(start) == Some(ui.layer_id())
                    {
                        drag = Some(RowDrag { context, node: object.id, version, position, start });
                    }
                    if label.clicked() {
                        command = Some(slot.focus_command(object.id));
                    }
                    if let Some(width) = width {
                        if let Some(width) = edit_width(ui, width) {
                            let mut update = params();
                            update["width"] = json!(width);
                            command = Some(Command::with_params("appearance.update", update));
                        }
                    }
                    for (icon, label, to) in [
                        ("↑", "appearance.move_up", position.checked_add(1).filter(|position| *position < order.len())),
                        ("↓", "appearance.move_down", position.checked_sub(1)),
                    ] {
                        let response = dtool_design::icon_button_with(ui, icon, &dtool_design::ControlState::enabled(to.is_some()));
                        if response.on_hover_text(text(label)).clicked() && let Some(to) = to {
                            command = Some(move_row(position, to));
                        }
                    }
                    if dtool_design::icon_button(ui, "×").on_hover_text(text("appearance.remove")).clicked() {
                        command = Some(Command::with_params("appearance.remove", params()));
                    }
                });
                if let Some(point) = release.or(pointer)
                    && row.response.rect.contains(point) && ui.clip_rect().contains(point)
                {
                    hit = Some((position, row.response.rect, point));
                }
                ui.separator();
            });
        }
        if let Some(drag) = drag
            && let Some((position, rect, point)) = hit
            && drag.start.distance(point) >= 4.0
        {
            let above = point.y < rect.center().y;
            let gap = position + usize::from(above);
            let to = gap - usize::from(drag.position < gap);
            if to != drag.position {
                let y = if above { rect.top() } else { rect.bottom() };
                ui.painter().hline(rect.x_range(), y, egui::Stroke::new(2.0, ui.visuals().selection.bg_fill));
                if release.is_some() {
                    command = Some(move_row(drag.position, to));
                }
            }
        }
        ui.data_mut(|data| {
            if let Some(drag) = drag.filter(|_| down && release.is_none()) {
                data.insert_temp(drag_id, drag);
            } else {
                data.remove::<RowDrag>(drag_id);
            }
        });
    });
    command
}

fn edit_width(ui: &mut Ui, width: f32) -> Option<f32> {
    let id = ui.id().with("appearance.width");
    let mut value = ui.data(|data| data.get_temp::<f32>(id)).unwrap_or(width);
    let response = ui.add(egui::DragValue::new(&mut value).range(0.0..=10_000.0).speed(0.2));
    if response.dragged() {
        ui.data_mut(|data| data.insert_temp(id, value));
        return None;
    }
    if response.changed() || response.drag_stopped() {
        ui.data_mut(|data| data.remove::<f32>(id));
        return (value != width).then_some(value);
    }
    None
}
