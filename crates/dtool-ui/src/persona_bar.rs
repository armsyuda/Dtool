use crate::icon_cache::IconCache;
use crate::persona::{PersonaEntry, PersonaId};
use dtool_core::{Command, CommandError, Session};
use dtool_strings::text;

pub const LEAVE_APPLY: &str = "__leave_apply__";
pub const LEAVE_CANCEL: &str = "__leave_cancel__";

pub fn selection_command(selected: &str) -> Command {
    if selected == LEAVE_APPLY {
        Command::with_params("ui.persona.leave_sub", serde_json::json!({"apply": true}))
    } else if selected == LEAVE_CANCEL {
        Command::with_params("ui.persona.leave_sub", serde_json::json!({"apply": false}))
    } else if let Some(sub_id) = selected.strip_prefix("enter_sub:") {
        Command::with_params("ui.persona.enter_sub", serde_json::json!({"id": sub_id}))
    } else {
        Command::with_params("ui.persona.set", serde_json::json!({"id": selected}))
    }
}

pub fn show(ui: &mut egui::Ui, session: &mut Session) -> Option<Result<serde_json::Value, CommandError>> {
    let current = session.run_named("ui.persona.query", serde_json::json!({})).ok();
    let command = pick(ui, current.as_ref())?;
    Some(session.run(&command))
}

pub fn persona_icon_for(key: &str) -> Option<&'static str> {
    let id = format!("persona-{key}");
    dtool_design::icons::svg_ids().into_iter().find(|known| *known == id)
}

fn cached_texture(ui: &egui::Ui, icons: &mut Option<&mut IconCache>, icon: Option<&str>, selected: bool) -> Option<egui::TextureHandle> {
    let icon = icon?;
    let cache = icons.as_deref_mut()?;
    let px = dtool_design::icon_texture_px(ui);
    let colour = if selected { dtool_design::selected_icon_ink(ui) } else { dtool_design::icons::current_icon_colour() };
    cache.texture(ui.ctx(), icon, colour, px)
}

pub fn pick(ui: &mut egui::Ui, current: Option<&serde_json::Value>) -> Option<Command> {
    pick_with(ui, current, None)
}

pub fn pick_icons(ui: &mut egui::Ui, current: Option<&serde_json::Value>, icons: &mut IconCache) -> Option<Command> {
    pick_with(ui, current, Some(icons))
}

/// Inline persona controls for the shared 52.8 dip application header.
pub fn pick_strip_icons(ui: &mut egui::Ui, current: Option<&serde_json::Value>, icons: &mut IconCache) -> Option<Command> {
    pick_with(ui, current, Some(icons))
}

fn toolbar_item(ui: &mut egui::Ui, session: Option<&Session>, name: &str, icons: &mut IconCache, editing: bool, in_menu: bool) -> egui::Response {
    let enabled = editing || match name {
        "doc.new" | "doc.open" => true,
        "doc.save" => session.is_some(),
        "page.add" => session.is_some_and(|s| s.has_document && s.document.layout_artboard_origins.is_some()),
        "layer.pixel.add" => session.is_some_and(|s| s.can_execute(&Command::with_params(name, serde_json::json!({"current_surface": true})))),
        crate::context_bar::align::ALIGN_POPUP_COMMAND => session.is_some_and(|s| !s.selection.ids.is_empty()),
        "node.group" => session.is_some_and(|s| s.selection.ids.len() >= 2),
        "node.ungroup" => session.is_some_and(|s| !s.selection.ids.is_empty() && s.selection.ids.iter().all(|&id| {
            s.document.get_object(id).is_some_and(|object| object.is_group() && !object.is_layer_folder())
        })),
        _ => session.is_some_and(|s| match crate::menu_tree::bridge_action(name) {
            Some(action) => crate::menu_tree::bridge_action_enabled(action, s)
                && !crate::dialogs::locked_delete::LockedDeleteDialog::selected_nodes(s).is_empty(),
            None => s.can_execute_named(name),
        }),
    };
    let spec = crate::persona::TOOLBAR_ACTIONS.iter().find(|(id, _, _)| *id == name);
    let fallback = format!("menu.{name}");
    let (icon, key) = spec.map(|(_, icon, key)| (Some(*icon), *key)).unwrap_or((None, &fallback));
    let selected = session.is_some_and(|s| match name {
        "ruler.toggle" => s.document.rulers.show,
        "guides.toggle" => s.guides.show,
        _ => false,
    });
    let label = text(key);
    if !in_menu {
        return toolbar_button(ui, icons, icon, label, enabled, selected);
    }

    let texture = icon.and_then(|id| icons.themed_texture(ui.ctx(), id, dtool_design::icon_texture_px(ui)));
    let button = match &texture {
        Some(texture) => egui::Button::image_and_text(
            egui::Image::new(texture).fit_to_exact_size(egui::Vec2::splat(16.0)), label),
        None => egui::Button::new(label),
    };
    ui.add_enabled(enabled, button.selected(selected).frame(false))
}

fn toolbar_slot(ui: &mut egui::Ui, session: Option<&Session>, name: &str, icons: &mut IconCache, editing: bool)
    -> (egui::Response, Option<Command>)
{
    let Some(commands) = crate::persona::toolbar_group(name) else {
        let response = toolbar_item(ui, session, name, icons, editing, false);
        let action = (!editing && response.clicked()).then(|| {
            if name == "layer.pixel.add" {
                Command::with_params(name, serde_json::json!({"current_surface": true}))
            } else {
                Command::new(name)
            }
        });
        if action.is_some() && name == crate::context_bar::align::ALIGN_POPUP_COMMAND {
            crate::context_bar::remember_alignment_origin(ui, &response, name);
        }
        return (response, action);
    };
    let group = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.separator();
        ui.add_space(2.0);
        let mut action = None;
        for &id in commands {
            if toolbar_item(ui, session, id, icons, editing, false).clicked() && !editing {
                action = Some(Command::new(id));
            }
        }
        ui.add_space(2.0);
        ui.separator();
        action
    });
    (group.response, group.inner)
}

pub(crate) enum ToolbarEdit {
    Place(usize, &'static str),
    Remove(&'static str),
}

impl ToolbarEdit {
    pub(crate) fn apply(self, items: &mut Vec<String>) {
        match self {
            Self::Place(mut index, id) => {
                if let Some(from) = items.iter().position(|item| item == id) {
                    items.remove(from);
                    index -= usize::from(from < index);
                }
                items.insert(index.min(items.len()), id.to_owned());
            }
            Self::Remove(id) => items.retain(|item| item != id),
        }
    }
}

fn begin_toolbar_drag(ui: &egui::Ui, response: &egui::Response, name: &str) {
    let Some((id, _, _)) = crate::persona::TOOLBAR_ACTIONS.iter().find(|(id, _, _)| *id == name) else { return };
    let pointer = ui.input(|input| input.pointer.primary_pressed().then(|| input.pointer.press_origin()).flatten());
    if pointer.is_some_and(|pos| response.rect.intersect(ui.clip_rect()).contains(pos)
        && ui.ctx().layer_id_at(pos) == Some(ui.layer_id()))
    {
        egui::DragAndDrop::set_payload(ui.ctx(), crate::studio_editor::ToolbarCatalogItem { id, placed: true });
    }
}

/// Common commands belong in the header; document tools belong in the vertical rail.
pub(crate) fn toolbar_icons(
    ui: &mut egui::Ui, session: Option<&Session>, toolbar: &[String], icons: &mut IconCache,
    editing: bool,
) -> (Option<Command>, Option<ToolbarEdit>) {
    use crate::studio_editor::ToolbarCatalogItem;

    const SLOT_WIDTH: f32 = 38.0;
    let control_height = dtool_design::size("bar.toolbar.control.height");
    let spacing = ui.spacing().item_spacing.x;
    let available = ui.available_width();
    let mut measure = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("toolbar.measure")
            .max_rect(egui::Rect::from_min_size(ui.cursor().min, egui::vec2(10_000.0, ui.available_height())))
            .layout(egui::Layout::left_to_right(egui::Align::Center))
            .invisible()
            .sizing_pass(),
    );
    let origin = measure.min_rect().left();
    let ends: Vec<_> = toolbar.iter().map(|name| {
        toolbar_slot(&mut measure, session, name, icons, editing);
        measure.min_rect().right() - origin
    }).collect();
    let full = ends.last().copied().unwrap_or(0.0);
    let slot = if editing { SLOT_WIDTH + if toolbar.is_empty() { 0.0 } else { spacing } } else { 0.0 };
    let overflow = !toolbar.is_empty() && full + slot > available;
    let visible = if overflow {
        ends.iter().take_while(|width| **width + spacing + control_height <= available).count()
    } else {
        toolbar.len()
    };

    let mut drop = None;
    let mut action = None;
    for (index, name) in toolbar[..visible].iter().enumerate() {
        let (button, picked) = toolbar_slot(ui, session, name, icons, editing);
        if editing {
            begin_toolbar_drag(ui, &button, name);
            if let Some(item) = button.dnd_hover_payload::<ToolbarCatalogItem>() {
                let rect = button.rect;
                let after = ui.input(|input| input.pointer.hover_pos().is_some_and(|pos| pos.x >= rect.center().x));
                let x = if after { rect.right() } else { rect.left() };
                ui.painter().vline(x, rect.y_range(), egui::Stroke::new(2.0, ui.visuals().selection.bg_fill));
                if button.dnd_release_payload::<ToolbarCatalogItem>().is_some() {
                    drop = Some(ToolbarEdit::Place(index + usize::from(after), item.id));
                }
            }
        } else if picked.is_some() {
            action = picked;
        }
    }
    let append_target = if overflow {
        let gap = ui.available_width() - control_height - spacing;
        if gap > 0.0 {
            ui.add_space(gap);
        }
        let label = text("context.toolbar.more");
        let more = toolbar_button(ui, icons, Some("cmd-more-vertical"), label, true, false);
        egui::Popup::menu(&more).align(egui::RectAlign::BOTTOM_END).show(|ui| {
            egui::ScrollArea::vertical().max_height((ui.ctx().content_rect().height() * 0.55).min(360.0)).show(ui, |ui| {
                for name in &toolbar[visible..] {
                    if editing {
                        let response = toolbar_item(ui, session, name, icons, true, true);
                        begin_toolbar_drag(ui, &response, name);
                    } else {
                        let single = [name.as_str()];
                        for &id in crate::persona::toolbar_group(name).unwrap_or(&single) {
                            if toolbar_item(ui, session, id, icons, false, true).clicked() {
                                action = Some(Command::new(id));
                                ui.close();
                            }
                        }
                    }
                }
            });
        });
        Some(more)
    } else if editing {
        Some(ui.add_sized([SLOT_WIDTH, control_height], egui::Label::new("+").sense(egui::Sense::hover())))
    } else {
        None
    };
    if editing && let Some(slot) = append_target {
        let stroke = if slot.dnd_hover_payload::<ToolbarCatalogItem>().is_some() {
            ui.visuals().selection.stroke
        } else {
            ui.visuals().widgets.noninteractive.bg_stroke
        };
        ui.painter().rect_stroke(slot.rect, 3.0, stroke, egui::StrokeKind::Inside);
        if let Some(item) = slot.dnd_release_payload::<ToolbarCatalogItem>() {
            drop = Some(ToolbarEdit::Place(toolbar.len(), item.id));
        }
    }
    (action, drop)
}

pub(crate) fn workspace_icons(ui: &mut egui::Ui, session: Option<&Session>, icons: &mut IconCache) -> Option<Command> {
    let enabled = session.is_some();
    let snapping = session.is_some_and(|s| s.snap_prefs.query().enabled);
    let preview = session.is_some_and(|s| s.view.canvas_flags.preview_mode);
    let mut action = None;
    if toolbar_button(ui, icons, Some("cmd-export"), text("menu.file.export.export"), enabled, false).clicked() {
        action = Some(Command::new("ui.export.open"));
    }
    if toolbar_button(ui, icons, Some("cmd-preview-mode"), text("menu.view.preview_mode"), enabled, preview).clicked() {
        action = Some(Command::with_params("view.flags.set", serde_json::json!({"preview_mode": !preview})));
    }
    if toolbar_button(ui, icons, Some("cmd-guides"), text("menu.view.guides_manager"), enabled, false).clicked() {
        action = Some(Command::new(crate::guides_dialog::SHOW_GUIDES_MANAGER));
    }
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let options = ui.add_enabled(enabled, egui::Button::new("▾")
            .min_size(egui::vec2(22.0, dtool_design::size("bar.toolbar.control.height"))));
        let options = dtool_design::tooltip(options, text("dialog.snapping.options"));
        if options.clicked() {
            action = Some(Command::new(crate::snapping_dialog::MENU_ID));
        }
        if toolbar_button(ui, icons, Some("cmd-snap-magnet"), text("dialog.snapping.enabled"), enabled, snapping).clicked() {
            action = Some(Command::new(dtool_core::commands::toggles::SNAP_TOGGLE));
        }
    });
    action
}

pub(crate) fn quick_mask_icons(ui: &mut egui::Ui, session: &Session, icons: &mut IconCache) -> Option<Command> {
    use dtool_core::commands::quickmask::{Display, TOGGLE};
    use serde_json::json;

    let state = session.quick_mask;
    let label = text("menu.selection.quick_mask");
    let mut action = None;
    let menu = ui.menu_button("▾", |ui| {
        for (display, key) in [
            (Display::Overlay, "quickmask.show_overlay"),
            (Display::Black, "quickmask.show_black"),
            (Display::White, "quickmask.show_white"),
            (Display::Transparent, "quickmask.show_transparent"),
        ] {
            if ui.selectable_label(display == state.display, text(key)).clicked() {
                action = Some(Command::with_params(TOGGLE, json!({"display": display})));
                ui.close();
            }
        }
        ui.separator();
        let mut ink = state.ink;
        if ui.add(egui::Slider::new(&mut ink, 0..=255).text(text("quickmask.paint_value"))).changed() {
            action = Some(Command::with_params(TOGGLE, json!({"ink": ink})));
        }
    });
    dtool_design::tooltip(menu.response, label);
    if toolbar_button(ui, icons, Some("layer-mask-preview"), label, true, state.active).clicked() {
        action = Some(Command::new(TOGGLE));
    }
    action
}

pub(crate) fn view_mode_switch(ui: &mut egui::Ui, session: &Session, icons: &mut IconCache) -> Option<Command> {
    use dtool_core::{ViewMode, commands::view_flags::VIEW_FLAGS_SET};
    let active = match session.view.canvas_flags.view_mode {
        ViewMode::Pixel2x => ViewMode::Pixel,
        ViewMode::Wireframe => ViewMode::Outline,
        mode => mode,
    };
    let mut action = None;
    switch_group(ui, |ui| {
        for (mode, icon, key) in [
            (ViewMode::Normal, "persona-vector", "view.switch.vector"),
            (ViewMode::Pixel, "cmd-grid", "view.switch.pixel"),
            (ViewMode::Outline, "tool-rectangle", "view.switch.xray"),
        ] {
            if toolbar_button(ui, icons, Some(icon), text(key), true, mode == active).clicked() {
                action = Some(Command::with_params(VIEW_FLAGS_SET, serde_json::json!({"view_mode": mode})));
            }
        }
    });
    action
}

fn switch_group(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let surface = dtool_design::theme::surface(ui.ctx(), "bars");
    let edge = if ui.visuals().dark_mode { egui::Color32::BLACK } else { egui::Color32::WHITE };
    let track = surface.lerp_to_gamma(edge, 0.20);
    ui.horizontal(|ui| {
        egui::Frame::new().fill(track).corner_radius(6).inner_margin(egui::Margin::symmetric(2, 0)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            let visuals = ui.visuals_mut();
            let text = visuals.text_color();
            visuals.override_text_color = None;
            let idle = &mut visuals.widgets.inactive;
            idle.weak_bg_fill = egui::Color32::TRANSPARENT;
            idle.bg_stroke = egui::Stroke::NONE;
            idle.fg_stroke.color = text.lerp_to_gamma(track, 0.25);
            visuals.widgets.hovered.fg_stroke.color = text;
            add(ui);
        });
    });
}

fn switch_icon_tint(selected: bool) -> egui::Color32 {
    egui::Color32::WHITE.gamma_multiply(if selected { 1.0 } else { 0.70 })
}

fn toolbar_button(ui: &mut egui::Ui, icons: &mut IconCache, icon: Option<&str>, label: &str, enabled: bool, selected: bool) -> egui::Response {
    let icon_size = dtool_design::size("icon.size.tool");
    let px = (icon_size * ui.ctx().pixels_per_point()).ceil() as u32;
    let colour = if selected { dtool_design::selected_icon_ink(ui) } else { dtool_design::icons::current_icon_colour() };
    let texture = icon.and_then(|id| icons.texture(ui.ctx(), id, colour, px));
    let button = match &texture {
        Some(texture) => egui::Button::new(egui::Image::new(texture).fit_to_exact_size(egui::Vec2::splat(icon_size))
            .tint(switch_icon_tint(selected))),
        None => egui::Button::new(label),
    };
    let size = egui::Vec2::splat(dtool_design::size("bar.toolbar.control.height"));
    let response = dtool_design::with_icon_selection(ui, |ui| {
        dtool_design::tooltip(ui.add_enabled(enabled, button.selected(selected).min_size(size)), label)
    });
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled && ui.is_enabled(), selected, label));
    response
}

fn pick_with(ui: &mut egui::Ui, current: Option<&serde_json::Value>, mut icons: Option<&mut IconCache>) -> Option<Command> {
    let active = current.and_then(|value| value.get("id")).and_then(|value| value.as_str()).unwrap_or("").to_string();
    let in_sub_persona = current.and_then(|value| value.get("in_sub_persona")).and_then(|value| value.as_bool()).unwrap_or(false);
    let control_height = dtool_design::size("bar.toolbar.control.height");
    let mut selected = None;

    if in_sub_persona && active == "ai_image" {
        if dtool_design::button(ui, text("ai.image.back")).clicked() {
            selected = Some(LEAVE_CANCEL.to_string());
        }
    } else if in_sub_persona {
        ui.horizontal(|ui| {
            if dtool_design::button(ui, text("persona.apply")).clicked() {
                selected = Some(LEAVE_APPLY.to_string());
            }
            if dtool_design::button(ui, text("persona.cancel")).clicked() {
                selected = Some(LEAVE_CANCEL.to_string());
            }
        });
    } else {
        ui.horizontal(|ui| {
            let home_label = text("persona.home");
            let home = match cached_texture(ui, &mut icons, Some("app-logo"), false) {
                Some(texture) => {
                    let image = egui::Image::new(&texture).fit_to_exact_size(egui::Vec2::splat(22.0));
                    let button = ui.add(egui::Button::new(image).min_size(egui::Vec2::splat(control_height)));
                    button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, home_label));
                    dtool_design::tooltip(button, home_label)
                }
                None => dtool_design::tooltip(dtool_design::icon_button(ui, home_label), home_label),
            };
            if home.clicked() {
                selected = Some("home".to_string());
            }

            switch_group(ui, |ui| {
                for id in [PersonaId::Vector, PersonaId::Pixel, PersonaId::Layout, PersonaId::Slice, PersonaId::AiImage] {
                    let entry = PersonaEntry::Builtin(id);
                    let key = entry.key();
                    let name_key = format!("persona.{key}");
                    let name = text(&name_key);
                    let tooltip = dtool_strings::format("persona.tooltip", &[("name", name)]);
                    let selected_persona = key == active;
                    let icon = if id == PersonaId::AiImage { Some("tool-place-image") } else { persona_icon_for(&key) };
                    let texture = cached_texture(ui, &mut icons, icon, selected_persona);
                    let button = match &texture {
                        Some(texture) => egui::Button::new((
                            egui::Image::new(texture).fit_to_exact_size(egui::Vec2::splat(18.0))
                                .tint(switch_icon_tint(selected_persona)),
                            name,
                        )),
                        None => egui::Button::new(name),
                    };
                    let response = dtool_design::with_icon_selection(ui, |ui| {
                        ui.add(button.selected(selected_persona).min_size(egui::vec2(72.0, control_height)))
                    });
                    if dtool_design::tooltip(response, tooltip).clicked() {
                        selected = Some(if id == PersonaId::AiImage { format!("enter_sub:{key}") } else { key });
                    }
                }
            });

            let more_label = text("persona.more");
            let more = match icons.as_deref_mut() {
                Some(cache) => toolbar_button(ui, cache, Some("cmd-more-vertical"), more_label, true, false),
                None => dtool_design::tooltip(dtool_design::icon_button(ui, "⋮"), more_label),
            };
            more.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, more_label));
            egui::Popup::menu(&more).show(|ui| {
                for definition in crate::persona::persona_catalog() {
                    if matches!(definition.entry, PersonaEntry::Extra(_)) {
                        let label = format!("{} ({})", text(&definition.name_key), text("menu.planned.suffix"));
                        ui.add_enabled(false, egui::Button::new(label).frame(false));
                    }
                }
                ui.separator();
                if let Ok(studios) = crate::persona::host_action(&Command::new("ui.studio.list"))
                    && let Some(studios) = studios.as_array() {
                    for name in studios.iter().filter_map(|studio| studio["name"].as_str()) {
                        let key = PersonaEntry::Custom(name.to_owned()).key();
                        if ui.selectable_label(active == key, name).clicked() {
                            selected = Some(key);
                            ui.close();
                        }
                    }
                    if !studios.is_empty() { ui.separator(); }
                }
                if dtool_design::menu_item(ui, text("studio.create")).clicked() {
                    request_studio(ui.ctx(), crate::studio_editor::StudioRequest::Create);
                    ui.close();
                }
                if dtool_design::menu_item(ui, text("studio.manager")).clicked() {
                    request_studio(ui.ctx(), crate::studio_editor::StudioRequest::Manager);
                    ui.close();
                }
            });
        });
    }

    selected.map(|id| selection_command(&id))
}

pub fn show_restored_controls(ui: &mut egui::Ui, session: &Session, tools: &[crate::tools_panel::ToolId], toolbar: &[String]) -> Option<dtool_core::Command> {
    restored_controls_with(ui, session, tools, toolbar, None)
}

pub fn show_restored_controls_icons(ui: &mut egui::Ui, session: &Session, tools: &[crate::tools_panel::ToolId], toolbar: &[String], icons: &mut IconCache) -> Option<dtool_core::Command> {
    restored_controls_with(ui, session, tools, toolbar, Some(icons))
}

fn restored_controls_with(ui: &mut egui::Ui, session: &Session, tools: &[crate::tools_panel::ToolId], toolbar: &[String], mut icons: Option<&mut IconCache>) -> Option<dtool_core::Command> {
    let mut action = None;
    dtool_design::toolbar(ui, |ui| {
        for name in toolbar {
            if ui.add_enabled_ui(session.can_execute_named(name), |ui| dtool_design::menu_item(ui, text(&format!("menu.{name}")))).inner.clicked() {
                action = Some(dtool_core::Command::with_params(name, serde_json::json!({})));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for tool in tools {
            let selected = session.active_tool == tool.0;
            let response = match cached_texture(ui, &mut icons, dtool_design::icons::tool_icon_for(tool.0), tool.0 == session.active_tool) {
                Some(texture) => dtool_design::tooltip(dtool_design::icon_image_button_selected(ui, &texture, selected, &dtool_design::ControlState::default()), crate::tooltips::tool_tooltip(tool.0)),
                None => dtool_design::tab(ui, selected, text(tool.0)),
            };
            if response.clicked() {
                action = Some(dtool_core::Command::with_params("ui.tool.set", serde_json::json!({"id": tool.0})));
            }
        }
    });
    action
}

fn studio_request_id() -> egui::Id {
    egui::Id::unique("persona_bar.studio_request")
}

pub fn request_studio(ctx: &egui::Context, request: crate::studio_editor::StudioRequest) {
    ctx.data_mut(|data| data.insert_temp(studio_request_id(), Some(request)));
}

pub fn take_studio_request(ctx: &egui::Context) -> Option<crate::studio_editor::StudioRequest> {
    ctx.data_mut(|data| data.remove_temp::<Option<crate::studio_editor::StudioRequest>>(studio_request_id())).flatten()
}

pub const PENDING_EDIT_COMMIT: &str = "commit";
pub const PENDING_EDIT_DISCARD: &str = "cancel";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchChoice {
    Keep,
    Discard,
    Stay,
}

pub fn pending_edit_dirty(session: &Session) -> bool {
    let Some(edit) = session.text_edit.as_ref() else { return false };
    if session.text_composing.is_some() {
        return true;
    }
    let current = session.document.stories.iter().find(|story| story.id == edit.story);
    match (&edit.before, current) {
        (Some(before), Some(story)) => &before.story != story,
        (None, _) => false,
        (Some(_), None) => true,
    }
}

fn is_switch(command: &Command) -> bool {
    matches!(command.name.as_str(), "ui.persona.set" | "ui.persona.enter_sub" | "ui.persona.leave_sub")
}

#[derive(Debug, Default)]
pub struct SwitchGuard {
    pub pending: Option<Command>,
}

impl SwitchGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_asking(&self) -> bool {
        self.pending.is_some()
    }

    pub fn request(&mut self, session: Option<&Session>, command: Command) -> Option<Command> {
        let ask = is_switch(&command) && command.params.get("pending_edit").is_none() && session.is_some_and(pending_edit_dirty);
        if !ask {
            return Some(command);
        }
        self.pending = Some(command);
        None
    }

    pub fn resolve(&mut self, choice: SwitchChoice) -> Option<Command> {
        let mut command = self.pending.take()?;
        let mode = match choice {
            SwitchChoice::Keep => PENDING_EDIT_COMMIT,
            SwitchChoice::Discard => PENDING_EDIT_DISCARD,
            SwitchChoice::Stay => return None,
        };
        if !command.params.is_object() {
            command.params = serde_json::json!({});
        }
        command.params["pending_edit"] = mode.into();
        Some(command)
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<Command> {
        self.pending.as_ref()?;
        let mut choice = None;
        let mut open = true;
        egui::Window::new(text("persona.switch.title")).id(egui::Id::unique("persona.switch.window")).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            ui.label(text("persona.switch.pending_edit"));
            dtool_design::toolbar(ui, |ui| {
                if dtool_design::button(ui, text("persona.switch.keep")).clicked() {
                    choice = Some(SwitchChoice::Keep);
                }
                if dtool_design::button(ui, text("persona.switch.discard")).clicked() {
                    choice = Some(SwitchChoice::Discard);
                }
                if dtool_design::button(ui, text("persona.cancel")).clicked() {
                    choice = Some(SwitchChoice::Stay);
                }
            });
        });
        if !open {
            choice = Some(SwitchChoice::Stay);
        }
        choice.and_then(|choice| self.resolve(choice))
    }
}

pub fn close_export_preview() -> bool {
    match crate::shell::export_dialog().lock() {
        Ok(mut dialog) if dialog.is_open() => {
            dialog.close();
            true
        }
        _ => false,
    }
}
