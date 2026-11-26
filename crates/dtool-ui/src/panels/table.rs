use dtool_core::paint::{Colour, LineStyle, Paint};
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
    frame_colour: Hsva,
    frame_width: f64,
    fill_colour: Hsva,
    error: Option<String>,
    icons: crate::icon_cache::IconCache,
}

impl Default for TablePanel {
    fn default() -> Self {
        Self {
            border_mask: Edge::OUTSIDE, border_colour: Hsva::from_srgb([0; 3]),
            frame_colour: Hsva::from_srgb([0; 3]), frame_width: 1.0,
            fill_colour: Hsva::from_srgb([255; 3]), error: None, icons: Default::default(),
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

impl TablePanel {
    pub fn show(&mut self, ui: &mut Ui, session: Option<&mut Session>) {
        dtool_design::panel_frame(ui, |ui| {
            dtool_design::panel_header(ui, text("panel.table"));
            let Some(session) = session else {
                ui.label(text("table.panel.select_table"));
                return;
            };
            let current = if session.active_tool == "tool.table" { selection::current(session) } else { selection::TableSelection::default() };
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
                let tracks = appearance.as_ref().and_then(|_| {
                    egui::CollapsingHeader::new(text("table.panel.tracks")).default_open(true)
                        .show(ui, |ui| self.track_controls(ui, session, table, &current, id, suffix))
                        .body_returned.flatten()
                });
                let frame = egui::CollapsingHeader::new(text("table.panel.frame"))
                    .default_open(true).show(ui, |ui| self.frame_controls(ui, &frame, suffix)).body_returned.flatten();
                let stroke_fill = egui::CollapsingHeader::new(text("table.panel.stroke_fill"))
                    .default_open(true).show(ui, |ui| match &appearance {
                        Some(appearance) => self.cell_stroke_fill(ui, appearance, table, &session.document, &ids),
                        None => { ui.label(text("table.panel.select_cells")); None }
                    }).body_returned.flatten();
                let cells = egui::CollapsingHeader::new(text("table.panel.cells"))
                    .default_open(true).show(ui, |ui| match &appearance {
                        Some(appearance) => self.cell_controls(ui, appearance, suffix),
                        None => { ui.label(text("table.panel.select_cells")); None }
                    }).body_returned.flatten();
                frame.map(|patch| Command::with_params("table.format.set", json!({"id":id.0,"frame":patch})))
                    .or_else(|| stroke_fill.or(cells).map(|mut patch| {
                        patch["id"] = json!(id.0);
                        patch["cells"] = json!(current.cells);
                        Command::with_params(Command::TABLE_CELL_FORMAT_SET, patch)
                    })).or(tracks)
            }).inner;
            if let Some(command) = command {
                self.error = session.run(&command).err()
                    .map(|error| text(&error.message_key).to_string());
            }
            if let Some(error) = &self.error { ui.label(error); }
        });
    }

    fn track_controls(&mut self, ui: &mut Ui, session: &Session, table: &TableNode,
        current: &selection::TableSelection, id: dtool_core::ObjectId, suffix: &str) -> Option<Command> {
        let mut command = None;
        for (rows, axis, label, tracks, menu) in [
            (false, "column", "table.panel.cell_width", &table.col_widths_pt, [
                ("text.table.autofit_columns", "ctx-zoom-fit"),
                ("text.table.evenly_distribute_columns", "tool-distribute-h"),
            ]),
            (true, "row", "table.panel.cell_height", &table.row_heights_pt, [
                ("text.table.autofit_rows", "ctx-zoom-fit"),
                ("text.table.evenly_distribute_rows", "tool-distribute-v"),
            ]),
        ] {
            let indices = current.track_indices(table, rows);
            let Some(&first) = indices.first().and_then(|index| tracks.get(*index as usize)) else { continue; };
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
        }
        command
    }

    fn frame_controls(&mut self, ui: &mut Ui, frame: &TableFrameFormat, suffix: &str) -> Option<Value> {
        let none = Paint::None;
        let fill = frame.fill.as_ref().unwrap_or(&none);
        let mut patch = paint_choice(ui, "table.panel.frame_fill", Some(fill), &mut self.fill_colour)
            .map(|paint| json!({"fill":paint}));
        let stroke = frame.edges.each_ref().map(|edge| edge.as_ref().map_or(&none, |edge| &edge.paint));
        let mut frame_stroke = Some(stroke[0]);
        for paint in stroke.into_iter().skip(1) { common(&mut frame_stroke, paint); }
        let paint = paint_choice(ui, "table.panel.frame_outline", frame_stroke, &mut self.frame_colour);
        let active = frame.edges.each_ref().map(|edge| edge.as_ref().filter(|edge| edge.width_pt > 0.0 && !matches!(edge.paint, Paint::None)));
        let widths = active.map(|edge| edge.map_or(0.0, |edge| edge.width_pt));
        let mut width = Some(widths[0]);
        for value in widths.into_iter().skip(1) { common(&mut width, value); }
        let new_width = scalar(ui, "table.panel.frame_width", width, "pt");
        if let Some(value) = new_width {
            self.frame_width = if value > 0.0 { f64::from(value) } else { 1.0 };
        }
        let mut styles = active.iter().flatten().map(|edge| edge.line_style);
        let style = styles.next().unwrap_or_default();
        let new_style = stroke_pattern(ui, styles.all(|entry| entry == style).then_some(style));
        if paint.is_some() || new_width.is_some() || new_style.is_some() {
            let fallback = stroke.iter().find(|paint| !matches!(paint, Paint::None))
                .map(|paint| (*paint).clone()).unwrap_or_else(|| Paint::Solid { colour: picker_colour(self.frame_colour) });
            let has_edge = active.iter().any(Option::is_some);
            let edges = ["left", "top", "right", "bottom"].into_iter().zip(active).filter_map(|(side, edge)| {
                if edge.is_none() && has_edge && paint.is_none() && new_width.is_none() { return None; }
                let width_pt = new_width.unwrap_or_else(|| edge.map_or(self.frame_width as f32, |edge| edge.width_pt));
                let paint = paint.clone().unwrap_or_else(|| edge.map_or_else(|| fallback.clone(), |edge| edge.paint.clone()));
                let line_style = new_style.unwrap_or_else(|| edge.map_or(style, |edge| edge.line_style));
                let value = (width_pt > 0.0 && !matches!(paint, Paint::None)).then_some(EdgeStyle { paint, width_pt, line_style });
                Some((side.into(), json!(value)))
            }).collect::<serde_json::Map<_, _>>();
            patch.get_or_insert_with(|| json!({}))["edges"] = json!(edges);
        }
        if let Some(insets) = frame_insets(ui, frame.insets, suffix) {
            patch.get_or_insert_with(|| json!({}))["insets"] = json!(insets);
        }
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
        let width = if ui.available_width() < 148.0 { 21.0 } else { 25.0 };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for (mask, key) in BORDERS {
                if edge_button(ui, &mask, key, self.border_mask == mask, width).clicked() {
                    self.border_mask = mask;
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
        let parts = |edge: Option<EdgeStyle>| edge.map_or((Paint::None, 0.0, LineStyle::Solid), |edge| {
            (edge.paint, edge.width_pt, edge.line_style)
        });
        let (paint, width, style) = parts(first);
        let (mut paint, mut width, mut style) = (Some(paint), Some(width), Some(style));
        for edge in edges {
            let (next_paint, next_width, next_style) = parts(edge);
            common(&mut paint, next_paint);
            common(&mut width, next_width);
            common(&mut style, next_style);
        }
        let mut edge_patch = serde_json::Map::new();
        if let Some(paint) = paint_choice(ui, "table.panel.border_colour", paint.as_ref(), &mut self.border_colour) {
            edge_patch.insert("paint".into(), json!(paint));
        }
        if let Some(width) = ui.push_id(self.border_mask.0, |ui| scalar(ui, "stroke.width", width, "pt")).inner {
            edge_patch.insert("width_pt".into(), json!(width));
        }
        if let Some(style) = stroke_pattern(ui, style) {
            edge_patch.insert("line_style".into(), json!(style));
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
                let button = egui::Button::new(label).small().selected(selected);
                if ui.add(button).labelled_by(caption).clicked() { chosen = Some(rotation); }
            }
            chosen
        }) {
            patch = Some(json!({"rotation":rotation}));
        }
        ui.separator();
        ui.label(text("table.panel.insets"));
        for (index, (side, key)) in [
            ("left", "table.inset.left"), ("top", "table.inset.top"),
            ("right", "table.inset.right"), ("bottom", "table.inset.bottom"),
        ].into_iter().enumerate() {
            if let Some(value) = scalar(ui, key, state.insets[index], suffix) {
                patch = Some(json!({"inset_patch":{side:value}}));
            }
        }
        let mut linked = state.linked.unwrap_or_default();
        if wrapped_toggle(ui, &mut linked, "table.panel.link_insets", state.linked.is_none()) {
            patch = Some(json!({"inset_patch":{"linked":linked}}));
        }
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

fn option_row<R>(ui: &mut Ui, key: &str, controls: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    let choices = |ui: &mut Ui, caption| ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.spacing_mut().button_padding.x = 3.0;
        controls(ui, caption)
    }).inner;
    if ui.available_width() < 160.0 {
        let caption = ui.label(text(key)).id;
        choices(ui, caption)
    } else {
        dtool_design::studio_row(ui, text(key), choices)
    }
}

fn wrapped_toggle(ui: &mut Ui, value: &mut bool, key: &str, mixed: bool) -> bool {
    ui.scope(|ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        dtool_design::toggle_with(ui, value, text(key), &ControlState::default().mixed(mixed)).clicked()
    }).inner
}

pub(super) fn frame_insets(ui: &mut Ui, current: dtool_core::table::model::Insets, suffix: &str) -> Option<dtool_core::table::model::Insets> {
    egui::CollapsingHeader::new(text("table.frame.insets")).show(ui, |ui| {
        let mut value = current;
        let mut changed = false;
        for (index, (side, key)) in [
            (current.left, "table.inset.left"), (current.top, "table.inset.top"),
            (current.right, "table.inset.right"), (current.bottom, "table.inset.bottom"),
        ].into_iter().enumerate() {
            if let Some(next) = scalar(ui, key, Some(side), suffix).filter(|next| *next >= 0.0 && *next != side) {
                value = current.with_side(index, next);
                changed = true;
            }
        }
        changed |= wrapped_toggle(ui, &mut value.linked, "table.panel.link_insets", false);
        changed.then_some(value)
    }).body_returned.flatten()
}

pub(super) fn baseline_controls(ui: &mut Ui, current: &dtool_core::table::model::BaselineGrid) -> Option<dtool_core::table::model::BaselineGrid> {
    use dtool_core::table::model::BaselineOrigin;
    egui::CollapsingHeader::new(text("table.frame.baseline")).show(ui, |ui| {
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
        dtool_design::studio_row(ui, text("table.frame.baseline_colour"), |ui, label| {
            let parsed = value.color.is_ascii().then(|| dtool_core::paint::paint_from_legacy_str(&value.color)).flatten();
            let mut colour = match parsed {
                Some(Paint::Solid { colour }) => colour,
                _ => {
                    let [r, g, b, a] = dtool_design::color("accent").to_srgba_unmultiplied();
                    Colour::new(r, g, b, a)
                }
            };
            let before = colour;
            picker_button(ui, &mut colour).labelled_by(label);
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
    let changed = option_row(ui, key, |ui, label| {
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
    option_row(ui, key, |ui, label| {
        let shown = options.iter().find_map(|(option, label)| (Some(*option) == value).then_some(*label))
            .unwrap_or(dtool_design::MIXED_TEXT);
        let mut chosen = None;
        egui::ComboBox::from_id_salt(key).width(ui.available_width().max(1.0).min(130.0))
            .selected_text(shown).truncate().show_ui(ui, |ui| {
                for &(option, caption) in options {
                    if ui.selectable_label(Some(option) == value, caption).clicked() {
                        chosen = Some(option);
                    }
                }
            }).response.labelled_by(label).on_hover_text(shown);
        chosen
    })
}

pub(super) fn scalar(ui: &mut Ui, key: &str, value: Option<f32>, suffix: &str) -> Option<f32> {
    dtool_design::studio_row(ui, text(key), |ui, label| {
        let spec = dtool_design::NumericSpec::new("").range(0.0..=f64::from(f32::MAX)).suffix(suffix);
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
    paint_choice_with(ui, key, paint, draft, |_, _| {})
}

pub(super) fn paint_choice_with(ui: &mut Ui, key: &str, paint: Option<&Paint>, draft: &mut Hsva, draw: impl FnOnce(&Ui, &Response)) -> Option<Paint> {
    dtool_design::studio_row(ui, text(key), |ui, label| {
        let colour = paint.and_then(crate::fill_stroke::paint_well_colour);
        let value = if paint.is_none() { WellValue::Mixed } else { colour.map_or(WellValue::None, WellValue::Colour) };
        let well = dtool_design::colour_well_with(ui, value, key, &ControlState::default()).labelled_by(label);
        draw(ui, &well);
        if well.clicked() {
            if let Some(colour) = paint.and_then(dtool_core::scene::representative_colour) {
                *draft = picker_hsva(colour);
            }
        }
        let mut chosen = None;
        egui::Popup::menu(&well).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            egui::color_picker::color_picker_hsva_2d(ui, draft, egui::color_picker::Alpha::OnlyBlend);
            ui.horizontal(|ui| {
                if dtool_design::button(ui, text("button.apply")).clicked() {
                    chosen = Some(Paint::Solid { colour: picker_colour(*draft) });
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
        ui.add_sized([width, 23.0], egui::Button::new("").selected(selected)).on_hover_text(label)
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
