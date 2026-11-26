use std::path::{Path, PathBuf};

use dtool_core::{CommandError, ContextId, Document, longdoc::helpers::parse_page_ranges};
use dtool_io::import_pdf::{ImportAs, ImportColour, ImportFormat, ImportOptions, PageSel};
use dtool_strings::text;

use crate::dialog_chrome::{ClosePolicy, DialogResult, confirm_cancel_buttons, confirm_from_enter, options_divider, show_modal};
#[cfg(windows)]
use crate::panels::ThumbnailJob;

pub(crate) fn is_pdf(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

pub(crate) struct PdfLoad {
    pub target: Option<ContextId>,
    pub layout: bool,
    pub result: std::sync::mpsc::Receiver<Result<Document, CommandError>>,
}

pub(crate) struct PdfImportDialog {
    pub load: Option<PdfLoad>,
    pub path: PathBuf,
    pub target: Option<ContextId>,
    pub insert: bool,
    pub options: ImportOptions,
    pub error: Option<String>,
    page_count: usize,
    scroll_to_start: bool,
    all_pages: bool,
    range: String,
    encrypted: bool,
    password: String,
    #[cfg(windows)]
    preview_page: Option<usize>,
    preview: Option<egui::TextureHandle>,
    #[cfg(windows)]
    preview_job: Option<ThumbnailJob<usize, Option<egui::ColorImage>>>,
}

impl PdfImportDialog {
    pub fn new(path: PathBuf, target: Option<ContextId>, layout: bool) -> Result<Self, dtool_io::IoError> {
        let profile = dtool_io::admit(&path, ImportFormat::Pdf)?;
        Ok(Self {
            load: None,
            path,
            target,
            insert: target.is_some(),
            options: ImportOptions {
                import_as: if layout { ImportAs::Pages } else { ImportAs::Artboards },
                ..Default::default()
            },
            error: profile.reason_key.filter(|_| !profile.encrypted),
            page_count: profile.pages as usize,
            scroll_to_start: true,
            all_pages: true,
            range: String::new(),
            encrypted: profile.encrypted,
            password: String::new(),
            #[cfg(windows)]
            preview_page: None,
            preview: None,
            #[cfg(windows)]
            preview_job: None,
        })
    }

    pub fn start_load(&mut self, ctx: &egui::Context, layout: bool) {
        let target = self.target.filter(|_| self.insert);
        let path = self.path.clone();
        let options = self.options.clone();
        let (sender, result) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let document = dtool_io::import_commands::prepare_pdf(&path, &options, target.is_none() || layout)
                .map(|(document, _)| document);
            let _ = sender.send(document);
            ctx.request_repaint();
        });
        self.load = Some(PdfLoad { target, layout, result });
    }

    pub fn show_loading(&self, ctx: &egui::Context) {
        show_modal(ctx, "pdf-import-load", text("import.pdf.title"), ClosePolicy::JobRunning, false, |ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text("import.pdf.loading"));
            });
        });
    }

    #[cfg(windows)]
    fn update_preview(&mut self, ctx: &egui::Context, page: usize) -> bool {
        use std::sync::mpsc::TryRecvError;

        if self.encrypted || self.page_count == 0 || self.preview_page == Some(page) {
            return false;
        }
        self.preview = None;
        if let Some(job) = &self.preview_job {
            let image = match job.result.try_recv() {
                Ok(image) => image,
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => None,
            };
            let finished_page = job.revision;
            self.preview_job = None;
            if finished_page == page {
                self.preview_page = Some(page);
                self.preview = image.map(|image| {
                    ctx.load_texture("pdf-import-preview", image, egui::TextureOptions::LINEAR)
                });
                return false;
            }
        }
        self.preview_page = None;
        let path = self.path.clone();
        self.preview_job = ThumbnailJob::start(ctx, "dtool-pdf-import-preview", page, move || {
            let pixels = dtool_export::raster::pdf_file_thumbnail(&path, page as u32 - 1).ok()?;
            Some(egui::ColorImage::from_rgba_unmultiplied(
                [pixels.width() as usize, pixels.height() as usize],
                pixels.as_raw(),
            ))
        }).ok();
        if self.preview_job.is_none() {
            self.preview_page = Some(page);
        }
        self.preview_job.is_some()
    }

    fn selected_pages(&self) -> Option<Vec<usize>> {
        parse_page_ranges(&self.range, self.page_count).ok().filter(|pages| !pages.is_empty())
    }

    fn can_confirm(&self) -> bool {
        (self.all_pages || self.selected_pages().is_some()) && (!self.encrypted || !self.password.is_empty())
    }

    pub fn show(&mut self, ctx: &egui::Context, target_title: Option<&str>, target_layout: bool) -> DialogResult {
        if target_title.is_none() {
            self.insert = false;
        }
        let page = if self.all_pages { 1 } else {
            self.selected_pages().and_then(|pages| pages.first().copied()).unwrap_or(1)
        };
        #[cfg(windows)]
        let preview_pending = self.update_preview(ctx, page);
        #[cfg(not(windows))]
        let preview_pending = false;
        let focus_before = ctx.memory(|memory| memory.focused());
        let range_id = egui::Id::unique("dtool.pdf-import.range");
        let password_id = egui::Id::unique("dtool.pdf-import.password");
        let (keyboard, (confirm, cancel)) = show_modal(ctx, "pdf-import", text("import.pdf.title"),
            ClosePolicy::CancelOnly, self.can_confirm(), |ui| {
                let width = (ctx.content_rect().width() - 52.0).clamp(280.0, 590.0);
                ui.set_width(width);
                if let Some(name) = self.path.file_name() {
                    let name = name.to_string_lossy();
                    ui.add(egui::Label::new(name.as_ref()).truncate()).on_hover_text(name.as_ref());
                }
                let height = (ctx.content_rect().height() - 104.0).clamp(90.0, 325.0);
                let offset = std::mem::take(&mut self.scroll_to_start).then_some(0.0);
                dtool_design::dialog_options_scroll_with_offset(ui, "pdf-import-options", height, offset, |ui| {
                    if width >= 510.0 {
                        ui.columns(2, |columns| {
                            self.show_options(&mut columns[0], target_title, target_layout, range_id, password_id);
                            self.show_preview(&mut columns[1], page, preview_pending);
                        });
                    } else {
                        self.show_options(ui, target_title, target_layout, range_id, password_id);
                        self.show_preview(ui, page, preview_pending);
                    }
                    if let Some(error) = &self.error {
                        ui.label(text(error));
                    }
                });
                options_divider(ui);
                confirm_cancel_buttons(ui, text("common.ok"), self.can_confirm())
            });
        let result = if cancel || keyboard == DialogResult::Cancel {
            DialogResult::Cancel
        } else if confirm || confirm_from_enter(ctx, keyboard, focus_before, &[range_id, password_id]) {
            DialogResult::Confirm
        } else {
            DialogResult::Open
        };
        if result == DialogResult::Confirm {
            if !self.can_confirm() { return DialogResult::Open; }
            self.options.pages = if self.all_pages { PageSel::All } else { PageSel::Selection(self.range.clone()) };
            self.options.password = self.encrypted.then(|| self.password.clone());
            if self.insert {
                self.options.import_as = ImportAs::Pages;
            }
        }
        result
    }

    fn show_options(
        &mut self,
        ui: &mut egui::Ui,
        target_title: Option<&str>,
        target_layout: bool,
        range_id: egui::Id,
        password_id: egui::Id,
    ) {
        let count = self.page_count.to_string();
        ui.strong(if self.page_count > 0 {
            dtool_strings::format("import.pdf.page_count", &[("count", &count)])
        } else {
            text("import.pdf.page_selection").into()
        });
        ui.horizontal_wrapped(|ui| {
            dtool_design::choice_value(ui, &mut self.all_pages, true, text("import.pdf.all_pages"));
            ui.add_enabled_ui(self.page_count != 0, |ui| {
                dtool_design::choice_value(ui, &mut self.all_pages, false, text("import.pdf.selected_pages"));
            });
        });
        if !self.all_pages {
            ui.add(egui::TextEdit::singleline(&mut self.range).id(range_id).hint_text("1–3, 5"));
            if !self.range.is_empty() && self.selected_pages().is_none() {
                ui.small(text("import.pdf.invalid_range"));
            }
        }

        crate::dialog_chrome::options_group(ui, text("import.pdf.destination"));
        if let Some(title) = target_title {
            ui.horizontal_wrapped(|ui| {
                dtool_design::choice_value(ui, &mut self.insert, true, text("import.pdf.current_document")).on_hover_text(title);
                dtool_design::choice_value(ui, &mut self.insert, false, text("import.pdf.new_document"));
            });
        }
        if self.insert {
            ui.small(if target_layout { text("import.pdf.insert_after") } else { text("import.pdf.place_objects") });
        } else {
            ui.horizontal_wrapped(|ui| {
                dtool_design::choice_value(ui, &mut self.options.import_as, ImportAs::Pages, text("import.pdf.pages"));
                dtool_design::choice_value(ui, &mut self.options.import_as, ImportAs::Artboards, text("import.pdf.artboards"));
            });
        }

        crate::dialog_chrome::options_group(ui, text("import.pdf.appearance"));
        ui.horizontal(|ui| {
            ui.label("DPI");
            let mut dpi = f64::from(self.options.dpi);
            dtool_design::number_drag(ui, ui.id().with("pdf-import-dpi"), &mut dpi, 1.0..=2400.0, 1.0);
            self.options.dpi = dpi.round() as u32;
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(text("import.pdf.colour"));
            dtool_design::choice_value(ui, &mut self.options.colour, ImportColour::Rgb, "RGB");
            dtool_design::choice_value(ui, &mut self.options.colour, ImportColour::Cmyk, "CMYK");
            dtool_design::choice_value(ui, &mut self.options.colour, ImportColour::Grey, text("import.pdf.grey"));
        });
        dtool_design::toggle(ui, &mut self.options.favour_editable_text, text("import.pdf.editable_text"))
            .on_hover_text(text("import.pdf.editable_text.help"));
        dtool_design::toggle(ui, &mut self.options.group_text_lines, text("import.pdf.group_text"));
        if self.encrypted {
            ui.label(text("import.pdf.password"));
            ui.add(egui::TextEdit::singleline(&mut self.password).id(password_id).password(true));
        }
    }

    fn show_preview(&self, ui: &mut egui::Ui, page: usize, pending: bool) {
        ui.strong(dtool_strings::format("import.pdf.preview_page", &[("page", &page.to_string())]));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 240.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 4.0, dtool_design::color("dialog.separator"));
        if let Some(texture) = &self.preview {
            let size = texture.size_vec2();
            let bounds = rect.shrink(8.0);
            let scale = (bounds.width() / size.x).min(bounds.height() / size.y);
            let image = egui::Rect::from_center_size(bounds.center(), size * scale);
            ui.painter().image(texture.id(), image, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        } else if pending {
            let spinner = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(24.0));
            ui.put(spinner, egui::Spinner::new().size(24.0));
        } else {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text("import.pdf.preview_unavailable"),
                egui::TextStyle::Small.resolve(ui.style()), ui.visuals().weak_text_color());
        }
    }
}
