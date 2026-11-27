use std::sync::mpsc;

use dtool_core::Session;
use dtool_core::workspace::DocContext;
use dtool_export::raster::{ExportError, JobReceipt, OverwriteDecision};
use dtool_strings::text;
use dtool_core::slice::{OutputId, OutputSize, SliceOutput, SliceSource, SliceTarget};
use serde_json::json;

mod workspace;
pub(crate) use workspace::select_source;

struct OverwriteRequest {
    path: std::path::PathBuf,
    reply: mpsc::Sender<OverwriteDecision>,
}

#[derive(Default)]
pub struct SlicesPanel {
    error: Option<String>,
    job: Option<mpsc::Receiver<Result<JobReceipt, ExportError>>>,
    receipt: Option<JobReceipt>,
    overwrite_rx: Option<mpsc::Receiver<OverwriteRequest>>,
    overwrite: Option<OverwriteRequest>,
    draft: workspace::OutputDraft,
}

impl SlicesPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, context: &mut DocContext) {
        if !context.session.has_document { return; }
        let session = &mut context.session;
        let has_outputs = |only_marked: bool| session.document.slices.iter().any(|slice| {
            slice.enabled && (!only_marked || slice.export_selected)
                && slice.outputs.iter().any(|output| output.enabled)
        });
        self.poll_export();
        let mut export = None;
        dtool_design::panel_columns(ui, 2, 125.0, |ui, column| {
            let marked = column == 1;
            let label = if marked { "slice.export_marked" } else { "slice.export_all" };
            let enabled = self.job.is_none() && has_outputs(marked);
            if dtool_design::button_with(ui, text(label), &dtool_design::ControlState::enabled(enabled)).clicked() {
                export = Some(if marked { SliceTarget::SelectedSlices } else { SliceTarget::AllEnabled });
            }
        });
        if let Some(target) = export {
            self.start_export(ui.ctx(), context, target);
        }
        if self.job.is_some() {
            ui.label(text("slice.export_running"));
        }
        if let Some(receipt) = &self.receipt {
            let counts = [receipt.written().len(), receipt.skipped().len(), receipt.failed().len()].map(|n| n.to_string());
            ui.label(dtool_strings::format("slice.export_summary", &[
                ("written", &counts[0]), ("skipped", &counts[1]), ("failed", &counts[2]),
            ]));
            for result in receipt.failed().into_iter().chain(receipt.skipped()) {
                ui.label(result.path.display().to_string());
            }
        }
        let session = &mut context.session;
        if let [object] = session.selection.ids.as_slice()
            && dtool_design::button(ui, text("slice.link_selection")).clicked()
        {
            let name = session.document.get_object(*object).map(|object| object.name.trim())
                .filter(|name| !name.is_empty()).map(str::to_owned);
            let mut params = json!({ "source": object });
            if let Some(name) = name { params["name"] = json!(name); }
            match session.run_named("slice.create", params) {
                Ok(_) => self.error = None,
                Err(error) => self.error = Some(error.message_key),
            }
        }
        egui::ScrollArea::vertical().id_salt("slice.outputs").show(ui, |ui| {
            let selected = session.active_slice;
            let mut pick = None;
            for slice in &session.document.slices {
                if dtool_design::tab(ui, selected == Some(slice.id), &slice.name).clicked() {
                    pick = Some(slice.id);
                }
            }
            if let Some(id) = pick {
                let _ = session.run_named("slice.select", json!({ "id": id }));
            }
            self.show_active(ui, session);
        });
        if let Some(key) = &self.error { ui.label(text(key)); }
    }

    #[cfg(windows)]
    fn start_export(&mut self, ctx: &egui::Context, context: &DocContext, target: SliceTarget) {
        let directory = match crate::dialogs::pick_folder_path(text("slice.export_folder")) {
            Ok(Some(directory)) => directory,
            Ok(None) => return,
            Err(_) => {
                self.error = Some("error.export.failed".into());
                return;
            }
        };
        let document = context.session.document.clone();
        let title = context.title.clone();
        let file_name = context.path.as_ref().and_then(|path| path.file_stem())
            .map_or_else(|| title.clone(), |name| name.to_string_lossy().into_owned());
        let repaint = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        let (overwrite_tx, overwrite_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = if target == SliceTarget::LayerQueue {
                dtool_export::slices::run_confirming(&document, &target, &directory, &title, &file_name, |item| {
                    let (reply, answer) = mpsc::channel();
                    if overwrite_tx.send(OverwriteRequest { path: item.path.clone(), reply }).is_err() {
                        return OverwriteDecision::Cancel;
                    }
                    repaint.request_repaint();
                    answer.recv().unwrap_or(OverwriteDecision::Cancel)
                })
            } else {
                dtool_export::slices::run(&document, &target, &directory, &title, &file_name)
            };
            let _ = sender.send(result);
            repaint.request_repaint();
        });
        self.job = Some(receiver);
        self.overwrite_rx = Some(overwrite_rx);
        self.receipt = None;
        self.error = None;
    }

    #[cfg(not(windows))]
    fn start_export(&mut self, _ctx: &egui::Context, _context: &DocContext, _target: SliceTarget) {
        self.error = Some("error.export.option_unavailable".into());
    }

    fn poll_export(&mut self) {
        let Some(job) = &self.job else { return };
        let result = match job.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(ExportError::Io("slice worker".into())),
        };
        self.job = None;
        self.overwrite_rx = None;
        match result {
            Ok(receipt) => self.receipt = Some(receipt),
            Err(error) => self.error = Some(error.message_key().into()),
        }
    }

    fn show_active(&mut self, ui: &mut egui::Ui, session: &mut Session) {
        let Some(active) = session.active_slice else { return };
        let mut next = session.document.slices.iter().flat_map(|slice| slice.outputs.iter()).map(|output| output.id.0).max().unwrap_or(0).checked_add(1);
        let slices = session.document.slices.clone();
        for (index, mut slice) in slices.into_iter().enumerate().filter(|(_, slice)| slice.id == active) {
            let mut changed = false;
            let mut order = index as u64;
            let mut reset_bounds = false;
            let mut delete = false;
            let mut rect = None;
            ui.push_id(slice.id.0, |ui| {
                dtool_design::panel_frame(ui, |ui| {
                    changed |= ui.add(egui::TextEdit::singleline(&mut slice.name)
                        .hint_text(text("slice.name_required")).desired_width(ui.available_width())).changed();
                    dtool_design::panel_columns(ui, 2, 110.0, |ui, column| {
                        if column == 0 {
                            changed |= dtool_design::toggle(ui, &mut slice.enabled, text("slice.enabled")).changed();
                        } else {
                            changed |= dtool_design::toggle(ui, &mut slice.export_selected, text("slice.export_selected")).changed();
                        }
                    });
                    dtool_design::studio_row(ui, text("slice.path"), |ui, label| {
                        changed |= ui.text_edit_singleline(&mut slice.path).labelled_by(label).changed();
                    });
                    dtool_design::studio_row(ui, text("slice.order"), |ui, label| {
                        let mut value = (order + 1) as f64;
                        let last = session.document.slices.len() as f64;
                        let spec = dtool_design::NumericSpec::new("").range(1.0..=last).decimals(0).width(58.0);
                        if dtool_design::numeric_value(ui, ui.make_persistent_id("slice.order"), &mut value, &spec).labelled_by(label).changed() {
                            order = (value.round() as u64).saturating_sub(1);
                            changed = true;
                        }
                    });
                    if let Ok([left, top, right, bottom]) = slice.resolved_bounds(&session.document) {
                        let mut fields = [left, top, right - left, bottom - top].map(f64::from);
                        let mut bounds_changed = false;
                        dtool_design::panel_columns(ui, fields.len(), 90.0, |ui, axis| {
                            let key = ["slice.bounds_x", "slice.bounds_y", "slice.size.width", "slice.size.height"][axis];
                            let min = if axis < 2 { -1_000_000.0 } else { 0.01 };
                            let spec = dtool_design::NumericSpec::new(text(key)).range(min..=1_000_000.0).width(58.0);
                            bounds_changed |= dtool_design::numeric_value(ui, ui.make_persistent_id(key), &mut fields[axis], &spec).changed();
                        });
                        if bounds_changed {
                            let [x, y, w, h] = fields.map(|value| value as f32);
                            rect = Some([x, y, x + w, y + h]);
                            changed = true;
                        }
                        if matches!(slice.source, SliceSource::Linked(_)) && !slice.auto_bounds {
                            reset_bounds = dtool_design::button(ui, text("slice.reset_bounds")).clicked();
                        }
                    }
                    delete = dtool_design::button(ui, text("slice.delete")).clicked();
                    let mut remove = None;
                    let mut duplicate = None;
                    for (row, output) in slice.outputs.iter_mut().enumerate() {
                        ui.push_id(output.id.0, |ui| {
                            ui.separator();
                            changed |= dtool_design::toggle(ui, &mut output.enabled, text("slice.enabled")).changed();
                            let keys = ["slice.size.scale", "slice.size.width", "slice.size.height", "slice.size.square"];
                            let labels = keys.map(text);
                            let mut kind = match output.size {
                                OutputSize::Scale(_) => 0,
                                OutputSize::Width(_) => 1,
                                OutputSize::Height(_) => 2,
                                OutputSize::Square(_) => 3,
                            };
                            dtool_design::panel_columns(ui, 3, 81.0, |ui, column| match column {
                                0 => {
                                    egui::ComboBox::from_id_salt("format").width(ui.available_width())
                                        .selected_text(output.format.to_uppercase()).show_ui(ui, |ui| {
                                            for format in ["png", "jpeg", "tiff", "webp", "gif", "svg", "pdf"] {
                                                changed |= ui.selectable_value(&mut output.format, format.into(), format.to_uppercase()).changed();
                                            }
                                        });
                                }
                                1 => {
                                    let previous = kind;
                                    dtool_design::dropdown(ui, "size", &mut kind, &labels);
                                    if kind != previous {
                                        let [width, height] = session.document.slices[index].outputs[row]
                                            .dimensions(&session.document.slices[index], &session.document).unwrap_or([1, 1]);
                                        output.size = match kind {
                                            0 => OutputSize::Scale(1.0),
                                            1 => OutputSize::Width(width),
                                            2 => OutputSize::Height(height),
                                            _ => OutputSize::Square(width.min(height)),
                                        };
                                        changed = true;
                                    }
                                }
                                _ => {
                                    let id = ui.make_persistent_id(("size", kind));
                                    let (mut value, spec) = match output.size {
                                        OutputSize::Scale(scale) => (f64::from(scale), dtool_design::NumericSpec::new("")
                                            .range(0.01..=1024.0).step(0.1).suffix("×").reset_to(1.0)),
                                        OutputSize::Width(px) | OutputSize::Height(px) | OutputSize::Square(px) =>
                                            (f64::from(px), dtool_design::NumericSpec::new("")
                                                .range(1.0..=f64::from(dtool_export::raster::MAX_EXPORT_SIDE)).decimals(0).suffix(" px")),
                                    };
                                    if dtool_design::numeric_value(ui, id, &mut value, &spec.width(70.0)).changed() {
                                        output.size = match kind {
                                            0 => OutputSize::Scale(value as f32),
                                            1 => OutputSize::Width(value.round() as u32),
                                            2 => OutputSize::Height(value.round() as u32),
                                            _ => OutputSize::Square(value.round() as u32),
                                        };
                                        changed = true;
                                    }
                                }
                            });
                            dtool_design::studio_row(ui, text("slice.suffix"), |ui, label| {
                                changed |= ui.text_edit_singleline(&mut output.suffix).labelled_by(label).changed();
                            });
                            let mut path = output.path.clone().unwrap_or_default();
                            dtool_design::studio_row(ui, text("slice.path"), |ui, label| {
                                if ui.text_edit_singleline(&mut path).labelled_by(label).changed() {
                                    output.path = (!path.is_empty()).then_some(path);
                                    changed = true;
                                }
                            });
                            dtool_design::panel_columns(ui, 2, 114.0, |ui, column| {
                                if column == 0 {
                                    if dtool_design::button(ui, text("slice.add_size")).clicked() { duplicate = Some(row); }
                                } else if dtool_design::button(ui, text("slice.delete_output")).clicked() {
                                    remove = Some(row);
                                }
                            });
                        });
                    }
                    if let Some(row) = duplicate {
                        if let Some(id) = next {
                            let mut output = slice.outputs[row].clone();
                            output.id = OutputId(id);
                            let largest = slice.outputs.iter().filter(|item| item.format == output.format)
                                .filter_map(|item| match item.size {
                                    OutputSize::Scale(scale) => Some(scale),
                                    _ => None,
                                }).fold(1.0_f32, f32::max);
                            let scale = largest.floor() + 1.0;
                            output.size = OutputSize::Scale(scale);
                            let template = output.path.as_deref().unwrap_or(&slice.path);
                            if !output.suffix.is_empty() || (!template.is_empty() && !template.contains("{scale")) {
                                output.suffix.push_str(&format!("@{scale}x"));
                            }
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
            if changed || reset_bounds || delete {
                let mut params = json!({"id": slice.id.0, "name": slice.name, "enabled": slice.enabled, "export_selected": slice.export_selected, "path": slice.path, "order": order, "outputs": slice.outputs});
                if let Some(rect) = rect { params["rect"] = json!(rect); }
                let result = if delete {
                    session.run_named("slice.delete", json!({ "id": slice.id }))
                } else if reset_bounds {
                    session.run_named("slice.reset_auto", json!({ "id": slice.id }))
                } else {
                    session.run_named("slice.update", params)
                };
                match result {
                    Ok(_) => self.error = None,
                    Err(error) => self.error = Some(error.message_key),
                }
            }
        }
    }
}
