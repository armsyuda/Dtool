use dtool_core::workspace::ContextId;
use crate::settings::UiSettings;
use crate::shell::AppShell;
use dtool_design::{menu_item, toggle};

pub fn tab_title(ctx: &dtool_core::workspace::DocContext, s: &UiSettings) -> String {
    let name = if ctx.title.is_empty() {
        dtool_strings::text("doc.untitled").to_string()
    } else {
        ctx.title.clone()
    };

    let mut title = if s.tabs_show_zoom {
        let zoom = (ctx.view_state.zoom / ctx.session.actual_zoom() * 100.0).round();
        dtool_strings::format("tab.title", &[("name", &name), ("zoom", &zoom.to_string())])
    } else {
        name
    };

    if ctx.is_modified() {
        title.push('*');
    }

    title
}

fn document_picker(
    ui: &mut egui::Ui, shell: &AppShell, contexts: &[ContextId], active: Option<ContextId>, settings: &UiSettings,
) -> Option<ContextId> {
    use crate::menu_tree::{command_menu_tooltip, configure_command_menu};
    use dtool_strings::text;
    use egui::containers::menu::{MenuButton, MenuConfig};

    let search_id = ui.make_persistent_id("document_picker.search");
    let previous = ui.data(|data| data.get_temp::<String>(search_id));
    let mut search = previous.clone().unwrap_or_default();
    let config = MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let (button, popup) = MenuButton::new("⋯").config(config).ui(ui, |ui| {
        configure_command_menu(ui);
        let start = ui.cursor().top();
        let caption = text("tab.menu.search");
        let search_field = ui.add(egui::TextEdit::singleline(&mut search).id(search_id)
            .hint_text(caption).desired_width(ui.available_width()));
        search_field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, caption));
        if previous.is_none() {
            search_field.request_focus();
        }
        ui.separator();
        let query = search.trim().to_lowercase();
        let matches: Vec<_> = contexts.iter().filter_map(|&id| shell.workspace.get(id)).filter(|context| {
            query.is_empty() || context.title.to_lowercase().contains(&query)
                || context.title.is_empty() && text("doc.untitled").to_lowercase().contains(&query)
                || context.path.as_ref().is_some_and(|path| path.to_string_lossy().to_lowercase().contains(&query))
        }).collect();
        if matches.is_empty() {
            ui.label(text("tab.menu.no_documents"));
            return None;
        }
        if let [context] = matches.as_slice()
            && (search_field.has_focus() || search_field.lost_focus())
            && ui.input(|input| input.key_pressed(egui::Key::Enter))
        {
            ui.close();
            return Some(context.id);
        }
        let height = (ui.ctx().content_rect().height() - (ui.cursor().top() - start) - 24.0).max(1.0);
        let mut list = egui::ScrollArea::vertical().max_height(height).auto_shrink([false, true]);
        if search_field.changed() {
            list = list.vertical_scroll_offset(0.0);
        }
        list.show(ui, |ui| {
            let mut requested = None;
            for context in matches {
                let title = tab_title(context, settings);
                let path = context.path.as_ref().map(|path| path.to_string_lossy());
                let response = ui.selectable_label(active == Some(context.id), &title);
                if command_menu_tooltip(ui, response, &title, false, path.as_deref()).clicked() {
                    requested = Some(context.id);
                    ui.close();
                }
            }
            requested
        }).inner
    });
    let more = text("tab.menu.more_documents");
    button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, more));
    button.on_hover_text(more);
    ui.data_mut(|data| {
        if popup.is_some() {
            data.insert_temp(search_id, search);
        } else {
            data.remove_temp::<String>(search_id);
        }
    });
    popup.and_then(|popup| popup.inner)
}

pub fn tab_row(shell: &mut AppShell, ui: &mut egui::Ui) {
    let shade = if ui.visuals().dark_mode { 0.36 } else { 0.06 };
    let fill = dtool_design::theme::surface(ui.ctx(), "tab_row").lerp_to_gamma(egui::Color32::BLACK, shade);
    let tab_height = dtool_design::size("bar.tab.height");
    let margin = egui::Margin::symmetric(4, 3);
    egui::Panel::top("dtool.shell.document_tabs")
        .exact_size(tab_height + margin.sum().y)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(fill).inner_margin(margin))
        .show(ui, |ui| {
            ui.spacing_mut().interact_size.y = tab_height;
            show_tabs(shell, ui, tab_height);
        });
}

fn show_tabs(shell: &mut AppShell, ui: &mut egui::Ui, tab_height: f32) {
    let mut settings = shell.settings.clone();
    let mut settings_changed = false;
    let contexts: Vec<ContextId> = shell.workspace.contexts().iter().map(|c| c.id).collect();
    let active_id = shell.workspace.active();

    ui.horizontal_centered(|ui| {
        let width = ui.available_width();
        let spacing = ui.spacing().item_spacing.x;
        let close_width = 22.0;
        let minimum_slot = 108.0;
        let count_for = |width: f32| ((width + spacing) / (minimum_slot + spacing)).max(1.0) as usize;
        let overflow = contexts.len() > count_for(width);
        let overflow_width = if overflow { 32.0 + spacing } else { 0.0 };
        let available = width - overflow_width;
        let count = if available > close_width + spacing * 2.0 {
            contexts.len().min(count_for(available))
        } else {
            0
        };
        let active_index = contexts.iter().position(|id| Some(*id) == active_id).unwrap_or(0);
        let first = (active_index + 1).saturating_sub(count).min(contexts.len() - count);
        let title_width = ((available + spacing) / count.max(1) as f32 - close_width - spacing)
            .clamp(1.0, 220.0);

        for &id in &contexts[first..first + count] {
            let title = match shell.workspace.get(id) {
                Some(ctx) => tab_title(ctx, &settings),
                None => continue,
            };
            let is_active = active_id == Some(id);
            let fill = dtool_design::color(if is_active { "document_tab.active" } else { "document_tab.inactive" });
            let tab = egui::Frame::new().fill(fill).corner_radius(3).show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.horizontal(|ui| {
                    let padding = 12.0;
                    let caption = egui::WidgetText::from(title.as_str()).into_galley(
                        ui, Some(egui::TextWrapMode::Truncate), (title_width - padding).max(1.0), egui::TextStyle::Button,
                    );
                    let width = (caption.size().x + padding).clamp(64.0_f32.min(title_width), title_width);
                    let elided = caption.elided;
                    let response = ui.add(egui::Button::new(caption).frame(false).min_size(egui::vec2(width, tab_height)));
                    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, is_active, &title));
                    let response = if elided { response.on_hover_text(&title) } else { response };
                    let close = dtool_strings::text("tab.menu.close");
                    let response_close = ui.add(egui::Button::new("×").frame_when_inactive(false)
                        .min_size(egui::vec2(close_width, tab_height)));
                    response_close.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, close));
                    (response, response_close.on_hover_text(close))
                }).inner
            });
            if is_active {
                let rect = tab.response.rect.shrink(2.0);
                ui.painter().hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, ui.visuals().selection.bg_fill));
            }
            let (response, response_close) = tab.inner;

            if response.clicked() {
                let _ = shell.activate_context(id);
            }
            if response_close.clicked() {
                shell.request_close(id);
            }

            {
                let ctx_path = shell.workspace.get(id).and_then(|c| c.path.clone());
                
                egui::Popup::context_menu(&response).show(|ui| {
                    settings_changed |= toggle(ui, &mut settings.tabs_show_zoom, dtool_strings::text("tab.menu.show_zoom")).changed();
                    settings_changed |= toggle(ui, &mut settings.tabs_show_format, dtool_strings::text("tab.menu.show_format")).changed();
                    settings_changed |= toggle(ui, &mut settings.tabs_show_size, dtool_strings::text("tab.menu.show_size")).changed();
                    settings_changed |= toggle(ui, &mut settings.tabs_show_preview, dtool_strings::text("tab.menu.show_preview")).changed();
                    
                    ui.separator();

                    if menu_item(ui, dtool_strings::text("tab.menu.close")).clicked() {
                        shell.request_close(id);
                    }

                    if menu_item(ui, dtool_strings::text("tab.menu.close_all")).clicked() {
                        shell.close_all_documents();
                    }

                    if menu_item(ui, dtool_strings::text("tab.menu.close_others")).clicked() {
                        shell.close_other_documents(id);
                    }

                    ui.separator();

                    let is_floating = shell.float_manager.is_floating(id); 
                    let float_label = if is_floating {
                        dtool_strings::text("tab.menu.dock")
                    } else {
                        dtool_strings::text("tab.menu.float")
                    };
                    
                    if menu_item(ui, float_label).clicked() {
                        shell.float_manager.toggle_float(id);
                        ui.close();
                    }

                    if menu_item(ui, dtool_strings::text("window.float_all")).clicked() {
                        shell.float_manager.arrange(crate::float::FLOAT_ALL_MENU_ID, &shell.workspace);
                        ui.close();
                    }

                    if menu_item(ui, dtool_strings::text("window.dock_all")).clicked() {
                        shell.float_manager.arrange(crate::float::DOCK_ALL_MENU_ID, &shell.workspace);
                        ui.close();
                    }

                    ui.separator();

                    if let Some(path) = ctx_path {
                        if menu_item(ui, dtool_strings::text("tab.menu.show_in_explorer")).clicked() {
                            #[cfg(target_os = "windows")]
                            {
                                let _ = std::process::Command::new("explorer")
                                    .arg(format!("/select,{}", path.display()))
                                    .spawn();
                            }
                        }
                    }
                });
            }
        }
        if count < contexts.len()
            && let Some(id) = document_picker(ui, shell, &contexts, active_id, &settings)
        {
            let _ = shell.activate_context(id);
        }
    });

    if settings_changed {
        shell.settings = settings;
    }
}
