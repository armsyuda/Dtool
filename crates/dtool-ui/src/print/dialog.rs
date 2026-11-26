use std::path::{Path, PathBuf};

use dtool_core::colour::RenderingIntent;
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::{Document, ObjectKind};
use dtool_export::ExportArea;
use dtool_strings::text;
use egui::{Color32, Pos2, Rect, Stroke, StrokeKind, TextureHandle, Ui, Vec2};

use super::profiles::{self, ColourMode, PageLayout, PageSubset, PrintOptions, Profiles, SaveOutcome, SizeMode};
use super::sheet::{PrintContent, PrintMarks, PrintRegion, Sheet};
use super::windows::{ICC_UNAVAILABLE, IccOutput, Output, Paper, PrintArea, Printer, Sizing};
use crate::dialog_chrome::{ClosePolicy, DialogResult, options_scroll, show_modal};

#[derive(Clone, Copy, PartialEq)]
enum SettingsTab {
    Layout,
    Rasterization,
    Marks,
    Colour,
    ToFile,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrintPane {
    Settings,
    Preview,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Pages,
    Spreads,
    Artboards,
}

pub(crate) struct PrintDialog {
    owner: ContextId,
    printer: Option<Printer>,
    printer_name: String,
    default_icc: Option<PathBuf>,
    selected_icc: Option<PathBuf>,
    to_file: bool,
    source: Source,
    from: usize,
    to: usize,
    options: PrintOptions,
    profiles: Profiles,
    active_profile: Option<String>,
    profile_name: Option<String>,
    profile_error: Option<&'static str>,
    tab: SettingsTab,
    compact_pane: PrintPane,
    sheet: usize,
    preview: Option<(u64, Vec<Sheet>, Vec<TextureHandle>)>,
    error: Option<&'static str>,
}

impl PrintDialog {
    pub(crate) fn new(owner: ContextId, doc: &Document) -> Self {
        let printer = Printer::default_printer();
        let printer_name = printer.as_ref().map(Printer::name).unwrap_or_default();
        let default_icc = printer.as_ref().and_then(Printer::colour_profile);
        let source = if doc.layout_artboard_origins.is_none() && has_artboards(doc) {
            Source::Artboards
        } else {
            Source::Pages
        };
        let count = source_count(doc, source);
        let (profiles, profile_error) = match profiles::load() {
            Ok(entries) => (entries, None),
            Err(_) => (Profiles::new(), Some("print.native.profiles_read_error")),
        };
        Self {
            owner, printer, printer_name, default_icc, selected_icc: None, to_file: false, source, from: 1, to: count.max(1),
            options: PrintOptions::default(), profiles, active_profile: None, profile_name: None,
            profile_error, tab: SettingsTab::Layout, compact_pane: PrintPane::Settings,
            sheet: 0, preview: None, error: None,
        }
    }

    fn sizing(&self) -> Sizing {
        match self.options.size_mode {
            SizeMode::Actual => Sizing::Actual,
            SizeMode::Fit => Sizing::Fit,
            SizeMode::Scale => Sizing::Scale(self.options.scale),
        }
    }

    pub(crate) fn show(&mut self, ui: &Ui, workspace: &Workspace) -> bool {
        let Some(context) = workspace.get(self.owner) else { return true };
        let doc = &context.session.document;
        let mut regions = regions(doc, self.source);
        let mut sheets = Vec::new();
        let (chrome, (cancel, print)) = show_modal(ui.ctx(), "dtool.print", text("print.title"), ClosePolicy::CancelOnly, false, |ui| {
            let viewport = ui.ctx().content_rect();
            let width = (viewport.width() - 54.0).clamp(280.0, 980.0);
            let height = (viewport.height() - 154.0).clamp(170.0, 560.0);
            let inset = 2.0 * (dtool_design::space("sm") + dtool_design::stroke_width("thin"));
            ui.set_width(width);
            if width >= 700.0 {
                let gap = 12.0;
                let settings_width = (width * 0.48).clamp(324.0, 442.0);
                let preview_width = width - settings_width - gap;
                let (body, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
                let settings_bounds = Rect::from_min_size(
                    body.left_top() + Vec2::new(preview_width + gap, 0.0), Vec2::new(settings_width, height),
                );
                let mut settings = ui.new_child(egui::UiBuilder::new().id_salt("print.settings_panel").max_rect(settings_bounds));
                settings.set_clip_rect(settings.clip_rect().intersect(settings_bounds));
                dtool_design::dialog_section(&mut settings, |ui| {
                    ui.set_width(settings_width - inset);
                    options_scroll(ui, "print.settings", height - inset, |ui| self.controls(ui, doc, &mut regions));
                });
                sheets = self.sheets(doc, &regions);
                let preview_bounds = Rect::from_min_size(body.left_top(), Vec2::new(preview_width, height));
                let mut preview = ui.new_child(egui::UiBuilder::new().id_salt("print.preview_panel").max_rect(preview_bounds));
                dtool_design::dialog_section(&mut preview, |ui| {
                    ui.set_width(preview_width - inset);
                    ui.set_min_height(height - inset);
                    self.preview(ui, doc, &context.title, &sheets, height - inset, true);
                });
            } else {
                let tabs = ui.horizontal_wrapped(|ui| {
                    for (pane, key) in [(PrintPane::Settings, "print.native.settings"), (PrintPane::Preview, "print.native.preview")] {
                        if dtool_design::tab(ui, self.compact_pane == pane, text(key)).clicked() {
                            self.compact_pane = pane;
                        }
                    }
                });
                let height = (height - inset - tabs.response.rect.height() - ui.spacing().item_spacing.y).max(110.0);
                dtool_design::dialog_section(ui, |ui| {
                    ui.set_width(width - inset);
                    match self.compact_pane {
                        PrintPane::Settings => {
                            options_scroll(ui, "print.settings", height, |ui| self.controls(ui, doc, &mut regions));
                            sheets = self.sheets(doc, &regions);
                        }
                        PrintPane::Preview => {
                            sheets = self.sheets(doc, &regions);
                            options_scroll(ui, "print.preview", height, |ui| {
                                self.preview(ui, doc, &context.title, &sheets, height, false);
                            });
                        }
                    }
                });
            }
            ui.separator();
            if let Some(error) = self.error {
                ui.colored_label(ui.visuals().error_fg_color, text(error));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let blocked = if sheets.is_empty() {
                    Some("print.native.no_sheets")
                } else if self.printer.as_ref().and_then(Printer::paper).is_none() {
                    Some("print.native.no_paper")
                } else {
                    None
                };
                let mut response = ui.add_enabled_ui(blocked.is_none(), |ui| {
                    dtool_design::primary_button(ui, text("print.print"), Vec2::new(80.0, 24.0))
                }).inner;
                if let Some(reason) = blocked {
                    response = response.on_disabled_hover_text(text(reason));
                }
                let print = response.clicked();
                let cancel = dtool_design::button(ui, text("print.cancel")).clicked();
                (cancel, print)
            }).inner
        });
        if cancel || chrome == DialogResult::Cancel {
            return true;
        }
        if print && let Some(printer) = &self.printer {
            let output = Output {
                sizing: self.sizing(), dpi: self.options.dpi,
                clip_transparency: self.options.clip_transparency,
                prefer_vectors: self.options.prefer_vectors,
                copies: self.options.copies, collate: self.options.collate,
                to_file: self.to_file,
                page_layout: self.options.page_layout,
                icc: (self.options.colour_mode == ColourMode::Windows).then_some(IccOutput {
                    profile: self.selected_icc.as_deref(),
                    intent: self.options.intent,
                }),
            };
            match printer.print(doc, &context.title, &sheets, output) {
                Ok(true) => return true,
                Ok(false) => self.error = None,
                Err(error) if error.code == ICC_UNAVAILABLE => self.error = Some("print.native.icc_unavailable"),
                Err(_) => self.error = Some("print.native.failed_print"),
            }
        }
        false
    }

    fn profile_controls(&mut self, ui: &mut Ui) {
        let current = self.active_profile.as_deref().filter(|&name| self.profiles.get(name) == Some(&self.options));
        let at_defaults = current.is_none() && self.options == PrintOptions::default();
        let current_text = current.unwrap_or_else(|| text(if at_defaults {
            "print.native.profile_defaults"
        } else {
            "print.native.profile_custom"
        }));
        let pick = ui.horizontal_wrapped(|ui| {
            ui.label(text("print.profile"));
            let pick = egui::ComboBox::from_id_salt("print_profile").width(150.0).truncate()
                .selected_text(current_text).show_ui(ui, |ui| {
                    dtool_design::with_icon_selection(ui, |ui| {
                        if ui.selectable_label(at_defaults, text("print.native.profile_defaults")).clicked() {
                            return Some(None);
                        }
                        for name in self.profiles.keys() {
                            if ui.selectable_label(current == Some(name), name).clicked() {
                                return Some(Some(name.clone()));
                            }
                        }
                        None
                    })
                }).inner.flatten();
            if dtool_design::button(ui, text("print.native.profile_save_as")).clicked() {
                self.profile_name = Some(self.active_profile.clone().unwrap_or_default());
            }
            pick
        }).inner;
        if let Some(selection) = pick {
            self.options = selection.as_ref().and_then(|name| self.profiles.get(name)).copied().unwrap_or_default();
            self.active_profile = selection;
            self.profile_name = None;
            self.sheet = 0;
        }
        if let Some(mut draft) = self.profile_name.take() {
            let mut keep_editing = true;
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut draft)
                    .hint_text(text("print.native.profile_name")).desired_width(160.0).char_limit(80));
                let name = draft.trim();
                let existing = self.profiles.get(name).copied();
                let action = if existing.is_some() { "print.native.profile_replace" } else { "print.native.profile_save" };
                let valid = !name.is_empty() && !name.chars().any(char::is_control);
                if ui.add_enabled(valid, egui::Button::new(text(action))).clicked() {
                    match profiles::save(name, self.options, existing) {
                        Ok(SaveOutcome::Saved(entries)) => {
                            self.profiles = entries;
                            self.active_profile = Some(name.to_owned());
                            self.profile_error = None;
                            keep_editing = false;
                        }
                        Ok(SaveOutcome::Changed(entries)) => {
                            self.profiles = entries;
                            self.profile_error = Some("print.native.profile_changed");
                        }
                        Err(_) => self.profile_error = Some("print.native.profile_save_error"),
                    }
                }
                if dtool_design::button(ui, text("print.cancel")).clicked() {
                    keep_editing = false;
                }
            });
            self.profile_name = keep_editing.then_some(draft);
        }
        if let Some(error) = self.profile_error {
            ui.colored_label(ui.visuals().error_fg_color, text(error));
        }
    }

    fn controls(&mut self, ui: &mut Ui, doc: &Document, selected: &mut Vec<PrintRegion>) {
        self.profile_controls(ui);
        self.printer_controls(ui);
        ui.separator();
        self.job_controls(ui, doc, selected);
        ui.add_space(6.0);
        ui.separator();
        self.settings(ui, doc);
    }

    fn settings(&mut self, ui: &mut Ui, doc: &Document) {
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (SettingsTab::Layout, "print.layout"),
                (SettingsTab::Rasterization, "print.rasterization"),
                (SettingsTab::Marks, "print.bleed_marks"),
                (SettingsTab::Colour, "print.colour_management"),
                (SettingsTab::ToFile, "print.to_file"),
            ] {
                if dtool_design::tab(ui, self.tab == tab, text(label)).clicked() {
                    self.tab = tab;
                }
            }
        });
        ui.separator();
        match self.tab {
            SettingsTab::Layout => self.layout(ui),
            SettingsTab::Colour => self.colour(ui, doc),
            SettingsTab::ToFile => {
                if dtool_design::toggle(ui, &mut self.to_file, text("print.to_file")).changed() {
                    self.error = None;
                }
                ui.weak(text("print.native.to_file_hint"));
            }
            SettingsTab::Rasterization => {
                print_row(ui, "print.rasterize_dpi", |ui, label| {
                    self.options.dpi = number(ui, "print.raster_dpi", label, self.options.dpi as usize, 72..=600, "") as u32;
                });
                dtool_design::toggle(ui, &mut self.options.clip_transparency, text("print.native.clip_transparent_margins"))
                    .on_hover_text(text("print.native.clip_transparent_margins_hint"));
                dtool_design::toggle(ui, &mut self.options.prefer_vectors, text("print.native.prefer_vectors"))
                    .on_hover_text(text("print.native.prefer_vectors_hint"));
            }
            SettingsTab::Marks => {
                dtool_design::toggle_with(ui, &mut self.options.bleed, text("print.native.include_bleed"), &dtool_design::ControlState::enabled(self.source != Source::Artboards && doc.setup.bleed.enabled))
                    .on_disabled_hover_text(text("print.native.bleed_document"));
                dtool_design::toggle(ui, &mut self.options.crop_marks, text("print.crop_marks"));
                dtool_design::toggle_with(ui, &mut self.options.fold_marks, text("print.fold_marks"), &dtool_design::ControlState::enabled(self.source != Source::Artboards && doc.pages.iter().any(|page| page.fold.is_some())));
                dtool_design::toggle(ui, &mut self.options.registration_marks, text("print.registration_marks"));
                dtool_design::toggle(ui, &mut self.options.color_bars, text("print.color_bars"));
                dtool_design::toggle(ui, &mut self.options.page_info, text("print.page_info"));
            }
        }
    }

    fn colour(&mut self, ui: &mut Ui, doc: &Document) {
        print_row(ui, "print.native.source_profile", |ui, _| {
            ui.add(egui::Label::new(&doc.colour.profile).truncate()).on_hover_text(&doc.colour.profile);
        });
        print_row(ui, "print.native.colour_handling", |ui, label| {
            let mode = self.options.colour_mode;
            choose(ui, "print_colour_mode", label, &mut self.options.colour_mode, &[
                (ColourMode::Driver, "print.native.colour_driver"),
                (ColourMode::Windows, "print.native.colour_windows"),
            ]);
            if mode != self.options.colour_mode {
                self.error = None;
            }
        });
        if self.options.colour_mode == ColourMode::Windows {
            print_row(ui, "print.native.output_profile", |ui, _| {
                let path = self.selected_icc.as_deref().or(self.default_icc.as_deref());
                let name = path.and_then(Path::file_name).map(|name| name.to_string_lossy());
                let caption = name.as_deref().unwrap_or_else(|| text("print.native.output_default"));
                ui.add(egui::Label::new(caption).truncate()).on_hover_text(caption);
            });
            print_row(ui, "settings.colour.intent", |ui, label| {
                choose(ui, "print_rendering_intent", label, &mut self.options.intent, &[
                    (RenderingIntent::Perceptual, "colour.intent.perceptual"),
                    (RenderingIntent::RelativeColorimetric, "colour.intent.relative"),
                    (RenderingIntent::Saturation, "colour.intent.saturation"),
                    (RenderingIntent::AbsoluteColorimetric, "colour.intent.absolute"),
                ]);
            });
            ui.horizontal_wrapped(|ui| {
                if dtool_design::button(ui, text("print.native.pick_icc")).clicked() {
                    match crate::dialogs::pick_file_path_with_extensions(
                        text("print.native.pick_icc"), text("print.native.icc_filter"), &["icc", "icm"], None,
                    ) {
                        Ok(Some(path)) => {
                            self.selected_icc = Some(path);
                            self.error = None;
                        }
                        Ok(None) => {}
                        Err(_) => self.error = Some("print.native.icc_unavailable"),
                    }
                }
                if self.selected_icc.is_some() && dtool_design::button(ui, text("print.native.output_default")).clicked() {
                    self.selected_icc = None;
                    self.error = None;
                }
            });
            ui.weak(text("print.native.preview_colour"));
        }
    }

    fn printer_controls(&mut self, ui: &mut Ui) {
        let change = ui.horizontal_wrapped(|ui| {
            ui.label(text("print.printer"));
            let printer = if self.printer_name.is_empty() { text("print.native.no_printer") } else { &self.printer_name };
            ui.add(egui::Label::new(printer).truncate()).on_hover_text(printer);
            dtool_design::button(ui, text("print.native.printer_setup")).clicked()
        }).inner;
        if change {
            match Printer::choose() {
                Ok(Some(printer)) => {
                    self.printer_name = printer.name();
                    self.default_icc = printer.colour_profile();
                    self.selected_icc = None;
                    self.printer = Some(printer);
                    self.error = None;
                }
                Ok(None) => {}
                Err(_) => self.error = Some("print.native.failed_setup"),
            }
        }
    }

    fn job_controls(&mut self, ui: &mut Ui, doc: &Document, selected: &mut Vec<PrintRegion>) {
        print_row(ui, "print.copies", |ui, label| ui.horizontal_wrapped(|ui| {
            self.options.copies = number(ui, "print.copies", label, self.options.copies, 1..=99, "");
            dtool_design::toggle_with(ui, &mut self.options.collate, text("print.native.collate"),
                &dtool_design::ControlState::enabled(self.options.copies > 1));
        }).inner);
        print_row(ui, "print.range", |ui, label| {
            let source = self.source;
            let mut choices = vec![(Source::Pages, "print.native.pages")];
            if dtool_core::data_merge::layout::spread_ranges(doc).iter().any(|range| range.len() > 1)
                && (doc.layout_artboard_origins.is_some() || doc.pages.iter().all(|page| doc.linked_artboard(page.id).is_none()))
            {
                choices.push((Source::Spreads, "print.native.spreads"));
            }
            if has_artboards(doc) {
                choices.push((Source::Artboards, "print.native.artboards"));
            }
            ui.add_enabled_ui(choices.len() > 1, |ui| {
                choose(ui, "print_source", label, &mut self.source, &choices);
            });
            if source != self.source {
                *selected = regions(doc, self.source);
                self.from = 1;
                self.to = source_count(doc, self.source).max(1);
                self.sheet = 0;
            }
        });
        print_row(ui, "print.native.first_last", |ui, label| ui.horizontal_wrapped(|ui| {
            let count = source_count(doc, self.source).max(1);
            self.to = self.to.clamp(1, count);
            self.from = self.from.clamp(1, self.to);
            self.from = number(ui, "print.from", label, self.from, 1..=self.to, "");
            ui.label("–");
            self.to = number(ui, "print.to", label, self.to, self.from..=count, "");
        }).inner);
        print_row(ui, "print.include", |ui, label| ui.horizontal_wrapped(|ui| {
            choose(ui, "print_include", label, &mut self.options.subset, &[
                (PageSubset::All, "print.native.all"), (PageSubset::Odd, "print.native.odd"), (PageSubset::Even, "print.native.even"),
            ]);
            dtool_design::toggle(ui, &mut self.options.reverse, text("print.reverse"));
        }).inner);
        print_row(ui, "print.fit_type", |ui, label| {
            choose(ui, "print_sizing", label, &mut self.options.size_mode, &[
                (SizeMode::Actual, "print.native.actual"), (SizeMode::Fit, "print.native.fit"), (SizeMode::Scale, "print.native.custom"),
            ]);
        });
        if self.options.size_mode == SizeMode::Scale {
            print_row(ui, "print.native.custom", |ui, label| {
                self.options.scale = number(ui, "print.scale", label, self.options.scale as usize, 1..=500, "%") as u32;
            });
        }
    }

    fn layout(&mut self, ui: &mut Ui) {
        if let Some(paper) = self.printer.as_ref().and_then(Printer::paper) {
            let [width, height] = paper_mm(paper);
            ui.horizontal(|ui| {
                ui.label(text("print.native.driver_paper"));
                ui.label(format!("{width:.1} × {height:.1} mm"));
            });
        }
        print_row(ui, "print.native.page_layout", |ui, caption| ui.horizontal_wrapped(|ui| {
            let old_layout = self.options.page_layout;
            for (layout, columns, rows, key) in [
                (PageLayout::Single, 1, 1, "print.native.page_layout_one"),
                (PageLayout::Two, 2, 1, "print.native.page_layout_two"),
                (PageLayout::Four, 2, 2, "print.native.page_layout_four"),
            ] {
                let selected = layout == self.options.page_layout;
                let response = dtool_design::with_icon_selection(ui, |ui| {
                    ui.add_sized([28.0, 23.0], egui::Button::new("").selected(selected))
                });
                let ink = if selected { dtool_design::selected_icon_ink(ui) }
                    else { ui.style().interact(&response).fg_stroke.color };
                let sheet = egui::Rect::from_center_size(response.rect.center(), egui::vec2(13.0, 15.0));
                let stroke = egui::Stroke::new(1.0, ink);
                ui.painter().rect_stroke(sheet, 1.0, stroke, egui::StrokeKind::Inside);
                if columns == 2 {
                    ui.painter().vline(sheet.center().x, sheet.y_range(), stroke);
                }
                if rows == 2 {
                    ui.painter().hline(sheet.x_range(), sheet.center().y, stroke);
                }
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, text(key)));
                if response.labelled_by(caption).on_hover_text(text(key)).clicked() {
                    self.options.page_layout = layout;
                }
            }
            if old_layout != self.options.page_layout {
                self.sheet = 0;
                if old_layout == PageLayout::Single && self.options.size_mode == SizeMode::Actual {
                    self.options.size_mode = SizeMode::Fit;
                }
            }
        }).inner);
    }

    fn sheets(&self, doc: &Document, selected: &[PrintRegion]) -> Vec<Sheet> {
        let total = source_count(doc, self.source);
        let mut sheets: Vec<_> = selected.iter().filter_map(|&region| {
            let number = region.number;
            if !(self.from..=self.to).contains(&number) || !self.options.subset.includes(number) {
                return None;
            }
            let marks = PrintMarks {
                crop: self.options.crop_marks,
                fold: self.options.fold_marks,
                registration: self.options.registration_marks,
                color_bars: self.options.color_bars,
                page_info: self.options.page_info.then_some((number, total)),
            };
            Sheet::new(doc, region, self.source != Source::Artboards && self.options.bleed, marks)
        }).collect();
        if self.options.reverse {
            sheets.reverse();
        }
        sheets
    }

    fn preview(&mut self, ui: &mut Ui, doc: &Document, title: &str, regions: &[Sheet], height: f32, show_title: bool) {
        if show_title {
            dtool_design::panel_header(ui, text("print.native.preview"));
        }
        let layout = self.options.page_layout;
        let per_side = layout.per_side();
        let count = regions.len().div_ceil(per_side);
        self.sheet = self.sheet.min(count.saturating_sub(1));
        if count == 0 {
            ui.weak(text("print.native.no_sheets"));
            return;
        }
        let Some(paper) = self.printer.as_ref().and_then(Printer::paper) else {
            ui.weak(text("print.native.no_paper"));
            return;
        };
        let start = self.sheet * per_side;
        let side = &regions[start..(start + per_side).min(regions.len())];
        if self.preview.as_ref().is_none_or(|(version, pages, _)| *version != doc.version || pages != side) {
            let mut textures = Vec::with_capacity(side.len());
            for region in side {
                let zoom = 768.0 / region.media.2.max(region.media.3);
                let width = (region.media.2 * zoom).round().max(1.0) as usize;
                let height = (region.media.3 * zoom).round().max(1.0) as usize;
                let Some(pixels) = region.render(doc, title, [width, height]) else {
                    ui.colored_label(ui.visuals().error_fg_color, text("print.native.failed_preview"));
                    return;
                };
                let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels);
                textures.push(ui.ctx().load_texture("print-sheet", image, egui::TextureOptions::LINEAR));
            }
            self.preview = Some((doc.version, side.to_vec(), textures));
        }
        if let Some((_, _, textures)) = &self.preview {
            let stage_width = ui.available_width();
            let clipped = egui::Frame::new()
                .fill(dtool_design::theme::surface(ui.ctx(), "pasteboard"))
                .corner_radius(6.0)
                .inner_margin(12)
                .show(ui, |ui| {
                    let size = Vec2::new((stage_width - 24.0).max(1.0), (height - 104.0).max(84.0));
                    let (stage, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    preview_sheet(ui, doc, paper, side, self.sizing(), layout, textures, stage)
                }).inner;
            ui.vertical_centered(|ui| {
                self.preview_navigation(ui, count);
                let [width, height] = paper_mm(paper);
                ui.weak(format!("{width:.1} × {height:.1} mm"));
                if let [region] = side
                    && let PrintContent::Artboard(id) = region.content
                    && let Some(board) = doc.get_object(id)
                {
                    ui.add(egui::Label::new(&board.name).truncate());
                }
            });
            if clipped {
                ui.label(text(if layout == PageLayout::Single { "print.native.clipped" } else { "print.native.cell_clipped" }));
            }
        }
    }

    fn preview_navigation(&mut self, ui: &mut Ui, count: usize) {
        let current = self.sheet;
        ui.allocate_ui_with_layout(
            Vec2::new(144.0, ui.spacing().interact_size.y),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                for (index, (symbol, target, label)) in [
                    ("|‹", 0, "print.native.first_side"),
                    ("‹", current.saturating_sub(1), "menu.document.pages.previous"),
                    ("›", (current + 1).min(count - 1), "menu.document.pages.next"),
                    ("›|", count - 1, "print.native.last_side"),
                ].into_iter().enumerate() {
                    if index == 2 {
                        ui.add_sized([52.0, ui.spacing().interact_size.y], egui::Label::new(format!("{} / {count}", current + 1)));
                    }
                    let enabled = target != current;
                    let label = text(label);
                    let button = ui.add_enabled(enabled, egui::Button::new(symbol).min_size(Vec2::new(18.0, 0.0)))
                        .on_hover_text(label);
                    button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
                    if button.clicked() {
                        self.sheet = target;
                        ui.ctx().request_repaint();
                    }
                }
            },
        );
    }
}

fn print_row<R>(ui: &mut Ui, key: &str, draw: impl FnOnce(&mut Ui, egui::Id) -> R) -> R {
    let label = text(key);
    if ui.available_width() < 280.0 {
        let caption = ui.label(label);
        draw(ui, caption.id)
    } else {
        dtool_design::studio_row(ui, label, draw)
    }
}

fn number(
    ui: &mut Ui, key: &str, label: egui::Id, value: usize, range: std::ops::RangeInclusive<usize>, suffix: &str,
) -> usize {
    use dtool_design::{NumericEvent, NumericSpec, NumericValue, numeric_field};
    let spec = NumericSpec::new("").decimals(0).suffix(suffix).width(61.0)
        .range(*range.start() as f64..=*range.end() as f64);
    let id = ui.make_persistent_id(key);
    let (response, event) = numeric_field(ui, id, NumericValue::Value(value as f64), &spec);
    response.labelled_by(label);
    match event {
        NumericEvent::Commit(value) => value.round() as usize,
        _ => value,
    }
}

fn choose<T: Copy + PartialEq>(ui: &mut Ui, id: &str, label: egui::Id, field: &mut T, choices: &[(T, &str)]) {
    let selected = choices.iter().find(|(value, _)| value == field).map_or("", |(_, key)| text(key));
    egui::ComboBox::from_id_salt(id).width(ui.available_width().clamp(1.0, 150.0)).truncate()
        .selected_text(selected).show_ui(ui, |ui| {
            dtool_design::with_icon_selection(ui, |ui| {
                for &(value, key) in choices {
                    ui.selectable_value(field, value, text(key));
                }
            });
        }).response.labelled_by(label).on_hover_text(selected);
}

fn print_artboards(doc: &Document) -> impl Iterator<Item = &dtool_core::Object> {
    doc.objects().iter().filter(|object| object.kind == ObjectKind::Artboard
        && !doc.master_pages.roots.contains(&object.id))
}

fn has_artboards(doc: &Document) -> bool { print_artboards(doc).next().is_some() }

fn source_count(doc: &Document, source: Source) -> usize {
    match source {
        Source::Artboards => print_artboard_regions(doc).count(),
        Source::Spreads => dtool_export::spreads(doc).len(),
        Source::Pages => doc.pages.len().max(1),
    }
}

fn print_artboard_regions(doc: &Document) -> impl Iterator<Item = (PrintContent, super::sheet::Region)> + '_ {
    print_artboards(doc).filter(|object| dtool_core::scene::is_drawn(doc, object.id)).filter_map(|object| {
        let bounds = dtool_export::raster::area_region(doc, &ExportArea::Artboard(object.id)).ok()?;
        Some((PrintContent::Artboard(object.id), bounds))
    })
}

fn regions(doc: &Document, source: Source) -> Vec<PrintRegion> {
    let batches = match source {
        Source::Artboards => return print_artboard_regions(doc).enumerate()
            .map(|(index, (content, bounds))| PrintRegion { bounds, content, number: index + 1 }).collect(),
        Source::Spreads => dtool_export::spreads(doc),
        Source::Pages => (0..doc.pages.len().max(1)).map(|page| vec![page]).collect(),
    };
    let rects = dtool_export::output_page_rects(doc);
    let mut regions = Vec::new();
    for (index, batch) in batches.iter().enumerate() {
        for pages in dtool_export::included_page_runs(doc, batch) {
            let Some((&first, &last)) = pages.first().zip(pages.last()) else { continue };
            let Some(bounds) = dtool_export::page_batch_region(&rects, pages) else { continue };
            regions.push(PrintRegion { bounds, content: PrintContent::Pages { first, last }, number: index + 1 });
        }
    }
    regions
}

fn paper_mm(paper: Paper) -> [f64; 2] {
    std::array::from_fn(|axis| f64::from(paper.size[axis]) * 25.4 / f64::from(paper.dpi[axis]))
}

fn preview_sheet(ui: &Ui, doc: &Document, paper: Paper, pages: &[Sheet], sizing: Sizing, layout: PageLayout, textures: &[TextureHandle], stage: Rect) -> bool {
    let [width, height] = paper_mm(paper);
    let ratio = width / height;
    let height = stage.height().min(stage.width() / ratio as f32);
    let sheet = Rect::from_center_size(stage.center(), Vec2::new(height * ratio as f32, height));
    let scale = [sheet.width() / paper.size[0] as f32, sheet.height() / paper.size[1] as f32];
    let point = |xy: [f64; 2]| sheet.min + Vec2::new(xy[0] as f32 * scale[0], xy[1] as f32 * scale[1]);
    let xy = |v: [i32; 2]| v.map(f64::from);
    let size = |v: [f64; 2]| Vec2::new(v[0] as f32 * scale[0], v[1] as f32 * scale[1]);
    let area_rect = |area: PrintArea| Rect::from_min_size(point(xy(area.origin)), size(xy(area.size)));
    let printable = area_rect(paper.printable_area());
    ui.painter().rect_filled(sheet, 1.0, Color32::WHITE);
    let mut clipped = false;
    for (index, (texture, page)) in textures.iter().zip(pages).enumerate() {
        let cell = paper.cell(layout, index);
        let (origin, destination) = paper.placement(doc, page.media, sizing, layout, index);
        ui.painter().with_clip_rect(sheet.intersect(area_rect(cell)).intersect(ui.clip_rect())).image(
            texture.id(), Rect::from_min_size(point(origin), size(destination)),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE,
        );
        clipped |= (0..2).any(|axis| {
            let start = f64::from(cell.origin[axis]);
            let end = start + f64::from(cell.size[axis]);
            origin[axis] < start - 0.5 || origin[axis] + destination[axis] > end + 0.5
        });
    }
    ui.painter().rect_stroke(printable.intersect(sheet), 0.0, Stroke::new(0.6, Color32::GRAY), StrokeKind::Inside);
    clipped
}
