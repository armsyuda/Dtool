use dtool_core::data_merge::sources::{
    confirm_source_body, refresh_source_by_name, SourceBody,
};
use dtool_core::data_merge::{FileFormat, MergeBinding, SourceKind};
use dtool_design::{button, dialog_frame, icon_button, panel_frame, panel_header, toggle};
use dtool_strings::t;
use crate::Workspace;

#[derive(Clone, Default)]
struct DataMergeUi {
    pending: Option<(String, SourceBody)>,
    error: Option<String>,
    bind_field: String,
}

pub fn render_data_merge_panel(ui: &mut egui::Ui, ws: &mut Workspace) {
    let id = egui::Id::new("data_merge.ui");
    let mut state = ui.ctx().data_mut(|d| d.get_temp_or_default::<DataMergeUi>(id));

    panel_frame(ui, |ui| {
        panel_header(ui, t("panel.data_merge.title"));
        let Some(doc) = ws.session.document_mut() else {
            ui.label(t("error.no_document"));
            return;
        };
        if doc.merge_sources.is_empty() {
            ui.horizontal(|ui| {
                let _ = button(ui, t("merge.add_source"));
                dtool_design::button_with(ui, t("merge.later"), &dtool_design::ControlState::enabled(false));
            });
        } else {
            let mut refresh_name: Option<String> = None;
            for src in doc.merge_sources.iter_mut() {
                ui.horizontal(|ui| {
                    ui.label(&src.name);
                    toggle(ui, &mut src.enabled, t("merge.enabled"));
                });
                let loaded_text = t("merge.loaded")
                    .replace("{fields}", &src.fields.len().to_string())
                    .replace("{records}", &src.records.len().to_string());
                ui.label(loaded_text);
                match &mut src.format {
                    FileFormat::Csv { delimiter, quote } => {
                        ui.horizontal(|ui| {
                            ui.label(t("merge.delimiter"));
                            ui.label(format!("{:?}", delimiter));
                            if let Some(q) = quote {
                                ui.label(format!("{} {:?}", t("merge.quote"), q));
                            }
                        });
                    }
                    FileFormat::Json => {
                        ui.label(t("merge.format_json"));
                    }
                    FileFormat::Xlsx { sheet } => {
                        ui.horizontal(|ui| {
                            ui.label(t("merge.sheet"));
                            ui.text_edit_singleline(sheet);
                        });
                    }
                }
                match &src.kind {
                    SourceKind::Url { url } => {
                        ui.horizontal(|ui| {
                            ui.label(url);
                            if icon_button(ui, "refresh").clicked() {
                                refresh_name = Some(src.name.clone());
                            }
                        });
                    }
                    SourceKind::File { .. } => {
                        if icon_button(ui, "refresh").clicked() {
                            refresh_name = Some(src.name.clone());
                        }
                    }
                }
                ui.horizontal(|ui| {
                    let _ = button(ui, t("merge.insert_field"));
                    let _ = icon_button(ui, "reveal");
                });
            }
            if let Some(name) = refresh_name {
                match refresh_source_by_name(&mut doc.merge_sources, &name) {
                    Ok(result) if result.hash_changed => {
                        state.pending = result.body.map(|body| (name, body));
                        state.error = None;
                    }
                    Ok(_) => {
                        state.error = None;
                    }
                    Err(err) => {
                        state.error = Some(if err.code == "NOT_IMPLEMENTED" || err.code == "MERGE_FETCH_FAILED" || err.message_key == "merge.err_url" {
                            t("merge.err_url").to_string()
                        } else {
                            err.message_key
                        });
                    }
                }
            }
        }

        let selection = ws.selection();
        let has_caret = ws.session.document().map(|d| d.has_text_caret()).unwrap_or(false);
        let selected_objects: Vec<_> = doc.objects.iter().filter(|o| selection.contains(&o.id)).collect();
        let valid_target = selected_objects.iter().any(|o| {
            matches!(o.kind, dtool_core::object::ObjectKind::PictureFrame { .. } | dtool_core::object::ObjectKind::QrCode { .. })
        });

        ui.horizontal(|ui| {
            if button(ui, t("merge.bind_field")).clicked() {
                state.error = None;
            }
        });

        if !has_caret && !valid_target {
            ui.label(t("merge.bind_hint"));
        } else {
            ui.horizontal(|ui| {
                ui.label(t("merge.field_name"));
                ui.text_edit_singleline(&mut state.bind_field);
            });
            ui.horizontal(|ui| {
                let _ = button(ui, t("merge.record_index"));
                let _ = button(ui, t("merge.original_record_index"));
                let _ = button(ui, t("merge.source_name"));
            });

            if !state.bind_field.is_empty() && valid_target {
                for obj in &selected_objects {
                    match obj.kind {
                        dtool_core::object::ObjectKind::PictureFrame { .. } => {
                            doc.merge_bindings.push(MergeBinding::Image {
                                object_id: obj.id,
                                field: state.bind_field.clone(),
                            });
                        }
                        dtool_core::object::ObjectKind::QrCode { .. } => {
                            doc.merge_bindings.push(MergeBinding::Qr {
                                object_id: obj.id,
                                field: state.bind_field.clone(),
                            });
                        }
                        _ => {}
                    }
                }
                state.bind_field.clear();
            } else if !state.bind_field.is_empty() && has_caret {
                doc.merge_bindings.push(dtool_core::data_merge::MergeBinding::Text {
                    field: state.bind_field.clone(),
                });
                state.bind_field.clear();
            }
        }

        if let Some(err) = &state.error {
            ui.label(err);
        }
    });

    if let Some((name, body)) = state.pending.clone() {
        let mut open = true;
        egui::Window::new(t("panel.data_merge.title"))
            .open(&mut open)
            .collapsible(false)
            .show(ui.ctx(), |ui| {
                dialog_frame(ui, |ui| {
                    ui.label(t("merge.updated_prompt"));
                    ui.horizontal(|ui| {
                        if button(ui, t("button.yes")).clicked() {
                            if let Some(doc) = ws.session.document_mut() {
                                if let Some(src) = doc.merge_sources.iter_mut().find(|src| src.name == name) {
                                    if let Err(err) = confirm_source_body(src, &body) {
                                        state.error = Some(err.message_key);
                                    } else {
                                        state.error = None;
                                    }
                                }
                            }
                            state.pending = None;
                        }
                        if button(ui, t("button.no")).clicked() {
                            state.pending = None;
                        }
                    });
                });
            });
        if !open {
            state.pending = None;
        }
    }

    ui.ctx().data_mut(|d| d.insert_temp(id, state));
}
