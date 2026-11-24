use dtool_core::commands::picture_frame::{self, Anchor9, ImageFit, PictureFrame};
use dtool_core::object::ObjectId;
use dtool_core::{Command, CommandResult, Session};
use dtool_strings::text;
use serde_json::json;

pub const FITS: [(ImageFit, &str, &str); 4] = [
    (ImageFit::Native, "native", "frame.fit.native"),
    (ImageFit::MinFit, "min", "frame.fit.min"),
    (ImageFit::MaxFit, "max", "frame.fit.max"),
    (ImageFit::Stretch, "stretch", "frame.fit.stretch"),
];

const ANCHORS: [(Anchor9, &str, &str); 9] = [
    (Anchor9::TopLeft, "top_left", "frame.anchor.top_left"),
    (Anchor9::Top, "top", "frame.anchor.top"),
    (Anchor9::TopRight, "top_right", "frame.anchor.top_right"),
    (Anchor9::Left, "left", "frame.anchor.left"),
    (Anchor9::Center, "center", "frame.anchor.center"),
    (Anchor9::Right, "right", "frame.anchor.right"),
    (Anchor9::BottomLeft, "bottom_left", "frame.anchor.bottom_left"),
    (Anchor9::Bottom, "bottom", "frame.anchor.bottom"),
    (Anchor9::BottomRight, "bottom_right", "frame.anchor.bottom_right"),
];

#[derive(Debug, Clone, PartialEq)]
pub enum FrameBarEdit {
    Fit(ImageFit),
    Anchor(Anchor9),
    Scale(f32),
    Rotation(f32),
    Reset,
    ReplaceImage(String),
    PickImage,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EditKind {
    Frame,
    Replace,
}

#[derive(Debug, Clone, Default)]
pub struct PictureFrameBar {
    pub frame: Option<PictureFrame>,
    pub replace_path: String,
}

impl PictureFrameBar {
    pub fn load(session: &Session) -> Self {
        let frame = match session.selection.ids.as_slice() {
            [id] if session.document.get_object(*id).is_some() => picture_frame::get_frame(&session.document, *id),
            _ => None,
        };
        Self { frame, replace_path: String::new() }
    }

    pub fn reload(&mut self, session: &Session) {
        let replace_path = std::mem::take(&mut self.replace_path);
        *self = Self::load(session);
        self.replace_path = replace_path;
    }

    pub fn target(&self) -> Option<ObjectId> {
        self.frame.as_ref().map(|frame| frame.object)
    }

    pub fn command(&self, edit: FrameBarEdit) -> Option<Command> {
        let frame = self.frame.as_ref()?;
        let object = frame.object.0;
        let command = match edit {
            FrameBarEdit::Fit(fit) => {
                let name = FITS.iter().find(|(known, _, _)| *known == fit)?.1;
                Command::with_params(picture_frame::FRAME_IMAGE_FIT, json!({ "object": object, "fit": name }))
            }
            FrameBarEdit::Anchor(anchor) => {
                let name = ANCHORS.iter().find(|(known, _, _)| *known == anchor)?.1;
                Command::with_params(picture_frame::FRAME_IMAGE_ANCHOR, json!({ "object": object, "anchor": name }))
            }
            FrameBarEdit::Scale(scale) if scale > 0.0 => Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({ "object": object, "scale": [scale, scale] })),
            FrameBarEdit::Scale(_) => return None,
            FrameBarEdit::Rotation(rotation) => Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({ "object": object, "rotation": rotation })),
            FrameBarEdit::Reset => Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({ "object": object, "offset": [0.0, 0.0], "scale": [1.0, 1.0], "rotation": 0.0 })),
            FrameBarEdit::ReplaceImage(path) if !path.trim().is_empty() => Command::with_params("resource.replace", json!({ "object": object, "path": path, "link": false })),
            FrameBarEdit::ReplaceImage(_) | FrameBarEdit::PickImage => return None,
        };
        Some(command)
    }

    pub fn apply(&mut self, session: &mut Session, edit: FrameBarEdit) -> CommandResult {
        let edit_kind = if matches!(edit, FrameBarEdit::ReplaceImage(_)) { EditKind::Replace } else { EditKind::Frame };
        let command = self.command(edit).ok_or_else(dtool_core::CommandError::invalid_params)?;
        let result = session.run(&command);
        let replaced = matches!(edit_kind, EditKind::Replace) && result.is_ok();
        self.reload(session);
        if replaced {
            self.replace_path.clear();
        }
        result
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<FrameBarEdit> {
        let frame = self.frame.clone()?;
        let mut edit = None;
        ui.horizontal(|ui| {
            ui.label(text("frame.fit"));
            let shown = FITS.iter().find(|(fit, _, _)| *fit == frame.fit).map(|(_, _, key)| text(key)).unwrap_or_default();
            egui::ComboBox::from_id_salt("ctx.picture_frame.fit").selected_text(shown).show_ui(ui, |ui| {
                for (fit, _, key) in FITS {
                    if ui.selectable_label(fit == frame.fit, text(key)).clicked() && fit != frame.fit {
                        edit = Some(FrameBarEdit::Fit(fit));
                    }
                }
            });
            let anchor_label = ANCHORS.iter().find(|(point, _, _)| *point == frame.anchor).map(|(_, _, label)| *label).unwrap_or("frame.anchor");
            let anchor = dtool_design::button(ui, text("frame.anchor"))
                .on_hover_text(text(anchor_label));
            egui::Popup::menu(&anchor).show(|ui| {
                egui::Grid::new("ctx.picture_frame.anchor").spacing([2.0, 2.0]).show(ui, |ui| {
                    for (index, (anchor, _, label)) in ANCHORS.iter().enumerate() {
                        let selected = *anchor == frame.anchor;
                        let label = text(label);
                        let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(18.0), egui::Sense::click());
                        let visuals = ui.style().interact_selectable(&response, selected);
                        ui.painter().rect(rect, 1.0, visuals.bg_fill, visuals.fg_stroke, egui::StrokeKind::Inside);
                        let response = response.on_hover_text(label);
                        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
                        if response.clicked() {
                            if *anchor != frame.anchor {
                                edit = Some(FrameBarEdit::Anchor(*anchor));
                            }
                            ui.close();
                        }
                        if index % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            });
            ui.separator();
            ui.label(text("frame.scale"));
            let mut scale = frame.content.scale[0] * 100.0;
            let field = ui.add(egui::DragValue::new(&mut scale).range(1.0..=10000.0).suffix(" %"));
            if field.changed() && scale > 0.0 {
                edit = Some(FrameBarEdit::Scale(scale / 100.0));
            }
            ui.label(text("frame.rotation"));
            let mut rotation = frame.content.rotation;
            if ui.add(egui::DragValue::new(&mut rotation).range(-360.0..=360.0).suffix("\u{b0}")).changed() {
                edit = Some(FrameBarEdit::Rotation(rotation));
            }
            if dtool_design::button(ui, text("frame.reset_content")).clicked() {
                edit = Some(FrameBarEdit::Reset);
            }
            if dtool_design::button(ui, text("frame.replace_image")).clicked() {
                edit = Some(FrameBarEdit::PickImage);
            }
        });
        edit
    }
}