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
    Offset([f32; 2]),
    Scale(f32),
    Rotation(f32),
    Reset,
    ReplaceImage(String),
    PickImage,
    PickMultiple,
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
    multiple_frames: bool,
    offset_unit: &'static str,
}

impl PictureFrameBar {
    pub fn load(session: &Session) -> Self {
        let mut frame = match session.selection.ids.as_slice() {
            [id] if session.document.get_object(*id).is_some() => picture_frame::get_frame(&session.document, *id),
            _ => None,
        };
        if let Some(frame) = &mut frame {
            frame.image = session.document.placed_resource(frame.object).or(frame.image);
        }
        let locked = frame.as_ref().and_then(|frame| session.document.get_object(frame.object))
            .is_some_and(|object| object.locked);
        let multiple_frames = session.selection.ids.len() > 1
            && session.selection.ids.iter().any(|&id| session.document.get_object(id).is_some_and(|object| {
                !object.locked && object.kind == dtool_core::ObjectKind::PictureFrame
            }));
        let offset_unit = dtool_core::commands::ruler_units::suffix(session.document.units);
        Self { frame, replace_path: String::new(), locked, multiple_frames, offset_unit }
    }

    pub fn reload(&mut self, session: &Session) {
        let replace_path = std::mem::take(&mut self.replace_path);
        *self = Self::load(session);
        self.replace_path = replace_path;
    }

    pub fn target(&self) -> Option<ObjectId> {
        self.frame.as_ref().map(|frame| frame.object)
    }

    pub(crate) fn visible(&self) -> bool { self.frame.is_some() || self.multiple_frames }

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
            FrameBarEdit::Offset(offset) => Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({ "object": object, "offset": offset })),
            FrameBarEdit::Scale(scale) if scale > 0.0 => Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({ "object": object, "scale": [scale, scale] })),
            FrameBarEdit::Scale(_) => return None,
            FrameBarEdit::Rotation(rotation) => Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({ "object": object, "rotation": rotation })),
            FrameBarEdit::Reset => Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({ "object": object, "reset": true })),
            FrameBarEdit::ReplaceImage(path) if !path.trim().is_empty() => Command::with_params("resource.replace", json!({ "object": object, "path": path, "link": false })),
            FrameBarEdit::ReplaceImage(_) | FrameBarEdit::PickImage | FrameBarEdit::PickMultiple => return None,
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
        if self.multiple_frames {
            return dtool_design::button(ui, text("frame.place_multiple")).clicked().then_some(FrameBarEdit::PickMultiple);
        }
        let frame = self.frame.as_ref()?;
        ui.push_id(("picture_frame", frame.object.0), |ui| {
            ui.add_enabled_ui(!self.locked, |ui| {
                let width = ui.available_width();
                if width < 200.0 {
                    return options_popup(ui, frame, true, self.offset_unit);
                }
                ui.horizontal(|ui| {
                    let edit = fit_control(ui, frame, width < 730.0, false);
                    if width < 620.0 {
                        let actions = if width >= 340.0 { image_actions(ui, true) } else { None };
                        let options = options_popup(ui, frame, true, self.offset_unit);
                        return options.or(actions).or(edit);
                    }
                    let anchor = anchor_control(ui, frame, self.offset_unit);
                    if width < 730.0 {
                        let options = options_popup(ui, frame, false, self.offset_unit);
                        return options.or(image_actions(ui, false)).or(anchor).or(edit);
                    }
                    ui.separator();
                    let transform = transform_controls(ui, frame, false);
                    let actions = image_actions(ui, false);
                    actions.or(transform).or(anchor).or(edit)
                }).inner
            }).inner
        }).inner
    }
}

fn fit_control(ui: &mut egui::Ui, frame: &PictureFrame, compact: bool, focus_first: bool) -> Option<FrameBarEdit> {
    let mut edit = None;
    dtool_design::with_icon_selection(ui, |ui| ui.horizontal(|ui| {
        for (index, (fit, _, label)) in FITS.into_iter().enumerate() {
            let label = text(label);
            let selected = frame.manual_layout.is_none() && frame.fit == fit;
            let button = egui::Button::new(if compact { "" } else { label }).selected(selected);
            let response = if compact {
                ui.add_sized(egui::Vec2::splat(dtool_design::size("control.icon.height")), button)
            } else {
                ui.add(button)
            };
            if compact {
                let centre = response.rect.center();
                let ink = ui.style().interact(&response).fg_stroke.color;
                let outer = egui::Rect::from_center_size(centre, egui::vec2(15.0, 12.0));
                let dimensions = match fit {
                    ImageFit::Native => egui::vec2(6.0, 5.0),
                    ImageFit::MinFit => egui::vec2(11.0, 6.0),
                    ImageFit::MaxFit => egui::vec2(15.0, 8.0),
                    ImageFit::Stretch => egui::vec2(13.0, 10.0),
                };
                ui.painter().rect_stroke(outer, 1.0, egui::Stroke::new(1.0, ink), egui::StrokeKind::Inside);
                ui.painter().rect_filled(egui::Rect::from_center_size(centre, dimensions), 0.0, ink.gamma_multiply(0.45));
            }
            if focus_first && index == 0 { response.request_focus(); }
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, response.enabled(), selected, label));
            if response.on_hover_text(label).clicked() && !selected { edit = Some(FrameBarEdit::Fit(fit)); }
        }
    }));
    edit
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

fn image_actions(ui: &mut egui::Ui, compact: bool) -> Option<FrameBarEdit> {
    let label = text("frame.reset_content");
    let reset = dtool_design::icon_button(ui, "").on_hover_text(label);
    reset.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, reset.enabled(), label));
    let centre = reset.rect.center();
    let ink = ui.style().interact(&reset).fg_stroke.color;
    let points = (0..=16).map(|step| {
        let angle = 0.5 + step as f32 * 0.32;
        centre + 5.5 * egui::vec2(angle.cos(), angle.sin())
    }).collect::<Vec<_>>();
    let tip = points[points.len() - 1];
    ui.painter().add(egui::Shape::line(points, egui::Stroke::new(1.4, ink)));
    ui.painter().line_segment([tip + egui::vec2(-4.0, 0.0), tip,], egui::Stroke::new(1.4, ink));
    ui.painter().line_segment([tip + egui::vec2(0.0, -4.0), tip,], egui::Stroke::new(1.4, ink));
    let replace = dtool_design::button(ui, text("frame.replace_image")).clicked();
    let multiple = !compact && dtool_design::button(ui, text("frame.place_multiple")).clicked();
    if replace { Some(FrameBarEdit::PickImage) }
    else if multiple { Some(FrameBarEdit::PickMultiple) }
    else { reset.clicked().then_some(FrameBarEdit::Reset) }
}

fn options_popup(ui: &mut egui::Ui, frame: &PictureFrame, include_fit_and_anchor: bool, unit: &str) -> Option<FrameBarEdit> {
    let trigger = dtool_design::button(ui, text("frame.options"));
    frame_popup(&trigger, |ui, focus_first| {
        let content = ui.ctx().content_rect();
        let popup_margin = 2.0 * dtool_design::space("md");
        let stack_actions = content.width() < 260.0;
        if stack_actions {
            ui.set_max_width((content.width() - popup_margin).max(1.0));
        }
        let footer_rows = if stack_actions { 3.0 } else { 1.0 };
        let footer_height = footer_rows * (dtool_design::size("control.icon.height") + ui.spacing().item_spacing.y);
        let options_height = (content.height() - footer_height - popup_margin).max(1.0);
        let edit = egui::ScrollArea::vertical()
            .id_salt("frame.options.scroll")
            .max_height(options_height)
            .show(ui, |ui| {
                let mut edit = None;
                if include_fit_and_anchor {
                    ui.label(text("frame.fit"));
                    edit = fit_control(ui, frame, true, focus_first);
                    ui.separator();
                    ui.label(text("frame.anchor"));
                    edit = anchor_grid(ui, frame, false).or(edit);
                    edit = offset_controls(ui, frame, unit).or(edit);
                    ui.separator();
                }
                ui.horizontal_wrapped(|ui| transform_controls(ui, frame, focus_first && !include_fit_and_anchor))
                    .inner.or(edit)
            }).inner;
        let actions = if stack_actions {
            ui.vertical(|ui| image_actions(ui, false)).inner
        } else {
            ui.horizontal_wrapped(|ui| image_actions(ui, false)).inner
        };
        if matches!(actions, Some(FrameBarEdit::PickImage | FrameBarEdit::PickMultiple)) {
            ui.close();
        }
        actions.or(edit)
    })
}

#[derive(Default)]
pub(crate) struct FrameContentGesture {
    selected: Option<(dtool_core::ContextId, ObjectId)>,
    drag: Option<([f32; 2], [f32; 2])>,
}

#[derive(Default)]
pub(crate) struct ContentDragFrame {
    pub capture: bool,
    pub preview: Option<PictureFrame>,
    pub command: Option<Command>,
}

impl FrameContentGesture {
    pub fn update(
        &mut self, ui: &mut egui::Ui, response: &egui::Response,
        session: &Session, owner: dtool_core::ContextId, enabled: bool,
    ) -> ContentDragFrame {
        let mut result = ContentDragFrame::default();
        let Some(object) = enabled.then_some(session.selection.ids.as_slice()).and_then(|ids| {
            let [id] = ids else { return None; };
            let object = session.document.get_object(*id)?;
            (!object.locked).then_some(object)
        }) else { *self = Self::default(); return result; };
        let Some(frame) = picture_frame::get_frame(&session.document, object.id).filter(|frame| {
            picture_frame::frame_image_size(&session.document, object, frame).is_some()
        })
        else { *self = Self::default(); return result; };
        let key = (owner, object.id);
        if self.selected != Some(key) { *self = Self::default(); }
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            *self = Self::default();
            return result;
        }
        let to_doc = |p: egui::Pos2| {
            let local = p - response.rect.min;
            dtool_core::view_to_doc(&session.view, [local.x, local.y])
        };
        let to_local = |p| dtool_core::geometry::doc_to_object_local(object, to_doc(p));
        let centre = dtool_core::geometry::apply_matrix(&dtool_core::geometry::object_matrix(object),
            [object.pos[0] + object.size[0] * 0.5, object.pos[1] + object.size[1] * 0.5]);
        let centre = dtool_core::doc_to_view(&session.view, centre);
        let centre = response.rect.min + egui::vec2(centre[0], centre[1]);
        let handle = egui::Rect::from_center_size(centre, egui::Vec2::splat(18.0));
        let inside = |pos| picture_frame::contains_point(&session.document, object, to_doc(pos));
        let (down, up) = crate::input::primary_button_positions(ui);
        if response.double_clicked() && response.interact_pointer_pos().is_some_and(inside) {
            self.selected = Some(key);
            result.capture = true;
        }
        if let Some(pos) = down.filter(|p| response.interact_rect.contains(*p)
            && ui.ctx().layer_id_at(*p) == Some(response.layer_id) && !egui::Popup::is_any_open(ui.ctx()))
        {
            if (handle.contains(pos) || (self.selected == Some(key) && inside(pos)))
                && let Some(start) = to_local(pos)
            {
                self.selected = Some(key);
                self.drag = Some((start, frame.content.offset));
            } else if self.drag.is_none() {
                self.selected = None;
            }
        }
        if let Some((start, original)) = self.drag {
            result.capture = true;
            let point = up.or_else(|| ui.input(|input| input.pointer.interact_pos()));
            if let Some(end) = point.and_then(to_local) {
                let offset = [original[0] + end[0] - start[0], original[1] + end[1] - start[1]];
                if offset != original {
                    if up.is_some() {
                        result.command = Some(Command::with_params(picture_frame::FRAME_IMAGE_CONTENT, json!({
                            "object": object.id, "offset": offset,
                        })));
                    } else {
                        let mut preview = frame.clone();
                        preview.content.offset = offset;
                        result.preview = Some(preview);
                    }
                }
            }
            if up.is_some() || !ui.input(|input| input.pointer.primary_down()) { self.drag = None; }
        }
        let can_drag = response.hover_pos().is_some_and(|pos| handle.contains(pos) || (self.selected == Some(key) && inside(pos)));
        if response.interact_rect.contains(centre)
            && object.size.iter().all(|side| side.abs() * session.view.zoom > 32.0)
        {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Foreground, response.id.with("picture-frame-content")))
                .with_clip_rect(response.interact_rect);
            painter.circle(centre, 8.5, ui.visuals().extreme_bg_color, ui.visuals().selection.stroke);
            for axis in [egui::Vec2::X, egui::Vec2::Y] {
                painter.line_segment([centre - 4.0 * axis, centre + 4.0 * axis], ui.visuals().selection.stroke);
            }
        }
        if can_drag {
            ui.ctx().set_cursor_icon(if self.drag.is_some() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
        }
        result
    }
}
