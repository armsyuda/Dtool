use dtool_core::CommandError;
use dtool_core::command::{Session, TransformFlipArgs, TransformRotate90Args, ZOOM_PRESETS, ZOOM_STEP};
use dtool_core::clamp_zoom;
use dtool_core::document::Document;
use dtool_core::transform::{Anchor, FlipAxis, RotateDirection};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_strings::{format, text};
use crate::dock::{DockSide, Layout};
use crate::settings::UiSettings;

pub fn menu_layer_flip_horizontal(doc: &mut Document) -> Result<(), CommandError> {
    let args = TransformFlipArgs {
        ids: None,
        axis: FlipAxis::Horizontal,
        anchor: Some(Anchor::Centre),
        origin: None,
    };
    
    dtool_core::command::execute_transform_flip(doc, args)?;
    Ok(())
}

pub fn menu_layer_flip_vertical(doc: &mut Document) -> Result<(), CommandError> {
    let args = TransformFlipArgs {
        ids: None,
        axis: FlipAxis::Vertical,
        anchor: Some(Anchor::Centre),
        origin: None,
    };
    
    dtool_core::command::execute_transform_flip(doc, args)?;
    Ok(())
}

pub fn menu_layer_rotate_right(doc: &mut Document) -> Result<(), CommandError> {
    let args = TransformRotate90Args {
        ids: None,
        direction: RotateDirection::Right,
        anchor: Some(Anchor::Centre),
        origin: None,
    };
    
    dtool_core::command::execute_transform_rotate_90(doc, args)?;
    Ok(())
}

pub fn menu_layer_rotate_left(doc: &mut Document) -> Result<(), CommandError> {
    let args = TransformRotate90Args {
        ids: None,
        direction: RotateDirection::Left,
        anchor: Some(Anchor::Centre),
        origin: None,
    };
    
    dtool_core::command::execute_transform_rotate_90(doc, args)?;
    Ok(())
}

pub fn get_menu_label_flip_horizontal() -> String {
    text("menu.layer.flip_horizontal").to_string()
}

pub fn get_menu_label_flip_vertical() -> String {
    text("menu.layer.flip_vertical").to_string()
}

pub fn get_menu_label_rotate_right() -> String {
    text("menu.layer.rotate_right").to_string()
}

pub fn get_menu_label_rotate_left() -> String {
    text("menu.layer.rotate_left").to_string()
}

pub fn window_title(doc_name: &str, dirty: bool) -> String {
    let name = if doc_name.is_empty() {
        text("doc.untitled").to_string()
    } else {
        doc_name.to_string()
    };
    if dirty {
        format!("{}*", name)
    } else {
        name
    }
}

pub fn window_title_for(ws: &Workspace) -> String {
    match ws.active().and_then(|id| ws.get(id)) {
        Some(ctx) => window_title(&ctx.title, ctx.is_modified()),
        None => text("doc.untitled").to_string(),
    }
}

pub fn document_items(ws: &Workspace, s: &UiSettings) -> Vec<(ContextId, String, bool)> {
    let _status_visible = s.status_bar_visible;
    let active = ws.active();
    ws.contexts()
        .iter()
        .enumerate()
        .map(|(i, ctx)| {
            let index = i + 1;
            let title = window_title(&ctx.title, ctx.is_modified());
            let label = format("window.doc_item", &[("index", &index.to_string()), ("title", &title)]);
            let is_active = active == Some(ctx.id);
            (ctx.id, label, is_active)
        })
        .collect()
}

pub fn activate_document(ws: &mut Workspace, id: ContextId) {
    let _ = ws.activate(id);
}

pub fn is_status_bar_visible(s: &UiSettings) -> bool {
    s.status_bar_visible
}

pub fn set_status_bar_visible(s: &mut UiSettings, visible: bool) {
    s.status_bar_visible = visible;
}


pub fn get_menu_label_edit_undo() -> String {
    text("menu.edit.undo").to_string()
}

pub fn can_undo(ws: &Workspace, ctx_id: ContextId) -> bool {
    if let Some(session) = ws.get(ctx_id).map(|ctx| &ctx.session) {
        session.undo.can_undo()
    } else {
        false
    }
}

pub fn get_menu_label_edit_redo() -> String {
    text("menu.edit.redo").to_string()
}

pub fn can_redo(ws: &Workspace, ctx_id: ContextId) -> bool {
    if let Some(session) = ws.get(ctx_id).map(|ctx| &ctx.session) {
        session.undo.can_redo()
    } else {
        false
    }
}

pub fn get_menu_label_edit_cut() -> String {
    text("menu.edit.cut").to_string()
}

pub fn can_cut(ws: &Workspace, ctx_id: ContextId) -> bool {
    if let Some(session) = ws.get(ctx_id).map(|ctx| &ctx.session) {
        !session.selection.ids.is_empty()
    } else {
        false
    }
}

pub fn get_menu_label_edit_copy() -> String {
    text("menu.edit.copy").to_string()
}

pub fn can_copy(ws: &Workspace, ctx_id: ContextId) -> bool {
    if let Some(session) = ws.get(ctx_id).map(|ctx| &ctx.session) {
        !session.selection.ids.is_empty()
    } else {
        false
    }
}

pub fn get_menu_label_edit_paste() -> String {
    text("menu.edit.paste").to_string()
}

pub fn can_paste(ws: &Workspace, ctx_id: ContextId) -> bool {
    if let Some(session) = ws.get(ctx_id).map(|ctx| &ctx.session) {
        session.clipboard.get_payload().kind == dtool_core::clipboard::ClipboardKind::Objects
    } else {
        false
    }
}

pub fn get_menu_label_edit_paste_style() -> String {
    text("menu.edit.paste_style").to_string()
}

pub fn can_paste_style(ws: &Workspace, ctx_id: ContextId) -> bool {
    if let Some(session) = ws.get(ctx_id).map(|ctx| &ctx.session) {
        let kind = session.clipboard.get_payload().kind;
        (kind == dtool_core::clipboard::ClipboardKind::Style || kind == dtool_core::clipboard::ClipboardKind::Objects) && !session.selection.ids.is_empty()
    } else {
        false
    }
}

pub fn get_menu_label_edit_paste_fx() -> String {
    text("menu.edit.paste_fx").to_string()
}

pub fn can_paste_fx(ws: &Workspace, ctx_id: ContextId) -> bool {
    if let Some(session) = ws.get(ctx_id).map(|ctx| &ctx.session) {
        let kind = session.clipboard.get_payload().kind;
        (kind == dtool_core::clipboard::ClipboardKind::Effects || kind == dtool_core::clipboard::ClipboardKind::Objects) && !session.selection.ids.is_empty()
    } else {
        false
    }
}

pub fn get_menu_label_edit_paste_inside() -> String {
    text("menu.edit.paste_inside").to_string()
}

pub fn can_paste_inside(ws: &Workspace, ctx_id: ContextId) -> bool {
    if let Some(session) = ws.get(ctx_id).map(|ctx| &ctx.session) {
        let payload = session.clipboard.get_payload();
        if payload.kind != dtool_core::clipboard::ClipboardKind::Objects || payload.objects.is_none() {
            return false;
        }
        if session.selection.ids.len() != 1 {
            return false;
        }
        let id = session.selection.ids[0];
        match session.document.get_object(id) {
            Some(obj) => obj.kind == dtool_core::object::ObjectKind::Group,
            None => false,
        }
    } else {
        false
    }
}

pub fn get_menu_label_edit_duplicate() -> String {
    text("menu.edit.duplicate").to_string()
}

pub fn can_duplicate(ws: &Workspace, ctx_id: ContextId) -> bool {
    if let Some(session) = ws.get(ctx_id).map(|ctx| &ctx.session) {
        !session.selection.ids.is_empty()
    } else {
        false
    }
}

pub fn get_menu_label_edit_duplicate_layer() -> String {
    text("menu.edit.duplicate.layer").to_string()
}

pub fn can_duplicate_layer(ws: &Workspace, ctx_id: ContextId) -> bool {
    if let Some(session) = ws.get(ctx_id).map(|ctx| &ctx.session) {
        !session.selection.ids.is_empty()
    } else {
        false
    }
}

pub fn get_menu_label_edit_duplicate_selection() -> String {
    text("menu.edit.duplicate.selection").to_string()
}

pub fn can_duplicate_selection(ws: &Workspace, ctx_id: ContextId) -> bool {
    if let Some(session) = ws.get(ctx_id).map(|ctx| &ctx.session) {
        !session.selection.ids.is_empty()
    } else {
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPasteItem {
    Paste,
    PasteSpecial,
    Style,
    Fx,
    Inside,
    WithoutFormat,
}

pub const EDIT_PASTE_ORDER: [EditPasteItem; 6] = [
    EditPasteItem::Paste,
    EditPasteItem::PasteSpecial,
    EditPasteItem::Style,
    EditPasteItem::Fx,
    EditPasteItem::Inside,
    EditPasteItem::WithoutFormat,
];

pub fn edit_paste_item_command(item: EditPasteItem) -> &'static str {
    match item {
        EditPasteItem::Paste => "edit.paste",
        EditPasteItem::PasteSpecial => "edit.paste_special",
        EditPasteItem::Style => "edit.paste_style",
        EditPasteItem::Fx => "edit.paste_fx",
        EditPasteItem::Inside => "edit.paste_inside",
        EditPasteItem::WithoutFormat => "edit.paste_without_format",
    }
}

pub fn edit_paste_item_label(item: EditPasteItem) -> String {
    match item {
        EditPasteItem::Paste => text("menu.edit.paste").to_string(),
        EditPasteItem::PasteSpecial => text("menu.edit.paste_special").to_string(),
        EditPasteItem::Style => text("menu.edit.paste_style").to_string(),
        EditPasteItem::Fx => text("menu.edit.paste_fx").to_string(),
        EditPasteItem::Inside => text("menu.edit.paste_inside").to_string(),
        EditPasteItem::WithoutFormat => text("menu.edit.paste_without_format").to_string(),
    }
}

pub fn get_menu_label_edit_paste_without_format() -> String {
    text("menu.edit.paste_without_format").to_string()
}

pub fn get_menu_label_edit_paste_special() -> String {
    text("menu.edit.paste_special").to_string()
}

pub fn can_paste_without_format(session: &Session) -> bool {
    let payload = session.clipboard.get_payload();
    payload.kind == dtool_core::clipboard::ClipboardKind::Text
        && payload.text.as_ref().map_or(false, |t| !dtool_core::command::plain_text(t).is_empty())
}

pub fn can_paste_special(session: &Session) -> bool {
    !session.paste_special_formats().is_empty()
}

pub fn edit_paste_item_enabled(session: &Session, item: EditPasteItem) -> bool {
    match item {
        EditPasteItem::WithoutFormat => can_paste_without_format(session),
        EditPasteItem::PasteSpecial => can_paste_special(session),
        other => session.can_execute_named(edit_paste_item_command(other)),
    }
}

pub fn menu_edit_paste_without_format(session: &mut Session) -> Result<serde_json::Value, String> {
    session
        .run_named("edit.paste_without_format", serde_json::json!({}))
        .map_err(|e| text(&e.message_key).to_string())
}

pub fn menu_edit_paste_special(session: &mut Session, format: &str) -> Result<serde_json::Value, String> {
    session
        .run_named("edit.paste_special", serde_json::json!({ "format": format }))
        .map_err(|e| text(&e.message_key).to_string())
}

pub enum EditPasteAction {
    Ran(Result<serde_json::Value, String>),
    OpenPasteSpecial,
}

pub fn activate_edit_paste_item(session: &mut Session, item: EditPasteItem) -> EditPasteAction {
    match item {
        EditPasteItem::PasteSpecial => EditPasteAction::OpenPasteSpecial,
        EditPasteItem::WithoutFormat => EditPasteAction::Ran(menu_edit_paste_without_format(session)),
        other => EditPasteAction::Ran(
            session
                .run_named(edit_paste_item_command(other), serde_json::json!({}))
                .map_err(|e| text(&e.message_key).to_string()),
        ),
    }
}

pub fn show_edit_paste_menu(ui: &mut egui::Ui, session: &mut Session) -> Option<EditPasteAction> {
    let mut action = None;
    ui.menu_button(text("menu.edit.paste"), |ui| {
        for item in EDIT_PASTE_ORDER {
            let enabled = edit_paste_item_enabled(session, item);
            let clicked = ui
                .add_enabled_ui(enabled, |ui| dtool_design::menu_item(ui, edit_paste_item_label(item)))
                .inner
                .clicked();
            if clicked {
                action = Some(activate_edit_paste_item(session, item));
                ui.close();
            }
        }
    });
    action
}

pub fn get_menu_label_window_panels() -> String {
    text("menu.window.panels").to_string()
}

pub fn get_menu_label_window_panels_show(side: DockSide) -> String {
    text(side.menu_key()).to_string()
}

pub fn get_menu_label_window_panels_hide() -> String {
    text("menu.window.panels.hide").to_string()
}

pub fn is_panels_side_checked(layout: &Layout, side: DockSide) -> bool {
    layout.columns.is_shown(side)
}

pub fn menu_window_panels_show(layout: &mut Layout, side: DockSide) {
    layout.toggle_side(side);
}

pub fn menu_window_panels_hide(layout: &mut Layout) {
    layout.hide_all();
}

pub fn can_hide_panels(layout: &Layout) -> bool {
    !layout.all_hidden()
}

pub fn panels_menu(ui: &mut egui::Ui, layout: &mut Layout) {
    ui.menu_button(get_menu_label_window_panels(), |ui| {
        for side in DockSide::ALL {
            let mut checked = is_panels_side_checked(layout, side);
            if dtool_design::toggle(ui, &mut checked, get_menu_label_window_panels_show(side)).clicked() {
                layout.set_side_shown(side, checked);
                ui.close();
            }
        }
        ui.separator();
        let enabled = can_hide_panels(layout);
        let hide = ui.add_enabled_ui(enabled, |ui| dtool_design::menu_item(ui, get_menu_label_window_panels_hide())).inner;
        if hide.clicked() {
            menu_window_panels_hide(layout);
            ui.close();
        }
    });
}

pub fn get_menu_label_view_rotate() -> String {
    text("menu.view.rotate").to_string()
}

pub fn get_menu_label_view_rotate_reset() -> String {
    text("menu.view.rotate_reset").to_string()
}

pub fn view_rotation_deg(session: &Session) -> f32 {
    session.view.rotation_deg
}

pub fn can_view_rotate_reset(session: &Session) -> bool {
    session.view.rotation_deg != 0.0
}

pub fn menu_view_rotate_set(session: &mut Session, degrees: f32) -> Result<serde_json::Value, String> {
    session
        .run_named("view.rotate.set", serde_json::json!({ "degrees": degrees }))
        .map_err(|e| text(&e.message_key).to_string())
}

pub fn menu_view_rotate_reset(session: &mut Session) -> Result<serde_json::Value, String> {
    session
        .run_named("view.rotate.reset", serde_json::json!({}))
        .map_err(|e| text(&e.message_key).to_string())
}

pub fn view_rotate_menu(ui: &mut egui::Ui, session: &mut Session) -> Option<Result<serde_json::Value, String>> {
    let mut result = None;
    ui.menu_button(get_menu_label_view_rotate(), |ui| {
        let mut degrees = f64::from(session.view.rotation_deg);
        if dtool_design::number_drag(ui, egui::Id::NULL.with("dtool.view.rotate"), &mut degrees, 0.0..=359.9, 1.0).changed() {
            let degrees = degrees as f32;
            result = Some(menu_view_rotate_set(session, degrees));
        }
        ui.separator();
        let enabled = can_view_rotate_reset(session);
        let reset = ui.add_enabled_ui(enabled, |ui| dtool_design::menu_item(ui, get_menu_label_view_rotate_reset())).inner;
        if reset.clicked() {
            result = Some(menu_view_rotate_reset(session));
            ui.close();
        }
    });
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewZoomItem {
    ZoomIn,
    ZoomOut,
    Zoom50,
    Zoom100,
    Zoom200,
    Zoom400,
}

pub const VIEW_ZOOM_ORDER: [ViewZoomItem; 6] = [
    ViewZoomItem::ZoomIn,
    ViewZoomItem::ZoomOut,
    ViewZoomItem::Zoom50,
    ViewZoomItem::Zoom100,
    ViewZoomItem::Zoom200,
    ViewZoomItem::Zoom400,
];

pub fn get_menu_label_view_zoom() -> String {
    text("menu.view.zoom").to_string()
}

pub fn view_zoom_item_label(item: ViewZoomItem) -> String {
    let key = match item {
        ViewZoomItem::ZoomIn => "menu.view.zoom_in",
        ViewZoomItem::ZoomOut => "menu.view.zoom_out",
        ViewZoomItem::Zoom50 => "menu.view.zoom_50",
        ViewZoomItem::Zoom100 => "menu.view.zoom_actual",
        ViewZoomItem::Zoom200 => "menu.view.zoom_200",
        ViewZoomItem::Zoom400 => "menu.view.zoom_400",
    };
    text(key).to_string()
}

pub fn view_zoom_item_target(current: f32, item: ViewZoomItem) -> f32 {
    let target = match item {
        ViewZoomItem::ZoomIn => current * ZOOM_STEP,
        ViewZoomItem::ZoomOut => current / ZOOM_STEP,
        ViewZoomItem::Zoom50 => ZOOM_PRESETS[0],
        ViewZoomItem::Zoom100 => ZOOM_PRESETS[1],
        ViewZoomItem::Zoom200 => ZOOM_PRESETS[2],
        ViewZoomItem::Zoom400 => ZOOM_PRESETS[3],
    };
    clamp_zoom(target)
}

pub fn view_zoom_around(cursor_over_canvas: Option<[f32; 2]>, view_size: [f32; 2]) -> [f32; 2] {
    cursor_over_canvas.unwrap_or([view_size[0] * 0.5, view_size[1] * 0.5])
}

pub fn menu_view_zoom_set(session: &mut Session, zoom: f32, around: [f32; 2]) -> Result<serde_json::Value, String> {
    session
        .run_named("view.zoom.set", serde_json::json!({ "zoom": clamp_zoom(zoom), "around": around }))
        .map_err(|e| text(&e.message_key).to_string())
}

pub fn menu_view_zoom_item(
    session: &mut Session,
    item: ViewZoomItem,
    cursor_over_canvas: Option<[f32; 2]>,
    view_size: [f32; 2],
) -> Result<serde_json::Value, String> {
    if item == ViewZoomItem::Zoom100 {
        return session.run_named("view.zoom.actual", serde_json::json!({ "around": view_zoom_around(cursor_over_canvas, view_size) }))
            .map_err(|error| text(&error.message_key).to_string());
    }
    let actual = session.actual_zoom();
    let target = view_zoom_item_target(session.view.zoom / actual, item) * actual;
    menu_view_zoom_set(session, target, view_zoom_around(cursor_over_canvas, view_size))
}

pub fn view_zoom_menu(
    ui: &mut egui::Ui,
    session: &mut Session,
    cursor_over_canvas: Option<[f32; 2]>,
    view_size: [f32; 2],
) -> Option<Result<serde_json::Value, String>> {
    let mut result = None;
    ui.menu_button(get_menu_label_view_zoom(), |ui| {
        if dtool_design::menu_item(ui, text("menu.view.zoom_fit")).clicked() {
            result = Some(menu_view_zoom_fit(session, view_size));
            ui.close();
        }
        for item in VIEW_ZOOM_ORDER {
            if dtool_design::menu_item(ui, view_zoom_item_label(item)).clicked() {
                result = Some(menu_view_zoom_item(session, item, cursor_over_canvas, view_size));
                ui.close();
            }
        }
    });
    result
}

pub fn menu_view_zoom_fit(session: &mut Session, view_size: [f32; 2]) -> Result<serde_json::Value, String> {
    session.run_named("view.zoom.fit", serde_json::json!({ "view_size": view_size }))
        .map_err(|e| text(&e.message_key).to_string())
}

pub fn get_menu_label_view_snapping() -> String {
    text("menu.view.snapping").to_string()
}
