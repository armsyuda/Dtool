use dtool_core::paint::{Colour, LineStyle, Paint, StrokeAlign};
use dtool_core::table::commands::{selection_border_sides, selection_border_slots};
use dtool_core::table::model::{CellRotation, EdgeStyle, InitialAdvance, TableEdgeMask as Edge, TableFrameFormat, TableNode, VerticalAlign};
use dtool_core::table::{resolve_cell, resolve_frame, selection};
use dtool_core::{Command, ObjectKind, Session};
use dtool_design::{ControlState, WellValue};
use dtool_strings::text;
use egui::ecolor::Hsva;
use egui::{Color32, Response, Ui};
use serde_json::{json, Value};
use std::collections::HashSet;

use super::colour::{picker_button, picker_colour, picker_hsva};

pub const PANEL_KEY: &str = "table";

const BORDERS: [(Edge, &str); 13] = [
    (Edge::OUTSIDE, "table.edge.outside"), (Edge::INSIDE, "table.edge.inside"), (Edge::ALL, "table.edge.all"),
    (Edge::TOP, "table.edge.top"), (Edge::BOTTOM, "table.edge.bottom"), (Edge::OUTSIDE_H, "table.edge.horizontal_outer"),
    (Edge::INSIDE_H, "table.edge.horizontal_inner"), (Edge::ALL_H, "table.edge.horizontal_all"),
    (Edge::LEFT, "table.edge.left"), (Edge::RIGHT, "table.edge.right"), (Edge::INSIDE_V, "table.edge.vertical_inner"),
    (Edge::OUTSIDE_V, "table.edge.vertical_outer"), (Edge::ALL_V, "table.edge.vertical_all"),
];

pub struct TablePanel {
    border_mask: Edge,
    border_colour: Hsva,
    fill_colour: Hsva,
    text_colour: Hsva,
    error: Option<String>,
    icons: crate::icon_cache::IconCache,
}

impl Default for TablePanel {
    fn default() -> Self {
        Self {
            border_mask: Edge::ALL, border_colour: Hsva::from_srgb([0; 3]),
            fill_colour: Hsva::from_srgb([255; 3]), text_colour: Hsva::from_srgb([0; 3]),
            error: None, icons: Default::default(),
        }
    }
}

struct CellAppearance {
    fill: Option<Paint>,
    align: Option<VerticalAlign>,
    rotation: Option<CellRotation>,
    advance: Option<InitialAdvance>,
    minimum: Option<f32>,
    insets: [Option<f32>; 4],
    linked: Option<bool>,
}

fn common<T: PartialEq>(current: &mut Option<T>, next: T) {
    if current.as_ref() != Some(&next) { *current = None; }
}

pub(crate) const TABLE_FORMAT_EDIT: &str = "table.panel.format";

impl TablePanel {
    pub fn show(&mut self, ui: &mut Ui, session: Option<&mut Session>) {
        dtool_design::panel_frame(ui, |ui| {
            dtool_design::panel_header(ui, text("panel.table"));
            let Some(session) = session else {
                ui.label(text("table.panel.select_table"));
                return;
            };
            use dtool_core::edit_session::EditEnd;
            let format_edit = session.edit.as_ref().is_some_and(|edit| edit.key == TABLE_FORMAT_EDIT);
            let (cancel_edit, pointer_down) = ui.input(|input| (
                input.key_pressed(egui::Key::Escape), input.pointer.primary_down(),
            ));
            if format_edit && cancel_edit {
                self.error = session.end_edit(EditEnd::Cancel).err().map(|error| text(&error.message_key).into());
                ui.ctx().request_repaint();
                return;
            }
            if format_edit && !pointer_down && session.selection.ids.is_empty() {
                let _ = session.end_edit(EditEnd::Commit);
            }
            let current = selection::for_format(session);
            let Some(object) = current.table.or_else(|| match session.selection.ids.as_slice() {
                [id] => Some(*id),
                _ => None,
            }).and_then(|id| session.document.get_object(id)) else {
                ui.label(text("table.panel.select_table"));
                return;
            };
            let ObjectKind::Table(table) = &object.kind else {
                ui.label(text("table.panel.select_table"));
                return;
            };
            let id = object.id;
            let frame = resolve_frame(&session.document, table);
            let ids: HashSet<_> = current.cells.iter().copied().collect();
            let mut formats = table.cells.iter().filter(|cell| ids.contains(&cell.id))
                .map(|cell| resolve_cell(&session.document, table, cell));
            let appearance = formats.next().map(|first| {
                let mut state = CellAppearance {
                    fill: Some(first.fill.unwrap_or(Paint::None)), align: Some(first.vertical_align),
                    rotation: Some(first.rotation), advance: Some(first.initial_advance), minimum: Some(first.minimum_pt),
                    insets: [first.insets.left, first.insets.top, first.insets.right, first.insets.bottom].map(Some),
                    linked: Some(first.insets.linked),
                };
                for format in formats {
                    common(&mut state.fill, format.fill.unwrap_or(Paint::None));
                    common(&mut state.align, format.vertical_align);
                    common(&mut state.rotation, format.rotation);
                    common(&mut state.advance, format.initial_advance);
                    common(&mut state.minimum, format.minimum_pt);
                    common(&mut state.linked, format.insets.linked);
                    for (value, inset) in state.insets.iter_mut().zip([format.insets.left, format.insets.top, format.insets.right, format.insets.bottom]) {
                        common(value, inset);
                    }
                }
                state
            });
            let suffix = dtool_core::commands::ruler_units::suffix(session.document.units);
            let command = ui.add_enabled_ui(!object.locked, |ui| {
                let selection = ui.horizontal_wrapped(|ui| {
                    let select_all = Command::with_params("table.selection.set", json!({"id": id, "mode": "all"}));
                    let all = !ids.is_empty() && table.cells.iter().filter(|cell| cell.merged_into.is_none()).count() == ids.len();
                    let mut action = None;
                    if self.icons.studio_button(ui, "tool-table", text("table.panel.select_all"), all).clicked() {
                        action = Some(select_all);
                    }
                    for (name, key) in [
                        (dtool_core::table::merge::MERGE, "menu.text.table.merge_cells"),
                        (dtool_core::table::merge::UNMERGE, "menu.text.table.separate_cells"),
                    ] {
                        let command = Command::new(name);
                        let enabled = session.can_execute(&command);
                        if dtool_design::button_with(ui, text(key), &ControlState::enabled(enabled)).clicked() {
                            action = Some(command);
                        }
                    }
                    action
                }).inner;
                let stroke_fill = egui::CollapsingHeader::new(text("table.panel.stroke_fill"))
                    .default_open(true).show(ui, |ui| match &appearance {
                        Some(appearance) => self.cell_stroke_fill(ui, appearance, table, &session.document, &ids),
                        None => { ui.label(text("table.panel.select_cells")); None }
                    }).body_returned.flatten();
                let text_ranges = super::character::table_colour_ranges(session);
                let text_paint = match super::character::table_text_paint(&text_ranges) {
                    crate::fill_stroke::WellState::Single(paint) => Some(paint),
                    _ => None,
                };
                let text_colour = ui.add_enabled_ui(!text_ranges.is_empty(), |ui| {
                    paint_choice(ui, "table.panel.text_colour", text_paint.as_ref(), &mut self.text_colour)
                        .and_then(|paint| super::character::table_text_colour_command(&text_ranges, &paint))
                }).inner;
                let tracks = appearance.as_ref().and_then(|_| {
                    egui::CollapsingHeader::new(text("table.panel.tracks")).default_open(true)
                        .show(ui, |ui| self.track_controls(ui, session, table, &current, id, suffix))
                        .body_returned.flatten()
                });
                let (cells, paragraph) = egui::CollapsingHeader::new(text("table.panel.cells"))
                    .default_open(true).show(ui, |ui| {
                        let Some(appearance) = &appearance else {
                            ui.label(text("table.panel.select_cells"));
                            return (None, None);
                        };
                        let ranges = super::paragraph::active_ranges(session);
                        let mut styles = ranges.iter().filter_map(|(id, [start, end])| {
                            let story = session.document.stories.iter().find(|story| story.id == *id)?;
                            let style = dtool_core::text_cmd::query_para_style(&story.paras, *start, *end);
                            serde_json::from_value::<dtool_tools::dtool_text::ParaStyle>(style).ok()
                        }).map(|style| style.align);
                        let mut align = styles.next();
                        for value in styles { common(&mut align, value); }
                        let paragraph = ui.add_enabled_ui(!ranges.is_empty(), |ui| super::paragraph::alignment_buttons(ui, align, true))
                            .inner.and_then(|align| super::paragraph::patch_command(&ranges, json!({"align": align})));
                        (self.cell_controls(ui, appearance, suffix), paragraph)
                    }).body_returned.unwrap_or_default();
                let frame = self.frame_controls(ui, &frame);
                selection.or_else(|| frame.map(|patch| Command::with_params("table.format.set", json!({"id":id.0,"frame":patch})))
                    .or_else(|| stroke_fill.or(cells).map(|mut patch| {
                        patch["id"] = json!(id.0);
                        patch["cells"] = json!(current.cells);
                        Command::with_params(Command::TABLE_CELL_FORMAT_SET, patch)
                    })).or(text_colour).or(paragraph).or(tracks))
            }).inner;
            if let Some(command) = command {
                let preview = matches!(command.name.as_str(), "table.format.set" | Command::TABLE_CELL_FORMAT_SET
                    | super::character::STYLE_COMMAND)
                    || (command.name == super::character::BATCH_COMMAND && command.params["commands"].as_array()
                        .is_some_and(|parts| !parts.is_empty() && parts.iter().all(|part| part["name"] == super::character::STYLE_COMMAND)));
                let preview = preview && (pointer_down || format_edit);
                if format_edit && !preview {
                    let _ = session.end_edit(EditEnd::Commit);
                }
                let result = if command.name == selection::SET {
                    crate::canvas::frame_text_input::finish_text_input(session);
                    crate::tools_panel::handle_tool_command(session, &Command::with_params("ui.tool.set", json!({"id": "tool.table"})))
                        .and_then(|_| session.run(&command))
                } else if matches!(command.name.as_str(), dtool_core::table::merge::MERGE | dtool_core::table::merge::UNMERGE) {
                    crate::canvas::frame_text_input::finish_text_input(session);
                    session.run(&command)
                } else if preview {
                    session.preview(TABLE_FORMAT_EDIT, &command)
                } else {
                    session.run(&command)
                };
                self.error = result.err().map(|error| text(&error.message_key).to_string());
                ui.ctx().request_repaint();
            }
            if !pointer_down && session.edit.as_ref().is_some_and(|edit| edit.key == TABLE_FORMAT_EDIT) {
                if let Err(error) = session.end_edit(EditEnd::Commit) {
                    self.error = Some(text(&error.message_key).into());
                }
                ui.ctx().request_repaint();
            }
            if let Some(error) = &self.error { ui.label(error); }
        });
    }

    fn track_controls(&mut self, ui: &mut Ui, session: &Session, table: &TableNode,
        current: &selection::TableSelection, id: dtool_core::ObjectId, suffix: &str) -> Option<Command> {
        let mut command = None;
        let mut draw = |ui: &mut Ui, rows: bool| {
            let (axis, label, tracks, menu) = if !rows {
                ("column", "table.panel.cell_width", &table.col_widths_pt, [
                ("text.table.autofit_columns", "ctx-zoom-fit"),
                ("text.table.evenly_distribute_columns", "tool-distribute-h"),
                ])
            } else {
                ("row", "table.panel.cell_height", &table.row_heights_pt, [
                ("text.table.autofit_rows", "ctx-zoom-fit"),
                ("text.table.evenly_distribute_rows", "tool-distribute-v"),
                ])
            };
            let indices = current.track_indices(table, rows);
            let Some(&first) = indices.first().and_then(|index| tracks.get(*index as usize)) else { return; };
            let uniform = indices.iter().all(|index| tracks.get(*index as usize) == Some(&first));
            ui.push_id((id, axis, &indices), |ui| {
                if let Some(size) = scalar(ui, label, uniform.then_some(first), suffix).filter(|value| *value > 0.0) {
                    command = Some(Command::with_params("table.track.resize",
                        json!({"id":id.0,"axis":axis,"indices":indices,"size":size})));
                }
                ui.horizontal_wrapped(|ui| {
                    for (menu_id, icon) in menu {
                        let action = crate::table::menu::command(session, menu_id);
                        let label_key = format!("menu.{menu_id}");
                        let label = text(&label_key);
                        if ui.add_enabled_ui(action.is_some(), |ui| self.icons.studio_button(ui, icon, label, false)).inner.clicked() {
                            command = action;
                        }
                    }
                });
            });
        };
        dtool_design::panel_columns(ui, 2, dtool_design::size("panel.group.min_width"), |ui, index| {
            draw(ui, index == 1);
        });
        command
    }

    fn frame_controls(&mut self, ui: &mut Ui, frame: &TableFrameFormat) -> Option<Value> {
        let mut patch = None;
        let mut ignore = frame.ignore_baseline_grid;
        if wrapped_toggle(ui, &mut ignore, "table.frame.ignore_grid", false) {
            patch.get_or_insert_with(|| json!({}))["ignore_baseline_grid"] = json!(ignore);
        }
        let baseline = frame.baseline.clone().unwrap_or_default();
        if let Some(baseline) = baseline_controls(ui, &baseline) {
            patch.get_or_insert_with(|| json!({}))["baseline"] = json!(baseline);
        }
        patch
    }

    fn cell_stroke_fill(
        &mut self,
        ui: &mut Ui,
        state: &CellAppearance,
        table: &TableNode,
        document: &dtool_core::Document,
        ids: &HashSet<dtool_core::table::model::CellId>,
    ) -> Option<Value> {
        let mut patch = paint_choice(ui, "table.panel.cell_fill", state.fill.as_ref(), &mut self.fill_colour)
            .map(|paint| json!({"fill":paint}));
        ui.separator();
        ui.label(text("table.panel.cell_border"));
        ui.scope(|ui| {
            let width = dtool_design::size("control.icon.width");
            let gap = dtool_design::space("xs");
            ui.spacing_mut().item_spacing.x = gap;
            for masks in [&BORDERS[..3], &BORDERS[3..8], &BORDERS[8..]] {
                let columns = dtool_design::panel_column_count(ui, ui.available_width() - gap, masks.len(), width);
                for row in masks.chunks(columns) {
                    ui.horizontal(|ui| {
                        for (mask, key) in row {
                            if edge_button(ui, mask, key, &self.border_mask == mask, width).clicked() {
                                self.border_mask = mask.clone();
                            }
                        }
                    });
                }
            }
        });
        let selected = table.cells.iter().filter(|cell| ids.contains(&cell.id));
        let slots = selection_border_slots(selected.clone());
        let mut edges = selected.flat_map(|cell| {
            let sides = selection_border_sides(cell, &slots, &self.border_mask);
            let format = resolve_cell(document, table, cell);
            format.edges.into_iter().zip(sides).filter_map(|(edge, side)| {
                let visible = edge.as_ref().is_some_and(|edge| edge.width_pt > 0.0 && !matches!(edge.paint, Paint::None));
                (side == Some(true) || (side == Some(false) && visible)).then_some(edge)
            })
        });
        let Some(first) = edges.next() else { return patch; };
        let parts = |edge: Option<EdgeStyle>| edge.map_or((Paint::None, 0.0, LineStyle::Solid, StrokeAlign::Centre), |edge| {
            (edge.paint, edge.width_pt, edge.line_style, edge.align)
        });
        let (paint, width, style, align) = parts(first);
        let (mut paint, mut width, mut style, mut align) = (Some(paint), Some(width), Some(style), Some(align));
        for edge in edges {
            let (next_paint, next_width, next_style, next_align) = parts(edge);
            common(&mut paint, next_paint);
            common(&mut width, next_width);
            common(&mut style, next_style);
            common(&mut align, next_align);
        }
        let mut edge_patch = serde_json::Map::new();
        if let Some(paint) = paint_choice(ui, "table.panel.border_colour", paint.as_ref(), &mut self.border_colour) {
            edge_patch.insert("paint".into(), json!(paint));
        }
        let (new_width, new_style) = ui.push_id(self.border_mask.0,
            |ui| width_and_pattern(ui, "stroke.width", width, style)).inner;
        if let Some(width) = new_width {
            edge_patch.insert("width_pt".into(), json!(width));
        }
        if let Some(style) = new_style {
            edge_patch.insert("line_style".into(), json!(style));
        }
        if let Some(align) = border_alignment(ui, align) {
            edge_patch.insert("align".into(), json!(align));
        }
        if !edge_patch.is_empty() {
            patch.get_or_insert_with(|| json!({}))["borders"] = json!({
                "selection_mask": self.border_mask.0, "edge_patch": edge_patch
            });
        }
        patch
    }

    fn cell_controls(&mut self, ui: &mut Ui, state: &CellAppearance, suffix: &str) -> Option<Value> {
        let mut patch = None;
        if let Some(align) = option_row(ui, "table.panel.vertical_align", |ui, caption| {
            let mut chosen = None;
            for (align, icon, key) in [
                (VerticalAlign::Top, "ctx-text-valign-top", "table.align.top"),
                (VerticalAlign::Center, "ctx-text-valign-center", "table.align.center"),
                (VerticalAlign::Bottom, "ctx-text-valign-bottom", "table.align.bottom"),
                (VerticalAlign::Justify, "ctx-text-valign-justify", "table.align.justify"),
            ] {
                let label = text(key);
                let selected = state.align == Some(align);
                let button = match self.icons.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui)) {
                    Some(texture) => egui::Button::image(egui::Image::new(&texture).fit_to_exact_size(egui::Vec2::splat(14.0))),
                    None => egui::Button::new(label).truncate(),
                };
                let response = dtool_design::with_icon_selection(ui, |ui| {
                    ui.add_sized([22.0, 22.0], button.selected(selected)).labelled_by(caption).on_hover_text(label)
                });
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
                if response.clicked() { chosen = Some(align); }
            }
            chosen
        }) {
            patch = Some(json!({"vertical_align":align}));
        }
        if let Some(rotation) = option_row(ui, "table.panel.rotation", |ui, caption| {
            let mut chosen = None;
            for (rotation, label) in [
                (CellRotation::R0, "0°"), (CellRotation::R90, "90°"),
                (CellRotation::R180, "180°"), (CellRotation::R270, "270°"),
            ] {
                let selected = state.rotation == Some(rotation);
                let angle = match rotation {
                    CellRotation::R0 => 0.0, CellRotation::R90 => std::f32::consts::FRAC_PI_2,
                    CellRotation::R180 => std::f32::consts::PI, CellRotation::R270 => -std::f32::consts::FRAC_PI_2,
                };
                let texture = self.icons.themed_texture(ui.ctx(), "ctx-text-valign-top", dtool_design::icon_texture_px(ui));
                let button = texture.as_ref().map_or_else(|| egui::Button::new(label), |texture|
                    egui::Button::image(egui::Image::new(texture).fit_to_exact_size(egui::vec2(14.0, 14.0))
                        .rotate(angle, egui::vec2(0.5, 0.5))));
                let response = dtool_design::studio_icon_button(ui, button, 24.0, selected)
                    .labelled_by(caption).on_hover_text(label);
                if response.clicked() { chosen = Some(rotation); }
            }
            chosen
        }) {
            patch = Some(json!({"rotation":rotation}));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong(text("table.panel.insets"));
            if self.icons.studio_button(ui, if state.linked == Some(true) { "panel-aspect-linked" } else { "panel-aspect-unlinked" },
                text("table.panel.link_insets"), state.linked == Some(true)).clicked()
            {
                patch = Some(json!({"inset_patch":{"linked": state.linked != Some(true)}}));
            }
        });
        let sides = [
            (0, "left", "table.inset.left"), (2, "right", "table.inset.right"),
            (1, "top", "table.inset.top"), (3, "bottom", "table.inset.bottom"),
        ];
        dtool_design::panel_columns(ui, 2, 108.0, |ui, column| {
            for &(index, side, key) in sides.iter().skip(column).step_by(2) {
                if let Some(value) = scalar(ui, key, state.insets[index], suffix) {
                    patch = Some(json!({"inset_patch":{side:value}}));
                }
            }
        });
        ui.separator();
        if let Some(advance) = choice(ui, "table.panel.initial_advance", state.advance, &[
            (InitialAdvance::Default, text("table.advance.default")), (InitialAdvance::Ascent, text("table.advance.ascent")),
            (InitialAdvance::CapHeight, text("table.advance.cap")), (InitialAdvance::XHeight, text("table.advance.x")),
            (InitialAdvance::Leading, text("table.advance.leading")), (InitialAdvance::Fixed, text("table.advance.fixed")),
        ]) {
            patch = Some(json!({"initial_advance":advance}));
        }
        if let Some(minimum) = scalar(ui, "table.panel.minimum", state.minimum, "pt") {
            patch = Some(json!({"minimum":minimum}));
        }
        patch
    }

}

fn width_and_pattern(ui: &mut Ui, label: &str, width: Option<f32>, style: Option<LineStyle>)
    -> (Option<f32>, Option<LineStyle>)
{
    let mut picked = None;
    property_row(ui, label, 112.0, |ui, caption| ui.horizontal(|ui| {
        let mut value = width.unwrap_or(0.0);
        let field_width = 50.0;
        let rail_width = (ui.available_width() - field_width - ui.spacing().item_spacing.x).max(24.0);
        let maximum = value.max(24.0);
        let rail = dtool_design::logarithmic_slider_rail(ui, &mut value, 0.0..=maximum, rail_width, text(label), true, 0.05);
        if rail.changed() {
            picked = Some((value * 20.0).round() / 20.0);
        }
        let spec = dtool_design::NumericSpec::new("").range(0.0..=f64::from(f32::MAX))
            .width(field_width).decimals(2).suffix("pt");
        let display = picked.or(width).map_or(dtool_design::NumericValue::Mixed, |number| {
            dtool_design::NumericValue::Value(f64::from(number))
        });
        let (response, event) = dtool_design::numeric_field(ui, ui.make_persistent_id(label), display, &spec);
        response.labelled_by(caption);
        if let dtool_design::NumericEvent::Preview(number) | dtool_design::NumericEvent::Commit(number) = event {
            picked = Some(number as f32);
        }
    }));
    (picked, stroke_pattern(ui, style))
}

pub(super) fn stroke_pattern(ui: &mut Ui, current: Option<LineStyle>) -> Option<LineStyle> {
    option_row(ui, "stroke.style", |ui, caption| {
        let mut chosen = None;
        for (style, key) in [
            (LineStyle::Solid, "stroke.style.solid"),
            (LineStyle::Dashed, "stroke.style.dashed"),
            (LineStyle::Dotted, "stroke.style.dotted"),
        ] {
            if crate::stroke_panel::line_style_button(ui, 24.0, style, current == Some(style), text(key))
                .labelled_by(caption).clicked()
            {
                chosen = Some(style);
            }
        }
        chosen
    })
}

fn property_row<R>(ui: &mut Ui, key: &str, control_width: f32, controls: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    let label = text(key);
    let available = ui.available_width();
    let caption_width = dtool_design::size("panel.label_width").min(available / 3.0);
    let text_width = ui.painter().layout_no_wrap(
        label.to_owned(), egui::TextStyle::Body.resolve(ui.style()), ui.visuals().text_color(),
    ).size().x;
    if text_width > caption_width || caption_width + ui.spacing().item_spacing.x + control_width > available {
        let caption = ui.label(label).id;
        controls(ui, caption)
    } else {
        dtool_design::studio_row(ui, label, controls)
    }
}

fn option_row<R>(ui: &mut Ui, key: &str, controls: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    property_row(ui, key, 100.0, |ui, caption| ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.spacing_mut().button_padding.x = 3.0;
        controls(ui, caption)
    }).inner)
}

fn border_alignment(ui: &mut Ui, current: Option<StrokeAlign>) -> Option<StrokeAlign> {
    option_row(ui, "table.panel.border_alignment", |ui, caption| {
        let mut changed = None;
        for (align, label) in [
            (StrokeAlign::Inside, "stroke.align.inside"),
            (StrokeAlign::Centre, "stroke.align.centre"),
            (StrokeAlign::Outside, "stroke.align.outside"),
        ] {
            let label = text(label);
            let selected = current == Some(align);
            let button = dtool_design::studio_icon_button(ui, egui::Button::new(""), 24.0, selected)
                .labelled_by(caption).on_hover_text(label);
            let ink = if selected && button.enabled() { dtool_design::selected_icon_ink(ui) }
                else { ui.style().interact(&button).fg_stroke.color };
            crate::stroke_panel::paint_align(ui.painter(), button.rect.center(), ink, align);
            button.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, button.enabled(), selected, label));
            if button.clicked() && !selected { changed = Some(align); }
        }
        changed
    })
}

fn wrapped_toggle(ui: &mut Ui, value: &mut bool, key: &str, mixed: bool) -> bool {
    let label = text(key);
    let state = ControlState::default().mixed(mixed);
    let label_width = ui.painter().layout_no_wrap(
        label.to_owned(), egui::TextStyle::Button.resolve(ui.style()), ui.visuals().text_color(),
    ).size().x;
    let switch_width = dtool_design::size("control.switch.width") + dtool_design::space("sm");
    if label_width + switch_width <= ui.available_width() {
        return dtool_design::toggle_with(ui, value, label, &state).changed();
    }
    ui.vertical(|ui| {
        let caption = ui.add(egui::Label::new(label).wrap());
        let response = dtool_design::toggle_with(ui, value, "", &state)
            .labelled_by(caption.id).on_hover_text(label);
        response.widget_info(|| {
            if mixed && !response.changed() {
                egui::WidgetInfo::labeled(egui::Role::CheckBox, ui.is_enabled(), label)
            } else {
                egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), *value, label)
            }
        });
        response.changed()
    }).inner
}

pub(super) fn baseline_controls(ui: &mut Ui, current: &dtool_core::table::model::BaselineGrid) -> Option<dtool_core::table::model::BaselineGrid> {
    use dtool_core::table::model::BaselineOrigin;
    egui::CollapsingHeader::new(text("table.frame.baseline")).default_open(current.enabled).show(ui, |ui| {
        let mut value = current.clone();
        let mut changed = wrapped_toggle(ui, &mut value.enabled, "table.frame.use_grid", false);
        changed |= grid_number(ui, "table.frame.baseline_start", &mut value.start_pt, -36000.0);
        if let Some(origin) = choice(ui, "table.frame.baseline_origin", Some(value.relative_to), &[
            (BaselineOrigin::Frame, text("table.frame.origin_frame")),
            (BaselineOrigin::Page, text("table.frame.origin_page")),
            (BaselineOrigin::Spread, text("table.frame.origin_spread")),
        ]) {
            value.relative_to = origin;
            changed = true;
        }
        changed |= grid_number(ui, "table.frame.baseline_spacing", &mut value.spacing_pt, 0.01);
        property_row(ui, "table.frame.baseline_colour", dtool_design::size("icon.size.lg"), |ui, label| {
            let parsed = value.color.is_ascii().then(|| dtool_core::paint::paint_from_legacy_str(&value.color)).flatten();
            let mut colour = match parsed {
                Some(Paint::Solid { colour }) => colour,
                _ => {
                    let [r, g, b, a] = dtool_design::color("accent").to_srgba_unmultiplied();
                    Colour::new(r, g, b, a)
                }
            };
            let before = colour;
            picker_button(ui, &mut colour).labelled_by(label)
                .on_hover_text(text("table.frame.baseline_colour"));
            if colour != before {
                let Colour { r, g, b, a, .. } = colour;
                value.color = format!("#{r:02x}{g:02x}{b:02x}{a:02x}");
                changed = true;
            }
        });
        changed.then_some(value)
    }).body_returned.flatten()
}

fn grid_number(ui: &mut Ui, key: &str, number: &mut f32, min: f64) -> bool {
    let changed = property_row(ui, key, 60.0, |ui, label| {
        let spec = dtool_design::NumericSpec::new("").range(min..=36000.0).suffix("pt").width(60.0);
        committed_number(ui, key, label, Some(*number), &spec)
    });
    if let Some(value) = changed { *number = value; }
    changed.is_some()
}

pub(crate) fn baseline_colour(value: &str) -> Color32 {
    let paint = value.is_ascii().then(|| dtool_core::paint::paint_from_legacy_str(value)).flatten();
    paint.as_ref().and_then(crate::fill_stroke::paint_well_colour).unwrap_or_else(|| dtool_design::color("accent"))
}

pub(super) fn choice<T: Copy + Default + PartialEq>(ui: &mut Ui, key: &str, value: Option<T>, options: &[(T, &str)]) -> Option<T> {
    property_row(ui, key, 106.0, |ui, label| {
        let shown = options.iter().find_map(|(option, label)| (Some(*option) == value).then_some(*label))
            .unwrap_or(dtool_design::MIXED_TEXT);
        let mut chosen = None;
        egui::ComboBox::from_id_salt(key).width(ui.available_width().max(1.0).min(130.0))
            .selected_text(shown).truncate().show_ui(ui, |ui| {
                dtool_design::with_icon_selection(ui, |ui| {
                    for &(option, caption) in options {
                        if ui.selectable_label(Some(option) == value, caption).clicked() {
                            chosen = Some(option);
                        }
                    }
                });
            }).response.labelled_by(label).on_hover_text(shown);
        chosen
    })
}

pub(super) fn scalar(ui: &mut Ui, key: &str, value: Option<f32>, suffix: &str) -> Option<f32> {
    property_row(ui, key, 62.0, |ui, label| {
        let spec = dtool_design::NumericSpec::new("")
            .range(0.0..=f64::from(f32::MAX))
            .width(ui.available_width().clamp(32.0, 62.0))
            .suffix(suffix);
        committed_number(ui, key, label, value, &spec)
    })
}

fn committed_number(ui: &mut Ui, key: &str, label: egui::Id, value: Option<f32>, spec: &dtool_design::NumericSpec<'_>) -> Option<f32> {
    let value = value.map_or(dtool_design::NumericValue::Mixed, |number| dtool_design::NumericValue::Value(number.into()));
    let (response, event) = dtool_design::numeric_field(ui, ui.make_persistent_id(key), value, spec);
    let response = response.labelled_by(label);
    match event {
        dtool_design::NumericEvent::Commit(number) if response.changed() => Some(number as f32),
        _ => None,
    }
}

pub(super) fn paint_choice(ui: &mut Ui, key: &str, paint: Option<&Paint>, draft: &mut Hsva) -> Option<Paint> {
    paint_popup(ui, key, paint, draft, true, |_, _| {})
}

pub(super) fn paint_choice_with(ui: &mut Ui, key: &str, paint: Option<&Paint>, draft: &mut Hsva, draw: impl FnOnce(&Ui, &Response)) -> Option<Paint> {
    paint_popup(ui, key, paint, draft, false, draw)
}

fn paint_popup(ui: &mut Ui, key: &str, paint: Option<&Paint>, draft: &mut Hsva, live: bool,
    draw: impl FnOnce(&Ui, &Response)) -> Option<Paint> {
    property_row(ui, key, dtool_design::size("icon.size.lg"), |ui, label| {
        let colour = paint.and_then(crate::fill_stroke::paint_well_colour);
        let value = if paint.is_none() { WellValue::Mixed } else { colour.map_or(WellValue::None, WellValue::Colour) };
        let outline = live && key == "table.panel.border_colour";
        let well = if live {
            dtool_design::colour_well_sized(ui, value, key, &ControlState::default(),
                egui::vec2(ui.available_width().clamp(25.0, 80.0), 24.0), outline)
        } else {
            dtool_design::colour_well_with(ui, value, key, &ControlState::default())
        }.labelled_by(label).on_hover_text(text(key));
        draw(ui, &well);
        if well.clicked() {
            if let Some(colour) = paint.and_then(dtool_core::scene::representative_colour) {
                *draft = picker_hsva(colour);
            }
        }
        let mut chosen = None;
        egui::Popup::menu(&well).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            let width = (ui.ctx().content_rect().width() - 24.0).clamp(120.0, 276.0);
            ui.set_width(width);
            ui.spacing_mut().slider_width = width;
            if egui::color_picker::color_picker_hsva_2d(ui, draft, egui::color_picker::Alpha::OnlyBlend) && live {
                chosen = Some(Paint::Solid { colour: picker_colour(*draft) });
            }
            ui.horizontal(|ui| {
                let finish = if live { "button.close" } else { "button.apply" };
                if dtool_design::button(ui, text(finish)).clicked() {
                    if !live { chosen = Some(Paint::Solid { colour: picker_colour(*draft) }); }
                    ui.close();
                }
                if dtool_design::button(ui, text("paint.none")).clicked() {
                    chosen = Some(Paint::None);
                    ui.close();
                }
            });
        });
        chosen
    })
}

fn edge_button(ui: &mut Ui, mask: &Edge, key: &str, selected: bool, width: f32) -> Response {
    let label = text(key);
    let response = dtool_design::with_icon_selection(ui, |ui| {
        let height = dtool_design::size("control.icon.height");
        ui.add_sized([width, height], egui::Button::new("").selected(selected)).on_hover_text(label)
    });
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
    let box_rect = egui::Rect::from_center_size(response.rect.center(), egui::vec2(14.0, 14.0));
    let left = box_rect.left_center();
    let right = box_rect.right_center();
    let top = box_rect.center_top();
    let bottom = box_rect.center_bottom();
    let segments = [
        [box_rect.left_top(), box_rect.left_bottom()], [box_rect.right_top(), box_rect.right_bottom()],
        [top, bottom], [box_rect.left_top(), box_rect.right_top()],
        [box_rect.left_bottom(), box_rect.right_bottom()], [left, right],
    ];
    let foreground = if selected && response.enabled() { dtool_design::selected_icon_ink(ui) }
        else { ui.style().interact(&response).fg_stroke.color };
    for (index, segment) in segments.into_iter().enumerate() {
        let chosen = mask.0 & (1 << index) != 0;
        let stroke = if chosen { egui::Stroke::new(1.5, foreground) }
            else { egui::Stroke::new(0.75, foreground.gamma_multiply(0.3)) };
        ui.painter().line_segment(segment, stroke);
    }
    response
}
