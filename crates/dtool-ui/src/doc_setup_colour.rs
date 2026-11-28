use dtool_core::colour::{ColourFormat, ColourPrefs, DocColour};
use dtool_core::paint::Paint;
use dtool_core::Document;
use dtool_strings::text;

pub fn colour_format_list() -> &'static [ColourFormat] {
    ColourFormat::colour_format_list()
}

pub fn default_profile_for_format(format: ColourFormat, prefs: &ColourPrefs) -> String {
    format.default_profile(prefs)
}

fn write_paint(paint: &mut Paint, format: ColourFormat, prefs: &ColourPrefs) {
    match paint {
        Paint::Solid { colour } => {
            *colour = colour.write_for_document(format, prefs);
        }
        Paint::Gradient { stops, .. } => {
            for stop in stops {
                stop.colour = stop.colour.write_for_document(format, prefs);
            }
        }
        Paint::Hatch(hatch) => {
            hatch.colour = hatch.colour.write_for_document(format, prefs);
        }
        _ => {}
    }
}

fn write_document_paints(document: &mut Document, format: ColourFormat, prefs: &ColourPrefs) {
    for object in &mut document.objects {
        write_paint(&mut object.fill, format, prefs);
        write_paint(&mut object.stroke.paint, format, prefs);
    }
}

#[derive(Default)]
pub struct DocSetupColourDialog {
    pub open: bool,
    pub selected_format: usize,
}

impl DocSetupColourDialog {
    pub fn open(&mut self, document: &Document) {
        self.selected_format = ColourFormat::LIST
            .iter()
            .position(|format| *format == document.colour.format)
            .unwrap_or(0);
        self.open = true;
    }

    pub fn show(&mut self, ctx: &egui::Context, document: &mut Document, prefs: &ColourPrefs) {
        if !self.open {
            return;
        }
        let labels: Vec<String> = ColourFormat::LIST.iter().map(ToString::to_string).collect();
        crate::dialog_chrome::show_floating(ctx, "document.setup.colour", text("doc.setup.colour"), |ui| {
            ui.horizontal(|ui| {
                ui.label(text("doc.setup.colour_format"));
                let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                if !label_refs.is_empty()
                    && dtool_design::dropdown(ui, "doc.setup.colour_format.list", &mut self.selected_format, &label_refs).changed()
                {
                    if let Some(format) = ColourFormat::LIST.get(self.selected_format).copied() {
                        document.colour = DocColour::for_format(format, prefs);
                        write_document_paints(document, format, prefs);
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(text("doc.setup.colour_profile"));
                ui.label(&document.colour.profile);
            });
            if dtool_design::button(ui, text("button.close")).clicked() {
                self.open = false;
            }
        });
    }
}
