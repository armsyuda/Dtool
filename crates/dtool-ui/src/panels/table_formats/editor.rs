use dtool_core::paint::Paint;
use dtool_core::table::model::{CellRotation, EdgeStyle, InitialAdvance, Insets, TableFormatDef, TableFormatId, TablePattern, VerticalAlign};
use dtool_core::{Command, CommandError, Document, Session};
use dtool_design::button;
use dtool_strings::text;
use egui::{Color32, Rect, Sense, Stroke, Ui, Vec2};
use egui::ecolor::Hsva;
use serde_json::json;

use crate::dialog_chrome::{show_modal, ClosePolicy, DialogResult};
use crate::panels::table::{baseline_controls, choice, frame_insets, paint_choice_with, scalar, stroke_pattern};
use super::images::Images;

pub(super) enum Outcome {
    Open,
    Saved(TableFormatId),
    Cancelled,
}

pub(super) struct FormatEditor {
    draft: TableFormatDef,
    original: Option<serde_json::Value>,
    row: u16,
    col: u16,
    style: u16,
    colour: Hsva,
    outline: Hsva,
    error: Option<String>,
}

impl Default for FormatEditor {
    fn default() -> Self {
        Self {
            draft: TableFormatDef {
                id: TableFormatId::default(), name: String::new(), frame: Default::default(),
                cell_formats: vec![(0, text("table.advance.default").into(), Default::default())],
                pattern: TablePattern { rows: 3, cols: 3, top_band_rows: 0, bottom_band_rows: 0, left_band_cols: 0, right_band_cols: 0, slots: vec![0; 9] },
            },
            original: None, row: 0, col: 0, style: 0, colour: Hsva::from_srgb([255; 3]), outline: Hsva::from_srgb([0; 3]), error: None,
        }
    }
}

impl FormatEditor {
    pub(super) fn existing(definition: &TableFormatDef) -> Result<Self, CommandError> {
        dtool_core::table::styles::validate_definition(definition)?;
        Ok(Self { draft: definition.clone(), original: Some(json!(definition)), style: definition.pattern.slots[0], ..Self::default() })
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, session: &mut Session, images: &mut Images) -> Outcome {
        let units = dtool_core::commands::ruler_units::suffix(session.document.units);
        let paragraphs = session.document.text_styles.names(dtool_core::text_styles::StyleKind::Paragraph);
        let (event, (save, cancel)) = show_modal(ctx, "table-format-editor", text("table.formats.edit"), ClosePolicy::CancelOnly, false, |ui| {
            let bounds = ui.ctx().content_rect();
            ui.set_width((bounds.width() - 36.0).clamp(220.0, 740.0));
            egui::ScrollArea::vertical().max_height((bounds.height() - 110.0).max(180.0)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(text("table.formats.name"));
                    ui.add(egui::TextEdit::singleline(&mut self.draft.name).desired_width(230.0));
                });
                ui.separator();
                if ui.available_width() >= 565.0 {
                    ui.columns(2, |columns| {
                        self.structure(&mut columns[0], units, &session.document, images);
                        self.cell_controls(&mut columns[1], units, &paragraphs, &session.document, images);
                    });
                } else {
                    self.structure(ui, units, &session.document, images);
                    self.cell_controls(ui, units, &paragraphs, &session.document, images);
                }
            });
            let name = self.draft.name.trim();
            let conflict = session.document.table_formats.iter().any(|format| {
                format.name == name && (self.original.is_none() || format.id != self.draft.id)
            });
            if conflict { ui.label(text("table.formats.name_conflict")); }
            if let Some(error) = &self.error { ui.label(text(error)); }
            ui.horizontal(|ui| {
                let save = ui.add_enabled(!name.is_empty() && !conflict, egui::Button::new(text("button.ok"))).clicked();
                let cancel = button(ui, text("button.cancel")).clicked();
                (save, cancel)
            }).inner
        });
        if cancel || event == DialogResult::Cancel { return Outcome::Cancelled; }
        if !save { return Outcome::Open; }
        if let Some(original) = &self.original {
            let current = session.document.table_formats.iter().find(|format| format.id == self.draft.id).map(|format| json!(format));
            if current.as_ref() != Some(original) {
                self.error = Some("table.formats.changed".into());
                return Outcome::Open;
            }
            if json!(self.draft) == *original { return Outcome::Saved(self.draft.id); }
        }
        let op = if self.original.is_none() { "create" } else { "update" };
        let command = Command::with_params("style.table.define", json!({
            "op": op, "name": self.draft.name.trim(), "definition": self.draft
        }));
        match session.run(&command) {
            Ok(result) => Outcome::Saved(TableFormatId(result["id"].as_u64().unwrap_or(self.draft.id.0))),
            Err(error) => { self.error = Some(error.message_key); Outcome::Open }
        }
    }

    fn structure(&mut self, ui: &mut Ui, units: &str, doc: &Document, images: &mut Images) {
        ui.label(egui::RichText::new(text("table.formats.structure")).strong());
        let pattern = &mut self.draft.pattern;
        let mut rows = pattern.rows;
        let mut cols = pattern.cols;
        let max = dtool_core::table::commands::MAX_ROWS_COLS as u16;
        ui.horizontal_wrapped(|ui| {
            count(ui, "table.formats.rows", &mut rows, 1..=max);
            count(ui, "table.formats.columns", &mut cols, 1..=max);
        });
        resize(pattern, rows, cols);
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols - 1);
        ui.horizontal_wrapped(|ui| {
            count(ui, "table.formats.header", &mut pattern.top_band_rows, 0..=rows - pattern.bottom_band_rows);
            count(ui, "table.formats.footer", &mut pattern.bottom_band_rows, 0..=rows - pattern.top_band_rows);
        });
        if rows > 8 || cols > 8 {
            ui.horizontal_wrapped(|ui| {
                let (mut row, mut col) = (self.row + 1, self.col + 1);
                count(ui, "table.formats.row", &mut row, 1..=rows);
                count(ui, "table.formats.column", &mut col, 1..=cols);
                if (row - 1, col - 1) != (self.row, self.col) {
                    (self.row, self.col) = (row - 1, col - 1);
                    self.style = pattern.slots[slot_index(pattern, self.row, self.col)];
                }
            });
        }
        self.pattern_grid(ui, doc, images);
        ui.separator();
        ui.label(egui::RichText::new(text("table.panel.frame")).strong());
        ui.push_id("frame", |ui| {
            let frame = &mut self.draft.frame;
            paint_controls(ui, &mut frame.fill, &mut frame.edges, &mut self.colour, &mut self.outline, doc, images);
            if let Some(insets) = frame_insets(ui, frame.insets, units) {
                frame.insets = insets;
            }
            dtool_design::toggle(ui, &mut frame.ignore_baseline_grid, text("table.frame.ignore_grid"));
            if let Some(baseline) = baseline_controls(ui, &frame.baseline.clone().unwrap_or_default()) {
                frame.baseline = Some(baseline);
            }
        });
    }

    fn pattern_grid(&mut self, ui: &mut Ui, doc: &Document, images: &mut Images) {
        let pattern = &self.draft.pattern;
        let rows = pattern.rows.min(8);
        let cols = pattern.cols.min(8);
        let row_at = self.row.saturating_sub(3).min(pattern.rows - rows);
        let col_at = self.col.saturating_sub(3).min(pattern.cols - cols);
        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(340.0), 145.0), Sense::click());
        let step = egui::vec2(rect.width() / f32::from(cols), rect.height() / f32::from(rows));
        ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
        images.fill(ui.painter(), rect, self.draft.frame.fill.as_ref(), doc);
        let slots: std::collections::HashMap<_, _> = self.draft.cell_formats.iter().map(|(id, _, style)| (*id as u16, style)).collect();
        for row in 0..rows {
            for col in 0..cols {
                let pos = rect.min + egui::vec2(f32::from(col) * step.x, f32::from(row) * step.y);
                let cell_rect = Rect::from_min_size(pos, step);
                let at = (row_at + row, col_at + col);
                let id = pattern.slots[slot_index(pattern, at.0, at.1)];
                let style = slots[&id];
                images.fill(ui.painter(), cell_rect, style.fill.as_ref(), doc);
                ui.painter().rect_stroke(cell_rect, 0.0, Stroke::new(0.7, Color32::from_gray(130)), egui::StrokeKind::Inside);
                super::paint_edges(ui.painter(), cell_rect, &style.edges);
                if at == (self.row, self.col) {
                    ui.painter().rect_stroke(cell_rect, 0.0, Stroke::new(2.0, dtool_design::color("accent")), egui::StrokeKind::Inside);
                }
            }
        }
        super::paint_edges(ui.painter(), rect, &self.draft.frame.edges);
        if let Some(pos) = response.interact_pointer_pos().filter(|_| response.clicked()) {
            self.row = row_at + (((pos.y - rect.top()) / step.y).floor() as u16).min(rows - 1);
            self.col = col_at + (((pos.x - rect.left()) / step.x).floor() as u16).min(cols - 1);
            self.style = pattern.slots[slot_index(pattern, self.row, self.col)];
        }
    }

    fn cell_controls(&mut self, ui: &mut Ui, units: &str, paragraphs: &[String], doc: &Document, images: &mut Images) {
        ui.label(egui::RichText::new(text("table.formats.cell_styles")).strong());
        let selected = self.draft.cell_formats.iter().find(|(id, _, _)| *id == u64::from(self.style)).map(|(_, name, _)| name.as_str()).unwrap_or("");
        egui::ComboBox::from_id_salt("table-format-cell").selected_text(selected).show_ui(ui, |ui| {
            for (id, name, _) in &self.draft.cell_formats {
                ui.selectable_value(&mut self.style, *id as u16, name);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if button(ui, text("table.formats.assign")).clicked() {
                let index = slot_index(&self.draft.pattern, self.row, self.col);
                self.draft.pattern.slots[index] = self.style;
            }
            if button(ui, text("table.formats.add_cell")).clicked() {
                let used: std::collections::HashSet<_> = self.draft.cell_formats.iter().map(|(id, _, _)| *id).collect();
                if let Some(id) = (0..=u64::from(u16::MAX)).find(|id| !used.contains(id)) {
                    self.draft.cell_formats.push((id, format!("{} {}", text("table.formats.cell"), id + 1), Default::default()));
                    self.style = id as u16;
                }
            }
            let unused = !self.draft.pattern.slots.contains(&self.style);
            if ui.add_enabled(unused, egui::Button::new(text("button.delete"))).clicked() {
                self.draft.cell_formats.retain(|(id, _, _)| *id != u64::from(self.style));
                self.style = self.draft.pattern.slots[slot_index(&self.draft.pattern, self.row, self.col)];
            }
        });
        let Some((_, name, cell)) = self.draft.cell_formats.iter_mut().find(|(id, _, _)| *id == u64::from(self.style)) else { return; };
        ui.push_id(("cell", self.style), |ui| {
            ui.horizontal(|ui| {
                ui.label(text("table.formats.cell_name"));
                ui.add(egui::TextEdit::singleline(name).desired_width(140.0));
            });
            paint_controls(ui, &mut cell.fill, &mut cell.edges, &mut self.colour, &mut self.outline, doc, images);
            egui::CollapsingHeader::new(text("table.panel.insets")).show(ui, |ui| insets(ui, &mut cell.insets, units));
            if let Some(value) = choice(ui, "table.panel.vertical_align", Some(cell.vertical_align), &[
                (VerticalAlign::Top, text("table.align.top")), (VerticalAlign::Center, text("table.align.center")),
                (VerticalAlign::Bottom, text("table.align.bottom")), (VerticalAlign::Justify, text("table.align.justify")),
            ]) { cell.vertical_align = value; }
            if let Some(value) = choice(ui, "table.panel.rotation", Some(cell.rotation), &[
                (CellRotation::R0, "0°"), (CellRotation::R90, "90°"), (CellRotation::R180, "180°"), (CellRotation::R270, "270°"),
            ]) { cell.rotation = value; }
            if let Some(value) = choice(ui, "table.panel.initial_advance", Some(cell.initial_advance), &[
                (InitialAdvance::Default, text("table.advance.default")), (InitialAdvance::Ascent, text("table.advance.ascent")),
                (InitialAdvance::CapHeight, text("table.advance.cap")), (InitialAdvance::XHeight, text("table.advance.x")),
                (InitialAdvance::Leading, text("table.advance.leading")), (InitialAdvance::Fixed, text("table.advance.fixed")),
            ]) { cell.initial_advance = value; }
            if let Some(value) = scalar(ui, "table.panel.minimum", Some(cell.minimum_pt), "pt") { cell.minimum_pt = value; }
            dtool_design::studio_row(ui, text("table.formats.paragraph"), |ui, label| {
                egui::ComboBox::from_id_salt("table-format-paragraph")
                    .selected_text(cell.paragraph_style.as_deref().unwrap_or(text("paint.none")))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut cell.paragraph_style, None, text("paint.none"));
                        for name in paragraphs { ui.selectable_value(&mut cell.paragraph_style, Some(name.clone()), name); }
                    }).response.labelled_by(label);
            });
        });
    }
}

fn count(ui: &mut Ui, key: &str, value: &mut u16, range: std::ops::RangeInclusive<u16>) {
    let label = ui.label(text(key));
    ui.add(egui::DragValue::new(value).range(range).update_while_editing(false)).labelled_by(label.id);
}

fn slot_index(pattern: &TablePattern, row: u16, col: u16) -> usize {
    usize::from(row) * usize::from(pattern.cols) + usize::from(col)
}

fn resize(pattern: &mut TablePattern, rows: u16, cols: u16) {
    if (rows, cols) == (pattern.rows, pattern.cols) { return; }
    let slots = (0..rows).flat_map(|row| (0..cols).map(move |col| (row, col)))
        .map(|(row, col)| {
            dtool_core::table::resolve::pattern_slot(pattern, rows.into(), cols.into(), row.into(), col.into())
                .unwrap_or(pattern.slots[0])
        })
        .collect();
    (pattern.rows, pattern.cols, pattern.slots) = (rows, cols, slots);
    pattern.top_band_rows = pattern.top_band_rows.min(rows);
    pattern.bottom_band_rows = pattern.bottom_band_rows.min(rows - pattern.top_band_rows);
    pattern.left_band_cols = pattern.left_band_cols.min(cols);
    pattern.right_band_cols = pattern.right_band_cols.min(cols - pattern.left_band_cols);
}

fn paint_controls(ui: &mut Ui, fill: &mut Option<Paint>, edges: &mut [Option<EdgeStyle>; 4], colour: &mut Hsva, outline: &mut Hsva, doc: &Document, images: &mut Images) {
    let none = Paint::None;
    let at_fill = Some(fill.as_ref().unwrap_or(&none));
    if let Some(paint) = texture_choice(ui, "table.panel.frame_fill", at_fill, colour, doc, images) { *fill = Some(paint); }
    let scope_id = ui.make_persistent_id("table-format-border-scope");
    let mut scope = ui.data(|data| data.get_temp::<usize>(scope_id)).unwrap_or(0);
    let options = ["table.formats.all_sides", "table.inset.left", "table.inset.top", "table.inset.right", "table.inset.bottom"];
    dtool_design::studio_row(ui, text("table.formats.border_sides"), |ui, label| {
        egui::ComboBox::from_id_salt(scope_id).selected_text(text(options[scope])).show_ui(ui, |ui| {
            for (index, key) in options.into_iter().enumerate() {
                ui.selectable_value(&mut scope, index, text(key));
            }
        }).response.labelled_by(label);
    });
    ui.data_mut(|data| data.insert_temp(scope_id, scope));
    let selected = if scope == 0 { &mut edges[..] } else { &mut edges[scope - 1..scope] };
    let first = active_edge(&selected[0]);
    let width = first.map_or(0.0, |edge| edge.width_pt);
    let edge_paint = first.map_or(&none, |edge| &edge.paint);
    let same_paint = selected.iter().all(|edge| active_edge(edge).map_or(&none, |edge| &edge.paint) == edge_paint);
    let same_width = selected.iter().all(|edge| active_edge(edge).map_or(0.0, |edge| edge.width_pt) == width);
    let mut styles = selected.iter().filter_map(active_edge).map(|edge| edge.line_style);
    let style = styles.next().unwrap_or_default();
    let same_style = styles.all(|entry| entry == style);
    if let Some(paint) = texture_choice(ui, "table.panel.frame_outline", same_paint.then_some(edge_paint), outline, doc, images) {
        for edge in &mut *selected {
            let width_pt = active_edge(edge).map_or(1.0, |edge| edge.width_pt);
            let line_style = active_edge(edge).map_or(style, |edge| edge.line_style);
            *edge = (!matches!(paint, Paint::None)).then(|| EdgeStyle { paint: paint.clone(), width_pt, line_style });
        }
    }
    if let Some(width) = scalar(ui, "table.panel.frame_width", same_width.then_some(width), "pt") {
        let fallback = Paint::Solid { colour: crate::panels::colour::picker_colour(*outline) };
        for edge in &mut *selected {
            let paint = active_edge(edge).map_or_else(|| fallback.clone(), |edge| edge.paint.clone());
            let line_style = active_edge(edge).map_or(style, |edge| edge.line_style);
            *edge = (width > 0.0).then_some(EdgeStyle { paint, width_pt: width, line_style });
        }
    }
    if let Some(line_style) = stroke_pattern(ui, same_style.then_some(style)) {
        let has_edge = selected.iter().any(|edge| active_edge(edge).is_some());
        for edge in selected {
            if let Some(current) = edge.as_mut().filter(|edge| edge.width_pt > 0.0 && !matches!(edge.paint, Paint::None)) {
                current.line_style = line_style;
            } else if !has_edge {
                *edge = Some(EdgeStyle {
                    paint: Paint::Solid { colour: crate::panels::colour::picker_colour(*outline) },
                    width_pt: 1.0,
                    line_style,
                });
            }
        }
    }
}

fn texture_choice(ui: &mut Ui, key: &str, paint: Option<&Paint>, draft: &mut Hsva, doc: &Document, images: &mut Images) -> Option<Paint> {
    paint_choice_with(ui, key, paint, draft, |ui, well| {
        if matches!(paint, Some(Paint::Bitmap(_) | Paint::Pattern(_))) {
            images.fill(ui.painter(), well.rect.shrink(1.0), paint, doc);
        }
    })
}

fn active_edge(edge: &Option<EdgeStyle>) -> Option<&EdgeStyle> {
    edge.as_ref().filter(|edge| edge.width_pt > 0.0 && !matches!(edge.paint, Paint::None))
}

fn insets(ui: &mut Ui, insets: &mut Insets, units: &str) {
    dtool_design::toggle(ui, &mut insets.linked, text("table.panel.link_insets"));
    let mut linked_value = None;
    for (key, target) in [("table.inset.left", &mut insets.left), ("table.inset.top", &mut insets.top),
        ("table.inset.right", &mut insets.right), ("table.inset.bottom", &mut insets.bottom)] {
        if let Some(value) = scalar(ui, key, Some(*target), units) {
            *target = value;
            if insets.linked { linked_value = Some(value); }
        }
    }
    if let Some(value) = linked_value { (insets.left, insets.top, insets.right, insets.bottom) = (value, value, value, value); }
}
