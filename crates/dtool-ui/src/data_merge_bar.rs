use dtool_core::data_merge::layout::find_or_create_layout;
use dtool_core::data_merge::{Corner, MergeLayout};
use dtool_core::Session;
use dtool_design::{dropdown, number_field, panel_frame, toggle};
use dtool_strings::text;

pub struct DataMergeBar {
    pub show_record_order: bool,
}

impl Default for DataMergeBar {
    fn default() -> Self {
        Self { show_record_order: false }
    }
}

impl DataMergeBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        panel_frame(ui, |ui| {
            ui.horizontal(|ui| {
                self.controls(ui, session);
            });
        });
    }

    fn controls(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        let Some(object_id) = session.selection.ids.first().copied() else {
            ui.label(text("tool.data_merge_layout.name"));
            return;
        };
        let Some(doc) = session.document_mut() else {
            return;
        };
        if doc.get_object(object_id).is_none() {
            return;
        }
        let layout = find_or_create_layout(doc, object_id);
        edit_layout(ui, layout);
        toggle(ui, &mut self.show_record_order, "Show Record Order");
    }
}

fn edit_layout(ui: &mut egui::Ui, layout: &mut MergeLayout) {
    let mut rows = f64::from(layout.rows);
    if number_field(ui, &mut rows, &text("ctx.merge.rows")).changed() && rows.is_finite() {
        layout.rows = rows.max(1.0).round() as u32;
    }
    let mut columns = f64::from(layout.columns);
    if number_field(ui, &mut columns, &text("ctx.merge.columns")).changed() && columns.is_finite() {
        layout.columns = columns.max(1.0).round() as u32;
    }
    let mut gutter_x = f64::from(layout.gutter[0]);
    let mut gutter_y = f64::from(layout.gutter[1]);
    if number_field(ui, &mut gutter_x, "Gutter").changed() && gutter_x.is_finite() {
        layout.gutter[0] = gutter_x as f32;
    }
    if number_field(ui, &mut gutter_y, "").changed() && gutter_y.is_finite() {
        layout.gutter[1] = gutter_y as f32;
    }
    let mut cell_w = f64::from(layout.cell[0]);
    let mut cell_h = f64::from(layout.cell[1]);
    if number_field(ui, &mut cell_w, "Cell Width").changed() && cell_w.is_finite() {
        layout.cell[0] = cell_w.max(0.0) as f32;
    }
    if number_field(ui, &mut cell_h, "Cell Height").changed() && cell_h.is_finite() {
        layout.cell[1] = cell_h.max(0.0) as f32;
    }
    toggle(ui, &mut layout.preserve_cell_size, "Preserve Cell Size");
    let mut offset = f64::from(layout.record_offset);
    if number_field(ui, &mut offset, "Record Offset").changed() && offset.is_finite() {
        layout.record_offset = offset.max(0.0).round() as u32;
    }
    let mut advance = f64::from(layout.record_advance);
    if number_field(ui, &mut advance, "Record Advance").changed() && advance.is_finite() {
        layout.record_advance = advance.max(1.0).round() as u32;
    }
    ui.label(text("ctx.merge.record_origin"));
    let mut origin = match layout.origin {
        Corner::TopLeft => 0,
        Corner::TopRight => 1,
        Corner::BottomLeft => 2,
        Corner::BottomRight => 3,
    };
    if dropdown(ui, "merge-record-origin", &mut origin, &["Top Left", "Top Right", "Bottom Left", "Bottom Right"]).changed() {
        layout.origin = match origin {
            1 => Corner::TopRight,
            2 => Corner::BottomLeft,
            3 => Corner::BottomRight,
            _ => Corner::TopLeft,
        };
    }
    let mut order = if layout.rows_first { 0 } else { 1 };
    if dropdown(ui, "merge-row-column-order", &mut order, &["Rows then Columns", "Columns then Rows"]).changed() {
        layout.rows_first = order == 0;
    }
}
