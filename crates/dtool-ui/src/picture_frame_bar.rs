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
    ManualFit,
    Anchor(Anchor9),
    Offset([f32; 2]),
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
    locked: bool,
    offset_unit: &'static str,
}

impl PictureFrameBar {
    pub fn load(session: &Session) -> Self {
        let frame = match session.selection.ids.as_slice() {
            [id] if session.document.get_object(*id).is_some() => picture_frame::get_frame(&session.document, *id),
            _ => None,
        };
        let locked = frame.as_ref().and_then(|frame| session.document.get_object(frame.object))
            .is_some_and(|object| object.locked);
        let offset_unit = dtool_core::commands::ruler_units::suffix(session.document.units);
        Self { frame, replace_path: String::new(), locked, offset_unit }
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
            FrameBarEdit::ManualFit => Command::with_params(picture_frame::FRAME_IMAGE_FIT, json!({ "object": object, "fit": "manual" })),
            FrameBarEdit::Anchor(anchor) => {
                let name = ANCHORS.iter().find(|(known, _, _)| *known == anchor)?.1;
                Command::with_params(picture_frame::FRAME_IMAGE_ANCHOR, json!({ "object": object, "anchor": name }))
            }
            FrameBarEdit::Offset(offset) => Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({ "object": object, "offset": offset })),
            FrameBarEdit::Scale(scale) if scale > 0.0 => Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({ "object": object, "scale": [scale, scale] })),
            FrameBarEdit::Scale(_) => return None,
            FrameBarEdit::Rotation(rotation) => Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({ "object": object, "rotation": rotation })),
            FrameBarEdit::Reset => Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({ "object": object, "reset": true })),
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
        let frame = self.frame.as_ref()?;
        ui.push_id(("picture_frame", frame.object.0), |ui| {
            ui.add_enabled_ui(!self.locked, |ui| {
                let width = ui.available_width();
                if width < 300.0 {
                    return options_popup(ui, frame, true, self.offset_unit);
                }
                ui.horizontal(|ui| {
                    let edit = fit_control(ui, frame);
                    let anchor = anchor_control(ui, frame, self.offset_unit);
                    if width < 600.0 {
                        let options = options_popup(ui, frame, false, self.offset_unit);
                        return options.or(anchor).or(edit);
                    }
                    ui.separator();
                    let transform = transform_controls(ui, frame, false);
                    let actions = image_actions(ui);
                    actions.or(transform).or(anchor).or(edit)
                }).inner
            }).inner
        }).inner
    }
}

fn fit_control(ui: &mut egui::Ui, frame: &PictureFrame) -> Option<FrameBarEdit> {
    let caption = ui.label(text("frame.fit"));
    let mut selected = if frame.manual_layout.is_some() {
        FITS.len()
    } else {
        FITS.iter().position(|(fit, _, _)| *fit == frame.fit)?
    };
    let options: Vec<_> = FITS.iter().map(|(_, _, key)| text(key))
        .chain(std::iter::once(text("frame.fit.manual"))).collect();
    let response = dtool_design::dropdown(ui, "frame.fit", &mut selected, &options)
        .labelled_by(caption.id).on_hover_text(options[selected]);
    response.changed().then(|| FITS.get(selected).map_or(FrameBarEdit::ManualFit, |(fit, _, _)| FrameBarEdit::Fit(*fit)))
}

fn anchor_control(ui: &mut egui::Ui, frame: &PictureFrame, unit: &str) -> Option<FrameBarEdit> {
    let label = ANCHORS.iter().find(|(point, _, _)| *point == frame.anchor)
        .map_or("frame.anchor", |(_, _, label)| *label);
    let trigger = dtool_design::button(ui, text("frame.anchor")).on_hover_text(text(label));
    frame_popup(&trigger, |ui, focus_first| {
        let anchor = anchor_grid(ui, frame, focus_first);
        if anchor.is_some() {
            ui.close();
        }
        ui.separator();
        offset_controls(ui, frame, unit).or(anchor)
    })
}

fn frame_popup(
    trigger: &egui::Response, contents: impl FnOnce(&mut egui::Ui, bool) -> Option<FrameBarEdit>,
) -> Option<FrameBarEdit> {
    let id = egui::Popup::default_response_id(trigger);
    let focus_request = id.with("initial_focus");
    if trigger.clicked() && !egui::Popup::is_id_open(&trigger.ctx, id) {
        trigger.ctx.data_mut(|data| data.insert_temp(focus_request, true));
    }

    let result = egui::Popup::menu(trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            // The first popup pass measures its size without registering focus.
            let focus_first = !ui.is_sizing_pass()
                && ui.data_mut(|data| data.remove_temp::<bool>(focus_request)).unwrap_or(false);
            contents(ui, focus_first)
        })?;
    if result.response.should_close()
        && (result.inner.is_some() || trigger.ctx.input(|input| input.key_pressed(egui::Key::Escape)))
    {
        trigger.request_focus();
    }
    result.inner
}

fn anchor_grid(ui: &mut egui::Ui, frame: &PictureFrame, focus_first: bool) -> Option<FrameBarEdit> {
    let mut edit = None;
    let size = egui::Vec2::splat(dtool_design::size("control.icon.height"));
    dtool_design::with_icon_selection(ui, |ui| {
        egui::Grid::new("frame.anchor").spacing(egui::Vec2::splat(dtool_design::space("xs"))).show(ui, |ui| {
            for row in ANCHORS.chunks(3) {
                for &(anchor, _, label) in row {
                    let selected = anchor == frame.anchor;
                    let label = text(label);
                    let response = ui.add_sized(size, egui::Button::new("").selected(selected)).on_hover_text(label);
                    if focus_first && selected && response.enabled() {
                        response.request_focus();
                    }
                    if response.has_focus() {
                        ui.painter().rect_stroke(
                            response.rect.shrink(1.0),
                            dtool_design::radius("sm"),
                            egui::Stroke::new(dtool_design::stroke_width("thin"),
                                dtool_design::theme::widget_token("focus").unwrap_or(ui.visuals().selection.bg_fill)),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let ink = if selected && response.enabled() {
                        dtool_design::selected_icon_ink(ui)
                    } else {
                        ui.style().interact(&response).fg_stroke.color
                    };
                    ui.painter().circle_filled(response.rect.center(), if selected { 3.0 } else { 2.0 }, ink);
                    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
                    if response.clicked() && !selected {
                        edit = Some(FrameBarEdit::Anchor(anchor));
                    }
                }
                ui.end_row();
            }
        });
    });
    edit
}

fn transform_controls(ui: &mut egui::Ui, frame: &PictureFrame, focus_first: bool) -> Option<FrameBarEdit> {
    let scale_spec = dtool_design::NumericSpec::new(text("frame.scale"))
        .range(1.0..=10000.0).suffix(" %").width(58.0).reset_to(100.0);
    let mut scale = f64::from(frame.content.scale[0]) * 100.0;
    let scale_id = ui.make_persistent_id(("frame.scale", frame.object.0));
    let scale_response = dtool_design::numeric_value(ui, scale_id, &mut scale, &scale_spec);
    if focus_first {
        scale_response.request_focus();
    }
    let scale_edit = scale_response.changed().then(|| FrameBarEdit::Scale((scale / 100.0) as f32));

    let rotation_spec = dtool_design::NumericSpec::new(text("frame.rotation"))
        .range(-360.0..=360.0).suffix("°").width(54.0).reset_to(0.0);
    let mut rotation = f64::from(frame.content.rotation);
    let rotation_id = ui.make_persistent_id(("frame.rotation", frame.object.0));
    let rotation_edit = dtool_design::numeric_value(ui, rotation_id, &mut rotation, &rotation_spec).changed()
        .then(|| FrameBarEdit::Rotation(rotation as f32));
    rotation_edit.or(scale_edit)
}

fn offset_controls(ui: &mut egui::Ui, frame: &PictureFrame, unit: &str) -> Option<FrameBarEdit> {
    let unit = format!(" {unit}");
    let mut offset = frame.content.offset.map(f64::from);
    let mut edited = false;
    for (axis, key) in ["frame.offset_x", "frame.offset_y"].into_iter().enumerate() {
        let spec = dtool_design::NumericSpec::new(text(key))
            .range(-1_000_000.0..=1_000_000.0).suffix(&unit).width(72.0).reset_to(0.0);
        let id = ui.make_persistent_id((key, frame.object.0));
        edited |= dtool_design::numeric_value(ui, id, &mut offset[axis], &spec).changed();
    }
    edited.then(|| FrameBarEdit::Offset(offset.map(|value| value as f32)))
}

fn image_actions(ui: &mut egui::Ui) -> Option<FrameBarEdit> {
    let reset = dtool_design::button(ui, text("frame.reset_content")).clicked();
    let replace = dtool_design::button(ui, text("frame.replace_image")).clicked();
    if replace { Some(FrameBarEdit::PickImage) } else { reset.then_some(FrameBarEdit::Reset) }
}

fn options_popup(ui: &mut egui::Ui, frame: &PictureFrame, include_fit_and_anchor: bool, unit: &str) -> Option<FrameBarEdit> {
    let trigger = dtool_design::button(ui, text("frame.options"));
    frame_popup(&trigger, |ui, focus_first| {
        let footer_height = dtool_design::size("control.icon.height") + 2.0 * ui.spacing().item_spacing.y;
        let popup_margin = 2.0 * dtool_design::space("md");
        let options_height = (ui.ctx().content_rect().height() - footer_height - popup_margin).max(1.0);
        let edit = egui::ScrollArea::vertical()
            .id_salt("frame.options.scroll")
            .max_height(options_height)
            .show(ui, |ui| {
                let mut edit = None;
                if include_fit_and_anchor {
                    ui.label(text("frame.fit"));
                    dtool_design::with_icon_selection(ui, |ui| {
                        for (index, (fit, _, label)) in FITS.into_iter().enumerate() {
                            let selected = frame.manual_layout.is_none() && fit == frame.fit;
                            let response = ui.selectable_label(selected, text(label));
                            if focus_first && index == 0 {
                                response.request_focus();
                            }
                            if response.clicked() && !selected {
                                edit = Some(FrameBarEdit::Fit(fit));
                            }
                        }
                        let manual = frame.manual_layout.is_some();
                        if ui.selectable_label(manual, text("frame.fit.manual")).clicked() && !manual {
                            edit = Some(FrameBarEdit::ManualFit);
                        }
                    });
                    ui.separator();
                    ui.label(text("frame.anchor"));
                    edit = anchor_grid(ui, frame, false).or(edit);
                    edit = offset_controls(ui, frame, unit).or(edit);
                    ui.separator();
                }
                ui.horizontal_wrapped(|ui| transform_controls(ui, frame, focus_first && !include_fit_and_anchor))
                    .inner.or(edit)
            }).inner;
        let actions = ui.horizontal_wrapped(image_actions).inner;
        if actions == Some(FrameBarEdit::PickImage) {
            ui.close();
        }
        actions.or(edit)
    })
}
