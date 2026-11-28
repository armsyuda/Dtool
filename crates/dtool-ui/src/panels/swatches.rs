use dtool_core::colour::swatch::{SwatchTarget, target_paint};
use dtool_core::paint::{Colour, Paint};
use super::colour::{ColourPanel, ColourTarget, hex_of};
use dtool_core::palette::{self, Palette, PaletteScope, PaletteSort, PaletteUi, PaletteView};
use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_design::{button, colour_well, menu_item, panel_frame};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::json;

const SORTS: [PaletteSort; 4] = [PaletteSort::None, PaletteSort::Name, PaletteSort::Hue, PaletteSort::Lightness];
const SCOPES: [PaletteScope; 2] = [PaletteScope::Document, PaletteScope::Application];
const SWATCH_TILE_WIDTH: f32 = 20.0;
const QUICK_TILE_WIDTH: f32 = 16.0;
const COMMAND_WIDTH: f32 = 24.0;
const MIN_COMMAND_WIDTH: f32 = 19.0;
const MIN_QUICK_TILE_WIDTH: f32 = 10.0;

#[derive(Default)]
pub struct SwatchesPanel {
    pub selected: Option<u64>,
    pub rename: String,
    palette_name_dialog: Option<PaletteNameDialog>,
    standalone_target: ColourTarget,
    pub last_error: Option<String>,
    pub file_dialog: Option<PaletteFileDialog>,
    file_scope: Option<PaletteScope>,
    icons: crate::icon_cache::IconCache,
    shown_palette: Option<(Option<dtool_core::ContextId>, PaletteScope, String)>,
}

pub const PALETTE_FORMATS: [&str; 2] = ["ase", "json"];
pub const PALETTE_CONFLICTS: [&str; 3] = ["rename", "skip", "replace"];

struct PaletteNameDialog {
    scope: PaletteScope,
    original: Option<String>,
    name: String,
    focus_requested: bool,
}

impl PaletteNameDialog {
    fn show(&mut self, ctx: &egui::Context) -> (bool, Option<Command>) {
        let title = if self.original.is_some() { "panel.swatches.rename" } else { "panel.swatches.new_palette" };
        let mut open = true;
        let mut finished = false;
        let mut command = None;
        egui::Window::new(format!("{} · {}", text(title), text(scope_key(self.scope))))
            .id(egui::Id::new("swatches.palette_name"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                dtool_design::dialog_frame(ui, |ui| {
                    let label = ui.label(text("panel.swatches.palette"));
                    let field = ui.text_edit_singleline(&mut self.name).labelled_by(label.id);
                    if !self.focus_requested {
                        field.request_focus();
                        self.focus_requested = true;
                    }
                    let name = self.name.trim();
                    let valid = !name.is_empty() && self.original.as_deref() != Some(name);
                    ui.horizontal(|ui| {
                        let ok = ui.add_enabled(valid, egui::Button::new(text("button.ok")));
                        let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                        if valid && (ok.clicked() || enter) {
                            let scope = scope_name(self.scope);
                            command = Some(match &self.original {
                                Some(original) => Command::with_params("palette.rename", json!({ "scope": scope, "name": original, "to": name })),
                                None => Command::with_params("palette.create", json!({ "scope": scope, "name": name })),
                            });
                            finished = true;
                        }
                        finished |= button(ui, text("button.cancel")).clicked()
                            || ui.input(|input| input.key_pressed(egui::Key::Escape));
                    });
                });
            });
        (!open || finished, command)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaletteFileDialog {
    pub import: bool,
    pub path: String,
    pub format: usize,
    pub conflict: usize,
    pub palette_name: String,
}

impl PaletteFileDialog {
    pub fn import() -> Self {
        Self { import: true, ..Self::default() }
    }
    pub fn export() -> Self {
        Self::default()
    }
    pub fn title_key(&self) -> &'static str {
        if self.import { "palette.import" } else { "palette.export" }
    }
    pub fn command(&self, state: &PaletteUi) -> Option<Command> {
        let path = self.path.trim();
        if path.is_empty() {
            return None;
        }
        let format = PALETTE_FORMATS.get(self.format).copied().unwrap_or("ase");
        let scope = scope_name(state.scope);
        Some(if self.import {
            let mut params = json!({ "path": path, "format": format, "scope": scope, "on_conflict": PALETTE_CONFLICTS.get(self.conflict).copied().unwrap_or("rename") });
            if !self.palette_name.trim().is_empty() {
                params["palette_name"] = json!(self.palette_name.trim());
            }
            Command::with_params("palette.import", params)
        } else {
            Command::with_params("palette.export", json!({ "path": path, "format": format, "scope": scope, "palette": state.palette }))
        })
    }
    pub fn show(&mut self, ctx: &egui::Context, state: &PaletteUi) -> (bool, Option<Command>) {
        let mut close = false;
        let mut command = None;
        egui::Window::new(text(self.title_key())).collapsible(false).show(ctx, |ui| {
            dtool_design::dialog_frame(ui, |ui| {
                ui.label(text("dialog.file.path"));
                ui.text_edit_singleline(&mut self.path);
                ui.label(text("palette.file_filter"));
                ui.horizontal(|ui| {
                    ui.label(text("palette.format"));
                    for (index, format) in PALETTE_FORMATS.iter().enumerate() {
                        ui.selectable_value(&mut self.format, index, format.to_ascii_uppercase());
                    }
                });
                if self.import {
                    ui.label(text("palette.on_conflict"));
                    ui.horizontal(|ui| {
                        for (index, mode) in PALETTE_CONFLICTS.iter().enumerate() {
                            ui.selectable_value(&mut self.conflict, index, text(&format!("palette.on_conflict.{mode}")));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(text("panel.swatches.palette"));
                        ui.add(egui::TextEdit::singleline(&mut self.palette_name).desired_width(120.0));
                    });
                }
                ui.horizontal(|ui| {
                    if button(ui, text("button.ok")).clicked() {
                        command = self.command(state);
                        close = command.is_some();
                    }
                    if button(ui, text("button.cancel")).clicked() {
                        close = true;
                    }
                });
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        });
        (close, command)
    }
}

fn sort_key(sort: PaletteSort) -> &'static str {
    match sort {
        PaletteSort::None => "panel.swatches.sort_none",
        PaletteSort::Name => "panel.swatches.sort_name",
        PaletteSort::Hue => "panel.swatches.sort_hue",
        PaletteSort::Lightness => "panel.swatches.sort_lightness",
    }
}

fn scope_key(scope: PaletteScope) -> &'static str {
    match scope {
        PaletteScope::Application => "palette.application",
        _ => "palette.document",
    }
}

fn scope_name(scope: PaletteScope) -> &'static str {
    match scope {
        PaletteScope::Application => "application",
        PaletteScope::System => "system",
        PaletteScope::Document => "document",
    }
}

pub fn swatch_context_commands(id: u64, global: bool) -> Vec<(&'static str, Command)> {
    let mut items = vec![
        ("panel.swatches.apply_fill", Command::with_params("swatch.apply_to_selection", json!({ "swatch": id, "to": "fill" }))),
        ("panel.swatches.apply_stroke", Command::with_params("swatch.apply_to_selection", json!({ "swatch": id, "to": "stroke" }))),
    ];
    if !global {
        items.push(("swatches.make_global", Command::with_params("swatch.make_global", json!({ "id": id, "global": true }))));
    }
    items.push(("panel.swatches.delete", Command::with_params("swatch.delete", json!({ "id": id }))));
    items
}

fn apply_swatch(id: u64, target: &str, app_palette: Option<&str>) -> Command {
    let mut params = json!({ "swatch": id, "to": target });
    if let Some(palette) = app_palette {
        params["scope"] = json!("application");
        params["palette"] = json!(palette);
    }
    Command::with_params("swatch.apply_to_selection", params)
}

fn palette_label(palette: &Palette) -> String {
    if palette.name.is_empty() { text("palette.document").to_string() } else { palette.name.clone() }
}

fn visible_width(ui: &egui::Ui) -> f32 {
    ui.available_width().min(ui.clip_rect().right() - ui.cursor().min.x).max(0.0)
}

fn command_button(
    ui: &mut egui::Ui, icons: &mut crate::icon_cache::IconCache, icon: &str,
    label: &str, caption: Option<&str>, width: f32,
) -> egui::Response {
    let texture = icons.themed_texture(ui.ctx(), icon, dtool_design::icon_texture_px(ui));
    let button = match &texture {
        Some(texture) => {
            let size = (width - 8.0).clamp(8.0, 16.0);
            let image = egui::Image::new(texture).fit_to_exact_size(egui::Vec2::splat(size));
            match caption {
                Some(caption) => egui::Button::image_and_text(image, caption),
                None => egui::Button::image(image),
            }
        }
        None => egui::Button::new(caption.unwrap_or(label)),
    };
    let response = dtool_design::tooltip(ui.add_sized([width, 23.0], button.truncate()), label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    response
}

fn quick_swatches(ui: &mut egui::Ui, editable: bool, width: f32) -> Option<Paint> {
    ui.add_enabled_ui(editable, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let size = egui::vec2(width, QUICK_TILE_WIDTH);
        let label = text("colour.panel.no_paint");
        let tile = swatch_tile(ui, size, egui::Color32::WHITE, label, false, false);
        let inset = tile.rect.shrink(2.0);
        ui.painter().line_segment(
            [inset.left_bottom(), inset.right_top()],
            egui::Stroke::new(1.5, egui::Color32::from_rgb(224, 54, 65)),
        );
        let mut paint = dtool_design::tooltip(tile, label).clicked().then_some(Paint::None);
        for value in [0, 128, 255] {
            let colour = Colour::new(value, value, value, 255);
            let label = hex_of(&colour);
            let tile = swatch_tile(ui, size, egui::Color32::from_gray(value), &label, false, false);
            if dtool_design::tooltip(tile, label).clicked() {
                paint = Some(Paint::Solid { colour });
            }
        }
        paint
    }).inner
}

fn swatch_tile(ui: &mut egui::Ui, size: egui::Vec2, colour: egui::Color32, name: &str, selected: bool, global: bool) -> egui::Response {
    use egui::{Color32, Rect, Sense, Stroke, StrokeKind, vec2};
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if colour.a() < 255 {
            let half = rect.size() * 0.5;
            painter.rect_filled(rect, 0.0, Color32::from_gray(230));
            for origin in [rect.min, rect.center()] {
                painter.rect_filled(Rect::from_min_size(origin, half), 0.0, Color32::from_gray(178));
            }
        }
        painter.rect_filled(rect, 0.0, colour);
        if global {
            let corner = rect.right_bottom() - vec2(1.0, 1.0);
            let side = size.x.min(size.y) * 0.28;
            painter.add(egui::Shape::convex_polygon(
                vec![corner, corner - vec2(side, 0.0), corner - vec2(0.0, side)],
                Color32::WHITE, Stroke::new(0.7, Color32::from_gray(45)),
            ));
        }
        let stroke = if selected || response.has_focus() {
            Stroke::new(2.0, ui.visuals().selection.bg_fill)
        } else {
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)
        };
        painter.rect_stroke(rect, 0.0, stroke, StrokeKind::Inside);
    }
    response
}

fn swatch_menu(ui: &mut egui::Ui, id: u64, global: bool, name: &mut String, pending: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(name).desired_width(115.0));
        if ui.add_enabled(!name.trim().is_empty(), egui::Button::new(text("panel.swatches.rename"))).clicked() {
            pending.push(Command::with_params("swatch.rename", json!({ "id": id, "name": name.trim() })));
            ui.close();
        }
    });
    for (key, command) in swatch_context_commands(id, global) {
        if menu_item(ui, text(key)).clicked() {
            pending.push(command);
            ui.close();
        }
    }
}

impl SwatchesPanel {
    pub fn palettes(workspace: &CoreWorkspace, scope: PaletteScope) -> Vec<Palette> {
        match scope {
            PaletteScope::Application => palette::app_palettes(),
            _ => workspace.session().filter(|session| session.has_document).map(dtool_core::commands::palettes::doc_palettes).unwrap_or_default(),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        let mut colour_panel = ColourPanel::default();
        colour_panel.set_target(self.standalone_target);
        self.show_with_colour(ui, workspace, &mut colour_panel);
        self.standalone_target = colour_panel.target;
    }

    pub(crate) fn show_with_colour(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace, colour_panel: &mut ColourPanel) {
        colour_panel.sync(workspace);
        let editable = workspace.session().is_some_and(|session| {
            session.has_document && session.selection.ids.iter().all(|id| {
                session.document.get_object(*id).is_some_and(|object| !session.document.is_object_locked(object))
            })
        });
        let mut quick_paint = None;
        let recent = palette::recent_colours();
        let mut pending: Vec<Command> = Vec::new();
        let state = palette::ui_state();
        let palettes = Self::palettes(workspace, state.scope);
        let session = workspace.session().filter(|session| session.has_document);
        let has_document = session.is_some();
        let can_apply = session.is_some_and(|session| !session.selection.ids.is_empty());
        let selected_object = session.and_then(|session| {
            session.selection.ids.first().and_then(|id| session.document.get_object(*id))
        });
        let add_sources = [
            (SwatchTarget::Fill, "fill", "panel.swatches.add_fill"),
            (SwatchTarget::Stroke, "stroke", "panel.swatches.add_stroke"),
        ].map(|(target, name, key)| {
            let colour = selected_object.and_then(|object| match target_paint(object, target) {
                Paint::Solid { colour } => Some(*colour),
                _ => None,
            });
            (name, key, colour)
        });
        let display = dtool_core::paint::DisplayColourCtx::for_optional(session.map(|session| &session.document));
        panel_frame(ui, |ui| {
            let mut next = state.clone();
            let current = palettes.iter().find(|palette| palette.name == state.palette).or_else(|| palettes.first());
            let palette_name = current.map(|palette| palette.name.as_str()).unwrap_or_default();
            let palette_key = (workspace.active(), state.scope, palette_name.to_owned());
            if self.shown_palette.as_ref() != Some(&palette_key) {
                self.shown_palette = Some(palette_key);
                self.selected = None;
                self.rename.clear();
            }
            let app_palette = (state.scope == PaletteScope::Application).then_some(palette_name);
            let swatches = current.map(|palette| palette.swatches.as_slice()).unwrap_or_default();
            let document = state.scope == PaletteScope::Document;
            let selected = self.selected.and_then(|id| swatches.iter().find(|swatch| swatch.id == id));
            let inline_opacity = visible_width(ui) >= 170.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                colour_panel.swatch_targets(ui, workspace, editable);
                if inline_opacity {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        colour_panel.swatch_opacity(ui, workspace, editable);
                    });
                }
            });
            if !recent.is_empty() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.weak(text("glyphs.subset.recent"));
                    ui.add_enabled_ui(editable, |ui| {
                        let count = ((visible_width(ui) + 3.0) / 19.0).floor() as usize;
                        for colour in recent.iter().take(count) {
                            let shown = dtool_core::paint::display_colour(colour, None, &display);
                            let chip = egui::Color32::from_rgba_unmultiplied(shown.r, shown.g, shown.b, shown.a);
                            let label = hex_of(colour);
                            let tile = swatch_tile(ui, egui::Vec2::splat(16.0), chip, &label, false, false);
                            if dtool_design::tooltip(tile, label).clicked() {
                                quick_paint = Some(Paint::Solid { colour: *colour });
                            }
                        }
                    });
                });
            }
            let target = match colour_panel.target {
                ColourTarget::Fill => "fill",
                ColourTarget::Stroke => "stroke",
            };
            ui.horizontal(|ui| {
                let gap = 2.0;
                ui.spacing_mut().item_spacing.x = gap;
                let title = current.map(palette_label).unwrap_or_else(|| text(scope_key(state.scope)).to_owned());
                let palette_tip = format!("{} · {} / {title}", text("panel.swatches.palette"), text(scope_key(state.scope)));
                let available = visible_width(ui);
                let chip_width = ((available - 4.0 * (MIN_COMMAND_WIDTH + gap)) / 4.0).clamp(0.0, QUICK_TILE_WIDTH);
                let show_presets = chip_width >= MIN_QUICK_TILE_WIDTH;
                let quick_width = if show_presets { 4.0 * chip_width + gap } else { 0.0 };
                let command_space = available - 3.0 * gap - quick_width;
                let command_width = (command_space / 4.0).clamp(1.0, COMMAND_WIDTH);
                let palette_width = (command_space - 3.0 * command_width).min(154.0);
                let caption = (palette_width >= 72.0).then_some(title.as_str());
                let palette_menu = command_button(
                    ui, &mut self.icons, "cmd-grid", &palette_tip, caption,
                    if caption.is_some() { palette_width } else { command_width },
                );
                egui::Popup::menu(&palette_menu).show(|ui| {
                    egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                        for scope in SCOPES {
                            let other_palettes;
                            let choices = if scope == state.scope {
                                &palettes
                            } else {
                                other_palettes = Self::palettes(workspace, scope);
                                &other_palettes
                            };
                            if choices.is_empty() {
                                continue;
                            }
                            ui.weak(text(scope_key(scope)));
                            for palette in choices {
                                let active = scope == state.scope && current.is_some_and(|active| active.name == palette.name);
                                if ui.selectable_label(active, palette_label(palette)).clicked() {
                                    if !active {
                                        next.search.clear();
                                    }
                                    next.scope = scope;
                                    next.palette.clone_from(&palette.name);
                                    ui.close();
                                }
                            }
                            ui.add_space(4.0);
                        }
                    });
                });

                let add_palette = command_button(ui, &mut self.icons, "palette-add",
                    text("panel.swatches.manage_palettes"), None, command_width);
                egui::Popup::menu(&add_palette).show(|ui| {
                    ui.weak(text("panel.swatches.new_palette"));
                    for scope in SCOPES {
                        let action = ui.add_enabled_ui(scope != PaletteScope::Document || has_document, |ui| {
                            menu_item(ui, text(scope_key(scope)))
                        }).inner;
                        if action.clicked() {
                            self.palette_name_dialog = Some(PaletteNameDialog { scope, original: None, name: String::new(), focus_requested: false });
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.weak(text("palette.import"));
                    for scope in SCOPES {
                        let action = ui.add_enabled_ui(scope != PaletteScope::Document || has_document, |ui| {
                            menu_item(ui, text(scope_key(scope)))
                        }).inner;
                        if action.clicked() {
                            self.file_dialog = Some(PaletteFileDialog::import());
                            self.file_scope = Some(scope);
                            ui.close();
                        }
                    }
                    if current.is_some() {
                        ui.separator();
                        if !palette_name.is_empty() && menu_item(ui, text("panel.swatches.rename")).clicked() {
                            self.palette_name_dialog = Some(PaletteNameDialog {
                                scope: state.scope,
                                original: Some(palette_name.to_owned()),
                                name: palette_name.to_owned(),
                                focus_requested: false,
                            });
                            ui.close();
                        }
                        if menu_item(ui, text("palette.export")).clicked() {
                            let mut dialog = PaletteFileDialog::export();
                            dialog.palette_name = palette_name.to_owned();
                            self.file_dialog = Some(dialog);
                            self.file_scope = Some(state.scope);
                            ui.close();
                        }
                    }
                });

                let add_sources = add_sources.map(|(role, label, colour)| {
                    let paint = if can_apply {
                        colour
                    } else {
                        let target = if role == "fill" { ColourTarget::Fill } else { ColourTarget::Stroke };
                        ColourPanel::solid_swatch(workspace, target)
                    };
                    (role, label, paint)
                });
                let can_add = current.is_some() && add_sources.iter().any(|(_, _, colour)| colour.is_some());
                let add = ui.add_enabled_ui(can_add, |ui| {
                    command_button(ui, &mut self.icons, "swatch-add", text("panel.swatches.add"), None, command_width)
                }).inner;
                egui::Popup::menu(&add).show(|ui| {
                    for (target, label, colour) in &add_sources {
                        let action = ui.add_enabled_ui(colour.is_some(), |ui| menu_item(ui, text(label))).inner;
                        if action.clicked() {
                            let command = if document && can_apply {
                                Command::with_params("swatch.add_from_selection", json!({ "to": target, "palette": palette_name }))
                            } else {
                                Command::with_params("swatch.add", json!({
                                    "scope": scope_name(state.scope), "colour": colour, "palette": palette_name
                                }))
                            };
                            pending.push(command);
                            ui.close();
                        }
                    }
                    if document {
                        ui.separator();
                        let colour = add_sources.iter().find(|(role, _, _)| *role == target)
                            .and_then(|(_, _, colour)| *colour);
                        let action = ui.add_enabled_ui(editable && colour.is_some(), |ui| {
                            menu_item(ui, text("panel.swatches.add_global"))
                        }).inner;
                        if action.clicked() {
                            let command = if can_apply {
                                Command::with_params("swatch.add_from_selection", json!({
                                    "to": target, "palette": palette_name, "global": true
                                }))
                            } else {
                                Command::with_params("swatch.add", json!({
                                    "scope": "document", "colour": colour, "palette": palette_name, "global": true
                                }))
                            };
                            pending.push(command);
                            ui.close();
                        }
                    }
                });

                let options = command_button(ui, &mut self.icons, "cmd-more-vertical",
                    text("panel.swatches.options"), None, command_width);
                egui::Popup::menu(&options)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        if !inline_opacity {
                            ui.horizontal(|ui| {
                                ui.label(text("context.brush.opacity"));
                                colour_panel.swatch_opacity(ui, workspace, editable);
                            });
                            ui.separator();
                        }
                        if !show_presets {
                            if let Some(paint) = ui.horizontal(|ui| quick_swatches(ui, editable, QUICK_TILE_WIDTH)).inner {
                                quick_paint = Some(paint);
                                ui.close();
                            }
                            ui.separator();
                        }
                        for (view, key) in [(PaletteView::Grid, "panel.swatches.view_grid"), (PaletteView::List, "panel.swatches.view_list")] {
                            if ui.selectable_label(view == next.view, text(key)).clicked() {
                                next.view = view;
                                ui.close();
                            }
                        }
                        ui.separator();
                        ui.weak(text("panel.swatches.sort"));
                        for sort in SORTS {
                            if ui.selectable_label(sort == next.sort, text(sort_key(sort))).clicked() {
                                next.sort = sort;
                                ui.close();
                            }
                        }
                        if let Some(swatch) = selected.filter(|_| document) {
                            ui.separator();
                            swatch_menu(ui, swatch.id, swatch.global, &mut self.rename, &mut pending);
                        }
                    });
                if show_presets {
                    let spacer = visible_width(ui) - quick_width - gap;
                    if spacer > 0.0 {
                        ui.add_space(spacer);
                    }
                    if let Some(paint) = quick_swatches(ui, editable, chip_width) {
                        quick_paint = Some(paint);
                    }
                }
            });
            if let Some(error) = &self.last_error {
                ui.label(text(error));
            }
            ui.add_space(3.0);
            let order = palette::ordered_ids(swatches, state.sort, &state.search);
            let palette_scroll_id = ui.scope_id().with(&self.shown_palette);
            let mut draw = |ui: &mut egui::Ui, id: u64, tile_size: Option<f32>| {
                let Some(swatch) = swatches.iter().find(|swatch| swatch.id == id) else { return };
                let shown = dtool_core::paint::display_colour(&swatch.colour, None, &display);
                let fill = egui::Color32::from_rgba_unmultiplied(shown.r, shown.g, shown.b, shown.a);
                let response = match tile_size {
                    Some(size) => swatch_tile(ui, egui::Vec2::splat(size), fill, &swatch.name, self.selected == Some(id), swatch.global),
                    None => colour_well(ui, fill, &format!("swatch_{id}")),
                };
                let response = dtool_design::tooltip(response, swatch.name.as_str());
                if response.clicked() {
                    self.selected = Some(id);
                    self.rename = swatch.name.clone();
                    if can_apply {
                        pending.push(apply_swatch(id, target, app_palette));
                    } else if editable {
                        quick_paint = Some(Paint::Solid { colour: swatch.colour });
                    }
                }
                if state.scope == PaletteScope::Document {
                    if response.secondary_clicked() {
                        self.selected = Some(id);
                        self.rename = swatch.name.clone();
                    }
                    egui::Popup::context_menu(&response)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| swatch_menu(ui, id, swatch.global, &mut self.rename, &mut pending));
                }
                if state.view == PaletteView::List {
                    let marker = if swatch.global { format!("{}  ({})", swatch.name, text("panel.swatches.global")) } else { swatch.name.clone() };
                    let row = ui.add(egui::Button::selectable(self.selected == Some(id), &marker).truncate());
                    if dtool_design::tooltip(row, marker).clicked() {
                        self.selected = Some(id);
                        self.rename = swatch.name.clone();
                        if can_apply {
                            pending.push(apply_swatch(id, target, app_palette));
                        } else if editable {
                            quick_paint = Some(Paint::Solid { colour: swatch.colour });
                        }
                    }
                }
            };
            let footer = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
            let visible = ui.available_rect_before_wrap().intersect(ui.clip_rect());
            let height = (visible.height() - footer).max(0.0);
            let scroll = egui::ScrollArea::vertical().id_salt(("swatches.tiles", palette_scroll_id, state.view == PaletteView::Grid))
                .max_height(height).min_scrolled_height(0.0).auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded);
            if order.is_empty() {
                scroll.show(ui, |ui| { ui.weak(text("panel.swatches.empty")); });
            } else if state.view == PaletteView::Grid {
                let width = (visible_width(ui) - 11.0).max(1.0);
                let columns = (width / SWATCH_TILE_WIDTH).floor().max(1.0) as usize;
                let size = SWATCH_TILE_WIDTH.min(width);
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    scroll.show_rows(ui, size, order.len().div_ceil(columns), |ui, rows| {
                        for index in rows {
                            let start = index * columns;
                            let row = &order[start..(start + columns).min(order.len())];
                            ui.horizontal(|ui| {
                                for id in row {
                                    ui.push_id(*id, |ui| draw(ui, *id, Some(size)));
                                }
                            });
                        }
                    });
                });
            } else {
                scroll.show(ui, |ui| {
                    for id in &order {
                        ui.horizontal(|ui| draw(ui, *id, None));
                    }
                });
            }
            egui::Frame::new().fill(ui.visuals().extreme_bg_color).corner_radius(3.0)
                .inner_margin(egui::Margin::symmetric(5, 0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if let Some(texture) = self.icons.themed_texture(ui.ctx(), "tool-zoom", dtool_design::icon_texture_px(ui)) {
                            ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::Vec2::splat(13.0)));
                        }
                        ui.add(egui::TextEdit::singleline(&mut next.search)
                            .frame(egui::Frame::NONE)
                            .hint_text(text("panel.swatches.search"))
                            .desired_width(ui.available_width()));
                    });
                });
            if next != state {
                pending.insert(0, Command::with_params("palette.view", json!({
                    "scope": scope_name(next.scope), "palette": next.palette, "view": next.view, "sort": next.sort, "search": next.search
                })));
            }
        });
        match quick_paint {
            Some(Paint::Solid { colour }) => colour_panel.apply_swatch_colour(workspace, colour),
            Some(Paint::None) => colour_panel.clear_paint(workspace, colour_panel.target),
            _ => {}
        }
        if let Some(dialog) = &mut self.palette_name_dialog {
            let (close, command) = dialog.show(ui.ctx());
            pending.extend(command);
            if close {
                self.palette_name_dialog = None;
            }
        }
        if let Some(dialog) = &mut self.file_dialog {
            let mut dialog_state = state.clone();
            dialog_state.scope = self.file_scope.unwrap_or(state.scope);
            if !dialog.import {
                dialog_state.palette.clone_from(&dialog.palette_name);
            }
            let (close, command) = dialog.show(ui.ctx(), &dialog_state);
            if let Some(command) = command {
                pending.push(command);
            }
            if close {
                self.file_dialog = None;
                self.file_scope = None;
            }
        }
        for command in pending {
            let activated = match command.name.as_str() {
                "palette.create" => Some((command.params["scope"].clone(), command.params["name"].clone())),
                "palette.rename" => Some((command.params["scope"].clone(), command.params["to"].clone())),
                _ => None,
            };
            let (_, result) = workspace.run(Scope::active(), command);
            if let (Ok(_), Some((scope, name))) = (&result, activated) {
                let (_, result) = workspace.run(Scope::active(), Command::with_params("palette.view", json!({
                    "scope": scope, "palette": name, "search": ""
                })));
                self.last_error = result.err().map(|error| error.message_key);
                continue;
            }
            self.last_error = result.err().map(|error| error.message_key);
        }
    }
}

#[cfg(test)]
mod context_menu_tests {
    use super::*;

    #[test]
    fn local_swatch_menu_offers_fill_stroke_global_delete() {
        let items = swatch_context_commands(4, false);
        let names: Vec<&str> = items.iter().map(|(_, command)| command.name.as_str()).collect();
        assert_eq!(names, vec!["swatch.apply_to_selection", "swatch.apply_to_selection", "swatch.make_global", "swatch.delete"]);
        assert_eq!(items[0].1.params["to"], "fill");
        assert_eq!(items[1].1.params["to"], "stroke");
        assert!(items.iter().all(|(_, command)| command.params.get("id").or_else(|| command.params.get("swatch")) == Some(&json!(4))));
    }

    #[test]
    fn global_swatch_menu_hides_make_global() {
        let items = swatch_context_commands(9, true);
        assert!(items.iter().all(|(_, command)| command.name != "swatch.make_global"));
        assert_eq!(items.len(), 3);
    }
}
