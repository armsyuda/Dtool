use std::collections::HashMap;
use std::sync::mpsc::TryRecvError;

use dtool_core::commands::symbols;
use dtool_core::Session;
use dtool_core::workspace::{ContextId, Workspace};
use dtool_protocol::{Command, CommandError};
use dtool_strings::text;
use serde_json::json;

pub const PANEL_KEY: &str = "symbols";
pub const WINDOW_PANEL: &str = "window.vector.symbols";
const PREVIEW_SIDE: f32 = 32.0;
type PreviewStamp = (ContextId, u64, u64);
type PreviewJob = super::ThumbnailJob<PreviewStamp, Vec<(u64, Option<egui::ColorImage>)>>;

struct CachedPreview {
    stamp: PreviewStamp,
    texture: Option<egui::TextureHandle>,
}

fn render_preview(definition: &symbols::Definition, units: dtool_core::document::Units, dpi: f32) -> Option<egui::ColorImage> {
    use dtool_export::{ExportArea, ExportContext};

    let (doc, selection) = definition.preview_document(units, dpi);
    let context = ExportContext { selection, ..Default::default() };
    let areas = dtool_export::resolve_areas(&doc, &ExportArea::SelectionWithoutBackground, &context, None).ok()?;
    let ExportArea::Rect([x, y, w, h]) = areas.first()?.area else { return None };
    let scale = (PREVIEW_SIDE - 6.0) * 2.0 / w.max(h);
    let options = dtool_export::raster::ExportOptions {
        area: ExportArea::Rect([x, y, w, h]),
        width: Some((w * scale).round().max(1.0) as u32),
        height: Some((h * scale).round().max(1.0) as u32),
        ..Default::default()
    };
    let image = dtool_export::raster::render_preview(&doc, &options).ok()?;
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize], image.as_raw(),
    ))
}

#[derive(Clone, Copy)]
struct SymbolDrag {
    context: ContextId,
    definition: u64,
    start: egui::Pos2,
    viewport: egui::ViewportId,
}

#[derive(Clone, Copy)]
struct NativeDrag {
    payload: SymbolDrag,
    pointer: egui::Pos2,
    released: bool,
}

fn native_drag_key() -> egui::Id {
    egui::Id::unique("dtool.symbols.native_drag")
}

pub(crate) fn clear_native_drag_frame(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<NativeDrag>(native_drag_key()));
    if let Some(payload) = egui::DragAndDrop::payload::<SymbolDrag>(ctx)
        && payload.viewport != egui::ViewportId::ROOT
        && !ctx.input(|input| input.raw.viewports.contains_key(&payload.viewport))
    {
        egui::DragAndDrop::clear_payload(ctx);
    }
}

pub(crate) fn dragging(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<NativeDrag>(native_drag_key()).is_some())
        || egui::DragAndDrop::payload::<SymbolDrag>(ctx).is_some_and(|drag| drag.viewport == ctx.viewport_id())
}

fn primary_release(input: &egui::InputState) -> Option<egui::Pos2> {
    input.events.iter().rev().find_map(|event| match event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, .. } => Some(*pos),
        _ => None,
    })
}

fn relay_native_drag(ui: &egui::Ui) {
    let ctx = ui.ctx();
    let source = ctx.viewport_id();
    if source == egui::ViewportId::ROOT {
        return;
    }
    let Some(payload) = egui::DragAndDrop::payload::<SymbolDrag>(ctx).filter(|drag| drag.viewport == source) else { return };
    // Windows sends captured pointer events to the floating window, including
    // the release over the main window. Translate before egui clears the payload.
    let source_scale = ctx.pixels_per_point();
    let zoom = ctx.zoom_factor();
    let (pointer, release, active) = ui.input(|input| {
        let release = primary_release(input);
        (release.or_else(|| input.pointer.interact_pos()), release,
            input.focused && !input.viewport().close_requested()
                && (release.is_some() || input.pointer.primary_down()))
    });
    if !active {
        egui::DragAndDrop::clear_payload(ctx);
        return;
    }
    let Some(pointer) = pointer.filter(|point| payload.start.distance(*point) >= 6.0) else { return };
    let drag = ui.input(|input| {
        let screen = (input.viewport().inner_rect?.min + pointer.to_vec2()).to_vec2() * source_scale;
        let root = input.raw.viewports.get(&egui::ViewportId::ROOT)?;
        let in_floating_window = input.raw.viewports.iter().any(|(id, viewport)| {
            if *id == egui::ViewportId::ROOT { return false; }
            let scale = viewport.native_pixels_per_point.unwrap_or(1.0) * zoom;
            viewport.outer_rect.or(viewport.inner_rect)
                .is_some_and(|rect| rect.contains((screen / scale).to_pos2()))
        });
        if in_floating_window { return None; }
        let root_scale = root.native_pixels_per_point.unwrap_or(1.0) * zoom;
        let pointer = (screen / root_scale).to_pos2() - root.inner_rect?.min;
        Some(NativeDrag { payload: *payload, pointer: pointer.to_pos2(), released: release.is_some() })
    });
    if let Some(drag) = drag {
        ctx.data_mut(|data| data.insert_temp(native_drag_key(), drag));
        ctx.set_cursor_icon(egui::CursorIcon::Copy);
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

pub(crate) fn canvas_drop(
    ui: &egui::Ui, response: &egui::Response, context: ContextId, session: &mut Session,
) -> Option<CommandError> {
    let ctx = ui.ctx();
    let native = ctx.data_mut(|data| {
        let native = data.get_temp::<NativeDrag>(native_drag_key());
        data.remove::<NativeDrag>(native_drag_key());
        native
    });
    let (payload, pointer, released) = if let Some(native) = native {
        (native.payload, native.pointer, native.released)
    } else {
        let payload = egui::DragAndDrop::payload::<SymbolDrag>(ctx).filter(|drag| drag.viewport == ctx.viewport_id())?;
        let (release, pointer) = ui.input(|input| (primary_release(input), input.pointer.interact_pos()));
        let pointer = release.or(pointer)?;
        if payload.start.distance(pointer) < 6.0 { return None; }
        (*payload, pointer, release.is_some())
    };
    if payload.context != context {
        return None;
    }
    let definition = session.document.symbols.definitions.iter().find(|entry| entry.id == payload.definition)?;
    let size = definition.size();
    let rect = response.rect;
    if !response.interact_rect.contains(pointer) || ctx.layer_id_at(pointer) != Some(response.layer_id)
    {
        return None;
    }
    let center = dtool_core::view_to_doc(&session.view, [pointer.x - rect.min.x, pointer.y - rect.min.y]);
    if released {
        egui::DragAndDrop::clear_payload(ctx);
        let command = Command::with_params("symbol.place", json!({"definition": payload.definition, "center": center}));
        response.request_focus();
        ui.ctx().request_repaint();
        return session.run(&command).err();
    }
    let corners = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|[x, y]| {
        let point = [center[0] + x * size[0] * 0.5, center[1] + y * size[1] * 0.5];
        let [x, y] = dtool_core::doc_to_view(&session.view, point);
        rect.min + egui::vec2(x, y)
    });
    let selection = ui.visuals().selection;
    ui.painter_at(rect).add(egui::Shape::convex_polygon(
        corners.to_vec(), selection.bg_fill.gamma_multiply(0.18), selection.stroke,
    ));
    ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
    None
}

#[derive(Default)]
pub struct SymbolsPanel {
    name: String,
    fill: String,
    error: Option<String>,
    rename: Option<(ContextId, u64, String)>,
    preview_stamp: Option<PreviewStamp>,
    previews: HashMap<u64, CachedPreview>,
    preview_job: Option<PreviewJob>,
}

impl SymbolsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut Workspace) {
        let Some(context) = workspace.active() else { return };
        let Some(session) = workspace.session_mut() else { return };
        let stamp = (context, session.generation, session.document.version);
        if self.preview_stamp.map(|previous| previous.0) != Some(context) {
            self.previews.clear();
        } else if self.preview_stamp != Some(stamp) {
            self.previews.retain(|id, _| session.document.symbols.definitions.iter().any(|entry| entry.id == *id));
        }
        if self.preview_stamp != Some(stamp) {
            self.preview_stamp = Some(stamp);
        }
        if let Some(job) = &self.preview_job {
            match job.result.try_recv() {
                Ok(images) => {
                    if job.revision == stamp {
                        self.previews.extend(images.into_iter().map(|(id, image)| {
                            let texture = image.map(|image| ui.ctx().load_texture(
                                format!("symbol.{}.{id}", context.0), image, egui::TextureOptions::LINEAR,
                            ));
                            (id, CachedPreview { stamp, texture })
                        }));
                    }
                    self.preview_job = None;
                }
                Err(TryRecvError::Disconnected) => self.preview_job = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        let link = symbols::selection_link(session);
        let selected = session.selection.ids.first().and_then(|id| session.document.get_object(*id));
        let has_story = selected.is_some_and(|object| object.story_id().is_some());
        let has_curve = selected.is_some_and(|object| object.curve().is_some());
        let create = Command::with_params("symbol.create", json!({"name": self.name}));
        let can_create = symbols::can_execute(session, &create) == Some(true);
        if self.rename.as_ref().is_some_and(|(owner, id, _)| {
            *owner != context || !session.document.symbols.definitions.iter().any(|entry| entry.id == *id)
        }) {
            self.rename = None;
        }
        let mut action = None;
        let mut sync = session.symbol_sync;
        if dtool_design::toggle(ui, &mut sync, text("symbols.sync")).changed() {
            action = Some(("symbol.sync", json!({"enabled": sync})));
        }

        ui.horizontal(|ui| {
            let caption = text("symbols.create");
            let font = egui::TextStyle::Button.resolve(ui.style());
            let caption_width = ui.fonts_mut(|fonts| fonts.layout_no_wrap(caption.to_string(), font, egui::Color32::WHITE).size().x);
            let spare = 2.0 * ui.spacing().button_padding.x + ui.spacing().item_spacing.x + 8.0;
            let field_width = (ui.available_width() - caption_width - spare).clamp(24.0, 105.0);
            ui.add(egui::TextEdit::singleline(&mut self.name).hint_text(text("symbols.name")).desired_width(field_width));
            if ui.add_enabled(can_create, egui::Button::new(caption)).clicked() {
                action = Some(("symbol.create", json!({"name": self.name})));
            }
        });

        let mut pending = Vec::new();
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            if session.document.symbols.definitions.is_empty() {
                ui.weak(text("symbols.empty"));
            }
            for definition in &session.document.symbols.definitions {
                ui.push_id(definition.id, |ui| ui.horizontal(|ui| {
                    let editor_id = egui::Id::unique(("symbols.rename", context, definition.id));
                    if let Some((_, id, name)) = &mut self.rename && *id == definition.id {
                        let buttons_width = 88.0 + ui.spacing().item_spacing.x * 2.0;
                        let input_width = (ui.available_width() - buttons_width).max(24.0);
                        let editor = ui.add(egui::TextEdit::singleline(name).id(editor_id)
                            .desired_width(input_width).hint_text(text("symbols.name")));
                        let valid = !name.trim().is_empty();
                        let confirmed = ui.add_enabled(valid, egui::Button::new(text("common.ok"))).clicked()
                            || (valid && editor.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
                        if confirmed {
                            action = Some(("symbol.rename", json!({"definition": id, "name": name.trim()})));
                        }
                        if confirmed || ui.button(text("button.cancel")).clicked()
                            || (editor.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)))
                        {
                            self.rename = None;
                        }
                        return;
                    }
                    if ui.button(text("symbols.place")).clicked() {
                        action = Some(("symbol.place", json!({"definition": definition.id})));
                    }
                    let label = ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(PREVIEW_SIDE), egui::Sense::hover());
                        if ui.is_rect_visible(rect) {
                            ui.painter().rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
                            let preview = self.previews.get(&definition.id);
                            if let Some(texture) = preview.and_then(|entry| entry.texture.as_ref()) {
                                let size = texture.size_vec2() * ((PREVIEW_SIDE - 6.0) / texture.size_vec2().max_elem());
                                let bounds = egui::Rect::from_center_size(rect.center(), size);
                                ui.painter().image(texture.id(), bounds, egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                            }
                            if self.preview_job.is_none() && preview.is_none_or(|entry| entry.stamp != stamp) {
                                pending.push(definition);
                            }
                        }
                        ui.add(egui::Label::new(&definition.name).truncate());
                    }).response.interact(egui::Sense::click_and_drag())
                        .on_hover_text(format!("{}\n{}", definition.name, text("symbols.drag_to_canvas")))
                        .on_hover_cursor(egui::CursorIcon::Grab);
                    label.context_menu(|ui| {
                        if ui.button(text("symbols.rename")).clicked() {
                            self.rename = Some((context, definition.id, definition.name.clone()));
                            ui.memory_mut(|memory| memory.request_focus(editor_id));
                            ui.close();
                        }
                        if ui.button(text("symbols.delete")).on_hover_text(text("symbols.delete_hint")).clicked() {
                            action = Some(("symbol.delete", json!({"definition": definition.id})));
                            ui.close();
                        }
                    });
                    let press = ui.input(|input| input.events.iter().find_map(|event| match event {
                        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. }
                            if label.interact_rect.contains(*pos) => Some(*pos),
                        _ => None,
                    }));
                    if let Some(start) = press && ui.ctx().layer_id_at(start) == Some(label.layer_id) {
                        egui::DragAndDrop::set_payload(ui.ctx(), SymbolDrag {
                            context, definition: definition.id, start, viewport: ui.ctx().viewport_id(),
                        });
                    }
                }));
            }
        });
        if !pending.is_empty() {
            let definitions: Vec<_> = pending.into_iter().take(4).cloned().collect();
            let (units, dpi) = (session.document.units, session.document.dpi);
            self.preview_job = PreviewJob::start(ui.ctx(), "dtool-symbol-previews", stamp, move || {
                definitions.iter().map(|definition| (definition.id, render_preview(definition, units, dpi))).collect()
            }).ok();
        }
        relay_native_drag(ui);

        if let Some((instance, definition, node)) = link {
            ui.separator();
            if symbols::has_structure_override(session, instance)
                && dtool_design::button(ui, text("symbols.revert_structure")).clicked()
            {
                action = Some(("symbol.revert", json!({"instance": instance, "property": "structure"})));
            }
            if dtool_design::button(ui, text("symbols.detach")).clicked() {
                action = Some(("symbol.detach", json!({"instance": instance})));
            }
            if let Some(node) = node {
                ui.add(egui::TextEdit::singleline(&mut self.fill).hint_text("#RRGGBB").desired_width(110.0));
                ui.vertical(|ui| {
                    let set = json!({"fill": self.fill.trim()});
                    if dtool_design::button(ui, text("symbols.shared_fill")).clicked() {
                        action = Some(("symbol.edit", json!({"definition": definition, "node": node, "set": set})));
                    }
                    if dtool_design::button(ui, text("symbols.local_fill")).clicked() {
                        action = Some(("symbol.override", json!({"instance": instance, "node": node, "set": set})));
                    }
                    if dtool_design::button(ui, text("symbols.revert_fill")).clicked() {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node})));
                    }
                    if dtool_design::button(ui, text("symbols.revert_stroke")).clicked() {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node, "property": "stroke"})));
                    }
                    if has_story && dtool_design::button(ui, text("symbols.revert_text")).clicked() {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node, "property": "text"})));
                    }
                    if symbols::has_corner_override(session, instance, node)
                        && dtool_design::button(ui, text("symbols.revert_corner")).clicked()
                    {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node, "property": "corner"})));
                    }
                    if symbols::constraints::has_override(session, instance, node)
                        && dtool_design::button(ui, text("symbols.revert_constraint")).clicked()
                    {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node, "property": "constraint"})));
                    }
                    if has_curve && dtool_design::button(ui, text("symbols.revert_curve")).clicked() {
                        action = Some(("symbol.revert", json!({"instance": instance, "node": node, "property": "curve"})));
                    }
                });
            }
        }

        if let Some((name, params)) = action {
            self.error = session.run(&Command::with_params(name, params)).err().map(|error| error.message_key);
        }
        if let Some(error) = &self.error {
            ui.label(text(error));
        }
    }
}
