use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
use dtool_core::commands::qr;
use dtool_core::data_merge::commands::{field_names, qr_field};
use dtool_core::workspace::ContextId;
use dtool_core::{Command, CommandResult, ObjectId, Session};
use dtool_strings::text;
use serde_json::json;

#[derive(Clone, Default)]
struct Draft {
    context: Option<ContextId>,
    object: ObjectId,
    field: Option<String>,
    error: Option<String>,
    #[cfg(not(windows))]
    path: String,
}

fn key() -> egui::Id {
    egui::Id::new("dtool.qr.merge")
}

pub fn context_button(ui: &mut egui::Ui, session: &Session) {
    let [id] = session.selection.ids.as_slice() else { return };
    let Some(object) = session.document.get_object(*id) else { return };
    if object.locked || qr::payload(object).is_none() {
        return;
    }
    if dtool_design::button_with(ui, text("qr.merge"), &dtool_design::ControlState::default()).clicked() {
        ui.ctx().data_mut(|data| data.insert_temp(key(), Draft {
            context: session.context,
            object: *id,
            field: qr_field(session, *id).map(str::to_owned),
            error: None,
            #[cfg(not(windows))]
            path: String::new(),
        }));
    }
}

pub fn show(ctx: &egui::Context, session: &Session) -> Option<Command> {
    show_warnings(ctx);
    let mut draft = ctx.data_mut(|data| data.remove_temp::<Draft>(key()))?;
    if session.context != draft.context || !session.document.get_object(draft.object)
        .is_some_and(|object| !object.locked && qr::payload(object).is_some())
    {
        return None;
    }
    let fields = field_names(session);
    let bound = qr_field(session, draft.object);
    let (result, (import, bind, generate, close)) = show_modal(
        ctx, "dtool.qr.merge.dialog", text("qr.merge"), ClosePolicy::CancelOnly, false, |ui| {
            ui.set_min_width(320.0);
            #[cfg(not(windows))]
            ui.text_edit_singleline(&mut draft.path);
            let import = ui.button(text("merge.add_source")).clicked();
            for source in session.document.merge_sources.iter().filter(|source| source.enabled) {
                ui.label(format!("{} · {}", source.name, dtool_strings::format("qr.merge_records", &[("count", &source.records.len().to_string())])));
            }
            ui.horizontal(|ui| {
                ui.label(text("merge.field_name"));
                egui::ComboBox::from_id_salt("qr_merge_field")
                    .selected_text(draft.field.as_deref().unwrap_or_else(|| text("qr.merge_none")))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut draft.field, None, text("qr.merge_none"));
                        for field in &fields {
                            ui.selectable_value(&mut draft.field, Some((*field).to_owned()), *field);
                        }
                    });
            });
            if let Some(error) = &draft.error {
                ui.label(text(error));
            }
            let pending = draft.field.as_deref() != bound;
            let can_bind = pending && draft.field.as_deref().is_none_or(|field| fields.contains(&field));
            let can_generate = !pending && bound.is_some_and(|field| fields.contains(&field));
            let (bind, generate, close) = ui.horizontal(|ui| {
                let bind = ui.add_enabled(can_bind, egui::Button::new(text("qr.merge_bind"))).clicked();
                let generate = ui.add_enabled(can_generate, egui::Button::new(text("qr.merge_generate"))).clicked();
                let close = ui.button(text("button.cancel")).clicked();
                (bind, generate, close)
            }).inner;
            (import, bind, generate, close)
        });
    if close || result == DialogResult::Cancel {
        return None;
    }
    let command = if import {
        #[cfg(not(windows))]
        let result: std::io::Result<Option<std::path::PathBuf>> = Ok((!draft.path.trim().is_empty()).then(|| std::path::PathBuf::from(draft.path.trim())));
        #[cfg(windows)]
        let result = crate::dialogs::pick_file_path_with_extensions(text("merge.add_source"), text("qr.merge_files"), &["csv", "tsv", "json"], None);
        match result {
            Ok(Some(path)) => Some(Command::with_params("merge.source.add", json!({"path": path}))),
            Ok(None) => None,
            Err(_) => {
                draft.error = Some("qr.merge_source_error".to_owned());
                None
            }
        }
    } else if bind {
        Some(Command::with_params("merge.bind", json!({"object_id": draft.object.0, "field": draft.field})))
    } else if generate {
        Some(Command::with_params("merge.run", json!({})))
    } else {
        None
    };
    ctx.data_mut(|data| data.insert_temp(key(), draft));
    command
}

pub fn reply(ctx: &egui::Context, command: &str, result: &CommandResult) {
    ctx.data_mut(|data| {
        if command == "merge.run" && result.is_ok() {
            data.remove_temp::<Draft>(key());
            let warnings = result.as_ref().ok().and_then(|value| value.get("warnings"))
                .and_then(|value| serde_json::from_value::<Vec<dtool_core::data_merge::RowWarning>>(value.clone()).ok())
                .unwrap_or_default();
            if !warnings.is_empty() {
                data.insert_temp(key().with("warnings"), warnings);
            }
        } else if let Some(mut draft) = data.remove_temp::<Draft>(key()) {
            draft.error = result.as_ref().err().map(|error| match error.code.as_str() {
                "MERGE_NO_DATA" => "qr.merge_no_data",
                "MERGE_IMAGE_UNAVAILABLE" => "qr.merge_image_unavailable",
                "MERGE_BAD_FIELD" => "qr.merge_bad_field",
                _ if command == "merge.source.add" => "qr.merge_source_error",
                _ => &error.message_key,
            }.to_owned());
            data.insert_temp(key(), draft);
        }
    });
}

fn show_warnings(ctx: &egui::Context) {
    let warning_key = key().with("warnings");
    let Some(warnings) = ctx.data_mut(|data| data.remove_temp::<Vec<dtool_core::data_merge::RowWarning>>(warning_key)) else { return };
    let (result, close) = show_modal(ctx, "dtool.qr.merge.warnings", text("qr.merge_warnings"), ClosePolicy::CancelOnly, true, |ui| {
        egui::ScrollArea::vertical().max_height(230.0).show(ui, |ui| {
            for warning in &warnings {
                ui.label(format!("{} · {}", warning.row_index.saturating_add(1), warning.message));
            }
        });
        ui.button(text("button.ok")).clicked()
    });
    if !close && result == DialogResult::Open {
        ctx.data_mut(|data| data.insert_temp(warning_key, warnings));
    }
}
