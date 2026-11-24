use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
use dtool_core::artboard::edit::{ToolSettings, page_at_point, page_index, selected_artboard, selection_bounds};
use dtool_core::context_toolbar::{
    ARTBOARD_ASPECT, ARTBOARD_HEIGHT, ARTBOARD_SOURCE, ARTBOARD_SPACING, ARTBOARD_TOOL,
    ARTBOARD_WIDTH, OPTIONS_SET,
};
use dtool_core::document::Units;
use dtool_core::{Command, Session};
use dtool_strings::text;
use serde_json::{Map, json};

fn sources() -> [(&'static str, &'static str); 3] {
    [
        ("document", text("artboard.source.document")),
        ("selection", text("artboard.source.selection")),
        ("custom", text("artboard.source.custom")),
    ]
}

pub(crate) fn aspect_locked(session: &Session) -> bool {
    session.tool_options.flag(ARTBOARD_TOOL, ARTBOARD_ASPECT).unwrap_or(false)
}

#[derive(Clone)]
struct BoardSize {
    original: [f32; 2],
    draft: [f32; 2],
}

pub fn show(ui: &mut egui::Ui, session: &Session, viewport: Option<[f32; 2]>) -> Vec<Command> {
    use dtool_core::view::ViewTarget;
    let doc = &session.document;
    let page = if session.selection.ids.is_empty() {
        viewport.and_then(|size| ViewTarget::at_view_center(doc, &session.view, size))
            .and_then(|target| match target {
                ViewTarget::Page(id) => doc.pages.iter().position(|page| page.id == id),
                ViewTarget::Artboard(id) => Some(dtool_core::data_merge::layout::object_page_index(doc, id)),
            }).unwrap_or(0)
    } else {
        page_index(session)
    };
    let Some(page_size) = doc.pages.get(page).map(|page| page.size) else { return Vec::new() };
    let selection = selection_bounds(session, page);
    let board = selected_artboard(session);
    let settings = ToolSettings::read(session, page);
    let mut source = settings.source.as_str();
    let mut spacing = settings.spacing;
    let mut aspect = settings.aspect;
    let key = board.map(|board| egui::Id::new("dtool.artboard.size").with(session.context).with(board.id));
    let board_draft = key.and_then(|key| ui.ctx().data(|data| data.get_temp::<BoardSize>(key)));
    let mut command = None;
    let mut options = Map::new();
    ui.horizontal_centered(|ui| {
        if board.is_none() {
            ui.label(text("artboard.source"));
            let modes = sources();
            let label = modes.iter().find(|(key, _)| *key == source).map_or(modes[0].1, |(_, label)| *label);
            egui::ComboBox::from_id_salt("artboard.source").width(85.0)
                .selected_text(label).show_ui(ui, |ui| {
                    for (mode, label) in modes {
                        ui.add_enabled_ui(mode != "selection" || selection.is_some(), |ui| {
                            ui.selectable_value(&mut source, mode, label);
                        });
                    }
                });
            ui.separator();
        }
        let mut size = if let Some(board) = board {
            board_draft.filter(|draft| draft.original == board.size).map_or(board.size, |size| size.draft)
        } else if source == "custom" {
            settings.size
        } else if source == "selection" {
            selection.map_or(page_size, |[_, _, w, h]| [w, h])
        } else {
            page_size
        };
        let editable = board.map_or(source == "custom", |board| !board.locked);
        size_fields(ui, &mut size, unit_suffix(doc.units), editable, aspect);
        ui.add_enabled(editable, egui::Checkbox::new(&mut aspect, text("artboard.aspect")));
        let switch = text("artboard.swap_size");
        let swapped = ui.add_enabled(editable, egui::Button::new("↔").frame(false)).on_hover_text(switch);
        swapped.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, editable, switch));
        if swapped.clicked() {
            size.swap(0, 1);
        }
        ui.separator();
        if let Some(board) = board {
            if let Some(key) = key {
                ui.ctx().data_mut(|data| data.insert_temp(key, BoardSize { original: board.size, draft: size }));
            }
            if ui.add_enabled(editable && size != board.size, egui::Button::new(text("artboard.resize"))).clicked() {
                command = Some(Command::with_params("artboard.update", json!({
                    "id": board.id.0, "rect": [board.pos[0], board.pos[1], size[0], size[1]], "resize": true,
                })));
            }
        } else if source == "custom" {
            for (axis, id) in [ARTBOARD_WIDTH, ARTBOARD_HEIGHT].into_iter().enumerate() {
                if settings.size[axis] != size[axis] {
                    options.insert(id.into(), json!(size[axis]));
                }
            }
        }
        ui.label(text("artboard.spacing"));
        ui.add(dimension(&mut spacing, unit_suffix(doc.units), 0.0));
        let can_convert_page = !dtool_core::artboard::edit::is_backing_page(doc, page);
        if can_convert_page && ui.button(text("artboard.convert_page")).clicked() {
            command = Some(Command::with_params("artboard.create", json!({ "source": "page", "page_index": page })));
        }
        if board.is_none() && source != "document" && ui.add_enabled(source != "selection" || selection.is_some(),
            egui::Button::new(text("artboard.insert"))).clicked()
        {
            command = Some(Command::with_params("artboard.create", json!({
                "source": source, "size": size, "spacing": spacing, "page_index": page,
            })));
        }
    });
    if settings.source != source { options.insert(ARTBOARD_SOURCE.into(), json!(source)); }
    if settings.aspect != aspect { options.insert(ARTBOARD_ASPECT.into(), json!(aspect)); }
    if settings.spacing != spacing { options.insert(ARTBOARD_SPACING.into(), json!(spacing)); }
    let mut commands = Vec::new();
    if !options.is_empty() {
        commands.push(Command::with_params(OPTIONS_SET, json!({"tool": ARTBOARD_TOOL, "options": options})));
    }
    commands.extend(command);
    commands
}

#[derive(Clone, Default)]
struct CustomDraft {
    context: Option<dtool_core::workspace::ContextId>,
    point: [f32; 2],
    size: [f32; 2],
    aspect: bool,
}

fn draft_key() -> egui::Id {
    egui::Id::new("dtool.artboard.custom")
}

pub(crate) fn open_custom(ctx: &egui::Context, session: &Session, point: [f32; 2]) {
    let page = page_at_point(&session.document, point).unwrap_or_else(|| page_index(session));
    let settings = ToolSettings::read(session, page);
    ctx.data_mut(|data| data.insert_temp(draft_key(), CustomDraft {
        context: session.context, point, size: settings.size, aspect: settings.aspect,
    }));
    ctx.request_repaint();
}

pub(crate) fn custom_dialog(ctx: &egui::Context, session: &Session) -> Option<Command> {
    let mut draft = ctx.data_mut(|data| data.remove_temp::<CustomDraft>(draft_key()))?;
    if draft.context != session.context || !session.has_document || session.active_tool != ARTBOARD_TOOL {
        return None;
    }
    let (result, (ok, cancel)) = show_modal(ctx, "dtool.artboard.custom.dialog",
        text("artboard.custom_dialog"), ClosePolicy::CancelOnly, false, |ui| {
            ui.horizontal(|ui| size_fields(ui, &mut draft.size, unit_suffix(session.document.units), true, draft.aspect));
            ui.checkbox(&mut draft.aspect, text("artboard.aspect"));
            ui.horizontal(|ui| {
                let ok = ui.button(text("button.ok")).clicked();
                let cancel = ui.button(text("button.cancel")).clicked();
                (ok, cancel)
            }).inner
        });
    if cancel || result == DialogResult::Cancel {
        return None;
    }
    if ok {
        let [x, y] = draft.point;
        let [w, h] = draft.size;
        return Some(Command::with_params("artboard.create", json!({
            "source": "drag", "rect": [x, y, w, h],
        })));
    }
    ctx.data_mut(|data| data.insert_temp(draft_key(), draft));
    None
}

fn size_fields(ui: &mut egui::Ui, size: &mut [f32; 2], unit: &str, editable: bool, aspect: bool) {
    let previous = *size;
    for axis in 0..2 {
        ui.label(if axis == 0 { "W" } else { "H" });
        let change = ui.add_enabled(editable, dimension(&mut size[axis], unit, 0.001)).changed();
        if change && aspect && previous[axis] > 0.0 {
            size[1 - axis] = (previous[1 - axis] * size[axis] / previous[axis]).clamp(0.001, 1_000_000.0);
        }
    }
}

fn unit_suffix(unit: Units) -> &'static str {
    match unit {
        Units::Px => "px",
        Units::Mm => "mm",
        Units::Cm => "cm",
        Units::Pt => "pt",
        Units::In => "in",
    }
}

fn dimension<'a>(value: &'a mut f32, unit: &str, minimum: f32) -> egui::DragValue<'a> {
    egui::DragValue::new(value).range(minimum..=1_000_000.0).max_decimals(3).suffix(format!(" {unit}"))
}

#[derive(Clone, Default)]
struct SetupDraft {
    context: Option<dtool_core::workspace::ContextId>,
    ids: Vec<dtool_core::ObjectId>,
    name: Option<String>,
    size: [f32; 2],
    apply_axis: [bool; 2],
}

fn setup_key() -> egui::Id { egui::Id::new("dtool.artboard.setup") }

pub(crate) fn setup_available(session: &Session) -> bool {
    !session.selection.ids.is_empty() && session.selection.ids.iter().all(|id| {
        session.document.get_object(*id).is_some_and(|board| board.kind == dtool_core::ObjectKind::Artboard)
    })
}

pub(crate) fn open_setup(ctx: &egui::Context, session: &Session) {
    let ids = session.selection.ids.clone();
    let Some(first) = ids.first().and_then(|id| session.document.get_object(*id)) else { return; };
    ctx.data_mut(|data| data.insert_temp(setup_key(), SetupDraft {
        context: session.context,
        name: (ids.len() == 1).then(|| first.name.clone()),
        size: first.size, apply_axis: [ids.len() == 1; 2], ids,
    }));
}

pub(crate) fn show_setup(ctx: &egui::Context, session: &Session) -> (bool, Option<Command>) {
    let Some(mut draft) = ctx.data_mut(|data| data.remove_temp::<SetupDraft>(setup_key())) else { return (false, None); };
    if draft.context != session.context { return (false, None); }
    let editable = draft.ids.iter().all(|id| session.document.get_object(*id).is_some_and(|board| !board.locked));
    let (result, (global, apply, cancel)) = show_modal(ctx, "dtool.artboard.setup.modal", text("artboard.setup"),
        ClosePolicy::CancelOnly, false, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                if let Some(name) = &mut draft.name {
                    ui.horizontal(|ui| { ui.label(text("artboard.name")); ui.text_edit_singleline(name); });
                }
                ui.horizontal(|ui| {
                    ui.menu_button(text("artboard.setup_preset"), |ui| {
                        for name in ["A3", "A4", "A5", "Letter", "Square 1080", "Full HD"] {
                            if ui.button(name).clicked()
                                && let Some((w, h, units, _)) = crate::dialogs::new_document_preset(name)
                            {
                                let doc = &session.document;
                                let convert = |side| dtool_core::units::to_px(side, units, doc.dpi)
                                    .and_then(|px| dtool_core::units::from_px(px, doc.units, doc.dpi)).unwrap_or(side);
                                draft.size = [convert(w), convert(h)];
                                draft.apply_axis = [true; 2];
                                ui.close();
                            }
                        }
                    });
                    if ui.button(text("artboard.swap_size")).clicked() {
                        draft.size.swap(0, 1);
                        draft.apply_axis = [true; 2];
                    }
                });
                for (axis, label) in ["artboard.setup_width", "artboard.setup_height"].into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut draft.apply_axis[axis], text(label));
                        ui.add_enabled(draft.apply_axis[axis], dimension(&mut draft.size[axis], unit_suffix(session.document.units), 0.001));
                    });
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                let global = ui.button(text("artboard.setup_all")).clicked();
                let apply = ui.add_enabled(editable && draft.name.as_ref().is_none_or(|name| !name.trim().is_empty())
                    && (draft.name.is_some() || draft.apply_axis.contains(&true)), egui::Button::new(text("button.ok"))).clicked();
                let cancel = ui.button(text("button.cancel")).clicked();
                (global, apply, cancel)
            }).inner
        });
    if global { return (true, None); }
    if apply {
        return (false, Some(Command::with_params("artboard.update", json!({
            "ids": draft.ids, "name": draft.name,
            "width": draft.apply_axis[0].then_some(draft.size[0]),
            "height": draft.apply_axis[1].then_some(draft.size[1]),
        }))));
    }
    if !cancel && result != DialogResult::Cancel {
        ctx.data_mut(|data| data.insert_temp(setup_key(), draft));
    }
    (false, None)
}
