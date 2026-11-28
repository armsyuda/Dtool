use dtool_core::commands::appearance::{Focus, Kind, PaintSlot};
use dtool_core::workspace::Workspace;
use dtool_core::paint::{Cap, Join, LineStyle, StrokeAlign, Arrow, PaintOrder, StrokeStyle};
use dtool_core::{Command, CommandError, ContextId, Object, ObjectId, Session};
use dtool_design::{ControlState, NumericEvent, NumericSpec, NumericValue};
use dtool_strings::text;
use serde_json::{json, Value};

pub struct StrokePanel {
    pub width_value: f32,
    pub dash_values: Vec<f32>,
    pub cap_value: Cap,
    pub join_value: Join,
    pub align_value: StrokeAlign,
    pub start_arrow_name: Option<String>,
    pub end_arrow_name: Option<String>,
    pub arrow_scale: f32,
    pub available_arrows: Vec<String>,
    pub scale_with_object: bool,
    pub pressure_profile: Vec<[f32; 2]>,
    pub paint_order_value: dtool_core::paint::PaintOrder,
    pub pending_commands: Vec<dtool_core::Command>,
    pub error_key: Option<String>,
    binding: Option<(Option<ContextId>, u64, Vec<ObjectId>, Option<PaintSlot>)>,
    width_preview: Option<f32>,
    width_rail_press: Option<bool>,
    preview_pass: Option<u64>,
}

#[derive(Clone, PartialEq)]
pub(crate) struct StrokeWidthPreview {
    pub(crate) ids: Vec<ObjectId>,
    pub(crate) index: usize,
    pub(crate) width: f32,
}

impl StrokePanel {
    pub fn new() -> Self { Self::default() }

    pub fn sync_from_selection(&mut self, ws: &Workspace) {
        let object = ws.session().and_then(|session| session.selection.ids.first().and_then(|id| session.document.objects.iter().find(|object| object.id == *id)));
        if let Some(object) = object {
            let stroke = focused_slot(ws).and_then(|focus| focus.slot.stroke(object)).unwrap_or(&object.stroke);
            self.width_value = stroke.width;
            self.dash_values = stroke.dash.clone();
            self.cap_value = stroke.cap;
            self.join_value = stroke.join;
            self.align_value = stroke.align;
            self.start_arrow_name = stroke.start_arrow.as_ref().map(|arrow| arrow.name.clone());
            self.end_arrow_name = stroke.end_arrow.as_ref().map(|arrow| arrow.name.clone());
            self.arrow_scale = stroke.end_arrow.as_ref().or(stroke.start_arrow.as_ref()).map(|arrow| arrow.scale).unwrap_or(1.0);
            self.scale_with_object = stroke.scale_with_object;
            self.pressure_profile = stroke.pressure.clone();
            self.paint_order_value = object.paint_order;
        }
    }

    pub fn render(&mut self, ui: &mut egui::Ui, workspace: &Workspace) {
        self.preview_pass = Some(ui.ctx().cumulative_pass_nr());
        self.sync_from_selection(workspace);
        let selected: Vec<_> = workspace.session().into_iter().flat_map(|session| {
            session.selection.ids.iter().filter_map(|id| session.document.get_object(*id))
        }).collect();
        let ids: Vec<_> = selected.iter().map(|object| object.id.0).collect();
        let focus = focused_slot(workspace);
        let binding = (
            workspace.active(),
            workspace.session().map_or(0, |session| session.generation),
            selected.iter().map(|object| object.id).collect(),
            focus.map(|focus| focus.slot),
        );
        if self.binding.as_ref() != Some(&binding) {
            self.binding = Some(binding);
            self.width_preview = None;
            self.width_rail_press = None;
            self.error_key = None;
        }
        let enabled = !selected.is_empty() && workspace.session().is_some_and(|session| selected.iter().all(|object| !session.document.is_object_locked(object)))
            && focus.is_none_or(|focus| focus.slot.kind == Kind::Stroke);
        let units_per_point = workspace.session().map_or(1.0, |session| {
            dtool_core::units::units_per_point(session.document.units, session.document.dpi)
        });
        let mut commands = Vec::new();
        dtool_design::panel_frame(ui, |ui| {
            dtool_design::panel_header(ui, text("panel.stroke.title"));
            ui.push_id(self.binding.as_ref().cloned(), |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    self.draw_controls(ui, workspace.session(), &selected, &ids, focus, units_per_point, &mut commands)
                });
            });
            if let Some(key) = &self.error_key {
                ui.label(text(key));
            }
        });
        if commands.len() > 1 {
            self.pending_commands.push(Command::with_params("edit.batch", json!({"commands": commands})));
        } else {
            self.pending_commands.extend(commands);
        }
    }

    pub(crate) fn width_preview(&self, ctx: &egui::Context, session: &Session) -> Option<StrokeWidthPreview> {
        let points = self.width_preview?;
        let (owner, generation, ids, slot) = self.binding.as_ref()?;
        if self.preview_pass != Some(ctx.cumulative_pass_nr())
            || *owner != session.context || *generation != session.generation
            || *ids != session.selection.ids || *slot != Focus::current(session).map(|focus| focus.slot)
        {
            return None;
        }
        Some(StrokeWidthPreview {
            ids: ids.clone(),
            index: slot.map_or(0, |slot| slot.index),
            width: points * dtool_core::units::units_per_point(session.document.units, session.document.dpi),
        })
    }

    fn draw_controls(&mut self, ui: &mut egui::Ui, session: Option<&Session>, selected: &[&Object], ids: &[u64], focus: Option<Focus>, units_per_point: f32, commands: &mut Vec<Command>) {
        let strokes: Vec<_> = selected.iter().map(|object| {
            focus.and_then(|focus| focus.slot.stroke(object)).unwrap_or(&object.stroke)
        }).collect();
        let mut patch = json!({});
        let mut preview = json!({});
        let stroke_command = |mut patch: Value, id: Option<u64>| {
            if let Some(focus) = focus {
                return focus.slot.stroke_command(focus.node, patch);
            }
            match id {
                Some(id) => patch["object_id"] = json!(id),
                None => patch["ids"] = json!(ids),
            }
            Command::with_params("stroke.set", patch)
        };
        let dash = common_value(&strokes, |stroke| stroke.dash.clone());
        let button_width = stroke_icon_width(ui);
        let style_width = stroke_group_width(ui, button_width, 3);
        stroke_row(ui, text("stroke.style"), style_width, |ui, _| {
            for (style, key) in [
                (LineStyle::Solid, "stroke.style.solid"),
                (LineStyle::Dashed, "stroke.style.dashed"),
                (LineStyle::Dotted, "stroke.style.dotted"),
            ] {
                if line_style_button(ui, button_width, style, dash.as_deref().map(LineStyle::from_dash) == Some(style), text(key)).clicked() {
                    patch["dash"] = json!(style.preset_dash());
                }
            }
        });
        let width = common_value(&strokes, |stroke| stroke.width);
        if let Some(width) = self.draw_width(ui, width, units_per_point) {
            patch["width"] = json!(width);
        }

        let extends = common_value(&strokes, |stroke| stroke.cap.extends());
        let round = common_value(&strokes, |stroke| stroke.cap.is_round());
        let (end_position, end_shape) = ui.horizontal_wrapped(|ui| {
            let position = cap_buttons(ui, extends, ["stroke.cap.at_node", "stroke.cap.past_node"], paint_cap_position);
            ui.separator();
            let shape = cap_buttons(ui, round, ["stroke.cap.square", "stroke.cap.round"], paint_cap_shape);
            (position, shape)
        }).inner;
        if end_position.is_some() || end_shape.is_some() {
            for (object, stroke) in selected.iter().zip(&strokes) {
                let cap = Cap::from_options(
                    end_position.unwrap_or_else(|| stroke.cap.extends()),
                    end_shape.unwrap_or_else(|| stroke.cap.is_round()),
                );
                if cap != stroke.cap {
                    commands.push(stroke_command(json!({"cap": cap}), Some(object.id.0)));
                }
            }
        }
        let join = common_value(&strokes, |stroke| match stroke.join {
            Join::Miter { .. } => Join::Miter { limit: 10.0 },
            other => other,
        });
        if let Some(join) = icon_choice(ui, "stroke.join", join, &[
            (Join::Miter { limit: 10.0 }, "stroke.join.miter"), (Join::Round, "stroke.join.round"), (Join::Bevel, "stroke.join.bevel"),
        ], paint_join) {
            patch["join"] = json!(join);
        }
        if let Some(Join::Miter { .. }) = join {
            let limit = common_value(&strokes, |stroke| match stroke.join {
                Join::Miter { limit } => Some(limit),
                _ => None,
            }).flatten();
            stroke_row(ui, text("stroke.join.miter_limit"), 52.0, |ui, label| {
                let spec = NumericSpec::new("").range(1.0..=f64::from(f32::MAX)).width(52.0).decimals(2).enabled(ui.is_enabled());
                let field = ui.make_persistent_id("miter_limit");
                let (response, event) = dtool_design::numeric_field(ui, field, numeric_value(limit), &spec);
                response.labelled_by(label.id).on_hover_text(text("stroke.join.miter_hint"));
                let event = reset_on_caption(ui, &label, event, 10.0);
                if let NumericEvent::Commit(limit) = event {
                    patch["join"] = json!(Join::Miter { limit: limit as f32 });
                } else if let Some(limit) = scrub_value(ui, field) {
                    preview["join"] = json!(Join::Miter { limit: limit as f32 });
                }
            });
        }
        let align = common_value(&strokes, |stroke| stroke.align);
        if let Some(align) = icon_choice(ui, "stroke.align", align, &[
            (StrokeAlign::Inside, "stroke.align.inside"), (StrokeAlign::Centre, "stroke.align.centre"), (StrokeAlign::Outside, "stroke.align.outside"),
        ], paint_align) {
            patch["align"] = json!(align);
        }
        let mut scale = self.scale_with_object;
        let scale_state = ControlState::default().mixed(common_value(&strokes, |stroke| stroke.scale_with_object).is_none());
        if dtool_design::toggle_with(ui, &mut scale, text("stroke.scale_with_object"), &scale_state).changed() {
            patch["scale_with_object"] = json!(scale);
        }
        let order = common_value(selected, |object| object.appearance_stack.is_empty().then_some(object.paint_order)).flatten();
        let order = ui.add_enabled_ui(focus.is_none_or(|focus| focus.slot.index == 0), |ui| {
            icon_choice(ui, "paint.order", order, &[
                (PaintOrder::StrokeOver, "paint.order.stroke_over"), (PaintOrder::StrokeUnder, "paint.order.stroke_under"),
            ], paint_order)
        }).inner;
        if let Some(order) = order {
            commands.push(Command::with_params("paint.order.set", json!({"ids": ids, "order": order})));
        }

        egui::CollapsingHeader::new(text("stroke.dash")).id_salt("dash_details").show(ui, |ui| {
            let parts = dash.as_deref().unwrap_or_default().iter().map(f32::to_string).collect::<Vec<_>>().join(", ");
            if let Some(input) = text_edit(ui, "dash_pattern", &parts, "") {
                let parsed = if input.trim().is_empty() { Some(Vec::new()) } else {
                    input.split(',').map(|value| value.trim().parse::<f32>().ok().filter(|value| value.is_finite() && *value >= 0.0)).collect()
                };
                if let Some(parsed) = parsed { patch["dash"] = json!(parsed); }
            }
            let has_dashes = strokes.iter().all(|stroke| !stroke.dash.is_empty());
            let offset = common_value(&strokes, |stroke| stroke.dash_offset);
            stroke_row(ui, text("stroke.dash_offset"), 64.0, |ui, label| {
                let limit = f64::from(f32::MAX);
                let spec = NumericSpec::new("").range(-limit..=limit).width(64.0).decimals(2).enabled(ui.is_enabled() && has_dashes);
                let field = ui.make_persistent_id("dash_offset");
                let (response, event) = dtool_design::numeric_field(ui, field, numeric_value(offset), &spec);
                response.labelled_by(label.id);
                let event = if has_dashes { reset_on_caption(ui, &label, event, 0.0) } else { event };
                if let NumericEvent::Commit(offset) = event {
                    patch["dash_offset"] = json!(offset);
                } else if let Some(offset) = scrub_value(ui, field) {
                    preview["dash_offset"] = json!(offset);
                }
            });
        });

        let arrows = egui::CollapsingHeader::new(text("stroke.arrow")).id_salt("arrow_details").show(ui, |ui| {
            let start = self.arrow_end(ui, &strokes, true);
            let end = self.arrow_end(ui, &strokes, false);
            let can_swap = strokes.iter().any(|stroke| stroke.start_arrow.is_some() || stroke.end_arrow.is_some());
            dtool_design::studio_row(ui, "", |ui, _| {
                let swap = ui.add_enabled_ui(can_swap, |ui| stroke_button(ui, 30.0, false, text("stroke.arrows.swap"))).inner;
                let colour = ui.style().interact(&swap).fg_stroke.color;
                let at = swap.rect.center();
                paint_arrow_preview(ui.painter(), at + egui::vec2(0.0, -3.0), colour, "open", false);
                paint_arrow_preview(ui.painter(), at + egui::vec2(0.0, 3.0), colour, "open", true);
                if swap.clicked() {
                    if focus.is_some() {
                        patch["start_arrow"] = json!(strokes[0].end_arrow);
                        patch["end_arrow"] = json!(strokes[0].start_arrow);
                    } else {
                        let ids = selected.iter().zip(&strokes)
                            .filter(|(_, stroke)| stroke.start_arrow.is_some() || stroke.end_arrow.is_some())
                            .map(|(object, _)| object.id.0).collect::<Vec<_>>();
                        commands.push(Command::with_params("stroke.arrows.swap", json!({"ids": ids})));
                    }
                }
            });
            (start, end)
        }).body_returned;
        let pressure = common_value(&strokes, |stroke| stroke.pressure.clone());
        let show_pressure = session.is_some_and(|session| session.active_tool == "tool.stroke_width");
        if let Some(profile) = pressure.as_ref().filter(|_| show_pressure) {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
            let plot = rect.shrink(4.0);
            let maximum = profile.iter().map(|point| point[1]).fold(1.0, f32::max);
            let position = |at: f32, weight: f32| {
                egui::pos2(plot.left() + at * plot.width(), plot.bottom() - weight / maximum * plot.height())
            };
            let points = std::iter::once(position(0.0, profile.first().map_or(1.0, |p| p[1])))
                .chain(profile.iter().map(|&[at, weight]| position(at, weight)))
                .chain(std::iter::once(position(1.0, profile.last().map_or(1.0, |p| p[1]))))
                .collect();
            let colour = crate::canvas_input::SELECTION_BLUE;
            painter.add(egui::Shape::line(points, egui::Stroke::new(1.25, colour)));
            for &[at, weight] in profile {
                painter.circle_filled(position(at, weight), 2.15, colour);
            }
        }
        egui::CollapsingHeader::new(text("stroke.pressure")).id_salt("pressure_details").show(ui, |ui| {
            let points = pressure.as_deref().unwrap_or_default().iter()
                .map(|p| format!("{},{}", p[0], p[1])).collect::<Vec<_>>().join("; ");
            if let Some(input) = text_edit(ui, "pressure_points", &points, text("stroke.pressure.none")) {
                let parsed = if input.trim().is_empty() { Some(Vec::new()) } else {
                    input.split(';').map(|point| {
                        let (x, y) = point.split_once(',')?;
                        Some([x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?])
                    }).collect()
                };
                if let Some(parsed) = parsed { patch["pressure"] = json!(parsed); }
            }
        });
        let mut preview_commands = Vec::new();
        if let Some(((start, start_scale, start_preview), (end, end_scale, end_preview))) = arrows {
            for (object, stroke) in selected.iter().zip(&strokes) {
                let mut changes = json!({});
                update_arrow(&mut changes, "start_arrow", &stroke.start_arrow, None, start_preview);
                update_arrow(&mut changes, "end_arrow", &stroke.end_arrow, None, end_preview);
                if changes != json!({}) {
                    let command = stroke_command(changes, Some(object.id.0));
                    preview_commands.push(command);
                }
            }
            if start.is_some() || start_scale.is_some() || end.is_some() || end_scale.is_some() {
                for (object, stroke) in selected.iter().zip(&strokes) {
                    let mut arrows = json!({});
                    let change = if focus.is_some() { &mut patch } else { &mut arrows };
                    update_arrow(change, "start_arrow", &stroke.start_arrow, start.as_ref(), start_scale);
                    update_arrow(change, "end_arrow", &stroke.end_arrow, end.as_ref(), end_scale);
                    if arrows.as_object().is_some_and(|fields| !fields.is_empty()) {
                        commands.push(stroke_command(arrows, Some(object.id.0)));
                    }
                }
            }
        }
        if patch != json!({}) {
            commands.push(stroke_command(patch, None));
        }
        if preview != json!({}) {
            preview_commands.push(stroke_command(preview, None));
        }
        if let Some(session) = session {
            match preview_commands.len() {
                0 => {}
                1 => crate::app_canvas::stage_geometry_preview(ui, session, preview_commands.remove(0)),
                _ => crate::app_canvas::stage_geometry_preview(ui, session,
                    Command::with_params("edit.batch", json!({"commands": preview_commands}))),
            }
        }
    }

    fn draw_width(&mut self, ui: &mut egui::Ui, width: Option<f32>, units_per_point: f32) -> Option<f32> {
        let label = text("stroke.width");
        let field_width = 60.0;
        let rail_minimum = 24.0;
        let control_width = field_width + rail_minimum + ui.spacing().item_spacing.x;
        stroke_row(ui, label, control_width, |ui, caption| {
            let enabled = ui.is_enabled();
            let display = width.map(|value| value / units_per_point);
            let mut points = self.width_preview.or(display).unwrap_or(0.0);
            let rail_width = (stroke_available_width(ui) - field_width - ui.spacing().item_spacing.x).min(180.0);
            let mut committed = None;
            if rail_width >= rail_minimum {
                let upper = display.unwrap_or(0.0).max(100.0);
                let rail = dtool_design::logarithmic_slider_rail(ui, &mut points, 0.0..=upper, rail_width, label, enabled, 0.1);
                // A quick field drag can cross the rail before egui sees the
                // release. The control where the press began owns the edit.
                let to_local = ui.ctx().layer_transform_from_global(rail.layer_id)
                    .unwrap_or(egui::emath::TSTransform::IDENTITY);
                if let Some(rail_press) = ui.input(|input| input.events.iter().rev().find_map(|event| {
                    match event {
                        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. } =>
                            Some(rail.interact_rect.contains(to_local * *pos)),
                        _ => None,
                    }
                })) {
                    self.width_rail_press = Some(rail_press);
                }
                if !enabled || (rail.dragged() && ui.input(|input| input.key_pressed(egui::Key::Escape))) {
                    self.width_preview = None;
                    if rail.dragged() { ui.ctx().stop_dragging(); }
                } else if rail.changed() && self.width_rail_press != Some(false) {
                    if rail.dragged() && !rail.drag_stopped() {
                        self.width_preview = Some(points);
                    } else {
                        committed = Some(points);
                    }
                }
                if rail.drag_stopped() && self.width_rail_press != Some(false) {
                    committed = committed.or(self.width_preview.take());
                } else if !ui.input(|input| input.pointer.primary_down()) {
                    self.width_preview = None;
                }
                if !ui.input(|input| input.pointer.primary_down()) {
                    self.width_rail_press = None;
                }
            } else {
                self.width_preview = None;
                self.width_rail_press = None;
            }
            let id = ui.make_persistent_id("stroke_width_value");
            let spec = NumericSpec::new("").range(0.0..=f64::from(f32::MAX))
                .width(field_width.min(stroke_available_width(ui))).suffix("pt").decimals(3).enabled(enabled);
            let shown = self.width_preview.or(committed).or(display);
            let (field, event) = dtool_design::numeric_field(ui, id, numeric_value(shown), &spec);
            field.labelled_by(caption.id).on_hover_text(label);
            match reset_on_caption(ui, &caption, event, 0.0) {
                NumericEvent::Commit(value) => {
                    self.width_preview = None;
                    committed = Some(value as f32);
                }
                NumericEvent::Cancel => self.width_preview = None,
                _ if self.width_rail_press != Some(true) => {
                    self.width_preview = scrub_value(ui, id).map(|value| value as f32);
                }
                _ => {}
            }
            committed.map(|value| value * units_per_point)
        })
    }

    fn arrow_end(&self, ui: &mut egui::Ui, strokes: &[&StrokeStyle], start: bool) -> (Option<String>, Option<f32>, Option<f32>) {
        let key = if start { "stroke.arrow.start" } else { "stroke.arrow.end" };
        let scale_for = |stroke: &StrokeStyle| {
            let arrow = if start { &stroke.start_arrow } else { &stroke.end_arrow };
            arrow.as_ref().map(|arrow| f64::from(arrow.scale) * 100.0)
        };
        let current = common_value(strokes, |stroke| arrow_name(if start { &stroke.start_arrow } else { &stroke.end_arrow }));
        let scale = common_value(strokes, |stroke| scale_for(stroke).unwrap_or(100.0));
        let scale_enabled = strokes.iter().any(|stroke| scale_for(stroke).is_some());
        let control_width = 110.0 + ui.spacing().item_spacing.x;
        stroke_row(ui, text(key), control_width, |ui, caption| {
            let mut picked = None;
            let combo = egui::ComboBox::from_id_salt(key).width(52.0)
                .selected_text(if current.is_none() { dtool_design::MIXED_TEXT } else { "" })
                .show_ui(ui, |ui| {
                    ui.set_min_width(118.0);
                    for name in std::iter::once("").chain(self.available_arrows.iter().map(String::as_str)) {
                        let selected = current.as_deref() == Some(name);
                        let label = arrow_label(name);
                        let row = ui.horizontal(|ui| {
                            let preview = stroke_button(ui, 30.0, selected, &label);
                            let colour = stroke_icon_ink(ui, &preview, selected);
                            paint_arrow_preview(ui.painter(), preview.rect.center(), colour, name, start);
                            let label = ui.selectable_label(selected, label);
                            preview.clicked() || label.clicked()
                        });
                        if row.inner {
                            if !selected {
                                picked = Some(name.to_owned());
                            }
                            ui.close();
                        }
                    }
                }).response.labelled_by(caption.id);
            if let Some(name) = current {
                let at = egui::pos2(combo.rect.left() + 17.0, combo.rect.center().y);
                paint_arrow_preview(ui.painter(), at, ui.style().interact(&combo).fg_stroke.color, &name, start);
                combo.on_hover_text(arrow_label(&name));
            }
            let spec = NumericSpec::new("").range(1.0..=f64::from(f32::MAX) * 100.0)
                .width(58.0).suffix("%").decimals(1).enabled(scale_enabled);
            let value = scale.map_or(NumericValue::Mixed, NumericValue::Value);
            let id = ui.make_persistent_id((key, "scale"));
            let (field, event) = dtool_design::numeric_field(ui, id, value, &spec);
            field.labelled_by(caption.id).on_hover_text(text("stroke.arrow.scale"));
            let event = if scale_enabled { reset_on_caption(ui, &caption, event, 100.0) } else { event };
            let size = match event {
                NumericEvent::Commit(percent) => Some((percent / 100.0) as f32),
                _ => None,
            };
            let preview = scrub_value(ui, id).map(|percent| (percent / 100.0) as f32);
            (picked, size, preview)
        })
    }

    pub fn apply_pending(&mut self, ws: &mut Workspace) -> Result<(), CommandError> {
        let session = ws.session_mut().ok_or_else(CommandError::no_document)?;
        for command in std::mem::take(&mut self.pending_commands) {
            if let Err(error) = session.run(&command) { self.error_key = Some(error.message_key.clone()); return Err(error); }
        }
        self.error_key = None;
        Ok(())
    }

    pub fn render_mut(&mut self, ui: &mut egui::Ui, ws: &mut Workspace) {
        self.render(ui, ws);
        if !self.pending_commands.is_empty() { let _ = self.apply_pending(ws); }
    }
}

impl Default for StrokePanel {
    fn default() -> Self {
        Self { width_value: 0.0, dash_values: Vec::new(), cap_value: Cap::Butt, join_value: Join::Miter { limit: 10.0 }, align_value: StrokeAlign::Centre, start_arrow_name: None, end_arrow_name: None, arrow_scale: 1.0, available_arrows: dtool_core::commands::stroke::ARROW_NAMES.iter().map(|name| name.to_string()).collect(), scale_with_object: true, pressure_profile: Vec::new(), paint_order_value: PaintOrder::StrokeOver, pending_commands: Vec::new(), error_key: None, binding: None, width_preview: None, width_rail_press: None, preview_pass: None }
    }
}

fn focused_slot(workspace: &Workspace) -> Option<Focus> {
    workspace.session().and_then(Focus::current)
}

fn common_value<S, T: PartialEq>(objects: &[&S], field: impl Fn(&S) -> T) -> Option<T> {
    let value = field(objects.first()?);
    objects.iter().skip(1).all(|object| field(object) == value).then_some(value)
}

fn numeric_value(value: Option<f32>) -> NumericValue {
    value.map_or(NumericValue::Mixed, |value| NumericValue::Value(f64::from(value)))
}

fn stroke_available_width(ui: &egui::Ui) -> f32 {
    ui.available_width().min(ui.clip_rect().right() - ui.cursor().min.x).max(0.0)
}

fn stroke_icon_width(ui: &egui::Ui) -> f32 {
    if stroke_available_width(ui) < 184.0 { 24.0 } else { 30.0 }
}

fn stroke_group_width(ui: &egui::Ui, button_width: f32, count: usize) -> f32 {
    button_width * count as f32 + ui.spacing().item_spacing.x * count.saturating_sub(1) as f32
}

fn scrub_value(ui: &egui::Ui, id: egui::Id) -> Option<f64> {
    ui.data(|data| data.get_temp::<dtool_design::NumericState>(id))
        .filter(|state| state.scrubbing && state.previewed)
        .map(|state| state.scrub)
}

fn reset_on_caption(ui: &egui::Ui, caption: &egui::Response, event: NumericEvent, value: f64) -> NumericEvent {
    if ui.is_enabled() && caption.double_clicked() {
        NumericEvent::Commit(value)
    } else {
        event
    }
}

fn stroke_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    control_width: f32,
    controls: impl FnOnce(&mut egui::Ui, egui::Response) -> R,
) -> R {
    let width = stroke_available_width(ui);
    let caption_width = if width < 184.0 { 48.0 } else { dtool_design::size("panel.label_width") };
    if width < caption_width + ui.spacing().item_spacing.x + control_width {
        let caption = ui.add(dtool_design::numeric_caption(label));
        ui.horizontal(|ui| controls(ui, caption)).inner
    } else {
        ui.horizontal(|ui| {
            let caption = ui.allocate_ui_with_layout(
                egui::vec2(caption_width, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| ui.add(dtool_design::numeric_caption(label).truncate()),
            ).inner;
            controls(ui, caption)
        }).inner
    }
}

fn icon_choice<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    key: &str,
    current: Option<T>,
    entries: &[(T, &str)],
    paint: fn(&egui::Painter, egui::Pos2, egui::Color32, T),
) -> Option<T> {
    let button_width = stroke_icon_width(ui);
    let control_width = stroke_group_width(ui, button_width, entries.len());
    stroke_row(ui, text(key), control_width, |ui, _| {
        let mut choice = None;
        for &(value, key) in entries {
            let selected = current == Some(value);
            let response = stroke_button(ui, button_width, selected, text(key));
            let colour = stroke_icon_ink(ui, &response, selected);
            paint(ui.painter(), response.rect.center(), colour, value);
            if response.clicked() { choice = Some(value); }
        }
        choice
    })
}

fn cap_buttons(ui: &mut egui::Ui, current: Option<bool>, labels: [&str; 2], paint: fn(&egui::Painter, egui::Pos2, egui::Color32, bool)) -> Option<bool> {
    let width = stroke_icon_width(ui);
    ui.horizontal(|ui| {
        let mut picked = None;
        for (value, key) in [false, true].into_iter().zip(labels) {
            let selected = current == Some(value);
            let button = stroke_button(ui, width, selected, text(key));
            let colour = stroke_icon_ink(ui, &button, selected);
            paint(ui.painter(), button.rect.center(), colour, value);
            if button.clicked() && !selected { picked = Some(value); }
        }
        picked
    }).inner
}

fn paint_cap_position(painter: &egui::Painter, at: egui::Pos2, colour: egui::Color32, extends: bool) {
    let mark = 2.0;
    let end = if extends { 7.0 } else { mark };
    painter.rect_filled(egui::Rect::from_min_max(at + egui::vec2(-8.0, -2.5), at + egui::vec2(end, 2.5)), 0.0, colour);
    for [top, bottom] in [[-7.0, -3.0], [3.0, 7.0]] {
        painter.line_segment([at + egui::vec2(mark, top), at + egui::vec2(mark, bottom)], egui::Stroke::new(1.0, colour));
    }
}

fn paint_cap_shape(painter: &egui::Painter, at: egui::Pos2, colour: egui::Color32, round: bool) {
    let end = if round { 3.0 } else { 7.0 };
    painter.rect_filled(egui::Rect::from_min_max(at + egui::vec2(-8.0, -4.0), at + egui::vec2(end, 4.0)), 0.0, colour);
    if round { painter.circle_filled(at + egui::vec2(end, 0.0), 4.0, colour); }
}

fn paint_join(painter: &egui::Painter, at: egui::Pos2, colour: egui::Color32, join: Join) {
    let points: &[(f32, f32)] = match join {
        Join::Miter { .. } => &[(-7.0, 5.0), (-7.0, -5.0), (8.0, -5.0)],
        Join::Round => &[(-7.0, 5.0), (-7.0, 0.0), (-6.0, -3.0), (-3.0, -5.0), (8.0, -5.0)],
        Join::Bevel => &[(-7.0, 5.0), (-7.0, 0.0), (-2.0, -5.0), (8.0, -5.0)],
    };
    painter.add(egui::Shape::line(points.iter().map(|&(x, y)| at + egui::vec2(x, y)).collect(), egui::Stroke::new(2.0, colour)));
}

pub(crate) fn paint_align(painter: &egui::Painter, at: egui::Pos2, colour: egui::Color32, align: StrokeAlign) {
    let offset = match align { StrokeAlign::Inside => 3.0, StrokeAlign::Centre => 0.0, StrokeAlign::Outside => -3.0 };
    let corner = |x: f32, y: f32| [at + egui::vec2(x, 6.0), at + egui::vec2(x, y), at + egui::vec2(8.0, y)];
    painter.add(egui::Shape::line(corner(-4.0, -3.0).to_vec(), egui::Stroke::new(1.0, colour.gamma_multiply(0.65))));
    painter.add(egui::Shape::line(corner(-4.0 + offset, -3.0 + offset).to_vec(), egui::Stroke::new(2.0, colour)));
}

fn paint_order(painter: &egui::Painter, at: egui::Pos2, colour: egui::Color32, order: PaintOrder) {
    let fill = egui::Rect::from_center_size(at + egui::vec2(-3.0, -2.0), egui::vec2(13.0, 12.0));
    let outline = egui::Rect::from_center_size(at + egui::vec2(3.0, 2.0), egui::vec2(12.0, 11.0));
    let paint_fill = || painter.rect_filled(fill, 1.0, colour.lerp_to_gamma(egui::Color32::from_gray(108), 0.65));
    let paint_outline = || painter.rect_stroke(outline, 1.0, egui::Stroke::new(2.0, colour), egui::StrokeKind::Inside);
    match order {
        PaintOrder::StrokeOver => { paint_fill(); paint_outline(); }
        PaintOrder::StrokeUnder => { paint_outline(); paint_fill(); }
    }
}

fn text_edit(ui: &mut egui::Ui, id: &str, current: &str, hint: &str) -> Option<String> {
    let id = ui.make_persistent_id(id);
    let mut value = ui.data(|data| data.get_temp::<String>(id)).unwrap_or_else(|| current.to_owned());
    let response = ui.add(egui::TextEdit::singleline(&mut value).id(id).hint_text(hint));
    if response.has_focus() {
        ui.data_mut(|data| data.insert_temp(id, value));
        None
    } else {
        ui.data_mut(|data| data.remove::<String>(id));
        (response.lost_focus() && value != current).then_some(value)
    }
}

fn arrow_name(arrow: &Option<Arrow>) -> String {
    arrow.as_ref().map(|arrow| {
        dtool_core::commands::stroke::canonical_arrow(&arrow.name).map(str::to_owned).unwrap_or_else(|| arrow.name.clone())
    }).unwrap_or_default()
}

fn arrow_label(name: &str) -> String {
    if name.is_empty() {
        text("stroke.arrow_head.none").to_owned()
    } else {
        let key = dtool_core::commands::stroke::arrow_label_key(name);
        let label = text(&key);
        if label == key { name.to_owned() } else { label.to_owned() }
    }
}

fn paint_arrow_preview(painter: &egui::Painter, at: egui::Pos2, colour: egui::Color32, name: &str, start: bool) {
    let offset = if start { -9.0 } else { 9.0 };
    let tip = at + egui::vec2(offset, 0.0);
    let tail = at - egui::vec2(offset, 0.0);
    painter.line_segment([tail, tip], egui::Stroke::new(1.25, colour));
    if let Some(head) = dtool_core::commands::stroke::arrowhead(name, [tip.x, tip.y], [tail.x, tail.y], 1.2, 1.4) {
        let points = head.points.into_iter().map(|[x, y]| egui::pos2(x, y)).collect();
        let shape = if head.filled {
            egui::Shape::convex_polygon(points, colour, egui::Stroke::NONE)
        } else {
            egui::Shape::line(points, egui::Stroke::new(head.line_width, colour))
        };
        painter.add(shape);
    }
}

fn update_arrow(patch: &mut Value, key: &str, current: &Option<Arrow>, change: Option<&String>, scale: Option<f32>) {
    if let Some(name) = change {
        let arrow = (!name.is_empty()).then(|| Arrow {
            name: name.clone(),
            scale: scale.or_else(|| current.as_ref().map(|arrow| arrow.scale)).unwrap_or(1.0),
        });
        patch[key] = json!(arrow);
    } else if let (Some(scale), Some(current)) = (scale, current) {
        patch[key] = json!(Arrow { name: current.name.clone(), scale });
    }
}

fn stroke_button(ui: &mut egui::Ui, width: f32, selected: bool, label: &str) -> egui::Response {
    let response = dtool_design::studio_icon_button(ui, egui::Button::new(""), width, selected);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
    ControlState::enabled(ui.is_enabled()).tooltip(label).decorate(response)
}

fn stroke_icon_ink(ui: &egui::Ui, response: &egui::Response, selected: bool) -> egui::Color32 {
    if selected && response.enabled() { dtool_design::selected_icon_ink(ui) }
    else { ui.style().interact(response).fg_stroke.color }
}

pub(crate) fn line_style_button(ui: &mut egui::Ui, width: f32, style: LineStyle, selected: bool, label: &str) -> egui::Response {
    let response = stroke_button(ui, width, selected, label);
    let center = response.rect.center();
    let stroke = egui::Stroke::new(ui.style().interact(&response).fg_stroke.width, stroke_icon_ink(ui, &response, selected));
    let line = |from: f32, to: f32| {
        ui.painter().line_segment([center + egui::vec2(from, 0.0), center + egui::vec2(to, 0.0)], stroke);
    };
    match style {
        LineStyle::Solid => { line(-9.0, 9.0); }
        LineStyle::Dashed => { line(-9.0, -1.0); line(2.0, 9.0); }
        LineStyle::Dotted => {
            for x in [-7.0, 0.0, 7.0] {
                ui.painter().circle_filled(center + egui::vec2(x, 0.0), 1.15, stroke.color);
            }
        }
    }
    response
}
