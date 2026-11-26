use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
use dtool_core::Command;
use dtool_core::table::sort::OPTIONS_SET;
use dtool_core::workspace::{ContextId, Workspace};
use dtool_strings::text;
use serde_json::json;

pub(crate) const MENU: &str = "text.table.sort.sort_options";
const ID: &str = "table.sort.options";

pub(crate) fn open(ctx: &egui::Context, owner: ContextId) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique(ID), owner));
}

pub(crate) fn show(ctx: &egui::Context, workspace: &mut Workspace) -> Option<String> {
    let key = egui::Id::unique(ID);
    let owner = ctx.data(|data| data.get_temp::<ContextId>(key))?;
    if workspace.active() != Some(owner) {
        ctx.data_mut(|data| data.remove::<ContextId>(key));
        return None;
    }
    let session = workspace.session_mut()?;
    let mut options = session.document.table_sort_options;
    let (result, (close, patch)) = show_modal(
        ctx,
        ID,
        text("table.sort.title"),
        ClosePolicy::CancelOnly,
        true,
        |ui| {
            ui.set_width(252.0);
            let mut patch = json!({});
            for (key, label, value) in [
                (
                    "case_sensitive",
                    "table.sort.case_sensitive",
                    &mut options.case_sensitive,
                ),
                (
                    "include_headers",
                    "table.sort.include_headers",
                    &mut options.include_headers,
                ),
                (
                    "treat_numbers_as_text",
                    "table.sort.numbers_as_text",
                    &mut options.treat_numbers_as_text,
                ),
            ] {
                if dtool_design::toggle(ui, value, text(label)).changed() {
                    patch[key] = json!(*value);
                }
            }
            ui.add_space(8.0);
            let close = ui
                .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    dtool_design::button(ui, text("button.close")).clicked()
                })
                .inner;
            (close, patch)
        },
    );
    if close || result != DialogResult::Open {
        ctx.data_mut(|data| data.remove::<ContextId>(key));
    }
    if patch.as_object().is_some_and(|patch| !patch.is_empty()) {
        return session
            .run(&Command::with_params(OPTIONS_SET, patch))
            .err()
            .map(|error| error.message_key);
    }
    None
}
