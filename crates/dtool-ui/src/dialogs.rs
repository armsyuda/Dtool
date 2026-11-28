use dtool_core::colour::ColourFormat;
use dtool_core::document::Units;
use dtool_core::command::Session;
use dtool_core::workspace::{CloseAnswer, ContextId};
use dtool_strings::{format, text};
use crate::dialog_chrome::{options_group, options_scroll, show_modal, ClosePolicy, DialogResult};

pub mod locked_delete;
pub mod resource_manager;
pub mod package;
pub mod compound;
pub(crate) mod move_duplicate;
pub mod document_setup;
pub mod export;
pub mod longdoc;
pub mod layer_effects;
pub mod blend_options;
pub(crate) mod tone_blend;
pub mod edit_text_style;
mod new_document_options;
pub(crate) use new_document_options::sides_editor;

use new_document_options::{DocumentKind, Options, SavedPresets};

#[derive(Debug, Clone)]
pub struct NewDocumentDialogState {
    pub width: f32,
    pub height: f32,
    pub units: Units,
    pub dpi: f32,
    pub pages: u32,
    pub preset: Option<String>,
    pub error_message: Option<String>,
    pub preset_search: String,
    options: Options,
    saved_presets: SavedPresets,
    favourites_only: bool,
    reset_dimension_edits: bool,
    option_icons: crate::icon_cache::IconCache,
}

impl Default for NewDocumentDialogState {
    fn default() -> Self {
        Self {
            width: 1080.0,
            height: 1080.0,
            units: Units::Px,
            dpi: 72.0,
            pages: 1,
            preset: Some("Square 1080".to_string()),
            error_message: None,
            preset_search: String::new(),
            options: Default::default(),
            saved_presets: Default::default(),
            favourites_only: false,
            reset_dimension_edits: true,
            option_icons: Default::default(),
        }
    }
}

pub const NEW_DOCUMENT_MENU: &str = "file.new";
pub(crate) const NEW_FROM_LAST_PRESET_MENU: &str = "file.new_from_last_preset";
pub(crate) const NEW_FROM_FAVOURITES_MENU: &str = "file.new_from_favourites";
pub const NEW_DOCUMENT_PRESETS: [&str; 18] = [
    "A3", "A4", "A5", "B5 (JIS)", "Letter", "Legal", "Ledger", "Trade Paperback", "Half Letter", "Oversize Magazine",
    "Square 1080", "Full HD", "4K UHD", "4R", "5R", "6R", "8R", "10R",
];
const CMYK_DOCUMENT_PRESETS: [&str; 10] = [
    "A3 (CMYK)", "A4 (CMYK)", "A5 (CMYK)", "B5 (JIS) (CMYK)", "Letter (CMYK)", "Legal (CMYK)",
    "Ledger (CMYK)", "Trade Paperback (CMYK)", "Half Letter (CMYK)", "Oversize Magazine (CMYK)",
];
pub const NEW_DOCUMENT_UNITS: [Units; 5] = [Units::Px, Units::Mm, Units::Cm, Units::In, Units::Pt];

fn builtin_preset_names() -> impl Iterator<Item = &'static str> {
    NEW_DOCUMENT_PRESETS.into_iter().chain(CMYK_DOCUMENT_PRESETS)
}

impl NewDocumentDialogState {
    pub(crate) fn from_favourites() -> Self {
        let mut state = Self { favourites_only: true, ..Self::default() };
        state.load_saved_presets();
        state.select_visible_favourite();
        state
    }

    fn select_visible_favourite(&mut self) {
        if self.favourites_only && !self.preset.as_deref().is_some_and(|name| self.is_favourite(name)) {
            if let Some(name) = self.favourite_names().into_iter().next() {
                self.select_preset(&name);
            }
        }
    }

    pub(crate) fn select_preset(&mut self, name: &str) -> bool {
        if self.select_saved_preset(name) {
            true
        } else if builtin_preset_names().any(|preset| preset == name) {
            apply_preset(self, name);
            true
        } else {
            false
        }
    }

    pub fn command(&self) -> dtool_core::Command {
        let title = self.preset.clone().unwrap_or_else(|| text("doc.untitled").to_string());
        let mut params = serde_json::json!({
            "width": self.width,
            "height": self.height,
            "units": self.units,
            "dpi": self.dpi,
            "pages": if self.options.kind == Some(DocumentKind::VectorArtboard) { 1 } else { self.pages },
            "title": title,
        });
        if let Some(preset) = &self.preset {
            params["preset"] = serde_json::json!(preset);
        }
        self.options.write_params(&mut params, self.units);
        dtool_core::Command::with_params("doc.new", params)
    }
}

pub fn create_new_document(workspace: &mut dtool_core::Workspace, state: &NewDocumentDialogState) -> Result<ContextId, dtool_core::CommandError> {
    let (target, result) = workspace.run(dtool_core::Scope::active(), state.command());
    result?;
    target.ok_or_else(dtool_core::CommandError::context_not_found)
}

pub fn submit_new_document(workspace: &mut dtool_core::Workspace, state: &mut NewDocumentDialogState) -> bool {
    match create_new_document(workspace, state) {
        Ok(_) => {
            state.error_message = None;
            true
        }
        Err(error) => {
            state.error_message = Some(text(&error.message_key).to_string());
            false
        }
    }
}

pub fn show_new_document_dialog(ui: &mut egui::Ui, state: &mut NewDocumentDialogState, workspace: &mut dtool_core::Workspace) -> bool {
    new_document_dialog_result(ui, state, workspace, None, &mut None) != NewDocumentOutcome::Open
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewDocumentOutcome {
    Open,
    Created,
    OpenedRecent,
    Cancelled,
}

pub(crate) fn new_document_dialog_result(
    ui: &mut egui::Ui,
    state: &mut NewDocumentDialogState,
    workspace: &mut dtool_core::Workspace,
    template_tools: Option<(&mut crate::home::HomeScreen, &dtool_core::prefs::Prefs)>,
    template_action: &mut Option<crate::home::HomeAction>,
) -> NewDocumentOutcome {
    let (title, width, height, units, dpi, pages, preset, ok, cancel) = get_dialog_labels();
    let unit_options = new_document_unit_options();
    state.load_saved_presets();
    state.options.kind.get_or_insert_with(|| {
        if dtool_core::commands::persona::layout_active() { DocumentKind::LayoutPages } else { DocumentKind::VectorArtboard }
    });
    let mut text_editing = false;
    let mut preset_interacted = false;
    let mut create_from_preset = false;
    let mut can_create = true;
    let mut invalid_dimension = false;
    let mut recent = None;
    let (key_action, button_action) = crate::dialog_chrome::show_resizable_modal(
        ui.ctx(), "dialog.new_document", &title, egui::vec2(1140.0, 720.0), egui::vec2(370.0, 265.0), |ui, size| {
        let dialog_width = size.x;
        let body_height = (size.y - if template_tools.is_some() { 116.0 } else { 84.0 }).max(60.0);
        let show_recent_column = dialog_width >= 820.0;
        let stacked = dialog_width < 560.0;
        let gap = 10.0;
        let inset = 2.0 * (dtool_design::space("sm") + dtool_design::stroke_width("thin"));
        let recent_width = (dialog_width * 0.19).clamp(148.0, 202.0);
        let settings_width = (dialog_width * 0.29).clamp(246.0, 328.0);
        ui.set_width(dialog_width);
        let mut draw_recent = |ui: &mut egui::Ui, height: f32| {
            egui::ScrollArea::vertical().id_salt("new_document.recent")
                .max_height(height).auto_shrink([false, !show_recent_column]).show(ui, |ui| {
                    let paths = if template_tools.as_ref().is_some_and(|(_, prefs)| !prefs.home_show_recent) {
                        &[][..]
                    } else {
                        workspace.recent()
                    };
                    if paths.is_empty() { ui.weak(text("dialog.new_document.no_recent")); }
                    for path in paths {
                        let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
                        if ui.add(egui::Button::new(name.as_ref()).truncate())
                            .on_hover_text(path.to_string_lossy()).clicked() {
                            recent = Some(path.clone());
                        }
                    }
                });
            if template_tools.is_some() && dtool_design::button(ui, text("menu.file.open")).clicked() {
                *template_action = Some(crate::home::HomeAction::Browse);
            }
        };
        let mut draw_presets = |ui: &mut egui::Ui, state: &mut NewDocumentDialogState, height: f32| {
            let search = ui.add(egui::TextEdit::singleline(&mut state.preset_search)
                .hint_text(text("dialog.new_document.search"))
                .desired_width(ui.available_width()));
            text_editing = search.has_focus() || search.lost_focus();
            let filter_changed = dtool_design::toggle(ui, &mut state.favourites_only, text("pref.home.favourites")).changed();
            if filter_changed { state.select_visible_favourite(); }
            let (interacted, create) = show_document_presets(ui, state, (height - 52.0).max(58.0));
            preset_interacted = interacted || filter_changed;
            create_from_preset = create;
        };
        let mut draw_settings = |ui: &mut egui::Ui, state: &mut NewDocumentDialogState| {
            let name = state.preset.as_deref().unwrap_or(preset.as_str());
            ui.add(egui::Label::new(egui::RichText::new(name).strong()).truncate());
            if state.options.show_kind(ui, &mut state.option_icons) {
                state.error_message = None;
            }
            let layout = state.options.kind == Some(DocumentKind::LayoutPages);
            let (area, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 100.0), egui::Sense::hover());
            ui.painter().rect_filled(area, 4.0, dtool_design::theme::surface(ui.ctx(), "pasteboard"));
            if state.options.facing_preview() && state.width > 0.0 && state.height > 0.0 {
                let half_width = (76.0 * (state.width / state.width.max(state.height))) * 0.5;
                let offset = half_width + 1.0;
                if !state.options.start_on_right() {
                    paint_page_preview(ui, area.center() - egui::vec2(offset, 0.0), state.width, state.height, 76.0);
                }
                if state.options.start_on_right() || state.pages > 1 {
                    paint_page_preview(ui, area.center() + egui::vec2(offset, 0.0), state.width, state.height, 76.0);
                }
            } else {
                paint_page_preview(ui, area.center(), state.width, state.height, 76.0);
            }
            options_group(ui, text("dialog.new_document.dimensions"));
            ui.horizontal(|ui| {
                ui.label(text("doc_setup.orientation"));
                let landscape = state.width > state.height;
                let can_rotate = state.width != state.height;
                for (label, target) in [("dialog.new_document.portrait", false), ("dialog.new_document.landscape", true)] {
                    let direction = ui.add_enabled_ui(can_rotate, |ui| {
                        orientation_button(ui, target, landscape == target, text(label))
                    }).inner;
                    if direction.clicked() && landscape != target {
                        std::mem::swap(&mut state.width, &mut state.height);
                        state.preset = None;
                        state.error_message = None;
                    }
                }
            });
            let mut values = [f64::from(state.width), f64::from(state.height), f64::from(state.dpi), f64::from(state.pages)];
            let mut unit = unit_options.iter().position(|(u, _)| *u == state.units).unwrap_or(0);
            let mut edited = false;
            let reset = std::mem::take(&mut state.reset_dimension_edits);
            let mut dimension = |ui: &mut egui::Ui, label: &str, value: &mut f64, decimals| {
                let label_widget = ui.label(label);
                let id = ui.make_persistent_id(label);
                if reset { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
                let spec = dtool_design::NumericSpec::new("").width(96.0).decimals(decimals);
                edited |= dtool_design::numeric_value(ui, id, value, &spec)
                    .labelled_by(label_widget.id).changed();
                let input = dtool_design::numeric_state(ui.ctx(), id);
                invalid_dimension |= input.edited && input.buffer.as_deref()
                    .is_some_and(|draft| spec.parse_text(draft).is_none());
                ui.end_row();
            };
            let unit_changed = egui::Grid::new("dialog.new_document.fields").num_columns(2).show(ui, |ui| {
                for (label, value) in [width.as_str(), height.as_str()].into_iter().zip(&mut values[..2]) {
                    dimension(ui, label, value, 2);
                }

                ui.label(units.as_str());
                let former_unit = unit;
                ui.horizontal_wrapped(|ui| {
                    for (index, (value, caption)) in unit_options.iter().enumerate() {
                        let response = dtool_design::with_icon_selection(ui, |ui| {
                            ui.add(egui::Button::selectable(index == unit, crate::captions::units_identity(*value)))
                        });
                        if response
                            .on_hover_text(caption).clicked() {
                            unit = index;
                        }
                    }
                });
                ui.end_row();

                for (index, (label, value)) in [dpi.as_str(), pages.as_str()].into_iter().zip(&mut values[2..]).enumerate() {
                    if index == 0 || layout {
                        dimension(ui, label, value, 0);
                    } else if reset {
                        dtool_design::numeric_cancel_edit(ui.ctx(), ui.make_persistent_id(label));
                    }
                }
                unit != former_unit
            }).inner;
            edited |= unit_changed;
            if edited {
                state.width = values[0] as f32;
                state.height = values[1] as f32;
                state.dpi = values[2] as f32;
                state.pages = values[3].max(0.0).round() as u32;
                if let Some((units, _)) = unit_options.get(unit) {
                    state.convert_units(*units);
                }
                state.preset = None;
                state.error_message = None;
            }
            let layout_changed = layout && state.options.show_layout(ui);
            if state.options.show(ui, state.units, state.dpi, reset, &mut invalid_dimension) || layout_changed {
                state.preset = None;
                state.error_message = None;
            }
        };
        if !show_recent_column {
            egui::CollapsingHeader::new(text("pref.home.recent"))
                .id_salt("new_document.recent_compact")
                .show(ui, |ui| draw_recent(ui, 68.0));
        }
        if stacked {
            options_scroll(ui, "new_document.compact", (body_height - 36.0).max(48.0), |ui| {
                    dtool_design::dialog_section(ui, |ui| draw_presets(ui, state, 214.0));
                    dtool_design::dialog_section(ui, |ui| draw_settings(ui, state));
                });
        } else {
            let height = if show_recent_column { body_height } else { (body_height - 36.0).max(48.0) };
            let preset_width = dialog_width - settings_width - gap
                - if show_recent_column { recent_width + gap } else { 0.0 };
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.vertical(|ui| {
                    ui.set_width(preset_width);
                    dtool_design::dialog_section(ui, |ui| {
                        ui.set_width(preset_width - inset);
                        draw_presets(ui, state, height - inset);
                    });
                });
                ui.vertical(|ui| {
                    ui.set_width(settings_width);
                    dtool_design::dialog_section(ui, |ui| {
                        ui.set_width(settings_width - inset);
                        options_scroll(ui, "new_document.settings", height - inset, |ui| draw_settings(ui, state));
                    });
                });
                if show_recent_column {
                    ui.vertical(|ui| {
                        ui.set_width(recent_width);
                        dtool_design::dialog_section(ui, |ui| {
                            ui.set_width(recent_width - inset);
                            dtool_design::panel_header(ui, text("pref.home.recent"));
                            draw_recent(ui, (height - 53.0 - inset).max(48.0));
                        });
                    });
                }
            });
        }

        if let Some((templates, prefs)) = template_tools {
            if let Some(action) = templates.show_template_links(ui, prefs) {
                *template_action = Some(action);
            }
        }

        if invalid_dimension {
            ui.colored_label(ui.visuals().error_fg_color, text("error.invalid_params"));
        } else if let Some(message) = &state.error_message {
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
        ui.separator();
        text_editing |= state.show_save_preset(ui, !invalid_dimension);
        let minimum = egui::vec2(dtool_design::size("button.width"), dtool_design::size("button.height"));
        ui.allocate_ui_with_layout(egui::vec2(dialog_width, minimum.y), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            can_create = !invalid_dimension && (!state.favourites_only || !state.favourite_names().is_empty());
            let create_clicked = ui.add_enabled_ui(can_create, |ui|
                dtool_design::primary_button(ui, &ok, minimum)).inner.clicked();
            let cancel_clicked = dtool_design::button(ui, cancel.as_str()).clicked();
            if create_clicked || (create_from_preset && can_create) {
                DialogResult::Confirm
            } else if cancel_clicked {
                DialogResult::Cancel
            } else {
                DialogResult::Open
            }
        }).inner
    });
    if let Some(path) = recent {
        let command = dtool_protocol::Command::with_params("doc.open", serde_json::json!({ "path": path }));
        return match workspace.run(dtool_core::Scope::active(), command).1 {
            Ok(_) => NewDocumentOutcome::OpenedRecent,
            Err(error) => {
                state.error_message = Some(text(&error.message_key).to_string());
                NewDocumentOutcome::Open
            }
        };
    }
    if template_action.is_some() { return NewDocumentOutcome::Open; }
    let action = if button_action == DialogResult::Open {
        if key_action == DialogResult::Confirm && (text_editing || preset_interacted || !can_create) {
            DialogResult::Open
        } else { key_action }
    } else {
        button_action
    };
    match action {
        DialogResult::Confirm if submit_new_document(workspace, state) => NewDocumentOutcome::Created,
        DialogResult::Cancel => NewDocumentOutcome::Cancelled,
        _ => NewDocumentOutcome::Open,
    }
}

pub(super) fn orientation_button(ui: &mut egui::Ui, landscape: bool, selected: bool, label: &str) -> egui::Response {
    let response = dtool_design::with_dialog_choice(ui, |ui| {
        ui.add_sized([28.0, 24.0], egui::Button::new("").selected(selected))
    });
    let ink = if selected { dtool_design::selected_icon_ink(ui) } else { ui.style().interact(&response).fg_stroke.color };
    let size = if landscape { egui::vec2(15.0, 10.0) } else { egui::vec2(10.0, 15.0) };
    let page = egui::Rect::from_center_size(response.rect.center(), size);
    ui.painter().rect_stroke(page, 1.0, egui::Stroke::new(1.2, ink), egui::StrokeKind::Inside);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
    response.on_hover_text(label)
}

fn show_document_presets(ui: &mut egui::Ui, state: &mut NewDocumentDialogState, height: f32) -> (bool, bool) {
    let filter = state.preset_search.trim().to_lowercase();
    let (print, rest) = NEW_DOCUMENT_PRESETS.split_at(10);
    let (digital, photo) = rest.split_at(3);
    let mut has_results = false;
    let mut interacted = false;
    let mut create = false;
    options_scroll(ui, "new_document.presets", height, |ui| {
        let (saved, changed, create_saved) = state.show_saved_presets(ui, &filter);
        has_results |= saved;
        interacted |= changed;
        create |= create_saved;
        for (category, names) in [
            ("dialog.new_document.print", print),
            ("dialog.new_document.print_cmyk", CMYK_DOCUMENT_PRESETS.as_slice()),
            ("dialog.new_document.photo", photo),
            ("dialog.new_document.digital", digital),
        ] {
            let matches: Vec<_> = names.iter().copied().filter(|name| state.preset_matches(name, &filter)).collect();
            if matches.is_empty() {
                continue;
            }
            has_results = true;
            ui.label(egui::RichText::new(text(category)).strong());
            ui.horizontal_wrapped(|ui| {
                for name in matches {
                    let Some((w, h, units, _)) = new_document_preset(name) else { continue; };
                    let (selected, double_clicked, favourite_clicked) =
                        preset_card(ui, name, w, h, units, state.preset.as_deref() == Some(name), state.is_favourite(name));
                    if favourite_clicked {
                        state.toggle_favourite(name);
                        interacted = true;
                    } else if selected {
                        apply_preset(state, name);
                        interacted = true;
                        create |= double_clicked;
                    }
                }
            });
            ui.add_space(10.0);
        }
        if !has_results {
            ui.weak(text(if state.favourites_only && filter.is_empty() {
                "dialog.new_document.no_favourites"
            } else { "dialog.new_document.no_presets" }));
        }
    });
    (interacted, create)
}

fn preset_card(ui: &mut egui::Ui, name: &str, width: f32, height: f32, units: Units, selected: bool, favourite: bool) -> (bool, bool, bool) {
    let (id, card) = ui.allocate_space(egui::vec2(116.0, 104.0));
    let response = ui.interact(card, id, egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, name));
    let visuals = ui.style().interact(&response);
    let accent = dtool_design::theme::surface(ui.ctx(), "dialog.choice_border");
    let fill = if selected { dtool_design::theme::surface(ui.ctx(), "dialog.choice") }
        else if response.hovered() || response.is_pointer_button_down_on() || response.has_focus() { visuals.bg_fill }
        else { dtool_design::theme::surface(ui.ctx(), "dialog.section") };
    let border = if response.has_focus() { egui::Stroke::new(2.0, ui.visuals().text_color()) }
        else if selected { egui::Stroke::new(1.0, accent) } else { visuals.bg_stroke };
    ui.painter().rect(card, 5.0, fill, border, egui::StrokeKind::Inside);
    paint_page_preview(ui, egui::pos2(card.center().x, card.top() + 32.0), width, height, 42.0);
    let dimensions = format!("{width} × {height} {}", crate::captions::units_identity(units));
    for (label, y, style, colour) in [
        (name, 65.0, egui::TextStyle::Body, ui.visuals().text_color()),
        (&dimensions, 84.0, egui::TextStyle::Small,
            if selected { ui.visuals().text_color() } else { ui.visuals().weak_text_color() }),
    ] {
        let galley = egui::WidgetText::from(label)
            .into_galley(ui, Some(egui::TextWrapMode::Truncate), card.width() - 8.0, style);
        let center = egui::pos2(card.center().x, card.top() + y);
        ui.painter().galley(center - galley.size() * 0.5, galley, colour);
    }
    let star_rect = egui::Rect::from_min_size(card.right_top() + egui::vec2(-23.0, 3.0), egui::vec2(20.0, 20.0));
    let star = ui.interact(star_rect, id.with("favourite"), egui::Sense::click());
    let star_colour = if favourite || selected { accent } else { ui.style().interact(&star).fg_stroke.color };
    ui.painter().text(star_rect.center(), egui::Align2::CENTER_CENTER,
        if favourite { "★" } else { "☆" }, egui::TextStyle::Body.resolve(ui.style()), star_colour);
    let action = text(if favourite { "dialog.new_document.unfavourite" } else { "dialog.new_document.favourite" });
    let label = std::format!("{action} · {name}");
    star.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), favourite, &label));
    let changed = star.on_hover_text(label).clicked();
    let response = response.on_hover_text(name);
    (response.clicked(), response.double_clicked(), changed)
}

pub(crate) fn paint_page_preview(ui: &egui::Ui, center: egui::Pos2, width: f32, height: f32, side: f32) {
    if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 {
        let size = egui::vec2(width, height) * (side / width.max(height));
        let page = egui::Rect::from_center_size(center, size);
        ui.painter().rect_filled(page, 2.0, egui::Color32::from_gray(240));
        ui.painter().rect_stroke(page, 2.0, egui::Stroke::new(1.0, dtool_design::color("border")), egui::StrokeKind::Inside);
    }
}

pub(crate) fn new_document_preset(name: &str) -> Option<(f32, f32, Units, f32)> {
    let name = if CMYK_DOCUMENT_PRESETS.contains(&name) { name.strip_suffix(" (CMYK)")? } else { name };
    match name {
        "A3" => Some((297.0, 420.0, Units::Mm, 300.0)),
        "A4" => Some((210.0, 297.0, Units::Mm, 300.0)),
        "A5" => Some((148.0, 210.0, Units::Mm, 300.0)),
        "B5 (JIS)" => Some((182.0, 257.0, Units::Mm, 300.0)),
        "Letter" => Some((8.5, 11.0, Units::In, 300.0)),
        "Legal" => Some((8.5, 14.0, Units::In, 300.0)),
        "Ledger" => Some((11.0, 17.0, Units::In, 300.0)),
        "Trade Paperback" => Some((6.0, 9.0, Units::In, 300.0)),
        "Half Letter" => Some((5.5, 8.5, Units::In, 300.0)),
        "Oversize Magazine" => Some((9.0, 12.0, Units::In, 300.0)),
        "Square 1080" => Some((1080.0, 1080.0, Units::Px, 72.0)),
        "Full HD" => Some((1920.0, 1080.0, Units::Px, 72.0)),
        "4K UHD" => Some((3840.0, 2160.0, Units::Px, 72.0)),
        "4R" => Some((6.0, 4.0, Units::In, 300.0)),
        "5R" => Some((7.0, 5.0, Units::In, 300.0)),
        "6R" => Some((8.0, 6.0, Units::In, 300.0)),
        "8R" => Some((10.0, 8.0, Units::In, 300.0)),
        "10R" => Some((12.0, 10.0, Units::In, 300.0)),
        _ => None,
    }
}

pub fn apply_preset(state: &mut NewDocumentDialogState, preset_name: &str) {
    if let Some((width, height, units, dpi)) = new_document_preset(preset_name) {
        state.width = width;
        state.height = height;
        state.units = units;
        state.dpi = dpi;
        if state.options.kind.is_none() { state.pages = 1; }
        let colour_format = if CMYK_DOCUMENT_PRESETS.contains(&preset_name) { ColourFormat::CMYK_8 } else { ColourFormat::RGBA_8 };
        state.options.reset_for_builtin_preset(colour_format);
        state.preset = Some(preset_name.to_string());
        state.error_message = None;
        state.reset_dimension_edits = true;
    } else {
        state.preset = None;
    }
}

pub fn create_document_from_dialog(state: &NewDocumentDialogState) -> Result<dtool_core::document::Document, dtool_core::CommandError> {
    prepare_new_document(state, state.pages)
}

fn prepare_new_document(state: &NewDocumentDialogState, pages: u32) -> Result<dtool_core::document::Document, dtool_core::CommandError> {
    let mut command = state.command();
    command.params["pages"] = serde_json::json!(pages);
    dtool_core::commands::document_colour::prepare_new_document(&command.params)
}

pub fn get_dialog_labels() -> (String, String, String, String, String, String, String, String, String) {
    (
        text("dialog.new_document.title").to_string(),
        text("dialog.new_document.width").to_string(),
        text("dialog.new_document.height").to_string(),
        text("dialog.new_document.units").to_string(),
        text("dialog.new_document.dpi").to_string(),
        text("dialog.new_document.pages").to_string(),
        text("dialog.new_document.preset").to_string(),
        text("dialog.new_document.ok").to_string(),
        text("dialog.new_document.cancel").to_string(),
    )
}

pub fn new_document_unit_options() -> Vec<(Units, String)> {
    crate::captions::units_options()
}

pub fn get_unit_labels() -> (String, String, String, String, String) {
    (
        text("units.px").to_string(),
        text("units.mm").to_string(),
        text("units.cm").to_string(),
        text("units.in").to_string(),
        text("units.pt").to_string(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOwner {
    App,
    Document(ContextId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DialogKind {
    CloseDocument,
    About,
    PasteSpecial,
    McpConfirm,
    File,
    NewDocument,
    Recovery,
    Export,
    Guides,
    GridAxis,
    BaselineGrid,
    LockedDelete,
    ResourceManager,
}

#[derive(Debug, Clone)]
pub struct OpenDialog {
    pub owner: DialogOwner,
    pub kind: DialogKind,
}

impl DialogOwner {
    pub fn context(&self) -> Option<ContextId> {
        match self {
            DialogOwner::App => None,
            DialogOwner::Document(id) => Some(*id),
        }
    }
}

pub fn dialog_session<'a>(workspace: &'a mut dtool_core::Workspace, owner: &DialogOwner) -> Result<&'a mut Session, dtool_core::CommandError> {
    match owner {
        DialogOwner::App => Err(dtool_core::CommandError::no_document()),
        DialogOwner::Document(id) => workspace.get_mut(*id).map(|context| &mut context.session).ok_or_else(dtool_core::CommandError::context_not_found),
    }
}

pub fn owner_alive(workspace: &dtool_core::Workspace, owner: &DialogOwner) -> bool {
    match owner {
        DialogOwner::App => true,
        DialogOwner::Document(id) => workspace.get(*id).is_some(),
    }
}

impl OpenDialog {
    pub fn new(owner: DialogOwner, kind: DialogKind) -> Self {
        Self { owner, kind }
    }
}

#[derive(Debug, Default, Clone)]
pub struct DialogRegistry {
    pub dialogs: Vec<OpenDialog>,
}

impl DialogRegistry {
    pub fn insert(&mut self, owner: DialogOwner, kind: DialogKind) {
        self.dialogs.retain(|dialog| dialog.kind != kind);
        self.dialogs.push(OpenDialog::new(owner, kind));
    }

    pub fn close_kind(&mut self, kind: DialogKind) {
        self.dialogs.retain(|dialog| dialog.kind != kind);
    }

    pub fn drop_gone(&mut self, workspace: &dtool_core::Workspace) -> bool {
        let before = self.dialogs.len();
        self.dialogs.retain(|dialog| owner_alive(workspace, &dialog.owner));
        self.dialogs.len() != before
    }

    pub fn owner_for(&self, kind: DialogKind) -> Option<&DialogOwner> {
        self.dialogs.iter().find(|dialog| dialog.kind == kind).map(|dialog| &dialog.owner)
    }

    pub fn confirm_session<'a>(
        &self,
        workspace: &'a mut dtool_core::Workspace,
        kind: DialogKind,
    ) -> Result<&'a mut Session, dtool_core::CommandError> {
        let owner = self.owner_for(kind).cloned().ok_or_else(dtool_core::CommandError::context_not_found)?;
        dialog_session(workspace, &owner)
    }

    pub fn sync(&mut self, kind: DialogKind, owner: Option<DialogOwner>) {
        match owner {
            Some(owner) if self.owner_for(kind) == Some(&owner) => {}
            Some(owner) => self.insert(owner, kind),
            None => self.close_kind(kind),
        }
    }

    pub fn sync_document(&mut self, kind: DialogKind, open: bool, id: Option<ContextId>) {
        self.sync(kind, if open { id.map(DialogOwner::Document) } else { None });
    }

    pub fn is_open(&self, kind: DialogKind) -> bool {
        self.owner_for(kind).is_some()
    }

    pub fn kinds_for(&self, id: ContextId) -> Vec<DialogKind> {
        self.dialogs.iter().filter(|dialog| dialog.owner == DialogOwner::Document(id)).map(|dialog| dialog.kind).collect()
    }

    pub fn gone_kinds(&self, workspace: &dtool_core::Workspace) -> Vec<DialogKind> {
        self.dialogs.iter().filter(|dialog| !owner_alive(workspace, &dialog.owner)).map(|dialog| dialog.kind).collect()
    }

    pub fn take_gone(&mut self, workspace: &dtool_core::Workspace) -> Vec<DialogKind> {
        let gone = self.gone_kinds(workspace);
        self.dialogs.retain(|dialog| !gone.contains(&dialog.kind));
        gone
    }
}

#[derive(Debug, Clone)]
pub struct CloseDocumentDialogState {
    pub context_id: ContextId,
    pub document_name: String,
    pub error_message: Option<String>,
}

impl CloseDocumentDialogState {
    pub fn new(context_id: ContextId, document_name: String) -> Self {
        Self {
            context_id,
            document_name,
            error_message: None,
        }
    }
}

pub fn show_close_document_dialog(
    ui: &mut egui::Ui,
    state: &mut CloseDocumentDialogState,
    close_with: &dyn Fn(ContextId, CloseAnswer) -> Result<bool, dtool_core::CommandError>,
) -> Option<CloseAnswer> {
    let mut answer: Option<CloseAnswer> = None;

    let (result, ()) = show_modal(ui.ctx(), "dialog.close", text("dialog.close.title"), ClosePolicy::CancelOnly, false, |ui| {
            ui.vertical(|ui| {
                ui.label(format("dialog.close.modified", &[("name", &state.document_name)]));
                ui.label(text("dialog.close.question"));

                if let Some(ref err) = state.error_message {
                    ui.label(err.as_str());
                }

                ui.add_space(dtool_design::space("md"));

                ui.horizontal(|ui| {
                    if dtool_design::button(ui, text("menu.file.save")).clicked() {
                        answer = Some(CloseAnswer::Yes);
                    }
                    if dtool_design::button(ui, text("dialog.close.discard")).clicked() {
                        answer = Some(CloseAnswer::No);
                    }
                    if dtool_design::button(ui, text("button.cancel")).clicked() {
                        answer = Some(CloseAnswer::Cancel);
                    }
                });

            });
        });
    if result == DialogResult::Cancel { answer = Some(CloseAnswer::Cancel); }

    if let Some(ans) = answer {
        match ans {
            CloseAnswer::Yes => {
                match close_with(state.context_id, CloseAnswer::Yes) {
                    Ok(_) => return Some(CloseAnswer::Yes),
                    Err(e) => {
                        state.error_message = Some(text(&e.message_key).to_string());
                        return None;
                    }
                }
            }
            CloseAnswer::No => {
                match close_with(state.context_id, CloseAnswer::No) {
                    Ok(_) => return Some(CloseAnswer::No),
                    Err(e) => {
                        state.error_message = Some(text(&e.message_key).to_string());
                        return None;
                    }
                }
            }
            CloseAnswer::Cancel => {
                return Some(CloseAnswer::Cancel);
            }
        }
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseResolution {
    Closed,
    Kept,
    NeedsSaveAs,
}

pub fn resolve_close_answer(workspace: &mut dtool_core::Workspace, id: ContextId, answer: CloseAnswer) -> Result<CloseResolution, dtool_core::CommandError> {
    if answer == CloseAnswer::Yes && workspace.get(id).is_some_and(|context| context.path.is_none()) {
        return Ok(CloseResolution::NeedsSaveAs);
    }
    match workspace.close_with(id, answer)? {
        true => Ok(CloseResolution::Closed),
        false => Ok(CloseResolution::Kept),
    }
}

pub fn close_document_name(workspace: &dtool_core::Workspace, id: ContextId) -> Option<String> {
    let context = workspace.get(id)?;
    Some(if context.title.is_empty() { text("doc.untitled").to_string() } else { context.title.clone() })
}

pub fn sync_close_dialog(pending: &mut Option<ContextId>, state: &mut Option<CloseDocumentDialogState>, workspace: &dtool_core::Workspace) {
    let Some(id) = *pending else {
        *state = None;
        return;
    };
    match close_document_name(workspace, id) {
        Some(name) => {
            if state.as_ref().is_none_or(|current| current.context_id != id) {
                *state = Some(CloseDocumentDialogState::new(id, name));
            }
        }
        None => {
            *pending = None;
            *state = None;
        }
    }
}

pub fn show_pending_close(
    ui: &mut egui::Ui,
    pending: &mut Option<ContextId>,
    state: &mut Option<CloseDocumentDialogState>,
    workspace: &mut dtool_core::Workspace,
) -> Option<ContextId> {
    sync_close_dialog(pending, state, workspace);
    let Some(current) = state.as_mut() else { return None; };
    let save_as = std::cell::Cell::new(None);
    let workspace = std::cell::RefCell::new(workspace);
    let answer = show_close_document_dialog(ui, current, &|id, answer| {
        let resolution = resolve_close_answer(&mut workspace.borrow_mut(), id, answer)?;
        if resolution == CloseResolution::NeedsSaveAs { save_as.set(Some(id)); }
        Ok(resolution == CloseResolution::Closed)
    });
    if answer.is_some() {
        *pending = None;
        *state = None;
    }
    save_as.get()
}

#[derive(Default)]
pub(crate) struct ExitReview {
    prompt: Option<CloseDocumentDialogState>,
    save_as: Option<FileDialogState>,
    // A later edit invalidates an earlier decision to close without saving.
    discarded: std::collections::HashMap<ContextId, u64>,
}

impl ExitReview {
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, workspace: &mut dtool_core::Workspace) -> DialogResult {
        if let Some(save_as) = &mut self.save_as {
            match file_dialog_result(ui, save_as, workspace) {
                DialogResult::Cancel => return DialogResult::Cancel,
                DialogResult::Confirm => {
                    self.save_as = None;
                    ui.ctx().request_repaint();
                }
                DialogResult::Open => {}
            }
            return DialogResult::Open;
        }

        let pending = |context: &dtool_core::workspace::DocContext| {
            (context.is_modified() || context.pending_edit)
                && self.discarded.get(&context.id) != Some(&context.session.generation)
        };
        let current = self.prompt.as_ref().and_then(|prompt| workspace.get(prompt.context_id)).filter(|context| pending(context));
        let Some(context) = current.or_else(|| workspace.contexts().iter().find(|context| pending(context))) else {
            return DialogResult::Confirm;
        };
        let id = context.id;
        let generation = context.session.generation;
        let untitled = context.path.is_none();
        if self.prompt.as_ref().is_none_or(|prompt| prompt.context_id != id) {
            let name = close_document_name(workspace, id).unwrap_or_default();
            self.prompt = Some(CloseDocumentDialogState::new(id, name));
        }
        let workspace = std::cell::RefCell::new(workspace);
        let answer = self.prompt.as_mut().and_then(|prompt| {
            show_close_document_dialog(ui, prompt, &|id, answer| {
                if answer == CloseAnswer::Yes {
                    let mut workspace = workspace.borrow_mut();
                    workspace.finish_pending_edit(id, true)?;
                    if !untitled {
                        workspace.save_context(id)?;
                    }
                }
                Ok(true)
            })
        });
        match answer {
            Some(CloseAnswer::Cancel) => return DialogResult::Cancel,
            Some(CloseAnswer::No) => { self.discarded.insert(id, generation); }
            Some(CloseAnswer::Yes) if untitled => {
                self.save_as = Some(FileDialogState::for_document(FileDialogMode::SaveAs, id));
            }
            Some(CloseAnswer::Yes) => {}
            None => return DialogResult::Open,
        }
        self.prompt = None;
        ui.ctx().request_repaint();
        DialogResult::Open
    }
}

pub fn paste_format_label(format: &str) -> String {
    let key = match format {
        dtool_core::command::PASTE_FORMAT_DTOOL => "dialog.paste_special.format.dtool",
        dtool_core::command::PASTE_FORMAT_RTF => "dialog.paste_special.format.rtf",
        dtool_core::command::PASTE_FORMAT_UNICODE_TEXT => "dialog.paste_special.format.cf_unicodetext",
        dtool_core::command::PASTE_FORMAT_TEXT => "dialog.paste_special.format.cf_text",
        _ => return format.to_string(),
    };
    text(key).to_string()
}

#[derive(Debug, Clone, Default)]
pub struct PasteSpecialDialogState {
    pub formats: Vec<String>,
    pub selected: Option<usize>,
    pub error_message: Option<String>,
}

impl PasteSpecialDialogState {
    pub fn from_session(session: &Session) -> Self {
        let formats = session.paste_special_formats();
        let selected = if formats.is_empty() { None } else { Some(0) };
        Self {
            formats,
            selected,
            error_message: None,
        }
    }

    pub fn selected_format(&self) -> Option<&str> {
        self.selected.and_then(|i| self.formats.get(i)).map(|s| s.as_str())
    }

    pub fn apply(&mut self, session: &mut Session) -> bool {
        let format = match self.selected_format() {
            Some(f) => f.to_string(),
            None => {
                self.error_message = Some(text("error.invalid_params").to_string());
                return false;
            }
        };
        match session.run_named("edit.paste_special", serde_json::json!({ "format": format })) {
            Ok(_) => true,
            Err(e) => {
                self.error_message = Some(text(&e.message_key).to_string());
                false
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteSpecialOutcome {
    Applied,
    Cancelled,
}

pub fn show_paste_special_dialog(
    ui: &mut egui::Ui,
    state: &mut PasteSpecialDialogState,
    session: &mut Session,
) -> Option<PasteSpecialOutcome> {
    let mut outcome: Option<PasteSpecialOutcome> = None;
    let mut accept = false;

    egui::Window::new(text("dialog.paste_special.title"))
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            dtool_design::dialog_frame(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(text("dialog.paste_special.paste_as"));
                    ui.add_space(dtool_design::space("xl"));
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical()
                            .max_height(dtool_design::size("dialog.max_body_height"))
                            .show(ui, |ui| {
                                for (i, format) in state.formats.iter().enumerate() {
                                    let selected = state.selected == Some(i);
                                    let response = ui.selectable_label(selected, paste_format_label(format));
                                    if response.clicked() {
                                        state.selected = Some(i);
                                    }
                                    if response.double_clicked() {
                                        state.selected = Some(i);
                                        accept = true;
                                    }
                                }
                            });
                    });
                });

                if let Some(ref err) = state.error_message {
                    ui.label(err.as_str());
                }

                ui.add_space(dtool_design::space("xl"));

                ui.horizontal(|ui| {
                    let can_accept = state.selected.is_some();
                    if ui
                        .add_enabled_ui(can_accept, |ui| dtool_design::button(ui, text("button.ok")))
                        .inner
                        .clicked()
                    {
                        accept = true;
                    }
                    if dtool_design::button(ui, text("button.cancel")).clicked() {
                        outcome = Some(PasteSpecialOutcome::Cancelled);
                    }
                });

                match dtool_design::dialog_keys(ui) {
                    Some(dtool_design::DialogKey::Cancel) => outcome = Some(PasteSpecialOutcome::Cancelled),
                    Some(dtool_design::DialogKey::Confirm) if state.selected.is_some() => accept = true,
                    _ => {}
                }
            });
        });

    if outcome.is_none() && accept {
        if state.apply(session) {
            outcome = Some(PasteSpecialOutcome::Applied);
        }
    }

    outcome
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDialogMode { Open, SaveAs, TemplateOpen, TemplateSave, ImportText, ReplaceImage(u64), SelectionSave, SelectionLoad }

impl FileDialogMode {
    pub fn title_key(self) -> &'static str {
        match self {
            Self::Open => "dialog.open.title",
            Self::SaveAs => "dialog.save_as.title",
            Self::TemplateOpen => "dialog.template.open",
            Self::TemplateSave => "dialog.template.save",
            Self::ImportText => "menu.file.import_from_files",
            Self::ReplaceImage(_) => "frame.replace_image",
            Self::SelectionSave => "menu.selection.save_file",
            Self::SelectionLoad => "menu.selection.load_file",
        }
    }
    pub fn command_name(self) -> &'static str {
        match self {
            Self::Open => "doc.open",
            Self::SaveAs => "doc.save_as",
            Self::TemplateOpen => "doc.template.open",
            Self::TemplateSave => "doc.template.save",
            Self::ImportText => "text.import",
            Self::ReplaceImage(_) => "resource.replace",
            Self::SelectionSave => "selection.save",
            Self::SelectionLoad => "selection.load",
        }
    }

    pub(crate) fn needs_owner(self) -> bool {
        matches!(self, Self::SaveAs | Self::TemplateSave | Self::ReplaceImage(_) | Self::SelectionSave | Self::SelectionLoad)
    }
}

#[derive(Debug, Clone)]
pub struct FileDialogState {
    pub mode: FileDialogMode,
    pub path: String,
    pub error_message: Option<String>,
    pub owner: DialogOwner,
    pub close_after_save: bool,
    focus_path: bool,
    overwrite: bool,
    outline_pdf_text: bool,
}

impl FileDialogState {
    pub fn new(mode: FileDialogMode) -> Self {
        Self {
            mode, path: String::new(), error_message: None, owner: DialogOwner::App,
            close_after_save: false, focus_path: true, overwrite: false, outline_pdf_text: false,
        }
    }
    pub fn for_document(mode: FileDialogMode, id: ContextId) -> Self {
        Self { owner: DialogOwner::Document(id), ..Self::new(mode) }
    }
    pub fn closing(mut self) -> Self {
        self.close_after_save = true;
        self
    }
    fn is_pdf_open(&self) -> bool {
        self.mode == FileDialogMode::Open
            && std::path::Path::new(self.path.trim()).extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
    }
    pub fn apply(&mut self, workspace: &mut dtool_core::Workspace) -> bool {
        if self.path.trim().is_empty() {
            self.error_message = Some(text("error.invalid_params").to_string());
            return false;
        }
        let owner = self.owner.context();
        if self.mode.needs_owner() && owner.is_none_or(|id| workspace.get(id).is_none()) {
            self.error_message = Some(text("error.context_not_found").to_string());
            return false;
        }
        if self.mode == FileDialogMode::SaveAs {
            let Some(id) = owner else { return false; };
            match save_document_as(workspace, id, std::path::Path::new(&self.path)) {
                Ok(()) => {
                    self.error_message = None;
                    if self.close_after_save {
                        if let Err(error) = workspace.close_with(id, dtool_core::workspace::CloseAnswer::No) {
                            self.error_message = Some(text(&error.message_key).to_string());
                            return false;
                        }
                    }
                    return true;
                }
                Err(error) => { self.error_message = Some(text(&error.message_key).to_string()); return false; }
            }
        }
        let scope = match owner { Some(id) => dtool_core::Scope::of(id), None => dtool_core::Scope::active() };
        let params = match self.mode {
            FileDialogMode::ReplaceImage(object) => serde_json::json!({ "object": object, "path": self.path.trim(), "link": false }),
            FileDialogMode::SelectionSave => {
                let path = dtool_core::selection::with_afselection_extension(std::path::Path::new(self.path.trim()));
                match path.try_exists() {
                    Ok(false) => serde_json::json!({ "to": "file", "path": path }),
                    Ok(true) => {
                        self.error_message = Some(text("selection.file_already_exists").into());
                        return false;
                    }
                    Err(_) => {
                        self.error_message = Some(text("error.io_error").into());
                        return false;
                    }
                }
            }
            FileDialogMode::SelectionLoad => serde_json::json!({ "from": "file", "path": self.path.trim() }),
            _ => serde_json::json!({ "path": self.path }),
        };
        let command = if self.is_pdf_open() && self.outline_pdf_text {
            dtool_core::Command::with_params("import.pdf", serde_json::json!({
                "path": self.path.trim(), "options": {"outline_embedded_text": true}
            }))
        } else {
            dtool_core::Command { name: self.mode.command_name().into(), params }
        };
        let (_, result) = workspace.run(scope, command);
        match result {
            Ok(_) => { self.error_message = None; true }
            Err(error) => { self.error_message = Some(text(&error.message_key).to_string()); false }
        }
    }
}

pub fn show_file_dialog(ui: &mut egui::Ui, state: &mut FileDialogState, workspace: &mut dtool_core::Workspace) -> bool {
    file_dialog_result(ui, state, workspace) != DialogResult::Open
}

pub(crate) fn file_dialog_result(ui: &mut egui::Ui, state: &mut FileDialogState, workspace: &mut dtool_core::Workspace) -> DialogResult {
    if state.overwrite {
        let (key, button) = show_modal(ui.ctx(), "dialog.file.overwrite", text("dialog.save_as.title"), ClosePolicy::CancelOnly, false, |ui| {
            ui.label(text("dialog.save_as.overwrite.question"));
            ui.add(egui::Label::new(&state.path).wrap());
            ui.horizontal(|ui| {
                let cancel = dtool_design::button(ui, text("button.cancel")).clicked();
                let overwrite = dtool_design::button(ui, text("dialog.save_as.overwrite.action")).clicked();
                if cancel {
                    DialogResult::Cancel
                } else if overwrite {
                    DialogResult::Confirm
                } else {
                    DialogResult::Open
                }
            }).inner
        });
        if key == DialogResult::Cancel || button == DialogResult::Cancel {
            state.overwrite = false;
            state.focus_path = true;
        } else if button == DialogResult::Confirm {
            state.overwrite = false;
            if state.apply(workspace) {
                return DialogResult::Confirm;
            }
            state.focus_path = true;
        }
        return DialogResult::Open;
    }
    #[cfg(windows)]
    let mut browse = false;
    let title = if state.is_pdf_open() { "import.pdf.title" } else { state.mode.title_key() };
    let (key_action, button_action) = show_modal(ui.ctx(), "dialog.file", text(title), ClosePolicy::CancelOnly, true, |ui| {
            let label = ui.label(text("dialog.file.path"));
            ui.horizontal(|ui| {
                let editor = ui.text_edit_singleline(&mut state.path).labelled_by(label.id);
                if std::mem::take(&mut state.focus_path) {
                    editor.request_focus();
                }
                #[cfg(windows)]
                if state.mode == FileDialogMode::SaveAs {
                    browse = dtool_design::button(ui, text("pref.browse")).clicked();
                }
            });
            let filter = match state.mode { FileDialogMode::Open => "dialog.file.filter.open", FileDialogMode::TemplateOpen | FileDialogMode::TemplateSave => "dialog.file.filter.template", FileDialogMode::ImportText => "dialog.file.filter.text", FileDialogMode::ReplaceImage(_) => "dialog.file.filter.image", FileDialogMode::SelectionSave | FileDialogMode::SelectionLoad => "dialog.file.filter.selection", _ => "dialog.file.filter.native" };
            if state.is_pdf_open() {
                dtool_design::toggle(ui, &mut state.outline_pdf_text, text("import.pdf.outline_embedded_text"));
                ui.label(text("import.pdf.outline_embedded_text.help"));
            } else if dtool_strings::has(filter) {
                ui.label(text(filter));
            }
            if let Some(message) = &state.error_message { ui.label(message); }
            ui.horizontal(|ui| {
                let confirm = dtool_design::button(ui, text("button.ok")).clicked();
                let cancel = dtool_design::button(ui, text("button.cancel")).clicked();
                if cancel { DialogResult::Cancel }
                else if confirm { DialogResult::Confirm }
                else { DialogResult::Open }
            }).inner
    });
    #[cfg(windows)]
    if browse && key_action != DialogResult::Cancel {
        let initial = if state.path.is_empty() {
            state.owner.context().and_then(|id| workspace.get(id)).and_then(|context| context.path.as_deref())
                .map(|path| path.to_string_lossy()).unwrap_or_default()
        } else {
            std::borrow::Cow::Borrowed(state.path.as_str())
        };
        match pick_file_path(text(state.mode.title_key()), text("dialog.file.filter.native"), "dtool", Some(&initial)) {
            Ok(Some(path)) => {
                state.path = path.to_string_lossy().into_owned();
                return if state.apply(workspace) { DialogResult::Confirm } else { DialogResult::Open };
            }
            Ok(None) => { state.focus_path = true; }
            Err(_) => { state.error_message = Some(text("error.io_error").to_string()); }
        }
        return DialogResult::Open;
    }
    let action = if key_action == DialogResult::Cancel || button_action == DialogResult::Open { key_action } else { button_action };
    if action == DialogResult::Confirm && state.mode == FileDialogMode::SaveAs {
        let path = std::path::Path::new(&state.path);
        if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("dtool")) {
            match path.symlink_metadata() {
                Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                    state.error_message = None;
                    state.overwrite = true;
                    return DialogResult::Open;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    state.error_message = Some(text("error.io_error").to_string());
                    state.focus_path = true;
                    return DialogResult::Open;
                }
            }
        }
    }
    match action {
        DialogResult::Confirm if !state.apply(workspace) => DialogResult::Open,
        result => result,
    }
}

#[cfg(windows)]
pub(crate) fn pick_document_path() -> std::io::Result<Option<std::path::PathBuf>> {
    pick_file_path_with_extensions(
        text(FileDialogMode::Open.title_key()), text("dialog.file.filter.open"),
        &["dtool", "svg", "pdf", "ai", "eps"], None,
    )
}

#[cfg(windows)]
pub(crate) fn pick_file_path(title: &str, description: &str, extension: &str, save_as: Option<&str>) -> std::io::Result<Option<std::path::PathBuf>> {
    pick_file_path_with_extensions(title, description, &[extension], save_as)
}

#[cfg(windows)]
pub(crate) fn pick_folder_path(title: &str) -> std::io::Result<Option<std::path::PathBuf>> {
    pick_folder(title, 0)
}

#[cfg(windows)]
pub(crate) fn pick_folder_path_with_address(title: &str) -> std::io::Result<Option<std::path::PathBuf>> {
    use windows_sys::Win32::UI::Shell::BIF_EDITBOX;
    pick_folder(title, BIF_EDITBOX)
}

#[cfg(windows)]
fn pick_folder(title: &str, flags: u32) -> std::io::Result<Option<std::path::PathBuf>> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows_sys::Win32::UI::Shell::{BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, GPFIDL_DEFAULT,
        SHBrowseForFolderW, SHGetPathFromIDListEx};

    let title: Vec<_> = title.encode_utf16().chain([0]).collect();
    let mut display_name = [0_u16; 260];
    let mut path = vec![0_u16; 32768];
    // The dialog and returned identifier are used on the same UI thread. Every
    // successful COM initialization is balanced, including when the user cancels.
    let converted = unsafe {
        let init = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        if init < 0 {
            return Err(std::io::Error::other(format!("Folder dialog initialization error {init:#x}")));
        }
        let dialog = BROWSEINFOW {
            hwndOwner: GetActiveWindow(),
            pszDisplayName: display_name.as_mut_ptr(),
            lpszTitle: title.as_ptr(),
            ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | flags,
            ..Default::default()
        };
        let item = SHBrowseForFolderW(&dialog);
        let converted = if item.is_null() {
            None
        } else {
            let ok = SHGetPathFromIDListEx(item, path.as_mut_ptr(), path.len() as u32, GPFIDL_DEFAULT) != 0;
            CoTaskMemFree(item.cast());
            Some(ok)
        };
        CoUninitialize();
        converted
    };
    if let Some(ok) = converted {
        if !ok { return Err(std::io::Error::other("Could not resolve the selected folder")); }
        let end = path.iter().position(|unit| *unit == 0).unwrap_or(path.len());
        return Ok(Some(std::ffi::OsString::from_wide(&path[..end]).into()));
    }
    Ok(None)
}

#[cfg(windows)]
pub(crate) fn pick_file_path_with_extensions(title: &str, description: &str, extensions: &[&str], save_as: Option<&str>) -> std::io::Result<Option<std::path::PathBuf>> {
    pick_paths(title, description, extensions, save_as, false).map(|paths| paths.into_iter().next())
}

#[cfg(windows)]
pub(crate) fn pick_multiple_file_paths(title: &str, description: &str, extensions: &[&str]) -> std::io::Result<Vec<std::path::PathBuf>> {
    pick_paths(title, description, extensions, None, true)
}

#[cfg(windows)]
fn pick_paths(title: &str, description: &str, extensions: &[&str], save_as: Option<&str>, multiple: bool) -> std::io::Result<Vec<std::path::PathBuf>> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Controls::Dialogs::{
        CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR,
        OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;

    let title: Vec<u16> = title.encode_utf16().chain([0]).collect();
    let pattern = extensions.iter().map(|extension| format!("*.{extension}")).collect::<Vec<_>>().join(";");
    let filter: Vec<u16> = format!("{description}\0{pattern}\0\0").encode_utf16().collect();
    let extension: Vec<u16> = extensions.first().copied().unwrap_or_default().encode_utf16().chain([0]).collect();
    let mut filename = vec![0_u16; if multiple { 262144 } else { 32768 }];
    let mut flags = OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    if let Some(initial) = save_as {
        flags |= OFN_OVERWRITEPROMPT;
        for (target, unit) in filename[..32767].iter_mut().zip(initial.encode_utf16()) {
            *target = unit;
        }
    } else {
        flags |= OFN_FILEMUSTEXIST;
        if multiple { flags |= OFN_ALLOWMULTISELECT; }
    }
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        // The picker runs on the same UI thread as the active DTool window.
        hwndOwner: unsafe { GetActiveWindow() },
        lpstrFilter: filter.as_ptr(),
        nFilterIndex: 1,
        lpstrFile: filename.as_mut_ptr(),
        nMaxFile: filename.len() as u32,
        lpstrTitle: title.as_ptr(),
        lpstrDefExt: extension.as_ptr(),
        Flags: flags,
        ..Default::default()
    };
    // The UTF-16 buffers above remain valid for the entire synchronous call.
    let chosen = unsafe {
        if save_as.is_some() { GetSaveFileNameW(&mut dialog) } else { GetOpenFileNameW(&mut dialog) }
    };
    if chosen == 0 {
        let error = unsafe { CommDlgExtendedError() };
        return if error == 0 { Ok(Vec::new()) } else { Err(std::io::Error::other(format!("File dialog error {error:#x}"))) };
    }
    let mut fields = filename.split(|unit| *unit == 0).take_while(|field| !field.is_empty())
        .map(std::ffi::OsString::from_wide);
    let Some(first) = fields.next() else { return Ok(Vec::new()); };
    let first = std::path::PathBuf::from(first);
    let rest = fields.map(|name| first.join(name)).collect::<Vec<_>>();
    Ok(if rest.is_empty() { vec![first] } else { rest })
}

pub fn save_document_as(workspace: &mut dtool_core::Workspace, id: ContextId, path: &std::path::Path) -> Result<(), dtool_core::CommandError> {
    if path.extension().is_none_or(|extension| !extension.eq_ignore_ascii_case("dtool")) {
        return Err(dtool_core::CommandError::invalid_params());
    }
    if workspace.get(id).is_none() {
        return Err(dtool_core::CommandError::context_not_found());
    }
    workspace.save_as(id, path)
}

#[derive(Default)]
pub struct RecoveryOfferState {
    pub candidates: std::collections::VecDeque<dtool_core::autosave::RecoveryCandidate>,
    pub error: Option<String>,
}

impl RecoveryOfferState {
    pub fn scan() -> Self {
        match dtool_core::autosave::recovery_candidates(&dtool_core::autosave::recovery_dir()) {
            Ok(candidates) => Self { candidates: candidates.into(), error: None },
            Err(error) => Self { candidates: Default::default(), error: Some(text(&error.message_key).to_string()) },
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut dtool_core::Workspace) {
        let Some(candidate) = self.candidates.front().cloned() else { return; };
        let mut choice = None;
        let (result, ()) = show_modal(ui.ctx(), "dialog.recover", text("dialog.recover.title"), ClosePolicy::CancelOnly, false, |ui| {
                ui.label(text("dialog.recover.question"));
                ui.label(candidate.original_path.as_deref().unwrap_or(&candidate.path).display().to_string());
                if let Some(error) = &self.error { ui.label(error); }
                ui.horizontal(|ui| {
                    if dtool_design::button(ui, text("button.yes")).clicked() { choice = Some(CloseAnswer::Yes); }
                    if dtool_design::button(ui, text("button.no")).clicked() { choice = Some(CloseAnswer::No); }
                    if dtool_design::button(ui, text("button.cancel")).clicked() { choice = Some(CloseAnswer::Cancel); }
                });
        });
        if result == DialogResult::Cancel { choice = Some(CloseAnswer::Cancel); }
        match choice {
            Some(CloseAnswer::Yes) => {
                let command = dtool_core::Command::with_params("doc.recover", serde_json::json!({ "path": candidate.path }));
                match workspace.run(dtool_core::Scope::active(), command).1 {
                    Ok(_) => { self.candidates.pop_front(); self.error = None; }
                    Err(error) => self.error = Some(text(&error.message_key).to_string()),
                }
            }
            Some(CloseAnswer::No) => { self.candidates.pop_front(); self.error = None; }
            Some(CloseAnswer::Cancel) => { self.candidates.clear(); self.error = None; }
            None => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecentMissingDialogState {
    pub path: String,
    pub resolved: Option<bool>,
}

impl RecentMissingDialogState {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            resolved: None,
        }
    }
}

pub fn show_recent_missing_dialog(
    ui: &mut egui::Ui,
    state: &mut RecentMissingDialogState,
) -> Option<bool> {
    let mut answer: Option<bool> = None;

    egui::Window::new(text("dialog.recent.missing.title"))
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            dtool_design::dialog_frame(ui, |ui| {
                ui.label(format("dialog.recent.missing.body", &[("path", &state.path)]));

                ui.add_space(dtool_design::space("md"));

                ui.horizontal(|ui| {
                    if dtool_design::button(ui, text("dialog.recent.missing.yes")).clicked() {
                        answer = Some(true);
                    }
                    if dtool_design::button(ui, text("dialog.recent.missing.no")).clicked() {
                        answer = Some(false);
                    }
                });

                if dtool_design::dialog_keys(ui) == Some(dtool_design::DialogKey::Cancel) {
                    answer = Some(false);
                }
            });
        });

    state.resolved = answer;
    answer
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpGateClass {
    C1CloseDiscard,
    C2Overwrite,
    C3DeleteFiles,
    Ungated,
}

#[derive(Debug, Clone)]
pub struct McpConfirmDialogState {
    pub gate_class: McpGateClass,
    pub message_key: String,
    pub confirmed: Option<bool>,
}

impl McpConfirmDialogState {
    pub fn new(class: McpGateClass) -> Self {
        let key = match class {
            McpGateClass::C1CloseDiscard => "mcp.confirm.c1",
            McpGateClass::C2Overwrite => "mcp.confirm.c2",
            McpGateClass::C3DeleteFiles => "mcp.confirm.c3",
            McpGateClass::Ungated => "",
        };
        Self { gate_class: class, message_key: key.to_string(), confirmed: None }
    }

    pub fn for_host(class: &str, message_key: &str) -> Self {
        let gate_class = match class {
            "C1" => McpGateClass::C1CloseDiscard,
            "C2" => McpGateClass::C2Overwrite,
            "C3" => McpGateClass::C3DeleteFiles,
            _ => McpGateClass::Ungated,
        };
        let mut state = Self::new(gate_class);
        if !message_key.is_empty() {
            state.message_key = message_key.to_string();
        }
        state
    }
}

pub fn show_mcp_confirm_dialog(
    ui: &mut egui::Ui,
    state: &mut McpConfirmDialogState,
) -> Option<bool> {
    let mut answer: Option<bool> = None;

    let (result, ()) = show_modal(ui.ctx(), "dialog.mcp.confirm", text("dialog.mcp.confirm.title"), ClosePolicy::CancelOnly, false, |ui| {
                ui.label(text(&state.message_key));

                ui.add_space(dtool_design::space("md"));

                ui.horizontal(|ui| {
                    if dtool_design::button(ui, text("button.yes")).clicked() {
                        answer = Some(true);
                    }
                    if dtool_design::button(ui, text("button.no")).clicked() {
                        answer = Some(false);
                    }
                });

        });
    if result == DialogResult::Cancel { answer = Some(false); }

    state.confirmed = answer;
    answer
}
