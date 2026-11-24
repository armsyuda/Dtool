use dtool_core::{CommandError, ObjectId, Session};
use dtool_strings::text;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskAction {
    Invert,
    FromSelection,
    Release,
}

pub const MASK_ACTIONS: [MaskAction; 3] = [MaskAction::Invert, MaskAction::FromSelection, MaskAction::Release];

impl MaskAction {
    pub fn command(self) -> &'static str {
        match self {
            MaskAction::Invert => "layer.mask.invert",
            MaskAction::FromSelection => "layer.mask.from_selection",
            MaskAction::Release => "layer.mask.release",
        }
    }

    pub fn label_key(self) -> &'static str {
        match self {
            MaskAction::Invert => "menu.layer.mask.invert",
            MaskAction::FromSelection => "menu.layer.mask.from_selection",
            MaskAction::Release => "menu.layer.mask.release",
        }
    }
}

pub fn target(session: &Session) -> Option<ObjectId> {
    match session.selection.ids.as_slice() {
        [id] if session.document.get_object(*id).is_some() => Some(*id),
        _ => None,
    }
}

pub fn has_mask(session: &Session, id: ObjectId) -> bool {
    session.document.layer_masks.iter().any(|mask| mask.object == id)
}

pub fn has_pixel_selection(session: &Session) -> bool {
    session.document.pixel_selection.as_ref().is_some_and(|selection| selection.coverage.iter().any(|value| *value > 0))
}

pub fn enabled(session: &Session, action: MaskAction) -> bool {
    let Some(id) = target(session) else { return false };
    if session.document.get_object(id).is_some_and(|object| object.locked) {
        return false;
    }
    match action {
        MaskAction::Invert | MaskAction::Release => has_mask(session, id),
        MaskAction::FromSelection => has_pixel_selection(session),
    }
}

pub fn run(session: &mut Session, action: MaskAction) -> Result<Value, CommandError> {
    let id = target(session).ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    session.run_named(action.command(), json!({ "id": id.0 }))
}

/// B29-T004 (K08 C): Alt-click mask-only view. Per-view, unsaved (`view.mask_view.set`), never touches the document.
pub fn mask_view_available(session: &Session) -> bool {
    target(session).is_some_and(|id| dtool_core::clip_mask::has_any_mask(&session.document, id))
}

pub fn mask_view_on(session: &Session) -> bool {
    session.view.canvas_flags.mask_view.is_some()
}

/// Toggle the mask-only view for `id`: same command the Layers panel's Alt-click on the thumbnail runs
/// (bot08 8ec1aed `panels::layers::mask_view_command`); the gesture itself lives only in the Layers panel.
pub fn alt_click_mask(session: &mut Session, id: ObjectId) -> Result<Value, CommandError> {
    session.run_named("view.mask_view.set", json!({ "id": mask_view_target(session, id) }))
}

/// Target for `view.mask_view.set`: `id`, or null when `id` is already shown (toggle). Kept here because
/// this file is also compiled standalone by c08_fix5_mask_actions; c08_ui_fx_maskview asserts it equals
/// `panels::layers::mask_view_command`.
pub fn mask_view_target(session: &Session, id: ObjectId) -> Value {
    if session.view.canvas_flags.mask_view == Some(id) { Value::Null } else { json!(id.0) }
}

pub fn toggle_mask_view(session: &mut Session) -> Result<Value, CommandError> {
    if mask_view_on(session) {
        return session.run_named("view.mask_view.set", json!({ "id": Value::Null }));
    }
    let id = target(session).ok_or_else(|| CommandError::new("NOTHING_SELECTED"))?;
    alt_click_mask(session, id)
}

pub fn show_mask_actions(ui: &mut egui::Ui, session: &mut Session) -> Option<Result<Value, CommandError>> {
    ui.horizontal(|ui| show_mask_actions_with(ui, session, |ui, _, label, enabled, selected| {
        ui.add_enabled(enabled, egui::Button::new(label).selected(selected)).clicked()
    })).inner
}

pub fn show_mask_actions_with(
    ui: &mut egui::Ui,
    session: &mut Session,
    mut button: impl FnMut(&mut egui::Ui, Option<MaskAction>, &str, bool, bool) -> bool,
) -> Option<Result<Value, CommandError>> {
    let mut chosen = None;

    for action in MASK_ACTIONS {
        if button(ui, Some(action), text(action.label_key()), enabled(session, action), false) {
            chosen = Some(action);
        }
    }
    let view_on = mask_view_on(session);
    let toggle_view = button(ui, None, text("layer.mask.view_only"), mask_view_available(session) || view_on, view_on);
    if toggle_view {
        return Some(toggle_mask_view(session));
    }
    chosen.map(|action| run(session, action))
}
