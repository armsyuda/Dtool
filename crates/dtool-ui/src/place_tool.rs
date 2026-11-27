use dtool_core::commands::image_place::DecodedImage;
use dtool_core::{Command, CommandError, ContextId, Scope, Session, Workspace};
use dtool_strings::text;
use serde_json::json;
use std::collections::HashMap;

pub const TOOL: &str = "tool.place_image";

#[derive(Default)]
pub struct PlaceTool {
    pub dialog_open: bool,
    pub path: String,
    pub error_key: Option<String>,
    pub link: bool,
    target: Option<ContextId>,
    previous_tool: String,
    image: Option<DecodedImage>,
    press: Option<[f32; 2]>,
    current_path: Option<String>,
    queue: Vec<String>,
    queued_images: Vec<(String, DecodedImage)>,
    frame_queue: bool,
    thumbnails: HashMap<String, Option<egui::TextureHandle>>,
}

/// Normalize any drag direction and preserve the image ratio unless Shift frees it.
pub fn drag_geometry(start: [f32; 2], end: [f32; 2], aspect: f32, free: bool) -> Option<([f32; 2], [f32; 2])> {
    if !start.iter().chain(&end).all(|v| v.is_finite()) || !aspect.is_finite() || aspect <= 0.0 { return None; }
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let (mut w, mut h) = (dx.abs(), dy.abs());
    if !free { w = w.max(h * aspect); h = w / aspect; }
    if w <= 0.0 || h <= 0.0 || !w.is_finite() || !h.is_finite() { return None; }
    Some(([if dx < 0.0 { start[0] - w } else { start[0] }, if dy < 0.0 { start[1] - h } else { start[1] }], [w, h]))
}

pub const PLACE_IMAGE_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "bmp", "webp", "tif", "tiff"];

pub fn split_paths(text: &str) -> Vec<String> {
    text.split(|c| c == ';' || c == '\n')
        .map(|part| part.trim().trim_matches('"').trim())
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn image_drop_paths(paths: &[std::path::PathBuf]) -> Vec<String> {
    paths
        .iter()
        .filter(|path| path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| PLACE_IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())))
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

pub const DROP_PLACE_EXTENSIONS: [&str; 5] = ["pdf", "svg", "svgz", "ai", "eps"];
pub const DROP_TEXT_EXTENSIONS: [&str; 3] = ["txt", "rtf", "docx"];

pub fn drop_commands(paths: &[std::path::PathBuf]) -> (Vec<Command>, Vec<Command>) {
    let mut texts = Vec::new();
    let mut opens = Vec::new();
    for path in paths {
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()).map(str::to_ascii_lowercase) else { continue };
        let params = json!({ "path": path.to_string_lossy() });
        if DROP_TEXT_EXTENSIONS.contains(&ext.as_str()) {
            texts.push(Command::with_params("text.import", params));
        } else if DROP_PLACE_EXTENSIONS.contains(&ext.as_str()) {
            opens.push(Command::with_params(dtool_core::import_hook::IMPORT_PLACE, params));
        }
    }
    (texts, opens)
}

fn place_document(workspace: &mut Workspace, command: Command) -> Result<(), CommandError> {
    if workspace.active().is_none() {
        let open = Command::with_params("doc.open", command.params);
        return workspace.run(Scope::active(), open).1.map(|_| ());
    }
    let path = command.params.get("path").and_then(|value| value.as_str()).ok_or_else(CommandError::invalid_params)?;
    dtool_io::import_into_current(workspace, std::path::Path::new(path)).map(|_| ())
}

impl PlaceTool {
    pub fn drop_files(&mut self, workspace: &mut Workspace, paths: &[std::path::PathBuf]) -> Result<bool, CommandError> {
        self.drop_files_at(workspace, paths, None)
    }

    pub(crate) fn drop_files_at(&mut self, workspace: &mut Workspace, paths: &[std::path::PathBuf], point: Option<[f32; 2]>) -> Result<bool, CommandError> {
        let (texts, opens) = drop_commands(paths);
        let images = image_drop_paths(paths);
        if texts.is_empty() && opens.is_empty() && images.is_empty() {
            return Ok(false);
        }
        let mut first_error = None;
        for command in texts {
            if let Err(error) = workspace.run(Scope::active(), command).1 {
                first_error.get_or_insert(error);
            }
        }
        if !images.is_empty() {
            if let Err(error) = self.drop_images_at(workspace, images, point) {
                first_error.get_or_insert(error);
            }
        }
        for command in opens {
            if let Err(error) = place_document(workspace, command) {
                first_error.get_or_insert(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(true),
        }
    }

    fn drop_images_at(&mut self, workspace: &mut Workspace, paths: Vec<String>, point: Option<[f32; 2]>) -> Result<(), CommandError> {
        let session = workspace.session().ok_or_else(CommandError::no_document)?;
        let frame = point.and_then(|point| frame_at(session, point));
        let multiple = paths.len() > 1;
        if !multiple && let (Some(frame), Some(point)) = (frame, point) {
            self.cancel(workspace);
            workspace.run(Scope::active(), Command::with_params("image.place", json!({
                "path": paths[0], "pos": point, "picture_frame": frame, "fill_frame": true,
            }))).1?;
            return Ok(());
        }
        let frames_selected = session.selection.ids.iter().any(|&id| {
            session.document.get_object(id).is_some_and(|item| {
                item.kind == dtool_core::ObjectKind::PictureFrame && !item.locked
            })
        });
        self.start_queue(workspace, paths, multiple && (frame.is_some() || frames_selected))
    }

    pub(crate) fn start_frame_queue(&mut self, workspace: &mut Workspace, paths: Vec<String>) -> Result<(), CommandError> {
        self.start_queue(workspace, paths, true)
    }

    fn start_queue(&mut self, workspace: &mut Workspace, paths: Vec<String>, frame_queue: bool) -> Result<(), CommandError> {
        self.cancel(workspace);
        self.queue_clear();
        let id = workspace.active().ok_or_else(CommandError::no_document)?;
        self.target = Some(id);
        self.previous_tool = workspace.get(id).ok_or_else(CommandError::context_not_found)?.session.active_tool.clone();
        self.frame_queue = frame_queue;
        self.link = false;
        let result = self.queue_set(workspace, paths);
        if result.is_err() {
            self.cancel(workspace);
        }
        result
    }

    pub fn open(&mut self, workspace: &mut Workspace) -> Result<(), CommandError> {
        self.cancel(workspace);
        let id = workspace.active().ok_or_else(CommandError::no_document)?;
        let session = &workspace.get(id).ok_or_else(CommandError::context_not_found)?.session;
        self.target = Some(id);
        self.previous_tool = session.active_tool.clone();
        self.error_key = None;
        self.path.clear();
        self.dialog_open = true;
        Ok(())
    }

    pub fn arm(&mut self, workspace: &mut Workspace, image: DecodedImage) -> Result<(), CommandError> {
        let id = self.target.or_else(|| workspace.active()).ok_or_else(CommandError::no_document)?;
        let context = workspace.get_mut(id).ok_or_else(CommandError::context_not_found)?;
        let [w,h] = image.resource.pixel_size;
        if w == 0 || h == 0 || (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)) != Some(image.rgba.len()) {
            return Err(CommandError::invalid_params());
        }
        if self.target.is_none() { self.previous_tool = context.session.active_tool.clone(); }
        self.target = Some(id);
        self.image = Some(image);
        self.press = None;
        self.dialog_open = false;
        self.error_key = None;
        context.session.active_tool = TOOL.into();
        Ok(())
    }

    pub fn active_for(&self, id: ContextId) -> bool { self.target == Some(id) && self.image.is_some() }

    pub(crate) fn show_frame_queue(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        if !self.frame_queue || self.image.is_none() {
            return;
        }
        if self.target != workspace.active() || ui.input(|input| input.key_pressed(egui::Key::Escape))
            || workspace.session().is_some_and(|session| session.active_tool != TOOL)
        {
            self.cancel(workspace);
            return;
        }
        let Some(active) = self.current_path.clone() else { return };
        if !self.thumbnails.contains_key(&active) {
            let texture = self.image.as_ref().map(|image| thumbnail(ui.ctx(), &active, image));
            self.thumbnails.insert(active.clone(), texture);
        }
        let count = self.queue.len() + 1;
        let cancel = egui::Panel::bottom("picture-frame-placement")
            .frame(egui::Frame::new().fill(dtool_design::theme::surface(ui.ctx(), "bars"))
                .inner_margin(dtool_design::space("sm")))
            .show(ui, |ui| {
                let cancel = ui.horizontal(|ui| {
                    let cancel = dtool_design::button(ui, text("button.cancel")).clicked();
                    let remaining = count.to_string();
                    ui.label(dtool_strings::format("frame.place_remaining", &[("count", &remaining)]));
                    cancel
                }).inner;
                let mut decoded = false;
                egui::ScrollArea::horizontal().id_salt(("frame-placement", active.as_str()))
                    .auto_shrink([false, false])
                    .show_viewport(ui, |ui, viewport| {
                        ui.horizontal(|ui| {
                            let margin = dtool_design::space("sm");
                            let mut x = 0.0;
                            for (index, path) in std::iter::once(&active).chain(self.queue.iter()).enumerate() {
                                let next = index == 0;
                                let width = if next { 76.0 } else { 58.0 };
                                let (slot, response) = ui.allocate_exact_size(egui::vec2(width, 56.0), egui::Sense::hover());
                                let in_view = viewport.x_range().intersects(egui::Rangef::new(x, x + width));
                                x += width + ui.spacing().item_spacing.x;
                                if !in_view { continue; }
                                if !decoded && !self.thumbnails.contains_key(path) {
                                    let image = dtool_io::image_import::decode_image(std::path::Path::new(path)).ok();
                                    self.thumbnails.insert(path.clone(), image.as_ref().map(|image| thumbnail(ui.ctx(), path, image)));
                                    decoded = true;
                                    ui.ctx().request_repaint();
                                }
                                let label = std::path::Path::new(path).file_name().and_then(|name| name.to_str()).unwrap_or(path);
                                response.on_hover_text(label);
                                let rect = slot.shrink(if next { 1.0 } else { 6.0 });
                                let radius = dtool_design::radius("sm");
                                ui.painter().rect_filled(rect, radius, dtool_design::theme::surface(ui.ctx(), "pasteboard"));
                                if let Some(texture) = self.thumbnails.get(path).and_then(Option::as_ref) {
                                    let bounds = rect.shrink(margin);
                                    let size = texture.size_vec2();
                                    let factor = (bounds.width() / size.x).min(bounds.height() / size.y);
                                    ui.painter().image(texture.id(), egui::Rect::from_center_size(rect.center(), size * factor),
                                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                                } else {
                                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, (index + 1).to_string(),
                                        egui::FontId::default(), ui.visuals().text_color());
                                }
                                if next {
                                    ui.painter().rect_stroke(rect, radius, egui::Stroke::new(2.0, dtool_design::color("accent")), egui::StrokeKind::Inside);
                                }
                            }
                        });
                    });
                cancel
            }).inner;
        if cancel { self.cancel(workspace); }
    }

    pub fn cancel(&mut self, workspace: &mut Workspace) {
        if let Some(path) = self.current_path.take() {
            self.queue.insert(0, path);
        }
        if let Some(context) = self.target.and_then(|id| workspace.get_mut(id)) {
            self.restore_tool(&mut context.session);
        }
        self.image = None;
        self.target = None;
        self.press = None;
        self.dialog_open = false;
        if self.frame_queue {
            self.queue_clear();
        }
    }

    pub fn queue_set(&mut self, workspace: &mut Workspace, paths: Vec<String>) -> Result<(), CommandError> {
        if paths.is_empty() {
            return Err(CommandError::invalid_params());
        }
        self.queued_images.clear();
        self.current_path = None;
        self.image = None;
        self.queue = paths;
        self.arm_next(workspace)
    }

    pub fn queue_set_decoded(&mut self, workspace: &mut Workspace, images: Vec<(String, DecodedImage)>) -> Result<(), CommandError> {
        if images.is_empty() {
            return Err(CommandError::invalid_params());
        }
        self.queue = images.iter().map(|(path, _)| path.clone()).collect();
        self.queued_images = images;
        self.current_path = None;
        self.image = None;
        self.arm_next(workspace)
    }

    pub fn queue_next(&self) -> Option<&str> {
        self.current_path.as_deref().or_else(|| self.queue.first().map(String::as_str))
    }

    pub fn queue_remaining(&self) -> &[String] {
        &self.queue
    }

    pub fn queue_clear(&mut self) {
        self.queue.clear();
        self.queued_images.clear();
        self.current_path = None;
        self.frame_queue = false;
        self.thumbnails.clear();
    }

    fn take_queued_image(&mut self, path: &str) -> Option<DecodedImage> {
        if let Some(index) = self.queued_images.iter().position(|(item, _)| item == path) {
            return Some(self.queued_images.remove(index).1);
        }
        None
    }

    fn arm_next(&mut self, workspace: &mut Workspace) -> Result<(), CommandError> {
        let Some(path) = self.queue.first().cloned() else {
            return Ok(());
        };
        self.queue.remove(0);
        let image = match self.take_queued_image(&path) {
            Some(image) => image,
            None => dtool_io::image_import::decode_image(std::path::Path::new(&path))?,
        };
        self.current_path = Some(path);
        self.arm(workspace, image)
    }

    fn arm_next_on_session(&mut self, id: ContextId, session: &mut Session) -> Result<(), CommandError> {
        let Some(path) = self.queue.first().cloned() else {
            return Ok(());
        };
        self.queue.remove(0);
        let image = match self.take_queued_image(&path) {
            Some(image) => image,
            None => dtool_io::image_import::decode_image(std::path::Path::new(&path))?,
        };
        let [w, h] = image.resource.pixel_size;
        if w == 0 || h == 0 || (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)) != Some(image.rgba.len()) {
            return Err(CommandError::invalid_params());
        }
        if self.target.is_none() { self.previous_tool = session.active_tool.clone(); }
        self.target = Some(id);
        self.current_path = Some(path);
        self.image = Some(image);
        self.press = None;
        self.dialog_open = false;
        self.error_key = None;
        session.active_tool = TOOL.into();
        Ok(())
    }

    fn restore_tool(&self, session: &mut Session) {
        if session.active_tool == TOOL { session.active_tool = self.previous_tool.clone(); }
    }

    pub fn place(&mut self, id: ContextId, session: &mut Session, position: [f32; 2], size: Option<[f32; 2]>) -> Result<(), CommandError> {
        if self.target != Some(id) { return Err(CommandError::context_not_found()); }
        let image = self.image.as_ref().ok_or_else(CommandError::invalid_params)?;
        let mut params = json!({"source":image.resource,"pixel_size":image.resource.pixel_size,"rgba":image.rgba,"pos":position});
        let frame = size.is_none().then(|| frame_at(session, position)).flatten();
        if self.frame_queue && frame.is_none() {
            return Ok(());
        }
        if let Some(size) = size {
            params["size"] = json!(size);
        } else if let Some(frame) = frame {
            params["picture_frame"] = json!(frame);
            params["fill_frame"] = json!(true);
        }
        if self.link { params["link"] = json!(true); }
        session.run_named("image.place", params)?;
        self.image = None;
        self.current_path = None;
        self.press = None;
        while !self.queue.is_empty() {
            let id = self.target.ok_or_else(CommandError::no_document)?;
            match self.arm_next_on_session(id, session) {
                Ok(()) => return Ok(()),
                Err(error) if !self.frame_queue => return Err(error),
                Err(error) => self.error_key = Some(error.message_key),
            }
        }
        self.restore_tool(session);
        self.target = None;
        self.queue_clear();
        Ok(())
    }

    pub fn show_dialog(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) -> Vec<std::path::PathBuf> {
        if !self.dialog_open { return Vec::new(); }
        let (mut confirm, mut cancel) = (false, false);
        egui::Window::new(text("menu.file.place")).collapsible(false).show(ui.ctx(), |ui| {
            dtool_design::dialog_frame(ui, |ui| {
                ui.label(text("dialog.file.path"));
                ui.text_edit_singleline(&mut self.path);
                if let Some(key) = &self.error_key { ui.label(text(key)); }
                ui.horizontal(|ui| {
                    confirm = dtool_design::button(ui, text("button.ok")).clicked();
                    cancel = dtool_design::button(ui, text("button.cancel")).clicked();
                });
                cancel |= ui.input(|input| input.key_pressed(egui::Key::Escape));
            });
        });
        if cancel { self.cancel(workspace); }
        else if confirm {
            let paths = split_paths(&self.path);
            let (pdfs, files): (Vec<_>, Vec<_>) = paths.iter().map(std::path::PathBuf::from)
                .partition(|path| crate::pdf_import_dialog::is_pdf(path));
            let (texts, opens) = drop_commands(&files);
            if !pdfs.is_empty() || !texts.is_empty() || !opens.is_empty() {
                self.cancel(workspace);
                self.dialog_open = false;
                if let Err(error) = self.drop_files(workspace, &files) {
                    self.dialog_open = true;
                    self.error_key = Some(error.message_key);
                }
                return pdfs;
            }
            let result = if paths.len() > 1 {
                self.queue_set(workspace, paths)
            } else {
                dtool_io::image_import::decode_image(std::path::Path::new(paths.first().map(String::as_str).unwrap_or_default()))
                    .and_then(|image| self.arm(workspace, image))
            };
            if let Err(error) = result { self.error_key = Some(error.message_key); }
        }
        Vec::new()
    }

    pub fn canvas(&mut self, ui: &mut egui::Ui, response: &egui::Response, id: ContextId, session: &mut Session, input: bool) {
        if !self.active_for(id) { return; }
        if session.active_tool != TOOL || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if let Some(path) = self.current_path.take() {
                self.queue.insert(0, path);
            }
            self.restore_tool(session);
            self.image = None;
            self.target = None;
            self.press = None;
            if self.frame_queue { self.queue_clear(); }
            return;
        }
        if !input { return; }
        if response.hovered() {
            ui.ctx().set_cursor_icon(if self.frame_queue { egui::CursorIcon::None } else { egui::CursorIcon::Crosshair });
        }
        let point = |p: egui::Pos2| dtool_core::view_to_doc(&session.view, [p.x - response.rect.min.x, p.y - response.rect.min.y]);
        if self.frame_queue {
            if let Some(pointer) = response.hover_pos() {
                paint_placement_cursor(ui, response, pointer);
            }
            if response.clicked()
                && let Some(position) = response.interact_pointer_pos().map(point)
            {
                if let Err(error) = self.place(id, session, position, None) {
                    self.error_key = Some(error.message_key);
                }
                ui.ctx().request_repaint();
            }
            return;
        }
        if response.drag_started() { self.press = ui.input(|i| i.pointer.press_origin()).map(point); }
        let end = response.interact_pointer_pos().map(point);
        let free = ui.input(|i| i.modifiers.shift);
        let aspect = self.image.as_ref().map(|image| image.resource.pixel_size[0] as f32 / image.resource.pixel_size[1] as f32).unwrap_or(1.0);
        let geometry = self.press.zip(end).and_then(|(start,end)| drag_geometry(start,end,aspect,free));
        if let Some((pos,size)) = geometry {
            let corners = [pos,[pos[0]+size[0],pos[1]],[pos[0]+size[0],pos[1]+size[1]],[pos[0],pos[1]+size[1]]]
                .map(|p| { let p=dtool_core::doc_to_view(&session.view,p); egui::pos2(response.rect.min.x+p[0],response.rect.min.y+p[1]) });
            ui.painter_at(response.rect).add(egui::Shape::closed_line(corners.to_vec(),ui.visuals().selection.stroke));
        }
        let placement = if response.drag_stopped() { self.press = None; geometry.map(|(p,s)|(p,Some(s))) }
            else if response.clicked() { end.map(|p|(p,None)) } else { None };
        if let Some((pos,size)) = placement {
            if let Err(error) = self.place(id,session,pos,size) { self.error_key = Some(error.message_key); }
            ui.ctx().request_repaint();
        }
    }
}

fn frame_at(session: &Session, point: [f32; 2]) -> Option<dtool_core::ObjectId> {
    session.document.objects().iter().rev().find(|object| {
        !object.locked && dtool_core::scene::is_drawn_in_view(&session.document, &session.view, object.id)
            && dtool_core::commands::picture_frame::contains_point(&session.document, object, point)
    }).map(|frame| frame.id)
}

fn thumbnail(ctx: &egui::Context, path: &str, image: &DecodedImage) -> egui::TextureHandle {
    let [width, height] = image.resource.pixel_size.map(|value| value as usize);
    let ratio = (96.0 / width.max(height) as f32).min(1.0);
    let [tw, th] = [width, height].map(|value| (value as f32 * ratio).round().max(1.0) as usize);
    let mut rgba = Vec::with_capacity(tw * th * 4);
    for y in 0..th {
        for x in 0..tw {
            let index = ((y * height / th) * width + x * width / tw) * 4;
            rgba.extend_from_slice(&image.rgba[index..index + 4]);
        }
    }
    ctx.load_texture(path, egui::ColorImage::from_rgba_unmultiplied([tw, th], &rgba), egui::TextureOptions::LINEAR)
}

fn paint_placement_cursor(ui: &egui::Ui, response: &egui::Response, point: egui::Pos2) {
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, response.id.with("place-image")))
        .with_clip_rect(response.interact_rect);
    let rect = egui::Rect::from_min_size(point + egui::vec2(7.0, 5.0), egui::vec2(16.0, 13.0));
    let ink = egui::Color32::WHITE;
    let edge = egui::Color32::from_gray(40);
    painter.rect(rect, 2.0, edge, egui::Stroke::new(1.0, ink), egui::StrokeKind::Inside);
    painter.circle_filled(rect.min + egui::vec2(10.5, 4.5), 1.5, ink);
    painter.add(egui::Shape::line([
        rect.left_bottom() + egui::vec2(2.0, -2.0),
        rect.min + egui::vec2(6.0, 6.0),
        rect.right_bottom() + egui::vec2(-2.0, -2.0),
    ].to_vec(), egui::Stroke::new(1.0, ink)));
    for axis in [egui::Vec2::X, egui::Vec2::Y] {
        painter.line_segment([point - axis * 4.0, point + axis * 4.0], egui::Stroke::new(3.0, edge));
        painter.line_segment([point - axis * 4.0, point + axis * 4.0], egui::Stroke::new(1.0, ink));
    }
}
