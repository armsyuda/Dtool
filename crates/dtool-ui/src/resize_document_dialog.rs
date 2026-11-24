use dtool_core::canvas_size::{
    apply_resize, document_pixel_size, px_to_unit, unit_to_px, ResampleMethod,
};
use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_strings::text;
use dtool_design::button;

const KEY_TITLE: &str = "resize.document.title";
const KEY_RESAMPLE: &str = "resize.resample";
const KEY_SIZE: &str = "menu.text.size";
const KEY_WIDTH: &str = "dialog.new_document.width";
const KEY_HEIGHT: &str = "dialog.new_document.height";
const KEY_LOCK: &str = "panel.transform.aspect_lock";
const KEY_UNITS: &str = "dialog.new_document.units";
const KEY_DPI: &str = "dialog.new_document.dpi";
const KEY_DESCRIPTION: &str = "meta.description";
const KEY_RESIZE: &str = "menu.document.setup.resize";
const KEY_CANCEL: &str = "dialog.cancel";
const KEY_PX: &str = "units.px";
const KEY_MM: &str = "units.mm";
const KEY_CM: &str = "units.cm";
const KEY_IN: &str = "units.in";
const KEY_PT: &str = "units.pt";
const KEY_NEAREST: &str = "pref.performance.view_quality.nearest";
const KEY_BILINEAR: &str = "pref.performance.view_quality.bilinear";
const KEY_BICUBIC: &str = "pref.performance.view_quality.bicubic";
const KEY_LANCZOS_SEP: &str = "pref.performance.view_quality.lanczos_sep";
const KEY_LANCZOS_NONSEP: &str = "pref.performance.view_quality.lanczos_nonsep";

pub struct ResizeDocumentDialog {
    pub open: bool,
    width: f32,
    height: f32,
    units: Units,
    dpi: f32,
    lock: bool,
    resample: bool,
    method: ResampleMethod,
    orig_px: (u32, u32),
    orig_dpi: f32,
    aspect: f32,
}

impl Default for ResizeDocumentDialog {
    fn default() -> Self {
        Self {
            open: false,
            width: 1.0,
            height: 1.0,
            units: Units::Px,
            dpi: 72.0,
            lock: true,
            resample: true,
            method: ResampleMethod::Bilinear,
            orig_px: (1, 1),
            orig_dpi: 72.0,
            aspect: 1.0,
        }
    }
}

impl ResizeDocumentDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_document(doc: &Document) -> Self {
        let mut dialog = Self::default();
        dialog.load(doc);
        dialog
    }

    pub fn load(&mut self, doc: &Document) {
        let (w, h) = document_pixel_size(doc).unwrap_or((1, 1));
        self.orig_px = (w, h);
        self.orig_dpi = if doc.dpi.is_finite() && doc.dpi > 0.0 {
            doc.dpi
        } else {
            72.0
        };
        self.units = doc.units;
        self.dpi = self.orig_dpi;
        self.width = px_to_unit(w as f32, self.units, self.dpi);
        self.height = px_to_unit(h as f32, self.units, self.dpi);
        self.aspect = if h == 0 { 1.0 } else { w as f32 / h as f32 };
        if !self.aspect.is_finite() || self.aspect <= 0.0 {
            self.aspect = 1.0;
        }
        self.lock = true;
        self.resample = true;
        self.method = ResampleMethod::Bilinear;
    }

    pub fn open_for(&mut self, doc: &Document) {
        self.load(doc);
        self.open = true;
    }

    fn set_units(&mut self, units: Units) {
        if self.units == units {
            return;
        }
        let wpx = unit_to_px(self.width, self.units, self.dpi);
        let hpx = unit_to_px(self.height, self.units, self.dpi);
        self.units = units;
        self.width = px_to_unit(wpx, units, self.dpi);
        self.height = px_to_unit(hpx, units, self.dpi);
    }

    fn set_width(&mut self, width: f32) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        self.width = width;
        if self.lock {
            self.height = width / self.aspect;
            if !self.height.is_finite() || self.height <= 0.0 {
                self.height = width;
            }
        }
    }

    fn set_height(&mut self, height: f32) {
        if !height.is_finite() || height <= 0.0 {
            return;
        }
        self.height = height;
        if self.lock {
            self.width = height * self.aspect;
            if !self.width.is_finite() || self.width <= 0.0 {
                self.width = height;
            }
        }
    }

    fn target_size(&self) -> Option<(u32, u32, f32)> {
        if !self.dpi.is_finite() || self.dpi <= 0.0 {
            return None;
        }
        if self.resample {
            let w = unit_to_px(self.width, self.units, self.dpi);
            let h = unit_to_px(self.height, self.units, self.dpi);
            if !w.is_finite() || !h.is_finite() || w < 0.5 || h < 0.5 {
                return None;
            }
            Some((w.round().max(1.0) as u32, h.round().max(1.0) as u32, self.dpi))
        } else {
            let w_in = unit_to_px(self.width, self.units, self.dpi) / self.dpi;
            if !w_in.is_finite() || w_in <= 0.0 {
                return None;
            }
            let dpi = self.orig_px.0 as f32 / w_in;
            if !dpi.is_finite() || dpi <= 0.0 {
                return None;
            }
            Some((self.orig_px.0, self.orig_px.1, dpi))
        }
    }

    fn description(&self) -> String {
        match self.target_size() {
            Some((w, h, dpi)) => {
                format!(
                    "{} x {} {}, {} {}",
                    w,
                    h,
                    text(KEY_PX),
                    format_num(dpi),
                    text(KEY_DPI)
                )
            }
            None => String::new(),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &mut Session) -> bool {
        if !self.open {
            return false;
        }
        let mut applied = false;
        let mut open = self.open;
        let mut close = false;
        let title = text(KEY_TITLE).to_string();
        egui::Window::new(title)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(text(KEY_SIZE));
                    ui.add_enabled_ui(self.resample, |ui| {
                        ui.label(text(KEY_WIDTH));
                        let mut width = f64::from(self.width);
                        if dtool_design::number_drag(ui, egui::Id::NULL.with("dtool.resize_document.width"), &mut width, 0.001..=1_000_000.0, 0.1).changed() {
                            self.set_width(width as f32);
                        }
                        let mut locked = self.lock;
                        if dtool_design::toggle(ui, &mut locked, text(KEY_LOCK)).changed() {
                            self.lock = locked;
                            if self.lock {
                                self.set_width(self.width);
                            }
                        }
                        ui.label(text(KEY_HEIGHT));
                        let mut height = f64::from(self.height);
                        if dtool_design::number_drag(ui, egui::Id::NULL.with("dtool.resize_document.height"), &mut height, 0.001..=1_000_000.0, 0.1).changed() {
                            self.set_height(height as f32);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label(text(KEY_UNITS));
                    let mut units = self.units;
                    let options: Vec<(Units, &str)> = [Units::Px, Units::Mm, Units::Cm, Units::In, Units::Pt].into_iter().map(|unit| (unit, unit_label(unit))).collect();
                    if dtool_design::dropdown_value(ui, "resize-document-units", &mut units, &options, &dtool_design::ControlState::default()).changed() {
                        self.set_units(units);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(text(KEY_DPI));
                    let mut dpi_value = f64::from(self.dpi);
                    if dtool_design::number_drag(ui, egui::Id::NULL.with("dtool.resize_document.dpi"), &mut dpi_value, 1.0..=10_000.0, 1.0).changed() {
                        let dpi = dpi_value as f32;
                        if dpi.is_finite() && dpi > 0.0 {
                            if !self.resample && self.units != Units::Px {
                                let wpx = unit_to_px(self.width, self.units, self.dpi);
                                let hpx = unit_to_px(self.height, self.units, self.dpi);
                                self.dpi = dpi;
                                self.width = px_to_unit(wpx, self.units, self.dpi);
                                self.height = px_to_unit(hpx, self.units, self.dpi);
                            } else {
                                self.dpi = dpi;
                            }
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(self.resample, |ui| {
                        ui.label(text(KEY_RESAMPLE));
                        let options: Vec<(ResampleMethod, &str)> = [ResampleMethod::Nearest, ResampleMethod::Bilinear, ResampleMethod::Bicubic, ResampleMethod::Lanczos3Separable, ResampleMethod::Lanczos3NonSeparable].into_iter().map(|method| (method, method_label(method))).collect();
                        dtool_design::dropdown_value(ui, "resize-document-method", &mut self.method, &options, &dtool_design::ControlState::default());
                    });
                });
                ui.horizontal(|ui| {
                    let mut resample = self.resample;
                    if dtool_design::toggle(ui, &mut resample, text(KEY_RESAMPLE)).changed() {
                        self.resample = resample;
                        if !self.resample {
                            self.width = px_to_unit(self.orig_px.0 as f32, self.units, self.dpi);
                            self.height = px_to_unit(self.orig_px.1 as f32, self.units, self.dpi);
                            self.lock = false;
                        } else {
                            self.lock = true;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(text(KEY_DESCRIPTION));
                    ui.label(self.description());
                });
                ui.horizontal(|ui| {
                    if button(ui, text(KEY_RESIZE)).clicked() {
                        if let Some((w, h, dpi)) = self.target_size() {
                            if apply_resize(session, w, h, dpi, self.method).is_ok() {
                                applied = true;
                                close = true;
                            }
                        }
                    }
                    if button(ui, text(KEY_CANCEL)).clicked() {
                        close = true;
                    }
                });
            });
        self.open = open && !close;
        applied
    }
}

fn format_num(v: f32) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as i32)
    } else {
        format!("{:.2}", v)
    }
}

fn unit_label(units: Units) -> &'static str {
    match units {
        Units::Px => text(KEY_PX),
        Units::Mm => text(KEY_MM),
        Units::Cm => text(KEY_CM),
        Units::In => text(KEY_IN),
        Units::Pt => text(KEY_PT),
    }
}

fn method_label(method: ResampleMethod) -> &'static str {
    match method {
        ResampleMethod::Nearest => text(KEY_NEAREST),
        ResampleMethod::Bilinear => text(KEY_BILINEAR),
        ResampleMethod::Bicubic => text(KEY_BICUBIC),
        ResampleMethod::Lanczos3Separable => text(KEY_LANCZOS_SEP),
        ResampleMethod::Lanczos3NonSeparable => text(KEY_LANCZOS_NONSEP),
    }
}
