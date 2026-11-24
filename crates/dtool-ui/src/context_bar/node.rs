use crate::canvas_curves::{editable, join_command, NodeEdit};
use crate::fill_stroke::{paint_well_colour_in, FillStrokeWells, WellState};
use crate::icon_cache::IconCache;
use crate::panels::colour::ColourTarget;
use dtool_core::context_toolbar::{CURVE_ENERGY, OPTION_SET};
use dtool_core::curve::NodeKind;
use dtool_core::paint::{DisplayColourCtx, LineStyle, PaintRole};
use dtool_core::{Command, Session};
use egui::{Color32, Rect, Stroke, StrokeKind, Ui, vec2};
use serde_json::json;

pub(crate) fn show(ui: &mut Ui, session: &Session, edit: &NodeEdit, wells: &FillStrokeWells, icons: &mut IconCache)
    -> (Option<Command>, Option<ColourTarget>)
{
    let text = dtool_strings::text;
    let objects: Vec<_> = session.selection.ids.iter().filter_map(|id| session.document.get_object(*id)).collect();
    let can_paint = !objects.is_empty() && objects.iter().any(|object| !object.locked);
    let display = DisplayColourCtx::for_optional(Some(&session.document));
    let mut paint = None;
    for (role, target, label) in [
        (PaintRole::SourceObject, ColourTarget::Fill, "fill.well"),
        (PaintRole::Edge, ColourTarget::Stroke, "stroke.well"),
    ] {
        let response = ui.add_enabled(can_paint, egui::Button::new("").min_size(vec2(38.0, 26.0)));
        let rect = Rect::from_center_size(response.rect.center(), vec2(28.0, 15.0));
        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, Color32::from_gray(27));
        match wells.role_state(role) {
            WellState::Single(value) if let Some(colour) = paint_well_colour_in(value, &display, None) => {
                painter.rect_filled(rect.shrink(if role == PaintRole::Edge { 3.0 } else { 1.0 }), 2.0, colour);
            }
            WellState::Mixed => { painter.text(rect.center(), egui::Align2::CENTER_CENTER, "—", egui::FontId::proportional(12.0), ui.visuals().weak_text_color()); }
            _ => { painter.line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(1.0, Color32::from_rgb(220, 89, 89))); }
        }
        painter.rect_stroke(rect, 3.0, ui.visuals().widgets.inactive.bg_stroke, StrokeKind::Inside);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, can_paint, text(label)));
        if response.on_hover_text(text(label)).clicked() { paint = Some(target); }
    }
    let mut action = None;
    let ids = objects.iter().filter(|object| !object.locked).map(|object| object.id.0).collect::<Vec<_>>();
    let style = ui.add_enabled(can_paint, egui::Button::new("").min_size(vec2(24.0, 26.0)));
    let style = style.on_hover_text(text("stroke.style"));
    style.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, can_paint, text("stroke.style")));
    let p = style.rect.center();
    for y in [-4.0, 0.0, 4.0] {
        ui.painter().line_segment([p + vec2(-7.0, y), p + vec2(7.0, y)],
            Stroke::new(1.15, ui.style().interact(&style).fg_stroke.color));
    }
    if can_paint {
        egui::Popup::menu(&style).show(|ui| {
            ui.horizontal(|ui| {
                for (line, label) in [
                    (LineStyle::Solid, "stroke.style.solid"),
                    (LineStyle::Dashed, "stroke.style.dashed"),
                    (LineStyle::Dotted, "stroke.style.dotted"),
                ] {
                    let selected = objects.iter().all(|object| LineStyle::from_dash(&object.stroke.dash) == line);
                    if crate::stroke_panel::line_style_button(ui, 30.0, line, selected, text(label)).clicked() {
                        action = Some(Command::with_params("stroke.set", json!({"ids": ids, "dash": line.preset_dash()})));
                        ui.close();
                    }
                }
            });
        });
    }
    let scale = dtool_core::doc_setup::unit_factor(dtool_core::document::Units::Pt, session.document.dpi,
        session.document.units, session.document.dpi);
    let common_width = objects.first().map(|object| object.stroke.width)
        .filter(|width| objects.iter().all(|object| object.stroke.width == *width));
    let mut points = common_width.unwrap_or(0.0) / scale;
    let mut field = egui::DragValue::new(&mut points).speed(0.1).range(0.0..=100_000.0).suffix(" pt");
    if common_width.is_none() { field = field.custom_formatter(|_, _| "—".into()); }
    if ui.add_enabled(can_paint, field).on_hover_text(text("stroke.width")).changed() {
        action = Some(Command::with_params("stroke.set", json!({
            "ids": ids,
            "width": points * scale,
        })));
    }
    ui.separator();

    let target = edit.target.filter(|id| session.selection.ids.as_slice() == [*id]);
    let contours = target.and_then(|id| editable(session, id));
    let kinds: Vec<_> = contours.iter().flat_map(|contours| edit.selected.iter().filter_map(|&(c, i)| {
        contours.get(c)?.points.get(i).map(|node| node.kind)
    })).collect();
    for (kind, key) in [
        (NodeKind::Sharp, "context.node.convert.sharp"),
        (NodeKind::Smooth, "context.node.convert.smooth"),
        (NodeKind::Smart, "context.node.convert.smart"),
    ] {
        let enabled = !kinds.is_empty();
        let selected = enabled && kinds.iter().all(|value| *value == kind);
        if conversion_button(ui, kind, text(key), enabled, selected) && let Some(command) = edit.convert_selected(session, kind) {
            action = Some(command);
        }
    }
    ui.separator();

    let close = target.filter(|_| contours.as_ref().is_some_and(|value| value.first().is_some_and(|c| !c.closed)))
        .map(|id| Command::with_params("path.close", json!({"id": id.0, "preserve_transform": true})));
    let reverse = target.and_then(|_| edit.reverse_command(session));
    for (icon, label, command) in [
        ("cmd-ungroup", "ctx.node.break", target.and_then(|_| edit.break_command(session))),
        ("cmd-group", "ctx.node.join", join_command(session)),
        ("ctx-close-curve", "ctx.node.close", close),
        ("cmd-undo", "context.node.reverse", reverse),
    ] {
        if super::align::icon_action(ui, icons, icon, text(label), command.is_some()).clicked() {
            action = command;
        }
    }
    ui.separator();
    for (id, icon, label) in [
        ("node.transform_mode", "cmd-transform-box", "context.node.transform_mode"),
        (CURVE_ENERGY, "ctx-settings", "context.curve_energy.settings"),
    ] {
        let active = session.tool_options.flag("tool.node", id).unwrap_or(false);
        let enabled = target.is_some() && !edit.primitive;
        let key = text(label);
        let state = dtool_design::ControlState::enabled(enabled).tooltip(key);
        let response = match icons.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui)) {
            Some(texture) => dtool_design::icon_image_button_selected(ui, &texture, active, &state),
            None => dtool_design::button_with(ui, key, &state),
        };
        if response.clicked() {
            action = Some(Command::with_params(OPTION_SET, json!({ "tool": "tool.node", "id": id, "value": !active })));
        }
    }
    (action, paint)
}

fn conversion_button(ui: &mut Ui, kind: NodeKind, label: &str, enabled: bool, selected: bool) -> bool {
    let response = ui.add_enabled(enabled, egui::Button::new("").selected(selected).min_size(vec2(28.0, 26.0)));
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, selected, label));
    let colour = ui.style().interact(&response).fg_stroke.color;
    let stroke = Stroke::new(1.25, colour);
    let p = response.rect.center();
    let (left, anchor, right) = (p + vec2(-7.0, 5.0), p + vec2(0.0, -5.0), p + vec2(7.0, 5.0));
    let painter = ui.painter();
    match kind {
        NodeKind::Sharp => painter.add(egui::Shape::line(vec![left, anchor, right], stroke)),
        NodeKind::Smooth | NodeKind::Smart => painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
            [left, p + vec2(-4.0, -10.0), p + vec2(4.0, -10.0), right], false, Color32::TRANSPARENT, stroke,
        )),
    };
    if kind == NodeKind::Smart {
        painter.circle_stroke(anchor, 2.5, stroke);
    } else {
        painter.rect_filled(Rect::from_center_size(anchor, vec2(3.0, 3.0)), 0.5, colour);
    }
    response.on_hover_text(label).clicked()
}
