use dtool_core::Session;
use dtool_core::slice::{OutputId, OutputSize, SliceOutput};
use serde_json::json;

#[derive(Default)]
pub struct SlicesPanel {
    error: Option<String>,
}

impl SlicesPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        if !session.has_document { return; }
        let mut next = session.document.slices.iter().flat_map(|slice| slice.outputs.iter()).map(|output| output.id.0).max().unwrap_or(0).checked_add(1);
        let slices = session.document.slices.clone();
        for (index, mut slice) in slices.into_iter().enumerate() {
            let mut changed = false;
            let mut order = index as u64;
            ui.push_id(slice.id.0, |ui| {
                dtool_design::panel_frame(ui, |ui| {
                    ui.horizontal(|ui| {
                        changed |= dtool_design::toggle(ui, &mut slice.enabled, dtool_strings::text("slice.enabled")).changed();
                        changed |= ui.add(egui::TextEdit::singleline(&mut slice.name).hint_text(dtool_strings::text("slice.name_required"))).changed();
                        changed |= dtool_design::toggle(ui, &mut slice.export_selected, dtool_strings::text("slice.export_selected")).changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label(dtool_strings::text("slice.path"));
                        changed |= ui.text_edit_singleline(&mut slice.path).changed();
                        ui.label(dtool_strings::text("slice.order"));
                        let mut order_value = order as f64;
                        let last = session.document.slices.len().saturating_sub(1) as f64;
                        if dtool_design::number_drag(ui, ui.make_persistent_id("dtool.slice.order"), &mut order_value, 0.0..=last, 1.0).changed() {
                            order = order_value.round() as u64;
                            changed = true;
                        }
                    });
                    let mut remove = None;
                    let mut duplicate = None;
                    for (row, output) in slice.outputs.iter_mut().enumerate() {
                        ui.push_id(output.id.0, |ui| {
                            ui.horizontal(|ui| {
                                changed |= dtool_design::toggle(ui, &mut output.enabled, dtool_strings::text("slice.enabled")).changed();
                                egui::ComboBox::from_id_salt("format").selected_text(output.format.to_uppercase()).show_ui(ui, |ui| {
                                    for format in ["png", "jpeg", "tiff", "webp", "svg", "pdf"] {
                                        changed |= ui.selectable_value(&mut output.format, format.into(), format.to_uppercase()).changed();
                                    }
                                });
                                let mut kind = match output.size { OutputSize::Scale(_) => 0, OutputSize::Width(_) => 1, OutputSize::Height(_) => 2, OutputSize::Square(_) => 3 };
                                let before = kind;
                                let keys = ["slice.size.scale", "slice.size.width", "slice.size.height", "slice.size.square"];
                                let labels: Vec<String> = keys.iter().map(|key| dtool_strings::text(key).to_string()).collect();
                                let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
                                dtool_design::dropdown(ui, "size", &mut kind, &labels);
                                if kind != before {
                                    output.size = match kind { 0 => OutputSize::Scale(1.0), 1 => OutputSize::Width(1), 2 => OutputSize::Height(1), _ => OutputSize::Square(1) };
                                    changed = true;
                                }
                                changed |= match &mut output.size {
                                    OutputSize::Scale(value) => {
                                        let mut scale = f64::from(*value);
                                        let response = dtool_design::number_drag(ui, ui.make_persistent_id("dtool.slice.scale"), &mut scale, f64::from(f32::MIN)..=f64::from(f32::MAX), 1.0);
                                        *value = scale as f32;
                                        response.changed()
                                    }
                                    OutputSize::Width(value) | OutputSize::Height(value) | OutputSize::Square(value) => {
                                        let mut pixels = f64::from(*value);
                                        let response = dtool_design::number_drag(ui, ui.make_persistent_id("dtool.slice.pixels"), &mut pixels, 0.0..=f64::from(u32::MAX), 1.0);
                                        *value = pixels.round() as u32;
                                        response.changed()
                                    }
                                };
                                ui.label(dtool_strings::text("slice.suffix"));
                                changed |= ui.text_edit_singleline(&mut output.suffix).changed();
                                if dtool_design::button(ui, dtool_strings::text("slice.add_size")).clicked() { duplicate = Some(row); }
                                if dtool_design::button(ui, dtool_strings::text("slice.delete_output")).clicked() { remove = Some(row); }
                            });
                            let mut path = output.path.clone().unwrap_or_default();
                            ui.horizontal(|ui| {
                                ui.label(dtool_strings::text("slice.path"));
                                if ui.text_edit_singleline(&mut path).changed() { output.path = (!path.is_empty()).then_some(path); changed = true; }
                            });
                        });
                    }
                    if let Some(row) = duplicate {
                        if let Some(id) = next {
                            let mut output = slice.outputs[row].clone();
                            output.id = OutputId(id);
                            slice.outputs.push(output);
                            next = id.checked_add(1);
                            changed = true;
                        }
                    }
                    if let Some(row) = remove { slice.outputs.remove(row); changed = true; }
                    if dtool_design::button(ui, dtool_strings::text("slice.add_format")).clicked() {
                        if let Some(id) = next {
                            slice.outputs.push(SliceOutput { id: OutputId(id), enabled: true, format: "png".into(), size: OutputSize::Scale(1.0), suffix: String::new(), path: None, options: None });
                            next = id.checked_add(1);
                            changed = true;
                        }
                    }
                });
            });
            if changed {
                let params = json!({"id": slice.id.0, "name": slice.name, "enabled": slice.enabled, "export_selected": slice.export_selected, "path": slice.path, "order": order, "outputs": slice.outputs});
                match session.run_named("slice.update", params) {
                    Ok(_) => self.error = None,
                    Err(error) => self.error = Some(error.message_key),
                }
            }
        }
        if let Some(key) = &self.error { ui.label(dtool_strings::text(key)); }
    }
}
