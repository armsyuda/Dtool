use crate::dialog_chrome::{show_modal, ClosePolicy, DialogResult};
use dtool_core::commands::tone_blend::{CREATE, UPDATE, target_group};
use dtool_core::tone_blend::{Content, ToneBlend};
use dtool_core::{Command, CommandError, ContextId, ObjectId, Session};
use dtool_strings::text;
use serde_json::json;

pub const MENU_ID: &str = "pixel.live_tone_blend_group";

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Preview {
    pub id: ObjectId,
    pub tone: ToneBlend,
}

pub(crate) struct Dialog {
    pub owner: ContextId,
    preview: Preview,
    generation: u64,
    error: Option<String>,
}

impl Dialog {
    pub(crate) fn open(owner: ContextId, session: &mut Session) -> Result<Self, CommandError> {
        let id = match target_group(session) {
            Some(id) => id,
            None => {
                let group = session.run(&Command::with_params(CREATE, json!({})))?;
                ObjectId(group["group"].as_u64().ok_or_else(CommandError::not_found)?)
            }
        };
        let tone = session.document.get_object(id).and_then(|group| group.tone_blend).ok_or_else(CommandError::not_found)?;
        Ok(Self { owner, preview: Preview { id, tone }, generation: session.generation, error: None })
    }

    pub(crate) fn preview(&self, owner: ContextId, session: &Session) -> Option<Preview> {
        (self.owner == owner && session.generation == self.generation).then_some(self.preview)
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context, session: &mut Session) -> bool {
        let (dismissal, clicked) = show_modal(ctx, "dtool.dialog.tone_blend", text("menu.pixel.live_tone_blend_group"),
            ClosePolicy::CancelOnly, false, |ui| {
            ui.set_width(306.0);
            let tone = &mut self.preview.tone;
            percent(ui, "tone.blend.strength", &mut tone.strength, 0.0);
            percent(ui, "tone.blend.colour", &mut tone.colour, 0.0);
            percent(ui, "tone.blend.contrast", &mut tone.contrast, -100.0);
            percent(ui, "tone.blend.low_pass", &mut tone.low_pass, 0.0);
            ui.label(text("tone.blend.content"));
            egui::ComboBox::from_id_salt("content").selected_text(text(content_key(tone.content))).show_ui(ui, |ui| {
                for content in [Content::Images, Content::Shapes, Content::Text] {
                    ui.selectable_value(&mut tone.content, content, text(content_key(content)));
                }
            });
            ui.separator();
            ui.label(text("tone.blend.bands"));
            for (value, key) in tone.bands.iter_mut().zip(["tone.blend.shadows", "tone.blend.midtones", "tone.blend.highlights"]) {
                percent(ui, key, value, 0.0);
            }
            if let Some(key) = &self.error { ui.colored_label(ui.visuals().error_fg_color, text(key)); }
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if dtool_design::primary_button(ui, text("dialog.ok"), egui::vec2(72.0, 22.0)).clicked() {
                    DialogResult::Confirm
                } else if dtool_design::button(ui, text("dialog.cancel")).clicked() {
                    DialogResult::Cancel
                } else {
                    DialogResult::Open
                }
            }).inner
        });
        if dismissal == DialogResult::Cancel || clicked == DialogResult::Cancel { return false; }
        if clicked == DialogResult::Confirm {
            if session.generation != self.generation {
                self.error = Some("error.stale".to_owned());
                return true;
            }
            let update = Command::with_params(UPDATE, json!({"id": self.preview.id.0, "settings": self.preview.tone}));
            match session.run(&update) {
                Ok(_) => return false,
                Err(error) => self.error = Some(error.message_key),
            }
        }
        true
    }
}

fn content_key(content: Content) -> &'static str {
    match content {
        Content::Images => "tone.blend.images",
        Content::Shapes => "tone.blend.shapes",
        Content::Text => "tone.blend.text",
    }
}

fn percent(ui: &mut egui::Ui, key: &str, value: &mut f32, min: f32) {
    let mut percent = *value * 100.0;
    if dtool_design::slider(ui, &mut percent, min..=100.0, text(key)).changed() {
        *value = percent / 100.0;
    }
}
