use std::collections::{HashMap, HashSet};

use dtool_core::command::Session;
use dtool_core::colour::{self, ColourFormat};
use dtool_core::document::PageId;
use dtool_core::doc_setup::{DocSetup, Margins, PreviewStore, ProfileConversion, ResizeMode, SetupScope, Units};
use dtool_protocol::{CommandError, CommandResult};
use dtool_strings::text;
use serde_json::{json, Value};

pub const DOC_SETUP_QUERY: &str = "doc.setup.query";
pub const DOC_SETUP_PREVIEW: &str = "doc.setup.preview";
pub const DOC_SETUP_APPLY: &str = "doc.setup.apply";
pub const DOC_SETUP_CANCEL: &str = "doc.setup.cancel";

const UNIT_KEYS: [&str; 5] = ["units.pt", "units.mm", "units.cm", "units.in", "units.px"];
const UNIT_VALUES: [Units; 5] = [Units::Pt, Units::Mm, Units::Cm, Units::In, Units::Px];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogResult {
    Ok,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMode {
    Command,
    Local,
}

#[derive(Clone)]
struct SpreadChoice {
    id: PageId,
    start: usize,
    label: String,
    size: [f32; 2],
    pages: Vec<[f32; 2]>,
}

#[derive(Clone, Copy)]
struct WindowResize {
    size: egui::Vec2,
    origin: egui::Pos2,
    axes: egui::Vec2,
}

struct NumberEdits {
    scope: egui::Id,
    invalid: bool,
}

#[derive(Clone)]
pub struct DocumentSetupDialog {
    pub open: bool,
    pub preview: bool,
    pub setup: DocSetup,
    pub original_setup: DocSetup,
    pub token: Option<u64>,
    pub mode: PreviewMode,
    pub error: Option<CommandError>,
    store: Option<PreviewStore>,
    next_token: u64,
    previewed: bool,
    spreads: Vec<SpreadChoice>,
    other_scope: Option<DocSetup>,
    spread_size_edited: bool,
    preview_spread: Option<PageId>,
    page_bounds: Vec<[f32; 4]>,
    thumbnails: HashMap<usize, egui::TextureHandle>,
    window_size: Option<egui::Vec2>,
    window_resize: Option<WindowResize>,
}

impl Default for DocumentSetupDialog {
    fn default() -> Self {
        Self {
            open: false,
            preview: true,
            setup: DocSetup::default(),
            original_setup: DocSetup::default(),
            token: None,
            mode: PreviewMode::Command,
            error: None,
            store: None,
            next_token: 1,
            previewed: false,
            spreads: Vec::new(),
            other_scope: None,
            spread_size_edited: false,
            preview_spread: None,
            page_bounds: Vec::new(),
            thumbnails: HashMap::new(),
            window_size: None,
            window_resize: None,
        }
    }
}

fn units_name(units: Units) -> &'static str {
    match units {
        Units::Pt => "points",
        Units::Mm => "millimeters",
        Units::Cm => "centimeters",
        Units::In => "inches",
        Units::Px => "pixels",
    }
}

fn margins_value(margins: &Margins) -> Value {
    json!({
        "enabled": margins.enabled,
        "left": margins.left,
        "right": margins.right,
        "top": margins.top,
        "bottom": margins.bottom,
        "linked": margins.linked
    })
}

pub fn setup_params(setup: &DocSetup, token: Option<u64>) -> Value {
    let mut params = json!({
        "units": units_name(setup.units),
        "dpi": setup.dpi,
        "page_size": setup.page_size,
        "landscape": setup.landscape,
        "facing_pages": setup.facing_pages,
        "arrange_vertical": setup.arrange_vertical,
        "start_on_right": setup.start_on_right,
        "reflow_pages": setup.reflow_pages,
        "colour_format": setup.colour_format,
        "colour_profile": setup.colour_profile,
        "transparent_background": setup.transparent_background,
        "margins": margins_value(&setup.margins),
        "bleed": margins_value(&setup.bleed),
        "page_origin": setup.page_origin,
        "drawing_scale": setup.drawing_scale,
        "measurement_unit": setup.measurement_unit,
        "profile_conversion": if setup.profile_conversion == ProfileConversion::Assign { "assign" } else { "convert" },
        "resize": if setup.resize == ResizeMode::Rescale { "rescale" } else { "anchor_to_page" },
        "anchor": { "x": setup.anchor.x, "y": setup.anchor.y },
        "scope": if setup.scope == SetupScope::SelectedSpreads { "selected_spreads" } else { "whole_document" },
        "page_ids": setup.page_ids
    });
    if let (Some(token), Some(map)) = (token, params.as_object_mut()) {
        map.insert("token".to_string(), json!(token));
    }
    params
}

fn missing_command(result: &CommandResult) -> bool {
    matches!(result, Err(err) if err.code == "UNKNOWN_COMMAND" || err.code == "NOT_IMPLEMENTED")
}

impl DocumentSetupDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self, session: &Session) {
        self.setup = DocSetup::from_document(&session.document);
        self.original_setup = self.setup.clone();
        self.other_scope = None;
        self.spread_size_edited = false;
        self.spreads.clear();
        self.preview_spread = None;
        self.thumbnails.clear();
        let doc = &session.document;
        self.page_bounds = dtool_core::scene::page_rects(doc);
        if doc.layout_artboard_origins.is_some() && !doc.pages.is_empty() {
            let rects = &self.page_bounds;
            let current = dtool_core::commands::pages::viewed_page(session);
            let factor = self.setup.unit_factor_for(doc);
            for spread in dtool_core::data_merge::layout::spread_ranges(doc) {
                let label_at = |index| dtool_core::longdoc::sections::section_page_label(doc, index);
                let first = label_at(spread.start);
                let label = if spread.len() == 1 { first } else { format!("{first}–{}", label_at(spread.end - 1)) };
                let [_, _, width, height] = rects[spread.start];
                let id = doc.pages[spread.start].id;
                let pages = rects[spread.clone()].iter().map(|rect| [rect[2] * factor, rect[3] * factor]).collect();
                self.spreads.push(SpreadChoice { id, start: spread.start, label, size: [width * factor, height * factor], pages });
                if doc.pages[spread].iter().any(|page| Some(page.id) == current) {
                    self.setup.page_ids = vec![id];
                }
            }
            if self.setup.page_ids.is_empty() {
                self.setup.page_ids.push(self.spreads[0].id);
            }
            self.preview_spread = self.setup.page_ids.first().copied();
        }
        self.store = Some(PreviewStore::new(self.next_token, &session.document));
        self.next_token += 1;
        self.token = None;
        self.mode = PreviewMode::Command;
        self.error = None;
        self.previewed = false;
        self.open = true;
    }

    pub fn size_changed(&self) -> bool {
        self.setup.page_size != self.original_setup.page_size
    }

    pub fn original(&self) -> Option<&PreviewStore> {
        self.store.as_ref()
    }

    fn restore_local(&self, session: &mut Session) {
        if let Some(store) = &self.store {
            dtool_core::commands::doc_setup::replace_document_preserving_view(session, store.original.clone());
        }
    }

    fn apply_local(&mut self, session: &mut Session) -> Result<(), CommandError> {
        let mut next = self.store.as_ref().map_or_else(|| session.document.clone(), |store| store.original.clone());
        if let Err(error) = self.setup.try_apply(&mut next) {
            self.restore_local(session);
            return Err(error);
        }
        dtool_core::commands::doc_setup::replace_document_preserving_view(session, next);
        Ok(())
    }

    pub fn preview_now(&mut self, session: &mut Session) {
        if !self.preview {
            return;
        }
        self.error = None;
        if self.setup.scope == SetupScope::SelectedSpreads && !self.spread_size_edited {
            if self.previewed { self.revert(session); }
            return;
        }
        self.previewed = true;
        if self.mode == PreviewMode::Command {
            let result = session.run_named(DOC_SETUP_PREVIEW, setup_params(&self.setup, self.token));
            if !missing_command(&result) {
                match result {
                    Ok(reply) => {
                        if let Some(token) = reply.get("token").and_then(Value::as_u64) {
                            self.token = Some(token);
                        }
                    }
                    Err(err) => self.error = Some(err),
                }
                return;
            }
            self.mode = PreviewMode::Local;
        }
        if let Err(err) = self.apply_local(session) {
            self.error = Some(err);
        }
    }

    pub fn field_changed(&mut self, session: &mut Session) {
        if self.preview {
            self.preview_now(session);
        }
    }

    pub fn preview_toggled(&mut self, session: &mut Session) {
        if self.preview {
            self.preview_now(session);
        } else if self.previewed {
            self.revert(session);
        }
    }

    fn revert(&mut self, session: &mut Session) {
        if self.mode == PreviewMode::Command {
            if let Some(token) = self.token.take() {
                let result = session.run_named(DOC_SETUP_CANCEL, json!({ "token": token }));
                if !missing_command(&result) {
                    self.previewed = false;
                    return;
                }
            }
        }
        if self.previewed {
            self.restore_local(session);
        }
        self.previewed = false;
    }

    pub fn cancel(&mut self, session: &mut Session) -> DialogResult {
        self.revert(session);
        self.open = false;
        self.store = None;
        self.thumbnails.clear();
        DialogResult::Cancel
    }

    pub fn ok(&mut self, session: &mut Session) -> Option<DialogResult> {
        if self.setup.scope == SetupScope::SelectedSpreads && !self.spread_size_edited {
            if self.previewed { self.revert(session); }
            self.finish();
            return Some(DialogResult::Ok);
        }
        if self.setup.scope == SetupScope::WholeDocument && let Err(err) = self.setup.validate() {
            self.error = Some(err);
            return None;
        }
        if self.mode == PreviewMode::Command {
            let result = session.run_named(DOC_SETUP_APPLY, setup_params(&self.setup, self.token));
            if !missing_command(&result) {
                return match result {
                    Ok(_) => {
                        self.finish();
                        Some(DialogResult::Ok)
                    }
                    Err(err) => {
                        self.error = Some(err);
                        None
                    }
                };
            }
            self.mode = PreviewMode::Local;
        }
        match self.apply_local(session) {
            Ok(()) => {
                session.generation += 1;
                session.document.version += 1;
                self.finish();
                Some(DialogResult::Ok)
            }
            Err(err) => {
                self.error = Some(err);
                None
            }
        }
    }

    fn finish(&mut self) {
        self.open = false;
        self.store = None;
        self.thumbnails.clear();
        self.token = None;
        self.previewed = false;
    }

    fn number(ui: &mut egui::Ui, value: &mut f32, key: &str, unit: &str, edits: &mut NumberEdits, reset: bool) -> bool {
        let mut field = *value as f64;
        let id = ui.make_persistent_id(edits.scope.with(key));
        if reset {
            dtool_design::numeric_cancel_edit(ui.ctx(), id);
        }
        let spec = dtool_design::NumericSpec::new("")
            .width(ui.available_width().min(122.0))
            .suffix(unit);
        dtool_design::tooltip(dtool_design::numeric_value(ui, id, &mut field, &spec), text(key));
        let edit = dtool_design::numeric_state(ui.ctx(), id);
        edits.invalid |= ui.is_enabled() && edit.edited
            && edit.buffer.as_deref().is_some_and(|draft| spec.parse_text(draft).is_none());
        let next = field as f32;
        if next != *value {
            *value = next;
            return true;
        }
        false
    }

    fn flag(ui: &mut egui::Ui, value: &mut bool, key: &str) -> bool {
        dtool_design::toggle(ui, value, text(key)).changed()
    }

    fn margins_section(ui: &mut egui::Ui, margins: &mut Margins, title: &str, unit: &str, edits: &mut NumberEdits) -> bool {
        ui.add_space(4.0);
        ui.separator();
        let id = ui.make_persistent_id(("document_setup_insets", title));
        let mut open = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
        let mut changed = ui.horizontal(|ui| {
            let arrow = if open { "▾" } else { "▸" };
            if ui.button(format!("{arrow}  {}", text(title))).clicked() { open = !open; }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                dtool_design::tooltip(dtool_design::toggle(ui, &mut margins.enabled, ""), text(title)).changed()
            }).inner
        }).inner;
        if changed { open = margins.enabled; }
        ui.data_mut(|data| data.insert_temp(id, open));
        if !open { return changed; }
        let label_width = (ui.available_width() * 0.42).clamp(94.0, 160.0);
        ui.push_id(title, |ui| {
            ui.add_enabled_ui(margins.enabled, |ui| {
                let mut linked_value = None;
                for (key, value) in [
                    ("doc_setup.margin.left", &mut margins.left),
                    ("doc_setup.margin.right", &mut margins.right),
                    ("doc_setup.margin.top", &mut margins.top),
                    ("doc_setup.margin.bottom", &mut margins.bottom),
                ] {
                    if setting_row(ui, key, label_width, |ui| Self::number(ui, value, key, unit, edits, false)) {
                        changed = true;
                        linked_value = Some(*value);
                    }
                }
                if Self::flag(ui, &mut margins.linked, "doc_setup.margin.linked") {
                    changed = true;
                    linked_value = Some(margins.left);
                }
                if margins.linked && let Some(value) = linked_value {
                    margins.left = value;
                    margins.right = value;
                    margins.top = value;
                    margins.bottom = value;
                }
            });
        });
        changed
    }

    fn set_scope(&mut self, scope: SetupScope) -> bool {
        if self.setup.scope == scope { return false; }
        let next = self.other_scope.take().unwrap_or_else(|| {
            let mut setup = self.original_setup.clone();
            setup.scope = scope;
            setup.page_ids = self.preview_spread.into_iter().collect();
            if let Some(choice) = self.spreads.iter().find(|choice| setup.page_ids.contains(&choice.id)) {
                setup.page_size = choice.size;
                setup.landscape = choice.size[0] > choice.size[1];
            }
            setup
        });
        self.other_scope = Some(std::mem::replace(&mut self.setup, next));
        true
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) -> (bool, bool) {
        let mut changed = false;
        let mut preview_toggled = false;
        ui.horizontal_wrapped(|ui| {
            if dtool_design::tab(ui, self.setup.scope == SetupScope::WholeDocument, text("doc_setup.scope.whole")).clicked() {
                changed |= self.set_scope(SetupScope::WholeDocument);
            }
            if !self.spreads.is_empty()
                && dtool_design::tab(ui, self.setup.scope == SetupScope::SelectedSpreads, text("doc_setup.scope.selected")).clicked()
            {
                changed |= self.set_scope(SetupScope::SelectedSpreads);
            }
            preview_toggled = Self::flag(ui, &mut self.preview, "menu.view.preview");
        });
        (changed, preview_toggled)
    }

    fn preview_pane(&mut self, ui: &mut egui::Ui, height: f32) -> (bool, bool) {
        use egui::{Align, Layout, vec2};

        let mut changed = false;
        let selected = self.setup.scope == SetupScope::SelectedSpreads;
        let preview = self.spreads.iter().find(|spread| Some(spread.id) == self.preview_spread);
        let original = &self.original_setup;
        let factor = dtool_core::doc_setup::unit_factor(original.units, original.dpi, self.setup.units, self.setup.dpi);
        let resizing = if selected {
            self.spread_size_edited && self.preview_spread.is_some_and(|id| self.setup.page_ids.contains(&id))
        } else {
            original.page_size.iter().zip(self.setup.page_size).any(|(size, current)| (size * factor - current).abs() > 0.01)
        };
        let pages = match preview {
            Some(spread) if resizing => vec![self.setup.page_size; spread.pages.len()],
            Some(spread) => spread.pages.iter().map(|[width, height]| [width * factor, height * factor]).collect(),
            None => vec![self.setup.page_size],
        };
        let has_spreads = preview.is_some();
        let start = preview.map_or(0, |spread| spread.start);
        let mut scene = None;
        let mut visible_pages = HashSet::new();
        let preview_height = if has_spreads { (height * 0.42).clamp(72.0, 320.0) } else { (height - 58.0).max(68.0) };
        let width = ui.available_width();
        ui.allocate_ui_with_layout(vec2(width, preview_height), Layout::top_down(Align::Center), |ui| {
            draw_pages(ui, &pages, vec2(width, preview_height), Some(&self.setup), |ctx, offset| {
                if resizing { return None; }
                let index = start + offset;
                visible_pages.insert(index);
                page_thumbnail(ctx, self.store.as_ref()?, &self.page_bounds, &mut self.thumbnails, &mut scene, index)
            });
        });
        ui.add_space(4.0);
        let (scope_changed, preview_toggled) = self.toolbar(ui);
        ui.separator();
        if !has_spreads { return (scope_changed, preview_toggled); }
        let thumbnail_height = (height - preview_height - 58.0).max(32.0);
        egui::ScrollArea::vertical()
            .id_salt("document_setup_spreads")
            .max_height(thumbnail_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for spread in &self.spreads {
                        ui.push_id(spread.id, |ui| {
                            let active = self.preview_spread == Some(spread.id);
                            let card = egui::Frame::new()
                                .fill(dtool_design::theme::surface(ui.ctx(), "panel.bg"))
                                .stroke(egui::Stroke::new(1.0, if active { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.noninteractive.bg_stroke.color }))
                                .corner_radius(5.0)
                                .inner_margin(7.0)
                                .show(ui, |ui| {
                                    ui.vertical(|ui| {
                                        let preview = draw_pages(ui, &spread.pages, vec2(112.0, 80.0), None, |ctx, offset| {
                                            let index = spread.start + offset;
                                            visible_pages.insert(index);
                                            page_thumbnail(ctx, self.store.as_ref()?, &self.page_bounds, &mut self.thumbnails, &mut scene, index)
                                        });
                                        if selected {
                                            let mut checked = self.setup.page_ids.contains(&spread.id);
                                            let can_toggle = !checked || self.setup.page_ids.len() > 1;
                                            if dtool_design::toggle_with(ui, &mut checked, &spread.label, &dtool_design::ControlState::enabled(can_toggle)).changed() {
                                                if checked {
                                                    self.setup.page_ids.push(spread.id);
                                                    self.preview_spread = Some(spread.id);
                                                } else {
                                                    self.setup.page_ids.retain(|id| *id != spread.id);
                                                    if self.preview_spread == Some(spread.id) {
                                                        self.preview_spread = self.setup.page_ids.first().copied();
                                                    }
                                                }
                                                changed = true;
                                            }
                                        } else {
                                            ui.label(&spread.label);
                                        }
                                        preview
                                    }).inner
                                });
                            let card = ui.interact(card.inner, ui.scope_id().with("sheet"), egui::Sense::click());
                            if card.clicked() {
                                self.preview_spread = Some(spread.id);
                            }
                        });
                    }
                });
            });
        self.thumbnails.retain(|index, _| visible_pages.contains(index));
        (changed || scope_changed, preview_toggled)
    }

    fn settings_pane(&mut self, ui: &mut egui::Ui, layout_document: bool, edits: &mut NumberEdits) -> bool {
        let mut changed = false;
        let selected = self.setup.scope == SetupScope::SelectedSpreads;
        let label_width = (ui.available_width() * 0.42).clamp(94.0, 160.0);
        if !selected {
            section(ui, "unit.document", true);
            ui.label(text("doc_setup.units"));
            dtool_design::with_icon_selection(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (units, key) in UNIT_VALUES.into_iter().zip(UNIT_KEYS) {
                        let selected = self.setup.units == units;
                        let response = ui.add(egui::Button::selectable(selected, crate::captions::units_identity(units)));
                        if response.on_hover_text(text(key)).clicked() && !selected {
                            self.setup.convert_units(units);
                            changed = true;
                        }
                    }
                });
            });
            setting_row(ui, "doc_setup.dpi", label_width, |ui| {
                changed |= Self::number(ui, &mut self.setup.dpi, "doc_setup.dpi", "", edits, false);
            });
        }

        if selected && !self.spread_size_edited
            && let Some(choice) = self.spreads.iter().find(|choice| self.setup.page_ids.contains(&choice.id))
        {
            self.setup.page_size = choice.size;
            self.setup.landscape = choice.size[0] > choice.size[1];
        }
        let previous_size = self.setup.page_size;
        section(ui, "doc_setup.dimensions", selected);
        let presets = super::NEW_DOCUMENT_PRESETS;
        let dimensions = |name| {
            let (width, height, units, dpi) = super::new_document_preset(name)?;
            let factor = dtool_core::doc_setup::unit_factor(units, dpi, self.setup.units, self.setup.dpi);
            let size = [width * factor, height * factor];
            Some(if (size[0] > size[1]) != self.setup.landscape { [size[1], size[0]] } else { size })
        };
        let current = presets.into_iter().find(|name| dimensions(name).is_some_and(|size| {
            size.iter().zip(self.setup.page_size).all(|(preset, current)| (preset - current).abs() < 0.01)
        }));
        let mut preset_size = None;
        setting_row(ui, "doc_setup.size_preset", label_width, |ui| {
            egui::ComboBox::from_id_salt("document_setup_page_preset")
                .width(ui.available_width())
                .selected_text(current.unwrap_or_else(|| text("doc_setup.size_custom")))
                .show_ui(ui, |ui| {
                    for preset in presets {
                        if ui.selectable_label(current == Some(preset), preset).clicked() {
                            preset_size = dimensions(preset);
                        }
                    }
                });
        });
        if let Some(size) = preset_size {
            self.setup.page_size = size;
            changed |= previous_size != size;
        }
        let unit = dtool_core::commands::ruler_units::suffix(self.setup.units);
        for (index, key) in ["doc_setup.page_width", "doc_setup.page_height"].into_iter().enumerate() {
            setting_row(ui, key, label_width, |ui| {
                changed |= Self::number(ui, &mut self.setup.page_size[index], key, unit, edits, preset_size.is_some());
            });
        }
        setting_row(ui, "doc_setup.orientation", label_width, |ui| {
            for (landscape, key) in [(false, "doc_setup.portrait"), (true, "doc_setup.landscape")] {
                let selected = self.setup.landscape == landscape;
                if super::orientation_button(ui, landscape, selected, text(key)).clicked() && !selected {
                    self.setup.landscape = landscape;
                    self.setup.page_size.swap(0, 1);
                    changed = true;
                }
            }
        });
        if selected {
            self.spread_size_edited |= self.setup.page_size != previous_size;
        }

        ui.add_enabled_ui(selected || self.size_changed(), |ui| {
            ui.horizontal_top(|ui| {
                dtool_design::with_icon_selection(ui, |ui| {
                    ui.vertical(|ui| {
                        for (mode, key) in [
                            (ResizeMode::Rescale, "doc_setup.rescale"),
                            (ResizeMode::AnchorToPage, "doc_setup.anchor_to_page"),
                        ] {
                            let selected = self.setup.resize == mode;
                            if ui.add(egui::Button::new(text(key)).selected(selected)).clicked() && !selected {
                                self.setup.resize = mode;
                                changed = true;
                            }
                        }
                    });
                });
                ui.add_space(10.0);
                ui.add_enabled_ui(self.setup.resize == ResizeMode::AnchorToPage, |ui| {
                    changed |= anchor_grid(ui, &mut self.setup.anchor);
                });
            });
        });
        if selected { return changed; }

        section(ui, "doc_setup.model", false);
        changed |= flag_row(ui, label_width, &mut self.setup.facing_pages, "doc_setup.facing_pages");
        if !layout_document {
            setting_row(ui, "doc_setup.arrange", label_width, |ui| {
                changed |= pair(ui, &mut self.setup.arrange_vertical, "dialog.guides.kind.horizontal", "dialog.guides.kind.vertical");
            });
        }
        let facing = self.setup.facing_pages;
        ui.add_enabled_ui(facing, |ui| {
            setting_row(ui, "doc_setup.start_on", label_width, |ui| {
                changed |= pair(ui, &mut self.setup.start_on_right, "doc_setup.start_on.left", "doc_setup.start_on.right");
            });
        });
        changed |= flag_row(ui, label_width, &mut self.setup.reflow_pages, "doc_setup.reflow");

        section(ui, "doc_setup.colour", false);
        changed |= self.colour_rows(ui, label_width);
        setting_row(ui, "doc_setup.profile_conversion", label_width, |ui| {
            let mut convert = self.setup.profile_conversion == ProfileConversion::Convert;
            if pair(ui, &mut convert, "doc_setup.assign", "doc_setup.convert") {
                self.setup.profile_conversion = if convert { ProfileConversion::Convert } else { ProfileConversion::Assign };
                changed = true;
            }
        });
        changed |= flag_row(ui, label_width, &mut self.setup.transparent_background, "menu.document.setup.transparent_background");
        changed |= Self::margins_section(ui, &mut self.setup.margins, "doc_setup.margins", unit, edits);
        changed |= Self::margins_section(ui, &mut self.setup.bleed, "doc_setup.bleed", unit, edits);

        crate::dialog_chrome::options_divider(ui);
        let (scale_changed, invalid_scale) = super::new_document_options::drawing_scale_section(
            ui, &mut self.setup.drawing_scale, &mut self.setup.measurement_unit, edits.scope, false,
        );
        changed |= scale_changed;
        edits.invalid |= invalid_scale;
        crate::dialog_chrome::options_divider(ui);
        egui::CollapsingHeader::new(text("doc_setup.page_origin")).show(ui, |ui| {
            for (index, key) in ["doc_setup.page_origin.x", "doc_setup.page_origin.y"].into_iter().enumerate() {
                setting_row(ui, key, label_width, |ui| {
                    changed |= Self::number(ui, &mut self.setup.page_origin[index], key, unit, edits, false);
                });
            }
        });
        changed
    }

    fn colour_rows(&mut self, ui: &mut egui::Ui, label_width: f32) -> bool {
        let Ok(services) = colour::services().read() else { return false; };
        let current = self.setup.colour_format().ok();
        let mut changed = false;
        ui.label(text("doc_setup.colour_format"));
        dtool_design::with_icon_selection(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                    for &format in ColourFormat::colour_format_list() {
                        let default = dtool_core::commands::document_colour::default_profile(format, &services);
                        let profiles = services.profiles.list(format.model);
                        let profile = if profiles.contains(&default.as_str()) {
                            Some(default.as_str())
                        } else {
                            profiles.first().copied()
                        };
                        let label = format.to_string();
                        let button = egui::Button::selectable(current == Some(format), &label)
                            .wrap_mode(egui::TextWrapMode::Extend);
                        if ui.add_enabled(profile.is_some(), button).clicked()
                            && current != Some(format)
                            && let Some(profile) = profile
                        {
                            self.setup.colour_format = label;
                            self.setup.colour_profile = profile.to_owned();
                            changed = true;
                        }
                    }
            });
        });
        if let Ok(format) = self.setup.colour_format() {
            setting_row(ui, "doc_setup.colour_profile", label_width, |ui| {
                let profile_box = egui::ComboBox::from_id_salt("document_setup_colour_profile")
                    .width(ui.available_width())
                    .selected_text(&self.setup.colour_profile)
                    .show_ui(ui, |ui| {
                        for profile in services.profiles.list(format.model) {
                            if ui.selectable_label(self.setup.colour_profile == profile, profile).clicked()
                                && self.setup.colour_profile != profile
                            {
                                self.setup.colour_profile = profile.to_owned();
                                changed = true;
                            }
                        }
                    });
                dtool_design::tooltip(profile_box.response, self.setup.colour_profile.as_str());
            });
        }
        changed
    }

    pub fn show(&mut self, ui: &mut egui::Ui, session: &mut Session) -> Option<DialogResult> {
        if !self.open {
            return None;
        }
        let mut window_open = true;
        let mut answer: Option<bool> = None;
        let mut changed = false;
        let mut preview_toggled = false;
        use egui::{Align, Layout, vec2};

        let available = ui.ctx().content_rect().size();
        let maximum = (available - vec2(36.0, 76.0)).max(vec2(320.0, 260.0));
        let initial = vec2(1060.0, 810.0).min(maximum);
        let minimum = vec2(460.0, 360.0).min(maximum);
        let size = self.window_size.unwrap_or(initial).clamp(minimum, maximum);
        let surface = dtool_design::theme::surface(ui.ctx(), "dialog.bg");
        let background = egui::Color32::from_rgb(surface.r(), surface.g(), surface.b());
        let frame = egui::Frame::window(ui.style()).fill(background);
        egui::Window::new(text("dialog.document_setup.title"))
            .id(egui::Id::unique("dtool.document_setup.window"))
            .frame(frame)
            .fade_in(false)
            .fade_out(false)
            .open(&mut window_open)
            .collapsible(false)
            .fixed_size(size)
            .default_pos(ui.ctx().content_rect().center() - initial * 0.5)
            .show(ui.ctx(), |ui| {
                let content = ui.available_rect_before_wrap();
                let edge = 10.0;
                let grip = 18.0;
                let row_height = 26.0;
                let spacing = ui.spacing().item_spacing.y;
                let separator_height = 1.0;
                let error_height = self.error.as_ref().map_or(0.0, |_| {
                    ui.text_style_height(&egui::TextStyle::Body) + spacing
                });
                let footer_height = row_height + separator_height + 2.0 * spacing + error_height;
                let height = (content.height() - footer_height - edge).max(160.0);
                let width = content.width() - edge;
                let layout_document = session.document.layout_artboard_origins.is_some();
                let mut invalid = false;
                if width < 545.0 {
                    crate::dialog_chrome::options_scroll(ui, "document_setup_compact", height, |ui| {
                            dtool_design::dialog_section(ui, |ui| {
                                (changed, preview_toggled) = self.preview_pane(ui, 220.0);
                            });
                            dtool_design::dialog_section(ui, |ui| {
                                let (settings_changed, bad_input) = self.settings_fields(ui, layout_document);
                                changed |= settings_changed;
                                invalid = bad_input;
                            });
                        });
                } else {
                    let gap = 10.0;
                    let inset = 2.0 * (dtool_design::space("sm") + dtool_design::stroke_width("thin"));
                    let settings_width = ((width - gap) * 0.42).clamp(272.0, 440.0);
                    let preview_width = width - gap - settings_width;
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        ui.allocate_ui_with_layout(vec2(preview_width, height), Layout::top_down(Align::Min), |ui| {
                            dtool_design::dialog_section(ui, |ui| {
                                ui.set_width(preview_width - inset);
                                (changed, preview_toggled) = self.preview_pane(ui, height - inset);
                            });
                        });
                        ui.allocate_ui_with_layout(vec2(settings_width, height), Layout::top_down(Align::Min), |ui| {
                            dtool_design::dialog_section(ui, |ui| {
                                ui.set_width(settings_width - inset);
                                let outcome = crate::dialog_chrome::options_scroll(ui, "document_setup_settings", height - inset,
                                    |ui| self.settings_fields(ui, layout_document));
                                changed |= outcome.0;
                                invalid = outcome.1;
                            });
                        });
                    });
                }
                ui.add(egui::Separator::default().spacing(separator_height));
                if let Some(err) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, text(&err.message_key));
                }
                ui.allocate_ui_with_layout(vec2(width, row_height), Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_sized([88.0, row_height], egui::Button::new(text("button.cancel"))).clicked() {
                        answer = Some(false);
                    }
                    if ui.add_enabled_ui(!invalid, |ui| {
                        dtool_design::primary_button(ui, text("common.ok"), vec2(88.0, row_height)).clicked()
                    }).inner {
                        answer = Some(true);
                    }
                    if invalid {
                        ui.colored_label(ui.visuals().error_fg_color, text("error.invalid_params"));
                    }
                });
                // Reserve a gutter so the resize targets never cover settings or footer buttons.
                ui.expand_to_include_rect(content);
                let corner = content.right_bottom();
                // The press and release of a short drag can both arrive in one frame.
                let (pressed, pointer, down) = ui.input(|input| {
                    let pressed = input.events.iter().rev().find_map(|event| match event {
                        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. } => Some(*pos),
                        _ => None,
                    });
                    (pressed, input.pointer.interact_pos(), input.pointer.primary_down())
                });
                let pressed = pressed.filter(|pos| ui.ctx().layer_id_at(*pos) == Some(ui.layer_id()));
                for (label, rect, cursor, axes) in [
                    ("dialog.document_setup.resize_width",
                        egui::Rect::from_min_max(content.right_top() - vec2(edge, 0.0), corner - vec2(0.0, grip)),
                        egui::CursorIcon::ResizeHorizontal, vec2(1.0, 0.0)),
                    ("dialog.document_setup.resize_height",
                        egui::Rect::from_min_max(content.left_bottom() - vec2(0.0, edge), corner - vec2(grip, 0.0)),
                        egui::CursorIcon::ResizeVertical, vec2(0.0, 1.0)),
                    ("dialog.document_setup.resize_window",
                        egui::Rect::from_min_max(corner - vec2(grip, grip), corner),
                        egui::CursorIcon::ResizeNwSe, vec2(1.0, 1.0)),
                ] {
                    let handle = ui.interact(rect, ui.id().with(label), egui::Sense::DRAG)
                        .on_hover_and_drag_cursor(cursor);
                    handle.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Splitter, true, text(label)));
                    if cursor == egui::CursorIcon::ResizeNwSe {
                        let anchor = corner - vec2(2.0, 2.0);
                        let stroke = ui.style().interact(&handle).fg_stroke;
                        for length in [4.0, 8.0, 12.0] {
                            ui.painter().line_segment([anchor - vec2(length, 0.0), anchor - vec2(0.0, length)], stroke);
                        }
                    }
                    if let Some(origin) = pressed.filter(|pos| rect.contains(*pos)) {
                        self.window_resize = Some(WindowResize { size, origin, axes });
                    }
                }
                if let Some(drag) = self.window_resize && let Some(pointer) = pointer {
                    self.window_size = Some((drag.size + (pointer - drag.origin) * drag.axes).clamp(minimum, maximum));
                    ui.ctx().request_repaint();
                }
                if !down {
                    self.window_resize = None;
                }
                if dtool_design::dialog_keys(ui) == Some(dtool_design::DialogKey::Cancel) {
                    answer = Some(false);
                }
            });
        if preview_toggled {
            self.preview_toggled(session);
        } else if changed {
            self.field_changed(session);
        }
        if !window_open {
            return Some(self.cancel(session));
        }
        match answer {
            Some(true) => self.ok(session),
            Some(false) => Some(self.cancel(session)),
            None => None,
        }
    }

    fn settings_fields(&mut self, ui: &mut egui::Ui, layout_document: bool) -> (bool, bool) {
        ui.spacing_mut().item_spacing.y = 4.0;
        let selected = self.setup.scope == SetupScope::SelectedSpreads;
        let mut edits = NumberEdits {
            scope: egui::Id::unique(("document_setup_inputs", self.next_token, selected)),
            invalid: false,
        };
        let changed = self.settings_pane(ui, layout_document, &mut edits);
        (changed, edits.invalid)
    }

    pub fn query(session: &mut Session) -> DocSetup {
        let _ = session.run_named(DOC_SETUP_QUERY, json!({}));
        DocSetup::from_document(&session.document)
    }
}

fn section(ui: &mut egui::Ui, key: &str, first: bool) {
    if first {
        ui.strong(text(key));
    } else {
        crate::dialog_chrome::options_group(ui, text(key));
    }
}

fn setting_row<T>(ui: &mut egui::Ui, key: &str, label_width: f32, add: impl FnOnce(&mut egui::Ui) -> T) -> T {
    ui.horizontal(|ui| {
        ui.allocate_ui(egui::vec2(label_width, 20.0), |ui| {
            ui.set_width(label_width);
            dtool_design::tooltip(ui.add(egui::Label::new(text(key)).truncate()), text(key));
        });
        add(ui)
    }).inner
}

fn flag_row(ui: &mut egui::Ui, label_width: f32, value: &mut bool, key: &str) -> bool {
    setting_row(ui, key, label_width, |ui| {
        dtool_design::tooltip(dtool_design::toggle(ui, value, ""), text(key)).changed()
    })
}

fn pair(ui: &mut egui::Ui, value: &mut bool, off: &str, on: &str) -> bool {
    let width = ((ui.available_width() - ui.spacing().item_spacing.x) * 0.5).max(42.0);
    let mut changed = false;
    for (next, key) in [(false, off), (true, on)] {
        let active = *value == next;
        if ui.add_sized([width, 22.0], egui::Button::new(text(key)).selected(active)).clicked() && !active {
            *value = next;
            changed = true;
        }
    }
    changed
}

fn anchor_grid(ui: &mut egui::Ui, anchor: &mut dtool_core::doc_setup::Anchor9) -> bool {
    let mut changed = false;
    egui::Grid::new("document_setup_anchor").spacing([3.0, 3.0]).show(ui, |ui| {
        for y in [0.0, 0.5, 1.0] {
            for x in [0.0, 0.5, 1.0] {
                let point = dtool_core::doc_setup::Anchor9 { x, y };
                let button = egui::Button::new("").selected(*anchor == point);
                if dtool_design::tooltip(ui.add_sized([14.0, 14.0], button), text("panel.transform.anchor")).clicked() {
                    *anchor = point;
                    changed = true;
                }
            }
            ui.end_row();
        }
    });
    changed
}

fn page_thumbnail(
    ctx: &egui::Context,
    store: &PreviewStore,
    bounds: &[[f32; 4]],
    thumbnails: &mut HashMap<usize, egui::TextureHandle>,
    scene: &mut Option<dtool_core::scene::Scene>,
    index: usize,
) -> Option<egui::TextureId> {
    let bounds = *bounds.get(index)?;
    store.original.pages.get(index)?;
    if !bounds.iter().all(|value| value.is_finite()) || bounds[2] <= 0.0 || bounds[3] <= 0.0 {
        return None;
    }
    Some(thumbnails.entry(index).or_insert_with(|| {
        crate::panels::pages::render_page_thumbnail(ctx, &store.original, scene, index, bounds, 320.0)
    }).id())
}

fn preview_offsets(settings: &Margins) -> Option<[f32; 4]> {
    let offsets = [settings.left, settings.top, settings.right, settings.bottom];
    (settings.enabled && offsets.iter().all(|side| side.is_finite() && *side >= 0.0) && offsets != [0.0; 4])
        .then_some(offsets)
}

fn draw_pages(
    ui: &mut egui::Ui,
    pages: &[[f32; 2]],
    size: egui::Vec2,
    setup: Option<&DocSetup>,
    mut thumbnail: impl FnMut(&egui::Context, usize) -> Option<egui::TextureId>,
) -> egui::Rect {
    let (area, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if !ui.is_rect_visible(area) { return area; }
    if setup.is_some() {
        ui.painter().rect_filled(area, 6.0, dtool_design::theme::surface(ui.ctx(), "pasteboard"));
    }
    let pages: Vec<_> = pages.iter().enumerate().filter(|(_, size)| size.iter().all(|v| v.is_finite() && *v > 0.0)).collect();
    if pages.is_empty() { return area; }
    let width: f32 = pages.iter().map(|(_, size)| size[0]).sum();
    let height = pages.iter().map(|(_, size)| size[1]).fold(0.0_f32, f32::max);
    let bleed = setup.and_then(|setup| preview_offsets(&setup.bleed));
    let margins = setup.and_then(|setup| preview_offsets(&setup.margins));
    let [left, top, right, bottom] = bleed.unwrap_or([0.0; 4]);
    let outer_width = width + left + right;
    let outer_height = height + top + bottom;
    let padding = if setup.is_some() { 28.0 } else { 6.0 };
    let scale = ((area.width() - padding).max(1.0) / outer_width)
        .min((area.height() - padding).max(1.0) / outer_height);
    let mut x = area.center().x + (left - right - width) * scale * 0.5;
    let y = area.center().y + (top - bottom - height) * scale * 0.5;
    let page_rects: Vec<_> = pages.into_iter().map(|(index, page)| {
        let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(page[0] * scale, page[1] * scale));
        x += rect.width();
        (index, rect)
    }).collect();
    if bleed.is_some() {
        let stroke = egui::Stroke::new(1.0, crate::rulers::BLEED_COLOUR);
        for (_, rect) in &page_rects {
            let outline = egui::Rect::from_min_max(
                rect.min - egui::vec2(left, top) * scale,
                rect.max + egui::vec2(right, bottom) * scale,
            );
            ui.painter().rect_stroke(outline, 0.0, stroke, egui::StrokeKind::Outside);
        }
    }
    for (index, rect) in page_rects {
        ui.painter().rect_filled(rect, 0.0, egui::Color32::from_gray(244));
        if let Some(texture) = thumbnail(ui.ctx(), index) {
            let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
            ui.painter().image(texture, rect, uv, egui::Color32::WHITE);
        }
        ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.0, egui::Color32::from_gray(146)), egui::StrokeKind::Inside);
        if let Some([left, top, right, bottom]) = margins {
            let outline = egui::Rect::from_min_max(
                rect.min + egui::vec2(left, top) * scale,
                rect.max - egui::vec2(right, bottom) * scale,
            );
            if outline.is_positive() {
                let stroke = egui::Stroke::new(1.0, dtool_design::color("smart_guide"));
                ui.painter().rect_stroke(outline, 0.0, stroke, egui::StrokeKind::Inside);
            }
        }
    }
    area
}
