use dtool_core::document::Document;
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::CommandError;
use egui::emath::GuiRounding;
use dtool_export::raster::{self, EncodedExport, EncodedPreview, ExportError, ExportJob, ExportOptions, JobReceipt, JobStep, OverwriteDecision, OverwritePolicy, PdfColourSpace, RgbaImage};
use dtool_export::{ExportArea, ExportFormat, ExportReceipt};
use dtool_strings::text;
use serde_json::{json, Value};
use std::sync::mpsc;
use crate::dialog_chrome::{options_group, options_scroll};

pub const UI_EXPORT_OPEN: &str = "ui.export.open";
pub const UI_EXPORT_SET: &str = "ui.export.set";
pub const UI_EXPORT_QUERY: &str = "ui.export.query";
pub const UI_EXPORT_PREVIEW: &str = "ui.export.preview";
pub const UI_EXPORT_CLOSE: &str = "ui.export.close";
pub const UI_EXPORT_COMMANDS: [&str; 5] = [UI_EXPORT_OPEN, UI_EXPORT_SET, UI_EXPORT_QUERY, UI_EXPORT_PREVIEW, UI_EXPORT_CLOSE];
pub const EXPORT_JOB_RUN: &str = raster::EXPORT_JOB_RUN;
pub const EXPORT_JOB_LIST: &str = raster::EXPORT_JOB_LIST;
pub const EXPORT_JOB_DELETE: &str = raster::EXPORT_JOB_DELETE;
pub const EXPORT_JOB_COMMANDS: [&str; 3] = raster::EXPORT_JOB_COMMANDS;

pub const SHORTCUT_OPEN: &str = "Ctrl+Alt+Shift+W";
pub const SHORTCUT_EXPORT: &str = "Enter";
pub const SHORTCUT_CANCEL: &str = "Escape";

pub const ZOOM_MIN: f32 = 0.001;
pub const ZOOM_MAX: f32 = 8.0;
pub const ZOOM_DEFAULT: f32 = 1.0;

pub fn is_vector(format: ExportFormat) -> bool {
    matches!(format, ExportFormat::Pdf | ExportFormat::Svg | ExportFormat::Eps)
}

pub fn format_choices() -> &'static [ExportFormat] {
    dtool_export::batch::E06_OFFERED_FORMATS
}

pub fn format_label(format: ExportFormat) -> &'static str {
    dtool_export::batch::e06_format_label(format)
}

fn format_colour(format: ExportFormat) -> egui::Color32 {
    let [red, green, blue] = match format {
        ExportFormat::Png => [59, 182, 205],
        ExportFormat::Jpeg => [105, 185, 130],
        ExportFormat::Pdf => [224, 100, 110],
        ExportFormat::Svg => [234, 165, 72],
        ExportFormat::Webp => [112, 152, 226],
        ExportFormat::Eps => [86, 189, 170],
        ExportFormat::Tiff => [197, 122, 174],
        ExportFormat::Gif => [154, 129, 213],
        ExportFormat::Wmf => [153, 160, 170],
    };
    egui::Color32::from_rgb(red, green, blue)
}

const PREVIEW_TEXTURE: &str = "dtool.export.preview";
const PREVIEW_SCROLL: &str = "dtool.export.preview.scroll";
const SEARCH_ID: &str = "dtool.export.search";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportShortcut {
    Open,
    Export,
    Cancel,
}

pub fn shortcut_for(modifiers: egui::Modifiers, key: egui::Key) -> Option<ExportShortcut> {
    if key == egui::Key::W && modifiers.ctrl && modifiers.alt && modifiers.shift {
        return Some(ExportShortcut::Open);
    }
    if modifiers.any() {
        return None;
    }
    match key {
        egui::Key::Enter => Some(ExportShortcut::Export),
        egui::Key::Escape => Some(ExportShortcut::Cancel),
        _ => None,
    }
}

pub fn open_shortcut_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.modifiers.ctrl && i.modifiers.alt && i.modifiers.shift && i.key_pressed(egui::Key::W))
}

pub fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(ZOOM_MIN, ZOOM_MAX)
    } else {
        ZOOM_DEFAULT
    }
}

pub fn format_size(bytes: Option<u64>) -> String {
    match bytes {
        None => text("common.value.unknown").to_string(),
        Some(b) if b < 1024 => format!("{} B", b),
        Some(b) if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        Some(b) => format!("{:.2} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

pub fn format_index(format: ExportFormat) -> usize {
    format_choices().iter().position(|f| *f == format).unwrap_or(0)
}

#[derive(Debug, Clone)]
pub enum ExportOutcome {
    Exported(ExportReceipt),
    Cancelled,
}

struct PdfPreviewWorker {
    generation: u64,
    result: mpsc::Receiver<Result<EncodedPreview, ExportError>>,
}

struct PdfSave {
    path: String,
    overwrite: bool,
    result: mpsc::Receiver<Result<ExportReceipt, ExportError>>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ExportPane {
    Formats,
    #[default]
    Settings,
    Preview,
}

fn default_area(doc: &Document, format: ExportFormat) -> ExportArea {
    if format == ExportFormat::Pdf && doc.layout_artboard_origins.is_some() {
        ExportArea::AllPages
    } else {
        ExportArea::WholeDocument
    }
}

#[derive(Default)]
pub struct ExportDialog {
    pub open: bool,
    pub options: ExportOptions,
    explicit_area: bool,
    pub context: dtool_export::ExportContext,
    pub document_viewport: Option<[f32; 2]>,
    pub last_paths: Vec<std::path::PathBuf>,
    pub zoom: f32,
    pub pan: [f32; 2],
    pub path: String,
    pub overwrite: bool,
    pub preview: Option<EncodedPreview>,
    pub error: Option<ExportError>,
    pub last_receipt: Option<ExportReceipt>,
    pub generation: u64,
    pub search_query: String,
    pub show_in_explorer: bool,
    pub ask_for_location: bool,
    compact_pane: ExportPane,
    texture: Option<egui::TextureHandle>,
    preview_checker: Option<egui::TextureHandle>,
    texture_generation: u64,
    texture_reduction: u32,
    preview_index: u32,
    preview_count: u32,
    pdf_preview_repaint: Option<egui::Context>,
    pdf_preview_worker: Option<PdfPreviewWorker>,
    pdf_preview_pending: bool,
    pdf_save: Option<PdfSave>,
    page_range_input: Option<String>,
    page_range_invalid: bool,
    tiles: Vec<(egui::TextureHandle, [u32; 4])>,
    viewport: [f32; 2],
    fit_preview: bool,
    doc_version: u64,
    pub job: ExportJob,
    pub job_receipt: Option<JobReceipt>,
    pending_overwrite: Option<(u64, std::path::PathBuf)>,
    single_overwrite: Option<String>,
    owner: Option<ContextId>,
    option_icons: crate::icon_cache::IconCache,
}

impl std::fmt::Debug for ExportDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportDialog")
            .field("open", &self.open)
            .field("options", &self.options)
            .field("zoom", &self.zoom)
            .field("pan", &self.pan)
            .field("estimated_size", &self.estimated_size())
            .finish()
    }
}

impl ExportDialog {
    pub fn set_session_context(&mut self, session: &dtool_core::Session) {
        let next = session.view_canvas_size.or(self.document_viewport)
            .filter(|size| size.iter().all(|side| side.is_finite() && *side > 1.0))
            .map(|size| dtool_export::ExportContext::for_view(session, size))
            .unwrap_or_else(|| dtool_export::ExportContext::from_session(session));
        if self.context != next {
            self.context = next;
            self.preview = None;
            self.pdf_preview_pending = false;
            self.error = None;
        }
    }

    fn expand_job_areas(&mut self, doc: &Document) -> Result<(), ExportError> {
        let mut next = self.job.clone();
        next.items.clear();
        for item in &self.job.items {
            let areas = raster::resolve_output_areas(doc, &item.options, &self.context)?;
            if item.options.format != ExportFormat::Pdf && (item.options.area.is_multi() || areas.len() > 1) {
                for area in areas {
                    let mut options = item.options.clone();
                    options.area = area.area;
                    let path = area.suffix.as_deref().map(|suffix| dtool_export::suffixed_path(&item.path, suffix)).unwrap_or_else(|| item.path.clone());
                    next.queue(&item.name, path, options);
                }
            } else {
                next.items.push(item.clone());
            }
        }
        self.job = next;
        Ok(())
    }

    pub fn new() -> Self {
        Self { zoom: ZOOM_DEFAULT, ..Self::default() }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn estimated_size(&self) -> Option<u64> {
        self.preview.as_ref().map(|p| p.estimated_size())
    }

    pub fn estimated_size_label(&self) -> String {
        format_size(self.estimated_size())
    }

    pub fn preview_size(&self) -> Option<(u32, u32)> {
        self.preview.as_ref().map(|p| p.dimensions())
    }

    pub fn preview_image(&self) -> Option<&RgbaImage> {
        self.preview.as_ref().map(|p| &p.image)
    }

    pub fn preview_bytes(&self) -> Option<&[u8]> {
        self.preview.as_ref().map(|p| p.bytes.as_slice())
    }

    pub fn display_size(&self) -> Option<[f32; 2]> {
        let zoom = clamp_zoom(self.zoom);
        self.preview_size().map(|(w, h)| [w as f32 * zoom, h as f32 * zoom])
    }

    pub fn open_with(&mut self, doc: &Document, format: Option<ExportFormat>) -> Value {
        let format = format.unwrap_or(self.options.format);
        if format != self.options.format || !self.open {
            self.options = ExportOptions { area: default_area(doc, format), ..ExportOptions::for_format(format) };
            self.explicit_area = false;
            if let Some(saved) = dtool_core::commands::export_presets::current_options(doc)
                && let Ok(next) = serde_json::from_value::<ExportOptions>(dtool_core::commands::export_presets::merged_options(serde_json::to_value(&self.options).unwrap_or_default(), &saved))
                && next.format == format
            {
                self.options = next;
                self.explicit_area = saved.contains_key("area");
            }
        }
        self.open = true;
        self.preview_index = 0;
        self.page_range_input = None;
        self.page_range_invalid = false;
        self.zoom = ZOOM_DEFAULT;
        self.fit_preview = true;
        self.pan = [0.0, 0.0];
        self.viewport = [0.0, 0.0];
        self.error = None;
        self.last_receipt = None;
        self.search_query.clear();
        self.compact_pane = ExportPane::Settings;
        self.show_in_explorer = false;
        self.ask_for_location = true;
        self.refresh(doc);
        self.query()
    }

    pub fn is_stale(&self, doc: &Document) -> bool {
        self.open && (self.preview.is_none() && self.error.is_none() && !self.pdf_preview_pending
            || self.doc_version != doc.version)
    }

    pub fn ensure_current(&mut self, doc: &Document) -> bool {
        let stale = self.is_stale(doc);
        if stale {
            self.refresh(doc);
        }
        self.update_pdf_preview(doc);
        stale
    }

    pub fn viewport(&self) -> [f32; 2] {
        self.viewport
    }

    pub fn set_viewport(&mut self, viewport: [f32; 2]) {
        let w = if viewport[0].is_finite() { viewport[0].max(0.0).round() } else { 0.0 };
        let h = if viewport[1].is_finite() { viewport[1].max(0.0).round() } else { 0.0 };
        self.viewport = [w, h];
        self.pan = self.clamp_pan(self.pan);
    }

    pub fn zoom_at(&mut self, zoom: f32, anchor: [f32; 2]) -> f32 {
        self.fit_preview = false;
        let before = clamp_zoom(self.zoom);
        let after = clamp_zoom(zoom);
        let anchor = anchor.map(|value| if value.is_finite() { value } else { 0.0 });
        let old_inset = self.preview_inset();
        let ratio = after / before;
        self.zoom = after;
        let new_inset = self.preview_inset();
        self.pan = self.clamp_pan(std::array::from_fn(|i| {
            anchor[i] - (anchor[i] - self.pan[i] - old_inset[i]) * ratio - new_inset[i]
        }));
        after
    }

    pub fn is_pixel_exact(&self) -> bool {
        (clamp_zoom(self.zoom) - 1.0).abs() <= f32::EPSILON
    }

    pub fn preview_pixel(&self, view: [f32; 2]) -> Option<(u32, u32)> {
        let inset = self.preview_inset();
        let origin = std::array::from_fn(|i| self.pan[i] + inset[i]);
        self.preview.as_ref().and_then(|p| raster::preview_pixel(p, view, origin, self.zoom))
    }

    fn preview_inset(&self) -> [f32; 2] {
        let display = self.display_size().unwrap_or([0.0; 2]);
        std::array::from_fn(|i| ((self.viewport[i] - display[i]).max(0.0) * 0.5).round())
    }

    pub fn verify_export(&self, path: &str) -> Result<bool, ExportError> {
        match &self.preview {
            Some(preview) => preview.matches_file(std::path::Path::new(path), self.options.format),
            None => Err(self.error.clone().unwrap_or_else(|| ExportError::Io("preview".to_string()))),
        }
    }

    pub fn refresh(&mut self, doc: &Document) {
        self.doc_version = doc.version;
        self.generation = self.generation.wrapping_add(1);
        self.preview_count = if self.options.format == ExportFormat::Pdf {
            use dtool_export::ExportArea;
            let area = match &self.options.area {
                ExportArea::CurrentPage => ExportArea::Page(self.context.current_page.min(doc.pages.len().saturating_sub(1)) as u32),
                ExportArea::CurrentSpread => ExportArea::Spread(dtool_export::spread_of_page(doc, self.context.current_page) as u32),
                area => area.clone(),
            };
            dtool_export::pdf_output_page_count(doc, &area, self.options.pages.as_deref()) as u32
        } else {
            raster::resolve_output_areas(doc, &self.options, &self.context).map_or(0, |areas| {
                if self.options.area.is_multi() || areas.len() > 1 { areas.len() as u32 } else { 0 }
            })
        };
        self.preview_index = self.preview_index.min(self.preview_count.saturating_sub(1));
        self.pdf_preview_pending = self.options.format == ExportFormat::Pdf && self.pdf_preview_repaint.is_some();
        if self.pdf_preview_pending {
            self.preview = None;
            self.error = None;
            return;
        }
        let result = self.render_preview(doc, self.preview_index);
        self.apply_preview(result);
    }

    /// The desktop frame schedules completed previews for display; headless callers render inline.
    pub fn enable_background_pdf_preview(&mut self, ctx: egui::Context) {
        self.pdf_preview_repaint = Some(ctx);
    }

    fn update_pdf_preview(&mut self, doc: &Document) {
        if let Some(worker) = &self.pdf_preview_worker {
            let result = match worker.result.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => Err(ExportError::Io("PDF preview worker".into())),
            };
            if self.pdf_preview_pending && worker.generation == self.generation {
                self.pdf_preview_pending = false;
                self.apply_preview(result);
            }
            self.pdf_preview_worker = None;
        }
        if !self.pdf_preview_pending {
            return;
        }
        let Some(ctx) = self.pdf_preview_repaint.clone() else { return };
        let doc = doc.clone();
        let options = self.options.clone();
        let context = self.context.clone();
        let page_index = self.preview_index;
        let (sender, result) = mpsc::channel();
        match std::thread::Builder::new().name("dtool-pdf-preview".into()).spawn(move || {
            let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Self::render_pdf_preview(&doc, &options, &context, page_index)
            })).unwrap_or_else(|_| Err(ExportError::Io("PDF preview worker".into())));
            let _ = sender.send(rendered);
            ctx.request_repaint();
        }) {
            Ok(_) => self.pdf_preview_worker = Some(PdfPreviewWorker { generation: self.generation, result }),
            Err(error) => {
                self.pdf_preview_pending = false;
                self.apply_preview(Err(ExportError::Io(error.to_string())));
            }
        }
    }

    fn apply_preview(&mut self, result: Result<EncodedPreview, ExportError>) {
        match result {
            Ok(preview) => {
                self.preview = Some(preview);
                self.error = None;
            }
            Err(error) => {
                self.preview = None;
                self.error = Some(error);
            }
        }
    }

    fn render_pdf_preview(doc: &Document, options: &ExportOptions, context: &dtool_export::ExportContext, page_index: u32) -> Result<EncodedPreview, ExportError> {
        if options.area.is_multi() {
            dtool_export::resolve_areas(doc, &options.area, context, options.pages.as_deref())
                .map_err(|_| ExportError::InvalidArea("pages".into()))?;
            raster::encode_pdf_preview_at(doc, options, page_index)
        } else {
            let (doc, options, _) = resolved_item(doc, options, context, 0)?;
            raster::encode_pdf_preview_at(&doc, &options, page_index)
        }
    }

    fn render_preview(&self, doc: &Document, index: u32) -> Result<EncodedPreview, ExportError> {
        if self.options.format == ExportFormat::Pdf {
            return Self::render_pdf_preview(doc, &self.options, &self.context, index);
        }
        resolved_item(doc, &self.options, &self.context, index).and_then(|(doc, mut options, suffix)| {
            let format = options.format;
            if format == ExportFormat::Svg {
                return raster::encode_svg_preview(&doc, &options);
            }
            let eps = if format == ExportFormat::Eps {
                let path = std::path::PathBuf::from(self.output_path());
                let path = match suffix.as_deref() {
                    Some(suffix) => dtool_export::suffixed_path(&path, suffix),
                    None => path,
                };
                Some(raster::encode_output(&doc, &options, &path)?.into_bytes())
            } else {
                None
            };
            if is_vector(format) {
                options.icc = None;
                options.embed_icc = false;
                options.embed_metadata = false;
                options.intent = raster::RenderIntent::Perceptual;
            }
            let image = raster::render_preview(&doc, &options)?;
            match eps {
                Some(bytes) => Ok(EncodedPreview { bytes, image }),
                None => raster::encode_preview_document(&doc, &image, &options),
            }
        })
    }

    pub fn set(&mut self, doc: &Document, params: &Value) -> Result<Value, ExportError> {
        let mut patch = params.clone();
        let mut zoom = None;
        if let Some(fields) = patch.as_object_mut() {
            zoom = fields.remove("zoom").and_then(|v| v.as_f64());
            fields.remove("path");
            fields.remove("overwrite");
        }
        let mut next = self.options.merge_json(&patch)?;
        self.explicit_area |= patch.get("area").is_some();
        if !self.explicit_area && next.format != self.options.format {
            next.area = default_area(doc, next.format);
        }
        if let Some(path) = params.get("path").and_then(|v| v.as_str()) {
            self.path = path.to_string();
            self.ask_for_location = path.trim().is_empty();
        }
        if let Some(overwrite) = params.get("overwrite").and_then(|v| v.as_bool()) {
            self.overwrite = overwrite;
        }
        if let Some(zoom) = zoom {
            self.fit_preview = false;
            self.zoom = clamp_zoom(zoom as f32);
        }
        if next.pages != self.options.pages || next.format != self.options.format {
            self.page_range_input = None;
            self.page_range_invalid = false;
        }
        if next.area != self.options.area || next.pages != self.options.pages || next.format != self.options.format {
            self.preview_index = 0;
        }
        let refresh = self.pdf_preview_repaint.is_none()
            || next.format != ExportFormat::Pdf
            || self.options != next
            || self.is_stale(doc)
            || self.error.is_some();
        self.options = next;
        if refresh {
            self.refresh(doc);
        }
        Ok(self.query())
    }

    pub fn query(&self) -> Value {
        json!({
            "open": self.open,
            "saving": self.pdf_save.is_some(),
            "options": serde_json::to_value(&self.options).unwrap_or(Value::Null),
            "estimated_size": self.estimated_size(),
            "estimated_size_label": self.estimated_size_label(),
            "preview": self.preview_value(),
            "path": self.path,
            "written_paths": self.last_paths,
            "error": self.error.as_ref().map(|e| e.code()),
            "show_in_explorer": self.show_in_explorer,
            "ask_for_location": self.ask_for_location,
            "job": self.job.list_json(),
            "job_receipt": self.job_receipt_value(),
            "pending_overwrite": self.pending_overwrite().map(|p| p.to_string_lossy().to_string()),
        })
    }

    fn preview_value(&self) -> Value {
        let size = self.preview_size();
        let display = self.display_size();
        json!({
            "width": size.map(|s| s.0),
            "height": size.map(|s| s.1),
            "zoom": clamp_zoom(self.zoom),
            "display_width": display.map(|d| d[0]),
            "display_height": display.map(|d| d[1]),
            "pan": self.pan,
            "viewport": self.viewport,
            "pixel_exact": self.is_pixel_exact(),
            "pending": self.pdf_preview_pending,
            "page": (self.preview_count > 0 && self.preview.is_some()).then_some(self.preview_index + 1),
            "page_count": (self.preview_count > 0 && self.preview.is_some()).then_some(self.preview_count),
            "tiles": self.tiles.len(),
            "generation": self.generation,
            "doc_version": self.doc_version,
            "bytes": self.estimated_size(),
            "format": raster::format_name(self.options.format),
        })
    }

    fn select_preview(&mut self, doc: &Document, index: u32) -> Result<(), ExportError> {
        if self.pdf_preview_pending && index < self.preview_count {
            if index != self.preview_index {
                self.preview_index = index;
                self.pan = [0.0, 0.0];
                self.refresh(doc);
            }
            return Ok(());
        }
        if self.preview.is_none() || index >= self.preview_count {
            return Err(ExportError::InvalidArea("pages".into()));
        }
        if index != self.preview_index {
            if self.options.format == ExportFormat::Pdf {
                let preview = self.preview.as_mut().ok_or_else(|| ExportError::InvalidArea("pages".into()))?;
                preview.image = raster::render_pdf_preview_page(&preview.bytes, doc, &self.options, index)?;
            } else {
                let preview = self.render_preview(doc, index)?;
                self.apply_preview(Ok(preview));
            }
            self.preview_index = index;
            self.pan = [0.0, 0.0];
            self.generation = self.generation.wrapping_add(1);
        }
        Ok(())
    }

    pub fn preview_command(&mut self, params: &Value) -> Value {
        if let Some(zoom) = params.get("zoom").and_then(|v| v.as_f64()) {
            let anchor = match params.get("anchor").and_then(|v| v.as_array()) {
                Some(a) => [a.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32, a.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32],
                None => [self.viewport[0] / 2.0, self.viewport[1] / 2.0],
            };
            self.zoom_at(zoom as f32, anchor);
        }
        if let Some(pan) = params.get("pan").and_then(|v| v.as_array()) {
            let x = pan.first().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let y = pan.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            self.pan = self.clamp_pan([x, y]);
        }
        self.preview_value()
    }

    pub fn clamp_pan(&self, pan: [f32; 2]) -> [f32; 2] {
        let display = self.display_size().unwrap_or([0.0, 0.0]);
        let limit_x = (display[0] - self.viewport[0]).max(0.0).round();
        let limit_y = (display[1] - self.viewport[1]).max(0.0).round();
        [
            if pan[0].is_finite() { pan[0].round().clamp(-limit_x, 0.0) } else { 0.0 },
            if pan[1].is_finite() { pan[1].round().clamp(-limit_y, 0.0) } else { 0.0 },
        ]
    }

    pub fn close(&mut self) -> Value {
        if self.pdf_save.is_some() {
            return json!({ "open": true, "written": false, "saving": true });
        }
        self.open = false;
        self.pdf_preview_pending = false;
        self.preview = None;
        self.texture = None;
        self.tiles.clear();
        self.error = None;
        self.pending_overwrite = None;
        self.single_overwrite = None;
        self.owner = None;
        json!({ "open": false, "written": false })
    }

    pub fn default_path(&self, doc_name: &str) -> String {
        let stem = if doc_name.trim().is_empty() { "export" } else { doc_name.trim() };
        format!("{}.{}", stem, raster::format_extension(self.options.format))
    }

    fn output_path(&self) -> String {
        if self.path.trim().is_empty() { self.default_path("export") } else { self.path.clone() }
    }

    pub fn export_to(&mut self, doc: &Document, path: &str) -> Result<ExportReceipt, ExportError> {
        self.last_paths.clear();
        if path.trim().is_empty() {
            return Err(ExportError::Io("path".to_string()));
        }
        if self.options.format == ExportFormat::Pdf {
            let receipt = save_pdf(doc, &self.options, &self.context, std::path::Path::new(path), self.overwrite)?;
            self.last_paths.push(receipt.path.clone());
            self.last_receipt = Some(receipt.clone());
            return Ok(receipt);
        }
        let areas = raster::resolve_output_areas(doc, &self.options, &self.context)?;
        let paths: Vec<_> = areas.iter().map(|area| area.suffix.as_deref().map(|suffix| dtool_export::suffixed_path(std::path::Path::new(path), suffix)).unwrap_or_else(|| path.into())).collect();
        if !self.overwrite {
            if let Some(path) = paths.iter().find(|path| path.exists()) { return Err(ExportError::Collision(path.clone())); }
        }
        let mut first = None;
        for (area, path) in areas.iter().zip(paths.iter()) {
            let render_doc = area.only.as_ref().map(|ids| std::borrow::Cow::Owned(dtool_export::only_objects(doc, ids))).unwrap_or(std::borrow::Cow::Borrowed(doc));
            let mut options = self.options.clone();
            options.area = area.area.clone();
            let receipt = raster::encode_output(&render_doc, &options, path)?.write(path, self.overwrite)?;
            self.last_paths.push(receipt.path.clone());
            if first.is_none() { first = Some(receipt); }
        }
        let receipt = first.ok_or_else(|| ExportError::InvalidArea("area".into()))?;
        self.last_receipt = Some(receipt.clone());
        Ok(receipt)
    }

    pub fn queue_current(&mut self, path: &str) -> Result<u64, ExportError> {
        if path.trim().is_empty() {
            return Err(ExportError::Io("path".to_string()));
        }
        Ok(self.job.queue("", path, self.options.clone()))
    }

    pub fn job_run(&mut self, doc: &Document, params: &Value) -> Result<Value, ExportError> {
        if self.job.is_running() {
            return Err(ExportError::Unavailable("job".to_string()));
        }
        let policy = OverwritePolicy::parse(params.get("overwrite"))?;
        let before = self.job.clone();
        let prepared = (|| -> Result<(), ExportError> {
            let base = self.options.clone();
            if let Some(items) = params.get("items").and_then(|v| v.as_array()) {
                for item in items { self.job.queue_json(&base, item)?; }
            }
            if self.job.items.is_empty() {
                let path = params.get("path").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_else(|| self.path.clone());
                self.queue_current(&path)?;
            }
            self.expand_job_areas(doc)
        })();
        if let Err(error) = prepared { self.job = before; return Err(error); }
        let context = self.context.clone();
        let receipt = self.job.run_with(policy, |item| encode_context_item(doc, item, &context), |_| OverwriteDecision::Skip, || false);
        let mut value = receipt.to_json();
        value["items"] = self.job.list_json()["items"].clone();
        self.job_receipt = Some(receipt);
        Ok(value)
    }

    pub fn job_receipt_value(&self) -> Value {
        self.job_receipt.as_ref().map(|r| r.to_json()).unwrap_or(Value::Null)
    }

    pub fn start_job(&mut self) -> bool {
        if self.job.is_running() || self.job.items.is_empty() {
            return false;
        }
        self.pending_overwrite = None;
        self.job_receipt = None;
        self.job.start(OverwritePolicy::Ask);
        self.job.is_running()
    }

    pub fn cancel_job(&mut self) {
        self.job.request_cancel();
    }

    pub fn step_job(&mut self, doc: &Document, decision: Option<OverwriteDecision>) -> JobStep {
        let context = self.context.clone();
        let mut produce = |item: &raster::ExportJobItem| encode_context_item(doc, item, &context);
        let step = self.job.step(decision, &mut produce);
        match &step {
            JobStep::NeedsDecision(id, path) => self.pending_overwrite = Some((*id, path.clone())),
            JobStep::Done | JobStep::Cancelled => {
                self.pending_overwrite = None;
                self.job_receipt = Some(self.job.receipt().clone());
            }
            _ => self.pending_overwrite = None,
        }
        step
    }

    pub fn pending_overwrite(&self) -> Option<&std::path::Path> {
        self.pending_overwrite.as_ref().map(|(_, p)| p.as_path()).or_else(|| self.single_overwrite.as_deref().map(std::path::Path::new))
    }

    fn overwrite_prompt(&mut self, ui: &mut egui::Ui) -> Option<OverwriteDecision> {
        let path = self.pending_overwrite()?.to_string_lossy().to_string();
        let mut decision = None;
        dtool_design::panel_header(ui, text("dialog.export.overwrite.label"));
        ui.label(path);
        ui.horizontal(|ui| {
            if dtool_design::button(ui, text("button.yes")).clicked() {
                decision = Some(OverwriteDecision::Overwrite);
            }
            if dtool_design::button(ui, text("button.no")).clicked() {
                decision = Some(OverwriteDecision::Skip);
            }
            if dtool_design::button(ui, text("common.action.cancel")).clicked() {
                decision = Some(OverwriteDecision::Cancel);
            }
        });
        decision
    }

    fn receipt_summary(&self, ui: &mut egui::Ui) {
        let Some(receipt) = &self.job_receipt else {
            return;
        };
        if let Some(key) = receipt.message_key() {
            ui.label(text(key));
        }
        ui.label(format!("{} / {} / {}", receipt.written().len(), receipt.skipped().len(), receipt.failed().len()));
        for failed in receipt.failed() {
            ui.label(failed.path.to_string_lossy().to_string());
        }
    }

    pub fn run(&mut self, doc: &Document, name: &str, params: &Value) -> Option<Result<Value, ExportError>> {
        if self.pdf_save.is_some() && blocks_during_pdf_save(name) {
            return Some(Err(ExportError::Unavailable("PDF save in progress".into())));
        }
        match name {
            "export.options.schema" => return Some(Ok(json!({"formats": raster::options_schema(), "areas": ["WholeDocument", "CurrentPage", "AllPages", "CurrentSpread", "AllSpreads", "SelectionWithBackground", "SelectionWithoutBackground", "Artboard"]}))),
            EXPORT_JOB_RUN => return Some(self.job_run(doc, params)),
            EXPORT_JOB_LIST => return Some(Ok(self.job.list_json())),
            EXPORT_JOB_DELETE => return Some(Ok(self.job.delete_json(params))),
            _ => {}
        }
        match name {
            UI_EXPORT_OPEN => {
                let format = match params.get("format").and_then(|v| v.as_str()) {
                    Some(name) => match raster::parse_format(name) {
                        Ok(format) if raster::export_available(format) => Some(format),
                        Ok(format) => return Some(Err(ExportError::Unavailable(raster::format_name(format).to_string()))),
                        Err(error) => return Some(Err(error)),
                    },
                    None => None,
                };
                Some(Ok(self.open_with(doc, format)))
            }
            UI_EXPORT_SET => {
                if !self.open {
                    self.open_with(doc, None);
                }
                Some(self.set(doc, params))
            }
            UI_EXPORT_QUERY => {
                if self.open { self.ensure_current(doc); }
                Some(Ok(self.query()))
            }
            UI_EXPORT_PREVIEW => Some((|| {
                if self.open { self.ensure_current(doc); }
                if let Some(page) = params.get("page") {
                    let index = page.as_u64().and_then(|n| u32::try_from(n).ok())
                        .and_then(|n| n.checked_sub(1)).ok_or_else(|| ExportError::InvalidArea("pages".into()))?;
                    self.select_preview(doc, index)?;
                }
                self.compact_pane = ExportPane::Preview;
                Ok(self.preview_command(params))
            })()),
            UI_EXPORT_CLOSE => Some(Ok(self.close())),
            _ => None,
        }
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        let max_side = ctx.input(|i| i.max_texture_side).max(1).min(u32::MAX as usize) as u32;
        let pdf = self.options.format == ExportFormat::Pdf;
        let thumbnail = self.preview.as_ref().filter(|_| pdf).and_then(|preview| {
            // Linear sampling alone only sees nearby source pixels and can skip
            // thin PDF lines entirely when a print-resolution page is fitted.
            let level = (-clamp_zoom(self.zoom).log2()).round().max(0.0) as u32;
            let reduction = 1_u32 << level;
            let (width, height) = preview.dimensions();
            let w = width.div_ceil(reduction);
            let h = height.div_ceil(reduction);
            (reduction > 1 && w <= max_side && h <= max_side)
                .then_some((reduction, w, h, [0, 0, width, height]))
        });
        let reduction = thumbnail.map_or(1, |(reduction, ..)| reduction);
        if self.texture.is_some() && self.texture_generation == self.generation && self.texture_reduction == reduction {
            return;
        }
        let texture_options = if pdf {
            egui::TextureOptions::LINEAR
        } else {
            egui::TextureOptions::NEAREST
        };
        self.tiles = match &self.preview {
            Some(preview) if let Some((_, w, h, source)) = thumbnail => {
                let thumbnail = preview.thumbnail(w, h);
                let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], thumbnail.as_raw());
                vec![(ctx.load_texture(PREVIEW_TEXTURE, image, texture_options), source)]
            }
            Some(preview) => preview
                .tiles(max_side)
                .into_iter()
                .enumerate()
                .map(|(index, tile)| {
                    let part = preview.tile_image(tile);
                    let image = egui::ColorImage::from_rgba_unmultiplied([tile[2] as usize, tile[3] as usize], part.as_raw());
                    (ctx.load_texture(format!("{}.{}", PREVIEW_TEXTURE, index), image, texture_options), tile)
                })
                .collect(),
            None => Vec::new(),
        };
        self.texture = self.tiles.first().map(|(handle, _)| handle.clone());
        self.texture_generation = self.generation;
        self.texture_reduction = reduction;
    }

    pub fn visible_formats(&self) -> Vec<(ExportFormat, bool)> {
        self.filtered_formats().into_iter().map(|(_, format, _, offered)| (format, offered)).collect()
    }

    fn format_matches_query(&self, format: ExportFormat, label: &str) -> bool {
        let query = self.search_query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        let name = raster::format_name(format).to_lowercase();
        name.contains(&query) || label.to_lowercase().contains(&query)
    }

    fn filtered_formats(&self) -> Vec<(usize, ExportFormat, String, bool)> {
        let mut rows = Vec::new();
        for (index, format) in format_choices().iter().enumerate() {
            if !raster::export_available(*format) {
                continue;
            }
            let label = format_label(*format).to_string();
            if self.format_matches_query(*format, &label) {
                rows.push((index, *format, label, true));
            }
        }
        for format in raster::E06_HELD_FORMATS {
            let label = format!("{} ({})", format_label(*format), text("menu.planned.suffix"));
            if self.format_matches_query(*format, &label) {
                rows.push((usize::MAX, *format, label, false));
            }
        }
        rows
    }

    fn left_panel(&mut self, ui: &mut egui::Ui, doc: &Document) -> (bool, bool) {
        let mut changed = false;
        let search = ui.add(egui::TextEdit::singleline(&mut self.search_query)
            .hint_text(text("dialog.export.search")).id(egui::Id::unique(SEARCH_ID))
            .desired_width(ui.available_width()));
        ui.add_space(dtool_design::space("sm"));
        let formats = self.filtered_formats();
        for (_index, format, label, offered) in &formats {
            let selected = *format == self.options.format;
            let colour = format_colour(*format).gamma_multiply(if *offered { 1.0 } else { 0.55 });
            let marker = egui::Atom::paint(egui::vec2(5.0, 14.0), move |ui, args| {
                ui.painter().rect_filled(args.rect, 2.0, colour);
            });
            let mut label = egui::RichText::new(label.as_str());
            if selected {
                label = label.color(ui.visuals().selection.stroke.color);
            }
            let row = egui::Button::selectable(selected, (marker, label))
                .right_text(())
                .truncate()
                .min_size(egui::vec2(ui.available_width(), 26.0));
            if ui.add_enabled(*offered, row).clicked() && !selected {
                let keep = self.options.clone();
                self.page_range_input = None;
                self.page_range_invalid = false;
                self.preview_index = 0;
                let area = if self.explicit_area { keep.area } else { default_area(doc, *format) };
                self.options = ExportOptions { area, pages: keep.pages, width: keep.width, height: keep.height, scale: keep.scale, dpi: keep.dpi, svg: keep.svg, ..ExportOptions::for_format(*format) };
                changed = true;
            }
        }
        (changed, search.has_focus() || search.lost_focus())
    }

    fn centre_preview(&mut self, ui: &mut egui::Ui, doc: &Document, width: f32, height: f32, show_title: bool) {
        let fit = ui.horizontal(|ui| {
            if show_title {
                dtool_design::panel_header(ui, text("dialog.export.preview"));
            }
            let mut fit = dtool_design::button(ui, text("dialog.export.preview.fit")).clicked();
            if self.preview_count > 1 && self.preview.is_some() {
                let index = self.preview_index;
                let previous = ui.add_enabled(index > 0, egui::Button::new("‹"))
                    .on_hover_text(text("menu.document.pages.previous")).clicked();
                ui.label(format!("{}/{}", index + 1, self.preview_count));
                let next = ui.add_enabled(index + 1 < self.preview_count, egui::Button::new("›"))
                    .on_hover_text(text("menu.document.pages.next")).clicked();
                if previous || next {
                    match self.select_preview(doc, if previous { index - 1 } else { index + 1 }) {
                        Ok(()) => fit = true,
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            fit
        }).inner;
        let pixels_per_point = ui.ctx().pixels_per_point().max(f32::EPSILON);
        let preview_height = (height - 59.0).max(64.0);
        self.fit_preview |= fit;
        if self.fit_preview && let Some((w, h)) = self.preview_size() {
            let available = egui::vec2(width, preview_height) * pixels_per_point;
            self.zoom = clamp_zoom((available.x.floor() / w as f32).min(available.y.floor() / h as f32));
            self.pan = [0.0, 0.0];
        }
        let mut percent = clamp_zoom(self.zoom) * 100.0;
        if dtool_design::slider(ui, &mut percent, ZOOM_MIN * 100.0..=ZOOM_MAX * 100.0, text("dialog.export.preview.zoom.label")).changed() {
            self.zoom_at(percent / 100.0, self.viewport.map(|side| side * 0.5));
        }
        self.ensure_texture(ui.ctx());
        if self.texture.is_none() || self.tiles.is_empty() {
            if self.pdf_preview_pending {
                ui.spinner();
            } else {
                ui.label(text("common.value.unknown"));
            }
            return;
        }
        let checker = self.preview_checker.get_or_insert_with(|| {
            let light = egui::Color32::from_gray(234);
            let dark = egui::Color32::from_gray(199);
            ui.ctx().load_texture(
                "dtool.export.checker",
                egui::ColorImage::new([2, 2], vec![light, dark, dark, light]),
                egui::TextureOptions::NEAREST_REPEAT,
            )
        }).id();
        let tiles = &self.tiles;
        let zoom = clamp_zoom(self.zoom);
        let display = self.display_size().unwrap_or([0.0, 0.0]);
        let size = egui::vec2(display[0] / pixels_per_point, display[1] / pixels_per_point);
        let output = egui::ScrollArea::both()
            .id_salt(PREVIEW_SCROLL)
            .max_width(width)
            .max_height(preview_height)
            .auto_shrink([false, false])
            .scroll_offset(egui::vec2(-self.pan[0] / pixels_per_point, -self.pan[1] / pixels_per_point))
            .show_viewport(ui, |ui, viewport| {
                let (surface, response) = ui.allocate_exact_size(size.max(viewport.size()), egui::Sense::drag());
                let image = egui::Rect::from_center_size(surface.center(), size);
                let origin = image.min.round_to_pixels(pixels_per_point);
                let image = egui::Rect::from_min_size(origin, size.round_to_pixels(pixels_per_point));
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                let painter = ui.painter_at(image);
                let checker_uv = egui::Rect::from_min_max(egui::Pos2::ZERO, (image.size() / 16.0).to_pos2());
                painter.image(checker, image, checker_uv, egui::Color32::WHITE);
                for (handle, tile) in tiles {
                    let min = origin + egui::vec2((tile[0] as f32 * zoom).round() / pixels_per_point, (tile[1] as f32 * zoom).round() / pixels_per_point);
                    let max = origin + egui::vec2(((tile[0] + tile[2]) as f32 * zoom).round() / pixels_per_point, ((tile[1] + tile[3]) as f32 * zoom).round() / pixels_per_point);
                    painter.image(handle.id(), egui::Rect::from_min_max(min, max), uv, egui::Color32::WHITE);
                }
                response
            });
        let viewport = output.inner_rect.size();
        self.viewport = [(viewport.x * pixels_per_point).round(), (viewport.y * pixels_per_point).round()];
        let delta = output.inner.drag_delta();
        let offset = output.state.clamped_offset() - delta;
        self.pan = self.clamp_pan([-offset.x * pixels_per_point, -offset.y * pixels_per_point]);
        if output.inner.hovered() {
            let factor = ui.ctx().input(|i| i.zoom_delta());
            if factor.is_finite() && (factor - 1.0).abs() > f32::EPSILON {
                let pointer = ui.ctx().pointer_hover_pos().unwrap_or(output.inner_rect.center());
                let local = pointer - output.inner_rect.min;
                self.zoom_at(zoom * factor, [local.x * pixels_per_point, local.y * pixels_per_point]);
            }
        }
    }

    fn page_range_field(&mut self, ui: &mut egui::Ui, doc: &Document) -> (bool, bool) {
        let input = self.page_range_input.get_or_insert_with(|| {
            self.options.pages.as_deref().unwrap_or_default().iter()
                .map(u32::to_string).collect::<Vec<_>>().join(", ")
        });
        let label = text("dialog.export.pdf.page_range");
        let label_id = ui.label(label).id;
        let response = ui.add(egui::TextEdit::singleline(input)
            .id_salt("export.page_range")
            .hint_text(text("dialog.export.pdf.page_range_hint"))
            .desired_width(ui.available_width()))
            .on_hover_text(text("dialog.export.pdf.page_range_hint")).labelled_by(label_id);
        let editing = response.has_focus() || response.lost_focus();
        let commit = response.lost_focus() || (response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        let trimmed = input.trim();
        let parsed = if trimmed.is_empty() {
            Ok(None)
        } else if trimmed.split(',').any(|part| part.trim().is_empty()) {
            Err(dtool_protocol::CommandError::invalid_params())
        } else {
            dtool_core::longdoc::helpers::parse_page_ranges(trimmed, doc.pages.len().max(1))
                .map(|pages| Some(pages.into_iter().map(|page| page as u32).collect()))
        };
        self.page_range_invalid = parsed.is_err();
        if self.page_range_invalid {
            ui.colored_label(ui.visuals().error_fg_color, text("dialog.export.pdf.page_range_error"));
        }
        let changed = if let (true, Ok(pages)) = (commit, parsed) {
            let changed = self.options.pages != pages;
            self.options.pages = pages;
            if changed { self.preview_index = 0; }
            changed
        } else {
            false
        };
        (changed, editing)
    }

    fn right_settings(&mut self, ui: &mut egui::Ui, doc: &Document) -> (bool, bool) {
        let mut changed = false;
        let mut editing = false;
        ui.horizontal(|ui| {
            let (marker, _) = ui.allocate_exact_size(egui::vec2(5.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(marker, 2.0, format_colour(self.options.format));
            dtool_design::panel_header(ui, format_label(self.options.format));
        });
        options_group(ui, text("dialog.export.range_section"));
        let document_area_label = if default_area(doc, self.options.format) == ExportArea::AllPages {
            "dialog.export.area.first_page"
        } else {
            "dialog.export.area.document"
        };
        let standard_areas = [
            (ExportArea::WholeDocument, document_area_label),
            (dtool_export::ExportArea::CurrentPage, "dialog.export.area.current_page"),
            (dtool_export::ExportArea::AllPages, "dialog.export.area.all_pages"),
            (dtool_export::ExportArea::CurrentSpread, "dialog.export.area.current_spread"),
            (dtool_export::ExportArea::AllSpreads, "dialog.export.area.all_spreads"),
            (dtool_export::ExportArea::SelectionWithBackground, "dialog.export.area.selection_background"),
            (dtool_export::ExportArea::SelectionWithoutBackground, "dialog.export.area.selection_only"),
        ];
        let mut areas: Vec<_> = standard_areas.into_iter().map(|(area, key)| (area, text(key).to_owned())).collect();
        let artboards = dtool_core::view::ViewTarget::artboards(doc)
            .filter(|board| board.kind == dtool_core::ObjectKind::Artboard);
        for board in artboards {
            let name = board.name.trim();
            let label = if name.is_empty() { format!("{} {}", text("tool.artboard"), board.id.0) }
                else { format!("{} · {name}", text("tool.artboard")) };
            areas.push((dtool_export::ExportArea::Artboard(board.id), label));
        }
        for slice in doc.slices.iter().filter(|slice| slice.enabled) {
            if let Ok([left, top, right, bottom]) = slice.resolved_bounds(doc) {
                let label = format!("{} · {}", text("tool.slice"), slice.name);
                areas.push((dtool_export::ExportArea::Rect([left, top, right - left, bottom - top]), label));
            }
        }
        let current = areas.iter().find(|(area, _)| *area == self.options.area).unwrap_or(&areas[0]);
        egui::ComboBox::from_id_salt("export.area").width(ui.available_width()).truncate()
            .selected_text(&current.1).show_ui(ui, |ui| {
                dtool_design::with_icon_selection(ui, |ui| {
                    for (area, label) in &areas {
                        let clicked = ui.selectable_label(*area == self.options.area, label).clicked();
                        self.explicit_area |= clicked;
                        if clicked && *area != self.options.area {
                            self.options.area = area.clone();
                            self.preview_index = 0;
                            if !area.is_page_based() {
                                self.options.include_bleed = false;
                                self.options.pdf_crop_marks = false;
                            }
                            if !area.is_page_based() || matches!(area, dtool_export::ExportArea::WholeDocument) {
                                self.options.pdf_fold_marks = false;
                            }
                            changed = true;
                        }
                    }
                });
            }).response.on_hover_text(&current.1);
        if self.options.area.is_multi() {
            let (range_changed, range_editing) = self.page_range_field(ui, doc);
            changed |= range_changed;
            editing |= range_editing;
        } else {
            self.page_range_invalid = false;
        }
        options_group(ui, text("dialog.export.size_section"));
        let current = self.options.width.or_else(|| self.preview_size().map(|s| s.0)).unwrap_or(0);
        let mut width = f64::from(current);
        let width_edited = dtool_design::number_field(ui, &mut width, text("dialog.export.width.label")).changed();
        let next = width.round().clamp(1.0, f64::from(raster::MAX_EXPORT_SIDE)) as u32;
        if width_edited && width.is_finite() && next != current {
            self.options.width = Some(next);
            self.options.height = None;
            changed = true;
        }
        if matches!(self.options.format, ExportFormat::Jpeg | ExportFormat::Webp) {
            options_group(ui, text("dialog.export.compression_section"));
        }
        if self.options.format == ExportFormat::Webp {
            changed |= dtool_design::toggle(ui, &mut self.options.lossless, text("dialog.export.lossless")).changed();
        }
        if matches!(self.options.format, ExportFormat::Jpeg) || (self.options.format == ExportFormat::Webp && !self.options.lossless) {
            let mut quality = f32::from(self.options.effective_quality());
            dtool_design::slider(ui, &mut quality, 1.0..=100.0, text("dialog.export.quality.label"));
            let next = quality.round().clamp(1.0, 100.0) as u8;
            if next != self.options.quality {
                self.options.quality = next;
                changed = true;
            }
        }
        if !is_vector(self.options.format) && self.options.format != ExportFormat::Jpeg {
            options_group(ui, text("dialog.export.background_section"));
            changed |= dtool_design::toggle(ui, &mut self.options.transparent,
                text("menu.document.setup.transparent_background")).changed();
        }
        if self.options.format == ExportFormat::Svg {
            let (svg_changed, svg_editing) = self.svg_options(ui);
            changed |= svg_changed;
            editing |= svg_editing;
        }
        if self.options.format == ExportFormat::Pdf {
            options_group(ui, text("dialog.export.pdf.vector_options"));
            changed |= dtool_design::toggle(ui, &mut self.options.pdf_outline_text,
                text("dialog.export.pdf.outline_text")).changed();
            changed |= dtool_design::toggle(ui, &mut self.options.pdf_outline_strokes,
                text("dialog.export.pdf.outline_strokes")).changed();
            options_group(ui, text("print.rasterization"));
            let (raster_changed, raster_editing) = self.pdf_raster_resolution(ui, doc);
            changed |= raster_changed;
            editing |= raster_editing;
            options_group(ui, text("doc_setup.colour"));
            changed |= self.pdf_colour_options(ui, doc);
            options_group(ui, text("dialog.export.pdf.navigation_section"));
            changed |= dtool_design::toggle(ui, &mut self.options.pdf_include_hyperlinks,
                text("dialog.export.pdf.include_hyperlinks")).changed();
            changed |= dtool_design::toggle(ui, &mut self.options.pdf_include_bookmarks,
                text("dialog.export.pdf.include_bookmarks")).changed();
        }
        if matches!(self.options.format, ExportFormat::Pdf | ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Webp) {
            options_group(ui, text(if self.options.format == ExportFormat::Pdf { "print.bleed_marks" } else { "doc_setup.bleed" }));
            changed |= ui.add_enabled_ui(self.options.area.is_page_based(), |ui| {
                dtool_design::toggle(ui, &mut self.options.include_bleed, text("doc_setup.bleed")).changed()
            }).inner;
        }
        if self.options.format == ExportFormat::Pdf {
            changed |= ui.add_enabled_ui(self.options.area.is_page_based(), |ui| {
                dtool_design::toggle(ui, &mut self.options.pdf_crop_marks, text("dialog.export.pdf.crop_marks")).changed()
            }).inner;
            let fold_scope = self.options.area.is_page_based()
                && !matches!(self.options.area, dtool_export::ExportArea::WholeDocument)
                && doc.pages.iter().any(|page| page.fold.is_some());
            changed |= ui.add_enabled_ui(fold_scope, |ui| {
                dtool_design::toggle(ui, &mut self.options.pdf_fold_marks, text("print.fold_marks")).changed()
            }).inner;
        }
        crate::dialog_chrome::options_divider(ui);
        ui.horizontal(|ui| {
            ui.label(text("dialog.export.estimated_size"));
            ui.label(self.estimated_size_label());
        });
        (changed, editing)
    }

    fn svg_options(&mut self, ui: &mut egui::Ui) -> (bool, bool) {
        use dtool_core::effect_raster::{MAX_DPI, MIN_DPI};
        use dtool_design::{NumericEvent, NumericSpec, NumericValue, numeric_field};
        use dtool_export::svg::{MAX_COORDINATE_PRECISION, RasterizeMode, SvgOptions, SvgPreset};

        options_group(ui, text("export.svg.preset"));
        let options = &mut self.options.svg;
        let presets = [
            (SvgPreset::ForExport, "cmd-export", "export.svg.preset_export"),
            (SvgPreset::DigitalSmall, "ctx-zoom-fit", "export.svg.preset_small"),
            (SvgPreset::DigitalHigh, "ctx-zoom-actual", "export.svg.preset_high"),
            (SvgPreset::Flatten, "tool-artistic-text", "export.svg.preset_outline"),
        ];
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for (preset, icon, label) in presets {
                let settings = SvgOptions::preset(preset);
                let selected = settings == *options;
                if self.option_icons.studio_button(ui, icon, text(label), selected).clicked() && !selected {
                    *options = settings;
                    changed = true;
                }
            }
        });
        let selected = presets.iter().find(|(preset, _, _)| SvgOptions::preset(*preset) == *options);
        ui.label(text(selected.map_or("export.svg.preset_custom", |(_, _, label)| label)));
        options_group(ui, text("export.svg.rasterize"));
        let modes = [
            (RasterizeMode::Nothing, "persona-vector", "export.svg.rasterize_nothing"),
            (RasterizeMode::Unsupported, "tool-filter-brush", "export.svg.rasterize_unsupported"),
            (RasterizeMode::Everything, "persona-pixel", "export.svg.rasterize_everything"),
        ];
        let previous = options.rasterize;
        ui.horizontal_wrapped(|ui| {
            for (mode, icon, label) in modes {
                if self.option_icons.studio_button(ui, icon, text(label), options.rasterize == mode).clicked() {
                    options.rasterize = mode;
                }
            }
            if let Some((_, _, label)) = modes.iter().find(|(mode, _, _)| *mode == options.rasterize) {
                ui.label(text(label)).on_hover_text(text("export.svg.rasterize_hint"));
            }
        });
        changed |= options.rasterize != previous;
        let spec = NumericSpec::new(text("export.svg.raster_dpi"))
            .width(52.0).range(f64::from(MIN_DPI)..=f64::from(MAX_DPI)).decimals(0);
        let id = ui.make_persistent_id("export.svg.raster_dpi");
        let (response, event) = ui.add_enabled_ui(options.rasterize != RasterizeMode::Nothing, |ui| {
            numeric_field(ui, id, NumericValue::Value(f64::from(options.raster_dpi)), &spec)
        }).inner;
        if let NumericEvent::Commit(dpi) = event {
            let dpi = dpi as u32;
            changed |= options.raster_dpi != dpi;
            options.raster_dpi = dpi;
        }
        options_group(ui, text("export.svg.source_section"));
        changed |= ui.add_enabled_ui(options.rasterize != RasterizeMode::Everything, |ui| {
            let outline = dtool_design::toggle(ui, &mut options.text_as_curves,
                text("export.svg.text_as_curves")).changed();
            let hex = dtool_design::toggle(ui, &mut options.hex_colours, text("export.svg.hex_colours")).changed();
            outline || hex
        }).inner;
        changed |= dtool_design::toggle(ui, &mut options.set_viewbox, text("export.svg.set_viewbox")).changed();
        changed |= dtool_design::toggle(ui, &mut options.line_breaks, text("export.svg.line_breaks"))
            .on_hover_text(text("export.svg.line_breaks_hint")).changed();
        let precision = NumericSpec::new(text("export.svg.precision"))
            .width(52.0).range(0.0..=f64::from(MAX_COORDINATE_PRECISION)).decimals(0);
        let id = ui.make_persistent_id("export.svg.precision");
        let (precision_response, precision_event) = numeric_field(ui, id, NumericValue::Value(f64::from(options.precision)), &precision);
        if let NumericEvent::Commit(places) = precision_event {
            let places = places as u8;
            changed |= options.precision != places;
            options.precision = places;
        }
        (changed, response.has_focus() || response.lost_focus()
            || precision_response.has_focus() || precision_response.lost_focus())
    }

    fn pdf_raster_resolution(&mut self, ui: &mut egui::Ui, doc: &Document) -> (bool, bool) {
        use dtool_core::effect_raster::{MAX_DPI, MIN_DPI};
        use dtool_design::{NumericEvent, NumericSpec, NumericValue, numeric_field};

        ui.horizontal_wrapped(|ui| {
            let previous = self.options.pdf_raster_dpi;
            let value = previous.unwrap_or_else(|| dtool_core::effect_raster::pdf_dpi(doc));
            let spec = NumericSpec::new(text("dialog.export.pdf.raster_dpi"))
                .width(52.0).range(f64::from(MIN_DPI)..=f64::from(MAX_DPI)).decimals(0);
            let id = ui.make_persistent_id("export.pdf.raster_dpi");
            let (response, event) = numeric_field(ui, id, NumericValue::Value(f64::from(value)), &spec);
            if let NumericEvent::Commit(dpi) = event {
                self.options.pdf_raster_dpi = Some(dpi as f32);
            }
            if ui.add_enabled(previous.is_some(), egui::Button::new(text("dialog.export.pdf.reset_raster_dpi"))).clicked() {
                self.options.pdf_raster_dpi = None;
            }
            (previous != self.options.pdf_raster_dpi, response.has_focus() || response.lost_focus())
        }).inner
    }

    fn pdf_colour_options(&mut self, ui: &mut egui::Ui, doc: &Document) -> bool {
        use dtool_core::colour::{ColourModel, ProfileStore};
        let previous = self.options.pdf_colour_space.unwrap_or_default();
        let mut space = previous;
        ui.horizontal_wrapped(|ui| {
            ui.label(text("dialog.export.pdf.vector_colour_space"));
            for choice in [PdfColourSpace::Rgb, PdfColourSpace::Cmyk] {
                let response = dtool_design::with_icon_selection(ui, |ui| {
                    ui.add(egui::Button::selectable(space == choice, choice.label()))
                });
                if response.clicked() {
                    space = choice;
                }
            }
        });
        let mut changed = space != previous;
        if changed {
            self.options.pdf_colour_space = Some(space);
            self.options.icc = None;
        }
        if dtool_design::toggle(ui, &mut self.options.embed_icc, text("dialog.export.pdf.embed_icc")).changed() {
            if !self.options.embed_icc {
                self.options.icc = None;
            }
            changed = true;
        }
        if !self.options.embed_icc {
            return changed;
        }
        ui.label(text("dialog.export.pdf.icc_profile"));
        if space == PdfColourSpace::Rgb {
            ui.label(ProfileStore::SRGB);
            return changed;
        }
        let profiles: Vec<String> = dtool_core::colour::services().read()
            .map(|services| services.profiles.list(ColourModel::Cmyk).into_iter().map(str::to_owned).collect())
            .unwrap_or_default();
        let current = self.options.icc.clone();
        let default = dtool_core::paint::DisplayColourCtx::for_document(doc).cmyk;
        egui::ComboBox::from_id_salt("export.pdf.icc")
            .width(ui.available_width()).truncate()
            .selected_text(current.as_deref().unwrap_or(&default))
            .show_ui(ui, |ui| {
                dtool_design::with_icon_selection(ui, |ui| {
                    ui.selectable_value(&mut self.options.icc, None, format!("{} ({default})", text("dialog.export.pdf.default_profile")));
                    for profile in profiles {
                        ui.selectable_value(&mut self.options.icc, Some(profile.clone()), profile);
                    }
                });
            }).response.on_hover_text(current.as_deref().unwrap_or(&default));
        changed || self.options.icc != current
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) -> (bool, bool, bool, bool) {
        let mut export = false;
        let mut cancel = false;
        let mut editing = false;
        let mut path_changed = false;
        #[cfg(windows)]
        ui.horizontal_wrapped(|ui| {
            dtool_design::toggle(ui, &mut self.ask_for_location, text("dialog.export.ask_for_location"));
            dtool_design::toggle(ui, &mut self.show_in_explorer, text("dialog.export.show_in_explorer"));
        });
        if !cfg!(windows) || !self.ask_for_location {
            let label = text("dialog.export.choose_location.label");
            let input = if ui.ctx().content_rect().height() < 320.0 {
                dtool_design::studio_row(ui, label, |ui, caption| {
                    ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(ui.available_width()))
                        .labelled_by(caption)
                })
            } else {
                let caption = ui.label(label);
                ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(ui.available_width()))
                    .labelled_by(caption.id)
            };
            editing = input.has_focus() || input.lost_focus();
            path_changed = input.changed();
        }
        ui.add_space(dtool_design::space("md"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled_ui(!self.page_range_invalid, |ui| {
                dtool_design::primary_button(ui, text("menu.file.export.label"),
                    egui::vec2(88.0, dtool_design::size("button.height")))
            }).inner.clicked() {
                export = true;
            }
            if dtool_design::button(ui, text("common.action.cancel")).clicked() {
                cancel = true;
            }
        });
        (export, cancel, editing, path_changed)
    }

    fn formats_panel(&mut self, ui: &mut egui::Ui, doc: &Document, width: f32, height: f32) -> (bool, bool) {
        dtool_design::dialog_section(ui, |ui| {
            ui.set_width(width);
            options_scroll(ui, "export.formats", height, |ui| self.left_panel(ui, doc))
        }).inner
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui, doc: &Document, width: f32, height: f32) -> (bool, bool) {
        dtool_design::dialog_section(ui, |ui| {
            ui.set_width(width);
            options_scroll(ui, "export.settings", height, |ui| self.right_settings(ui, doc))
        }).inner
    }

    fn body(&mut self, ui: &mut egui::Ui, doc: &Document, width: f32, height: f32) -> (bool, bool) {
        let inset = 2.0 * (dtool_design::space("sm") + dtool_design::stroke_width("thin"));
        if width < 780.0 {
            let tabs = ui.horizontal_wrapped(|ui| {
                for (pane, label) in [
                    (ExportPane::Formats, "dialog.export.formats"),
                    (ExportPane::Settings, "dialog.export.settings"),
                    (ExportPane::Preview, "dialog.export.preview"),
                ] {
                    if dtool_design::tab(ui, self.compact_pane == pane, text(label)).clicked() {
                        self.compact_pane = pane;
                    }
                }
            });
            let height = (height - tabs.response.rect.height() - ui.spacing().item_spacing.y).max(56.0);
            return match self.compact_pane {
                ExportPane::Formats => {
                    let result = self.formats_panel(ui, doc, width - inset, height - inset);
                    if result.0 {
                        self.compact_pane = ExportPane::Settings;
                    }
                    result
                }
                ExportPane::Settings => self.settings_panel(ui, doc, width - inset, height - inset),
                ExportPane::Preview => {
                    options_scroll(ui, "export.preview", height, |ui| {
                        self.centre_preview(ui, doc, width, height, false);
                    });
                    (false, false)
                }
            };
        }

        let left = (width * 0.17).clamp(104.0, 164.0);
        let right = (width * 0.29).clamp(180.0, 288.0);
        let gap = ui.spacing().item_spacing.x;
        let centre = width - left - right - 2.0 * gap;
        let (body, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let mut column = |id, offset, width| {
            let rect = egui::Rect::from_min_size(body.min + egui::vec2(offset, 0.0), egui::vec2(width, height));
            let mut child = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(rect));
            child.set_clip_rect(child.clip_rect().intersect(rect));
            child
        };
        let formats = self.formats_panel(&mut column("export.formats_panel", 0.0, left), doc, left - inset, height - inset);
        self.centre_preview(&mut column("export.preview_panel", left + gap, centre), doc, centre, height, true);
        let settings = self.settings_panel(&mut column("export.settings_panel", left + centre + 2.0 * gap, right),
            doc, right - inset, height - inset);
        (formats.0 || settings.0, formats.1 || settings.1)
    }

    pub fn show(&mut self, ui: &mut egui::Ui, doc: &Document) -> Option<ExportOutcome> {
        if !self.open {
            return None;
        }
        if self.pdf_save.is_some() {
            return self.show_pdf_save(ui);
        }
        self.ensure_current(doc);
        let mut window_open = true;
        let mut export = false;
        let mut cancel = false;
        let mut changed = false;
        let mut decision = None;
        let viewport = ui.ctx().content_rect();
        let bounds = (viewport.size() - egui::vec2(36.0, 72.0)).max(egui::vec2(280.0, 160.0));
        let initial = bounds.min(egui::vec2(1040.0, 660.0));
        egui::Window::new(text("dialog.export.title"))
            .open(&mut window_open)
            .collapsible(false)
            .resizable(true)
            .default_size(initial)
            .default_pos(viewport.center() - initial * 0.5)
            .min_size(egui::vec2(560.0, 330.0).min(bounds))
            .max_size(bounds)
            .show(ui.ctx(), |ui| {
                let width = ui.available_width().min(bounds.x);
                let footer = if cfg!(windows) && self.ask_for_location { 104.0 } else { 126.0 };
                let height = (ui.available_height().min(bounds.y) - footer).max(78.0);
                let popup_active = egui::Popup::is_any_open(ui.ctx());
                let (settings_changed, editing) = self.body(ui, doc, width, height);
                changed |= settings_changed;
                ui.separator();
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, text(error.message_key()));
                }
                let (export_clicked, cancel_clicked, path_editing, path_changed) = self.bottom_bar(ui);
                changed |= path_changed && self.options.format == ExportFormat::Eps;
                export = export_clicked;
                cancel = cancel_clicked;
                decision = self.overwrite_prompt(ui);
                self.receipt_summary(ui);
                if !popup_active && !egui::Popup::is_any_open(ui.ctx()) {
                    match dtool_design::dialog_keys(ui) {
                        Some(dtool_design::DialogKey::Cancel) => cancel = true,
                        Some(dtool_design::DialogKey::Confirm) if !editing && !path_editing
                            && self.pending_overwrite().is_none() && ui.input(|i| !i.modifiers.any()) => export = true,
                        _ => {}
                    }
                }
            });
        if changed {
            self.refresh(doc);
            self.update_pdf_preview(doc);
        }
        if self.job.is_running() {
            if cancel || !window_open {
                self.cancel_job();
            }
            if self.pending_overwrite.is_none() || decision.is_some() {
                let step = self.step_job(doc, decision);
                if matches!(step, JobStep::Done | JobStep::Cancelled) {
                    if let Some(receipt) = self.job_receipt.as_ref().and_then(|r| r.written().first().map(|w| w.path.clone())) {
                        let written = self.job_receipt.as_ref().map(|r| r.written()).unwrap_or_default();
                        self.last_receipt = Some(ExportReceipt {
                            path: receipt,
                            bytes: written.iter().map(|item| item.bytes).sum(),
                            warnings: written.iter().flat_map(|item| item.warnings.iter().cloned()).collect(),
                        });
                    }
                }
            }
            ui.ctx().request_repaint();
            return None;
        }
        if let Some(path) = self.single_overwrite.clone() {
            match decision {
                Some(OverwriteDecision::Overwrite) => {
                    self.single_overwrite = None;
                    return self.submit_export(doc, path, true);
                }
                Some(_) => self.single_overwrite = None,
                None => {}
            }
        }
        if cancel || !window_open {
            self.close();
            return Some(ExportOutcome::Cancelled);
        }
        if export && !self.page_range_invalid {
            if let Err(error) = self.expand_job_areas(doc) {
                self.error = Some(error);
                return None;
            }
            if self.start_job() {
                ui.ctx().request_repaint();
                return None;
            }
            let path = self.output_path();
            #[cfg(windows)]
            let path = if self.ask_for_location {
                let extension = raster::format_extension(self.options.format);
                let filter = format!("{} (*.{extension})", format_label(self.options.format));
                match super::pick_file_path(text("dialog.export.title"), &filter, extension, Some(&path)) {
                    Ok(Some(chosen)) => chosen.to_string_lossy().into_owned(),
                    Ok(None) => return None,
                    Err(error) => {
                        self.error = Some(ExportError::Io(error.to_string()));
                        return None;
                    }
                }
            } else {
                path
            };
            return self.submit_export(doc, path, self.overwrite);
        }
        None
    }

    fn submit_export(&mut self, doc: &Document, path: String, overwrite: bool) -> Option<ExportOutcome> {
        if let Some(ctx) = self.pdf_preview_repaint.clone().filter(|_| self.options.format == ExportFormat::Pdf) {
            let doc = doc.clone();
            let options = self.options.clone();
            let context = self.context.clone();
            let target = std::path::PathBuf::from(&path);
            let (sender, result) = mpsc::channel();
            self.last_paths.clear();
            self.last_receipt = None;
            self.error = None;
            let repaint = ctx.clone();
            match std::thread::Builder::new().name("dtool-pdf-save".into()).spawn(move || {
                let saved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    save_pdf(&doc, &options, &context, &target, overwrite)
                })).unwrap_or_else(|_| Err(ExportError::Io("PDF save worker".into())));
                let _ = sender.send(saved);
                repaint.request_repaint();
            }) {
                Ok(_) => {
                    self.pdf_save = Some(PdfSave { path, overwrite, result });
                    ctx.request_repaint();
                }
                Err(error) => self.error = Some(ExportError::Io(error.to_string())),
            }
            return None;
        }
        let keep = self.overwrite;
        self.overwrite = overwrite;
        let result = self.export_to(doc, &path);
        self.overwrite = keep;
        self.finish_export(result, path, overwrite)
    }

    fn show_pdf_save(&mut self, ui: &mut egui::Ui) -> Option<ExportOutcome> {
        let save = self.pdf_save.as_ref()?;
        let result = match save.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Disconnected) => Err(ExportError::Io("PDF save worker".into())),
            Err(mpsc::TryRecvError::Empty) => {
                egui::Window::new(text("dialog.export.title"))
                    .id(egui::Id::unique("dtool.export.saving"))
                    .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                    .collapsible(false)
                    .resizable(false)
                    .show(ui.ctx(), |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(text("dialog.export.saving_pdf"));
                        });
                    });
                return None;
            }
        };
        let save = self.pdf_save.take()?;
        if let Ok(receipt) = &result {
            self.last_paths.push(receipt.path.clone());
            self.last_receipt = Some(receipt.clone());
        }
        ui.ctx().request_repaint();
        self.finish_export(result, save.path, save.overwrite)
    }

    fn finish_export(&mut self, result: Result<ExportReceipt, ExportError>, path: String, overwrite: bool) -> Option<ExportOutcome> {
        match result {
            Ok(receipt) => {
                self.reveal_output(&receipt);
                self.close();
                Some(ExportOutcome::Exported(receipt))
            }
            Err(ExportError::Collision(_)) if !overwrite => {
                self.single_overwrite = Some(path);
                None
            }
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }

    fn reveal_output(&self, receipt: &ExportReceipt) {
        #[cfg(windows)]
        if self.show_in_explorer {
            if let Ok(path) = std::path::absolute(&receipt.path) {
                let mut argument = std::ffi::OsString::from("/select,");
                argument.push(path);
                let _ = std::process::Command::new("explorer.exe").arg(argument).spawn();
            }
        }
        #[cfg(not(windows))]
        let _ = receipt;
    }
}

impl ExportDialog {
    pub fn owner(&self) -> Option<ContextId> {
        self.owner
    }

    pub fn bind_owner(&mut self, id: Option<ContextId>) {
        if self.open && self.owner.is_none() {
            self.owner = id;
        }
    }

    pub fn open_for(&mut self, id: ContextId, doc: &Document, format: Option<ExportFormat>) -> Value {
        if self.open && self.owner != Some(id) {
            self.close();
        }
        self.owner = Some(id);
        self.open_with(doc, format)
    }

    pub fn owner_gone(&self, workspace: &Workspace) -> bool {
        self.open && self.owner.is_some_and(|id| workspace.get(id).is_none())
    }

    pub fn drop_if_gone(&mut self, workspace: &Workspace) -> bool {
        if self.pdf_save.is_some() || !self.owner_gone(workspace) {
            return false;
        }
        self.close();
        true
    }

    pub fn owner_document<'a>(&mut self, workspace: &'a Workspace) -> Option<&'a Document> {
        if !self.open {
            return None;
        }
        if self.owner.is_none() {
            self.owner = workspace.active();
        }
        let id = self.owner?;
        let context = workspace.get(id)?;
        self.set_session_context(&context.session);
        Some(&context.session.document)
    }

    pub fn show_owned(&mut self, ui: &mut egui::Ui, workspace: &Workspace) -> Option<ExportOutcome> {
        if self.pdf_save.is_some() {
            return self.show_pdf_save(ui);
        }
        match self.owner_document(workspace) {
            Some(doc) => self.show(ui, doc),
            None => {
                if self.open {
                    self.close();
                }
                None
            }
        }
    }

    pub fn run_for(&mut self, id: ContextId, doc: &Document, name: &str, params: &Value) -> Option<Result<Value, CommandError>> {
        let foreign = self.open && self.owner.is_some_and(|owner| owner != id);
        if foreign && matches!(name, UI_EXPORT_SET | UI_EXPORT_QUERY | UI_EXPORT_PREVIEW) {
            return Some(Err(CommandError::context_not_found()));
        }
        if self.pdf_save.is_some() && blocks_during_pdf_save(name) {
            return Some(Err(CommandError::new("EDITOR_BUSY")));
        }
        if foreign && name == UI_EXPORT_OPEN {
            self.close();
        }
        let result = self.run(doc, name, params)?;
        if result.is_ok() && self.open && (name == UI_EXPORT_OPEN || self.owner.is_none()) {
            self.owner = Some(id);
        }
        Some(result.map_err(|error| error.to_command_error()))
    }
}

fn blocks_during_pdf_save(name: &str) -> bool {
    matches!(name, UI_EXPORT_OPEN | UI_EXPORT_SET | UI_EXPORT_PREVIEW | UI_EXPORT_CLOSE | EXPORT_JOB_RUN | EXPORT_JOB_DELETE)
}

fn save_pdf(doc: &Document, options: &ExportOptions, context: &dtool_export::ExportContext, target: &std::path::Path, overwrite: bool) -> Result<ExportReceipt, ExportError> {
    if !overwrite && target.exists() {
        return Err(ExportError::Collision(target.to_path_buf()));
    }
    if options.area.is_multi() {
        dtool_export::resolve_areas(doc, &options.area, context, options.pages.as_deref()).map_err(|_| ExportError::InvalidArea("pages".into()))?;
        return raster::encode_output(doc, options, target)?.write(target, overwrite);
    }
    let (doc, options, _) = resolved_item(doc, options, context, 0)?;
    raster::encode_output(&doc, &options, target)?.write(target, overwrite)
}

fn resolved_item<'a>(doc: &'a Document, options: &ExportOptions, context: &dtool_export::ExportContext, index: u32) -> Result<(std::borrow::Cow<'a, Document>, ExportOptions, Option<String>), ExportError> {
    let areas = raster::resolve_output_areas(doc, options, context)?;
    let area = areas.get(index as usize).ok_or_else(|| ExportError::InvalidArea("area".into()))?;
    let doc = area.only.as_ref().map(|ids| std::borrow::Cow::Owned(dtool_export::only_objects(doc, ids))).unwrap_or(std::borrow::Cow::Borrowed(doc));
    let mut options = options.clone();
    options.area = area.area.clone();
    Ok((doc, options, area.suffix.clone()))
}

fn encode_context_item(doc: &Document, item: &raster::ExportJobItem, context: &dtool_export::ExportContext) -> Result<EncodedExport, ExportError> {
    if item.options.format == ExportFormat::Pdf && item.options.area.is_multi() {
        return raster::encode_output(doc, &item.options, &item.path);
    }
    let (doc, options, _) = resolved_item(doc, &item.options, context, 0)?;
    raster::encode_output(&doc, &options, &item.path)
}
