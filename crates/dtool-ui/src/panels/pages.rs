use std::collections::HashMap;
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

use dtool_core::commands::masters::master_name;
use dtool_core::document::PageId;
use dtool_core::page_fold::{FoldAxis, PageFold};
use dtool_core::ObjectId;
use dtool_strings::text;
use dtool_core::scene::{Scene, build_scene, rasterize_scene};
use dtool_core::view::center_region;
use dtool_core::{CanvasFlags, Command, Session};
use egui::{Color32, ColorImage, TextureHandle, TextureOptions, vec2};
use serde_json::json;

type PageRevision = (Option<dtool_core::ContextId>, u64, u64);
const TEXT_THUMBNAIL_DELAY: Duration = Duration::from_millis(200);

type PageThumbnailJob = super::ThumbnailJob<PageRevision, Vec<(PageId, ColorImage)>>;

struct MasterDraft {
    id: Option<ObjectId>,
    name: String,
    size: [f32; 2],
}

struct MasterRangeDraft {
    master: ObjectId,
    pages: String,
    replace: bool,
    keep_previous: bool,
}

struct PageSettingsDraft {
    id: PageId,
    size: [f32; 2],
    fold: Option<PageFold>,
    dimension_fields: [egui::Id; 2],
    invalid_dimensions: [bool; 2],
}

struct SpreadDraft {
    start: PageId,
    count: usize,
    custom: bool,
}

pub(crate) fn render_page_thumbnail(
    ctx: &egui::Context,
    doc: &dtool_core::Document,
    scene: &mut Option<Scene>,
    index: usize,
    bounds: [f32; 4],
    resolution: f32,
) -> TextureHandle {
    upload_thumbnail(ctx, doc.pages[index].id, page_thumbnail_image(doc, scene, index, bounds, resolution))
}

fn page_thumbnail_image(
    doc: &dtool_core::Document, scene: &mut Option<Scene>, index: usize, bounds: [f32; 4], resolution: f32,
) -> ColorImage {
    let [_, _, width, height] = bounds;
    let scale = resolution / width.max(height);
    let w = (width * scale).round().max(1.0) as u32;
    let h = (height * scale).round().max(1.0) as u32;
    let mut scoped = scene.get_or_insert_with(|| build_scene(doc, &CanvasFlags::default())).clone();
    let members = dtool_export::page_members(doc, &[index]);
    dtool_export::retain_page_scene(&mut scoped, &[index], &members);
    let pixels = rasterize_scene(doc, &scoped, bounds, w, h, None);
    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels)
}

fn upload_thumbnail(ctx: &egui::Context, page: PageId, image: ColorImage) -> TextureHandle {
    ctx.load_texture(format!("page-thumbnail-{}", page.0), image, TextureOptions::LINEAR)
}

#[derive(Default)]
pub struct PagesPanel {
    revision: Option<PageRevision>,
    pending_text_thumbnails: Option<(PageRevision, Instant)>,
    background_thumbnails: bool,
    thumbnail_job: Option<PageThumbnailJob>,
    thumbnails: HashMap<PageId, (PageRevision, TextureHandle)>,
    rename: Option<(PageId, String)>,
    drag: Option<(PageId, egui::Pos2)>,
    delete_page: Option<PageId>,
    page_settings: Option<PageSettingsDraft>,
    spread_settings: Option<SpreadDraft>,
    masters_open: bool,
    master_draft: Option<MasterDraft>,
    master_range: Option<MasterRangeDraft>,
}

impl PagesPanel {
    pub fn show_master_tab(&mut self, masters: bool) {
        self.masters_open = masters;
    }

    fn prepare_thumbnails(&mut self, ctx: &egui::Context, revision: PageRevision, session: &Session) -> bool {
        if self.revision == Some(revision) {
            self.pending_text_thumbnails = None;
            return false;
        }
        let same_document = self.revision.is_some_and(|old| old.0 == revision.0);
        let editing = session.text_edit.is_some();
        let after_text_edit = same_document && (editing || self.pending_text_thumbnails.is_some());
        if editing && same_document {
            let now = Instant::now();
            let (pending, deadline) = self.pending_text_thumbnails.get_or_insert((revision, now + TEXT_THUMBNAIL_DELAY));
            if *pending != revision {
                *pending = revision;
                *deadline = now + TEXT_THUMBNAIL_DELAY;
            }
            if let Some(remaining) = deadline.checked_duration_since(now) {
                ctx.request_repaint_after(remaining);
                return true;
            }
        }
        self.pending_text_thumbnails = None;
        self.revision = Some(revision);
        self.background_thumbnails = after_text_edit || !session.document.pixel_layers.is_empty();
        if !same_document || !self.background_thumbnails {
            self.thumbnails.clear();
        }
        false
    }

    fn collect_thumbnails(&mut self, ctx: &egui::Context, revision: PageRevision) {
        let Some(job) = &self.thumbnail_job else { return };
        match job.result.try_recv() {
            Ok(images) => {
                if job.revision == revision {
                    for (id, image) in images {
                        self.thumbnails.insert(id, (revision, upload_thumbnail(ctx, id, image)));
                    }
                }
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) if job.revision == revision => {
                self.background_thumbnails = false;
            }
            Err(TryRecvError::Disconnected) => {}
        }
        self.thumbnail_job = None;
    }

    fn start_thumbnails(
        &mut self, ctx: &egui::Context, doc: &dtool_core::Document,
        revision: PageRevision, requests: Vec<(usize, [f32; 4])>,
    ) {
        if requests.is_empty() || self.thumbnail_job.is_some() {
            return;
        }
        let doc = doc.clone();
        let worker = PageThumbnailJob::start(ctx, "dtool-page-thumbnails", revision, move || {
            let mut scene = None;
            requests.into_iter().map(|(index, bounds)| {
                (doc.pages[index].id, page_thumbnail_image(&doc, &mut scene, index, bounds, 144.0))
            }).collect()
        });
        if let Ok(job) = worker {
            self.thumbnail_job = Some(job);
        } else {
            self.background_thumbnails = false;
            ctx.request_repaint();
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, session: Option<&Session>, view_size: [f32; 2]) -> Option<Command> {
        let Some(session) = session else {
            self.revision = None;
            self.pending_text_thumbnails = None;
            self.background_thumbnails = false;
            self.rename = None;
            self.drag = None;
            self.delete_page = None;
            self.page_settings = None;
            self.spread_settings = None;
            self.master_draft = None;
            self.master_range = None;
            self.thumbnails.clear();
            ui.weak(dtool_strings::text("app.no_document"));
            return None;
        };
        let revision = (session.context, session.generation, session.document.version);
        if self.revision.map(|(context, _, _)| context) != Some(session.context) {
            self.rename = None;
            self.drag = None;
            self.delete_page = None;
            self.page_settings = None;
            self.spread_settings = None;
            self.master_draft = None;
            self.master_range = None;
        }
        let thumbnails_deferred = self.prepare_thumbnails(ui.ctx(), revision, session);
        self.collect_thumbnails(ui.ctx(), revision);

        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.masters_open, false, text("panel.pages"));
            ui.selectable_value(&mut self.masters_open, true, text("panel.master"));
        });
        if self.masters_open {
            self.drag = None;
            return self.show_masters(ui, session, view_size);
        }

        let doc = &session.document;
        let can_navigate = view_size.iter().all(|side| side.is_finite() && *side > 1.0);
        let current = dtool_core::commands::pages::viewed_page(session);
        if self.rename.as_ref().is_some_and(|(id, _)| !doc.pages.iter().any(|page| page.id == *id)) {
            self.rename = None;
        }
        let mut command = None;
        ui.horizontal(|ui| {
            if ui.button("+").on_hover_text(dtool_strings::text("panel.pages.add")).clicked() {
                command = Some(Command::with_params("page.add", json!({})));
            }
        });
        if let Some((id, name)) = &mut self.rename {
            let id = *id;
            let mut close = false;
            ui.horizontal(|ui| {
                let editor = egui::TextEdit::singleline(name)
                    .char_limit(128)
                    .hint_text(dtool_strings::text("panel.pages.rename"));
                let remaining = (ui.available_width() - 52.0).max(1.0);
                let response = ui.add_sized([remaining, 20.0], editor);
                let enter = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                let confirm = ui.button("✓").on_hover_text(dtool_strings::text("common.ok")).clicked();
                if enter || confirm {
                    command = Some(Command::with_params("page.rename", json!({"id": id, "name": name})));
                    close = true;
                }
                close |= ui.button("×").clicked()
                    || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)));
            });
            if close {
                self.rename = None;
            }
        }
        let mut chosen = None;
        let mut scene = None;
        let mut visible = Vec::new();
        let mut thumbnail_requests = Vec::new();
        let spreads = dtool_core::data_merge::layout::spread_ranges(doc);
        let (pressed, released) = crate::input::primary_button_positions(ui);
        if pressed.is_some() || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.drag = None;
        }

        const ROW_HEIGHT: f32 = 124.0;
        const THUMB_BOX_HEIGHT: f32 = 97.0;
        let facing = doc.setup.facing_pages;
        let right_cover = facing && doc.setup.start_on_right;
        let row_count = if doc.pages.is_empty() { 0 } else { spreads.len() };
        egui::ScrollArea::vertical().show_rows(ui, ROW_HEIGHT, row_count, |ui, rows| {
            let mut drop_targets = Vec::new();
            let rects = dtool_core::scene::page_rects(doc);
            let mut show_page = |ui: &mut egui::Ui, index: usize, spread: &std::ops::Range<usize>,
                                 cell_width: f32, columns: usize, align: egui::Align,
                                 scale: f32, top_inset: f32| {
                let page = &doc.pages[index];
                let [x, y, width, height] = rects[index];
                if ![x, y, width, height].iter().all(|n| n.is_finite()) || width <= 0.0 || height <= 0.0 {
                    ui.allocate_space(vec2(cell_width, ROW_HEIGHT));
                    return;
                }
                visible.push(page.id);
                if !thumbnails_deferred && self.thumbnails.get(&page.id).is_none_or(|(stamp, _)| *stamp != revision) {
                    let bounds = [x, y, width, height];
                    if self.background_thumbnails {
                        thumbnail_requests.push((index, bounds));
                    } else {
                        let texture = render_page_thumbnail(ui.ctx(), doc, &mut scene, index, bounds, 144.0);
                        self.thumbnails.insert(page.id, (revision, texture));
                    }
                }
                let texture = self.thumbnails.get(&page.id).map(|(_, handle)| handle);

                let number = dtool_core::longdoc::sections::section_page_label(doc, index);
                let mut label = if page.name.is_empty() {
                    dtool_strings::format("panel.pages.number", &[("number", &number)])
                } else {
                    format!("{number} · {}", page.name)
                };
                if page.spread_group.is_some() && spread.len() > 1 {
                    let first = dtool_core::longdoc::sections::section_page_label(doc, spread.start);
                    let last = dtool_core::longdoc::sections::section_page_label(doc, spread.end - 1);
                    label.push_str(&format!("  [{first}–{last}]"));
                }
                let selected = current == Some(page.id);
                let thumb_size = vec2(width * scale, height * scale);
                let row = ui.allocate_ui_with_layout(
                    vec2(cell_width, ROW_HEIGHT),
                    egui::Layout::top_down(align),
                    |ui| {
                        ui.set_max_width(cell_width);
                        let (rect, _) = ui.allocate_exact_size(vec2(thumb_size.x, THUMB_BOX_HEIGHT), egui::Sense::hover());
                        let thumb = egui::Rect::from_min_size(rect.min + vec2(0.0, top_inset), thumb_size);
                        ui.painter().rect_filled(thumb, 0.0, Color32::WHITE);
                        if let Some(texture) = texture {
                            ui.painter().image(
                                texture.id(), thumb,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE,
                            );
                        }
                        ui.add(egui::Button::selectable(selected, &label).truncate().sense(egui::Sense::hover())).on_hover_text(
                            doc.master_pages.for_page(page.id)
                                .and_then(|item| doc.get_object(item.master))
                                .map_or_else(String::new, |root| format!("{} · {}", dtool_strings::text("panel.master"), master_name(root)))
                        );
                    },
                );
                let response = ui.interact(row.response.rect, ui.id().with(("page", page.id.0)), egui::Sense::click_and_drag());
                let hitbox = response.interact_rect.intersect(ui.clip_rect());
                drop_targets.push((index, response.rect, hitbox, columns > 1));
                if let Some(point) = pressed.filter(|point| response.enabled() && hitbox.contains(*point)
                    && ui.ctx().layer_id_at(*point) == Some(response.layer_id)
                    && !egui::Popup::is_any_open(ui.ctx()))
                {
                    self.drag = Some((page.id, point));
                }
                response.context_menu(|ui| {
                    if doc.layout_artboard_origins.is_some()
                        && !(index == 0 && dtool_core::data_merge::layout::has_right_cover(doc))
                        && ui.button(text("panel.pages.spread_settings")).clicked()
                    {
                        let range = spread.clone();
                        let custom = page.spread_group.is_some();
                        let start = if custom { range.start } else { index };
                        self.spread_settings = Some(SpreadDraft {
                            start: doc.pages[start].id,
                            count: range.end - start,
                            custom,
                        });
                        ui.close();
                    }
                    let settings = egui::Button::new(text("panel.pages.settings"));
                    if ui.add_enabled(dtool_core::commands::pages::can_resize(doc, page.id), settings).clicked() {
                        let dimension_fields = ["width", "height"].map(|axis| ui.id().with(("page.settings.size", page.id, axis)));
                        for id in dimension_fields { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
                        self.page_settings = Some(PageSettingsDraft {
                            id: page.id, size: [width, height], fold: page.fold, dimension_fields, invalid_dimensions: [false; 2],
                        });
                        ui.close();
                    }
                    if ui.button(dtool_strings::text("panel.pages.rename")).clicked() {
                        self.rename = Some((page.id, page.name.clone()));
                        ui.close();
                    }
                    if ui.button(dtool_strings::text("panel.pages.insert_before")).clicked() {
                        command = Some(Command::with_params("page.add", json!({"before_id": page.id})));
                        ui.close();
                    }
                    if ui.add_enabled(index > 0, egui::Button::new(dtool_strings::text("panel.pages.move_up"))).clicked() {
                        command = Some(move_page(page.id, index));
                        ui.close();
                    }
                    if ui.add_enabled(index + 1 < doc.pages.len(), egui::Button::new(dtool_strings::text("panel.pages.move_down"))).clicked() {
                        command = Some(move_page(page.id, index + 2));
                        ui.close();
                    }
                    if ui.button(dtool_strings::text("panel.pages.duplicate")).clicked() {
                        command = Some(Command::with_params("page.duplicate", json!({"id": page.id})));
                        ui.close();
                    }
                    if ui.add_enabled(doc.pages.len() > 1, egui::Button::new(dtool_strings::text("panel.pages.delete"))).clicked() {
                        self.delete_page = Some(page.id);
                        ui.close();
                    }
                    if let Some(current) = doc.master_pages.for_page(page.id) {
                        if doc.master_pages.roots.iter().any(|&id| id != current.master && doc.get_object(id).is_some()) {
                            ui.menu_button(text("panel.master.replace"), |ui| {
                                for master in doc.master_pages.roots.iter().filter(|&&id| id != current.master).filter_map(|id| doc.get_object(*id)) {
                                    ui.menu_button(master_name(master), |ui| {
                                        for (keep_previous, label) in [(false, "panel.master.replace_remove"), (true, "panel.master.replace_keep")] {
                                            if ui.button(text(label)).clicked() {
                                                command = Some(Command::with_params("master.apply", json!({
                                                    "master_id": master.id, "page_id": page.id, "replace": true, "keep_previous": keep_previous
                                                })));
                                                ui.close();
                                            }
                                        }
                                    });
                                }
                            });
                        }
                        if ui.button(dtool_strings::text("panel.master.detach")).clicked() {
                            command = Some(Command::with_params("master.detach", json!({"page_id": page.id})));
                            ui.close();
                        }
                    } else if !doc.master_pages.roots.is_empty() {
                        ui.menu_button(dtool_strings::text("panel.master.apply"), |ui| {
                            for master in doc.master_pages.roots.iter().filter_map(|id| doc.get_object(*id)) {
                                if ui.button(master_name(master)).clicked() {
                                    command = Some(Command::with_params("master.apply", json!({"master_id": master.id, "page_id": page.id})));
                                    ui.close();
                                }
                            }
                        });
                    }
                });
                if can_navigate && response.clicked() {
                    chosen = Some(page.id);
                }
            };
            for row in rows {
                let spread = &spreads[row];
                let columns = spread.len().max(if facing { 2 } else { 1 });
                let gap = 5.0_f32.min(ui.available_width() / (2 * columns) as f32);
                let width = ui.available_width();
                let half = (width - gap) * 0.5;
                let cell = |column| {
                    let (width, align) = if !facing {
                        ((width - gap * (columns - 1) as f32) / columns as f32, egui::Align::Center)
                    } else if column == 0 {
                        (half, egui::Align::Max)
                    } else {
                        ((half - gap * (columns - 2) as f32) / (columns - 1) as f32, egui::Align::Min)
                    };
                    (width.max(1.0), align)
                };
                let leading_blank = usize::from(right_cover && row == 0 && spread.len() == 1);
                let mut scale = f32::INFINITY;
                let mut tallest = 0.0_f32;
                for (offset, index) in spread.clone().enumerate() {
                    let [_, _, w, h] = rects[index];
                    if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 {
                        let (cell_width, _) = cell(offset + leading_blank);
                        let max_width = (cell_width - 4.0).clamp(1.0, 104.0);
                        scale = scale.min(max_width / w).min(94.0 / h);
                        tallest = tallest.max(h);
                    }
                }
                let scale = if scale.is_finite() { scale } else { 1.0 };
                let top_inset = (THUMB_BOX_HEIGHT - tallest * scale) * 0.5;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for column in 0..columns {
                        let (cell_width, align) = cell(column);
                        if let Some(index) = column.checked_sub(leading_blank).and_then(|offset| spread.clone().nth(offset)) {
                            show_page(ui, index, spread, cell_width, columns, align, scale, top_inset);
                        } else {
                            ui.allocate_space(vec2(cell_width, ROW_HEIGHT));
                        }
                    }
                });
            }
            let pointer = released.or_else(|| ui.input(|input| input.pointer.interact_pos()));
            if let Some((id, start)) = self.drag
                && let Some(point) = pointer.filter(|point| start.distance(*point) >= 4.0)
                && let Some(from) = doc.pages.iter().position(|page| page.id == id)
                && let Some((index, rect, _, horizontal)) = drop_targets.iter().find(|(_, _, hitbox, horizontal)| {
                    let expansion = if *horizontal { vec2(2.0, 0.0) } else { vec2(0.0, 2.0) };
                    hitbox.expand2(expansion).contains(point)
                })
            {
                let after = if *horizontal { point.x >= rect.center().x } else { point.y >= rect.center().y };
                let insertion = index + usize::from(after);
                let to = insertion - usize::from(from < insertion);
                if from != to {
                    let stroke = ui.visuals().selection.stroke;
                    if *horizontal {
                        ui.painter().vline(if after { rect.right() } else { rect.left() }, rect.y_range(), stroke);
                    } else {
                        ui.painter().hline(rect.x_range(), if after { rect.bottom() } else { rect.top() }, stroke);
                    }
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    if released.is_some() && command.is_none() {
                        command = Some(move_page(id, to + 1));
                    }
                }
                if released.is_none() {
                    let clip = ui.clip_rect();
                    let scroll = (clip.top() + 24.0 - point.y).max(0.0) - (point.y - (clip.bottom() - 24.0)).max(0.0);
                    if scroll != 0.0 {
                        ui.scroll_with_delta_animation(vec2(0.0, scroll), egui::style::ScrollAnimation::none());
                        ui.ctx().request_repaint();
                    }
                }
            }
        });
        self.thumbnails.retain(|id, _| visible.contains(id));
        self.start_thumbnails(ui.ctx(), doc, revision, thumbnail_requests);
        if released.is_some() || !ui.input(|input| input.pointer.primary_down()) {
            self.drag = None;
        }

        if let Some(command) = self.show_page_settings(ui, doc) {
            return Some(command);
        }
        if let Some(command) = self.show_spread_settings(ui, doc) {
            return Some(command);
        }

        if let Some(id) = self.delete_page {
            use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
            if let Some(index) = doc.pages.iter().position(|page| page.id == id) {
                let (event, (remove, cancel)) = show_modal(ui.ctx(), "dtool.page.delete",
                    text("panel.pages.delete"), ClosePolicy::CancelOnly, false, |ui| {
                        ui.label(dtool_strings::format("panel.pages.delete_confirm", &[("number", &(index + 1).to_string())]));
                        ui.horizontal(|ui| {
                            let remove = ui.button(text("panel.pages.delete")).clicked();
                            let cancel = ui.button(text("panel.master.cancel")).clicked();
                            (remove, cancel)
                        }).inner
                    });
                if remove {
                    command = Some(Command::with_params("page.delete", json!({"id": id})));
                }
                if remove || cancel || event == DialogResult::Cancel {
                    self.delete_page = None;
                }
            } else {
                self.delete_page = None;
            }
        }
        if command.is_some() {
            return command;
        }
        Some(Command::with_params("page.navigate", json!({"id": chosen?})))
    }

    fn show_spread_settings(&mut self, ui: &egui::Ui, doc: &dtool_core::Document) -> Option<Command> {
        use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};

        let draft = self.spread_settings.as_mut()?;
        let Some(start) = doc.pages.iter().position(|page| page.id == draft.start) else {
            self.spread_settings = None;
            return None;
        };
        let (event, (accept, cancel)) = show_modal(ui.ctx(), "dtool.spread.settings",
            text("panel.pages.spread_settings"), ClosePolicy::CancelOnly, true, |ui| {
                let number = dtool_core::longdoc::sections::section_page_label(doc, start);
                ui.label(dtool_strings::format("panel.pages.number", &[("number", &number)]));
                ui.checkbox(&mut draft.custom, text("panel.pages.spread_custom"));
                ui.horizontal(|ui| {
                    ui.label(text("dialog.new_document.pages"));
                    let count = egui::DragValue::new(&mut draft.count).range(1..=doc.pages.len() - start);
                    ui.add_enabled(draft.custom, count);
                });
                ui.horizontal(|ui| {
                    let accept = ui.button(text("common.ok")).clicked();
                    let cancel = ui.button(text("button.cancel")).clicked();
                    (accept, cancel)
                }).inner
            });
        if accept || event == DialogResult::Confirm {
            let draft = self.spread_settings.take()?;
            let count = draft.custom.then_some(draft.count);
            return Some(Command::with_params("page.spread.set", json!({"id": draft.start, "count": count})));
        }
        if cancel || event == DialogResult::Cancel {
            self.spread_settings = None;
        }
        None
    }

    fn show_page_settings(&mut self, ui: &egui::Ui, doc: &dtool_core::Document) -> Option<Command> {
        use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};

        let draft = self.page_settings.as_mut()?;
        let Some(index) = doc.pages.iter().position(|page| page.id == draft.id) else {
            self.page_settings = None;
            return None;
        };
        let unit = crate::captions::units_identity(doc.units);
        let can_resize = dtool_core::commands::pages::can_resize(doc, draft.id);
        let ids = draft.dimension_fields;
        let dimensions_valid = |size: [f32; 2]| size.iter().all(|side| (0.01..=1_000_000.0).contains(side));
        let (event, (accept, cancel, valid)) = show_modal(ui.ctx(), "dtool.page.settings",
            text("panel.pages.settings"), ClosePolicy::CancelOnly, can_resize, |ui| {
                let number = (index + 1).to_string();
                ui.label(dtool_strings::format("panel.pages.number", &[("number", &number)]));
                let landscape = draft.size[0] > draft.size[1];
                let dimensions = |name| {
                    let (width, height, units, dpi) = crate::dialogs::new_document_preset(name)?;
                    let scale = dtool_core::doc_setup::unit_factor(units, dpi, doc.units, doc.dpi);
                    let mut size = [width * scale, height * scale];
                    if landscape { size.swap(0, 1); }
                    Some(size)
                };
                let presets = crate::dialogs::NEW_DOCUMENT_PRESETS;
                let selected = presets.into_iter().find(|name| dimensions(name).is_some_and(|size| {
                    size.iter().zip(draft.size).all(|(preset, current)| (preset - current).abs() < 0.01)
                }));
                ui.horizontal(|ui| {
                    ui.label(text("doc_setup.size_preset"));
                    egui::ComboBox::from_id_salt("page.settings.preset")
                        .selected_text(selected.unwrap_or_else(|| text("doc_setup.size_custom")))
                        .show_ui(ui, |ui| {
                            for name in presets {
                                if ui.selectable_label(selected == Some(name), name).clicked()
                                    && let Some(size) = dimensions(name)
                                {
                                    draft.size = size;
                                    draft.invalid_dimensions = [false; 2];
                                    for id in ids { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
                                }
                            }
                        });
                });
                let spec = dtool_design::NumericSpec::new("").width(116.0).suffix(unit);
                egui::Grid::new("page.settings.dimensions").show(ui, |ui| {
                    for (axis, label) in ["doc_setup.page_width", "doc_setup.page_height"].into_iter().enumerate() {
                        let caption = ui.label(text(label));
                        let mut value = f64::from(draft.size[axis]);
                        let response = dtool_design::numeric_value(ui, ids[axis], &mut value, &spec).labelled_by(caption.id);
                        let draft_input = dtool_design::numeric_state(ui.ctx(), ids[axis]);
                        if draft_input.edited {
                            let parsed = draft_input.buffer.as_deref().and_then(|input| spec.parse_text(input));
                            draft.invalid_dimensions[axis] = parsed.is_none();
                            if let Some(parsed) = parsed { value = parsed; }
                        } else if response.changed() {
                            draft.invalid_dimensions[axis] = false;
                        }
                        draft.size[axis] = value as f32;
                        ui.end_row();
                    }
                });
                let invalid_input = draft.invalid_dimensions.contains(&true);
                ui.horizontal(|ui| {
                    ui.label(text("doc_setup.orientation"));
                    let landscape = draft.size[0] > draft.size[1];
                    for (label, direction) in [("doc_setup.portrait", false), ("doc_setup.landscape", true)] {
                        let button = egui::Button::selectable(direction == landscape, text(label));
                        if ui.add_enabled(!invalid_input && draft.size[0] != draft.size[1], button).clicked() && direction != landscape {
                            draft.size.swap(0, 1);
                            for id in ids { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
                        }
                    }
                });
                if dimensions_valid(draft.size) {
                    let (area, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 80.0), egui::Sense::hover());
                    crate::dialogs::paint_page_preview(ui, area.center(), draft.size[0], draft.size[1], 66.0);
                }
                ui.separator();
                fold_settings(ui, draft, unit, doc.units, doc.dpi);
                ui.separator();
                let valid = !invalid_input && dimensions_valid(draft.size);
                if !valid { ui.label(text("panel.pages.invalid_size")); }
                ui.horizontal(|ui| {
                    let accept = ui.add_enabled(can_resize && valid, egui::Button::new(text("common.ok"))).clicked();
                    let cancel = ui.button(text("panel.master.cancel")).clicked();
                    (accept, cancel, valid)
                }).inner
            });
        if can_resize && valid && (accept || event == DialogResult::Confirm) {
            for id in ids { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
            let draft = self.page_settings.take()?;
            let fold = draft.fold.and_then(|fold| fold.positions(draft.size)
                .map(|positions| json!({"axis": fold.axis, "positions": positions})));
            return Some(Command::with_params("page.resize", json!({"id": draft.id, "size": draft.size, "fold": fold})));
        }
        if cancel || event == DialogResult::Cancel {
            for id in ids { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
            self.page_settings = None;
        }
        None
    }

    fn show_masters(&mut self, ui: &mut egui::Ui, session: &Session, view_size: [f32; 2]) -> Option<Command> {
        let doc = &session.document;
        let mut chosen = None;
        if ui.button("+").on_hover_text(text("panel.master.add")).clicked() {
            let next = doc.master_pages.roots.len() + 1;
            self.master_draft = Some(MasterDraft {
                id: None,
                name: format!("{} {next}", text("panel.master")),
                size: dtool_core::data_merge::layout::page_size(doc),
            });
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for root in doc.master_pages.roots.iter().filter_map(|id| doc.get_object(*id)) {
                let response = ui.selectable_label(false, master_name(root));
                if response.clicked() {
                    chosen = Some([root.pos[0], root.pos[1], root.size[0], root.size[1]]);
                }
                response.context_menu(|ui| {
                    if ui.button(text("panel.master.apply_range")).clicked() {
                        self.master_range = Some(MasterRangeDraft {
                            master: root.id,
                            pages: dtool_core::commands::pages::viewed_page(session)
                                .and_then(|id| doc.pages.iter().position(|page| page.id == id))
                                .map_or_else(|| "1".to_owned(), |index| (index + 1).to_string()),
                            replace: false,
                            keep_previous: true,
                        });
                        ui.close();
                    }
                    if ui.button(text("panel.master.options")).clicked() {
                        self.master_draft = Some(MasterDraft {
                            id: Some(root.id),
                            name: master_name(root).to_owned(),
                            size: root.size,
                        });
                        ui.close();
                    }
                });
            }
        });
        if let Some(command) = self.show_master_range(ui, doc) {
            return Some(command);
        }
        if let Some(draft) = &mut self.master_draft {
            use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
            let enabled = !draft.name.trim().is_empty() && draft.size.iter().all(|n| n.is_finite() && *n > 0.0);
            let (event, (save, cancel)) = show_modal(ui.ctx(), "dtool.master.options", text("panel.master.options"),
                ClosePolicy::CancelOnly, enabled, |ui| {
                    ui.set_min_width(290.0);
                    egui::Grid::new("master.options.fields").show(ui, |ui| {
                        ui.label(text("panel.master.name"));
                        ui.add_sized([230.0, 22.0], egui::TextEdit::singleline(&mut draft.name).char_limit(128));
                        ui.end_row();
                        ui.label(text("panel.master.width"));
                        ui.add(egui::DragValue::new(&mut draft.size[0]).range(0.01..=1_000_000.0));
                        ui.end_row();
                        ui.label(text("panel.master.height"));
                        ui.add(egui::DragValue::new(&mut draft.size[1]).range(0.01..=1_000_000.0));
                        ui.end_row();
                    });
                    ui.horizontal(|ui| {
                        let save = ui.add_enabled(enabled, egui::Button::new(text("common.ok"))).clicked();
                        let cancel = ui.button(text("panel.master.cancel")).clicked();
                        (save, cancel)
                    }).inner
                });
            if save || event == DialogResult::Confirm {
                let draft = self.master_draft.take()?;
                return Some(match draft.id {
                    Some(id) => Command::with_params("master.update", json!({"id": id, "name": draft.name, "size": draft.size})),
                    None => Command::with_params("master.create", json!({"name": draft.name, "size": draft.size})),
                });
            }
            if cancel || event == DialogResult::Cancel {
                self.master_draft = None;
            }
        }
        navigate(session, chosen?, view_size)
    }

    fn show_master_range(&mut self, ui: &mut egui::Ui, doc: &dtool_core::Document) -> Option<Command> {
        use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
        use dtool_core::longdoc::helpers::parse_page_ranges;

        let draft = self.master_range.as_mut()?;
        let Some(master) = doc.get_object(draft.master).filter(|_| doc.master_pages.roots.contains(&draft.master)) else {
            self.master_range = None;
            return None;
        };
        let pages = parse_page_ranges(&draft.pages, doc.pages.len()).unwrap_or_default();
        let has_conflict = pages.iter().any(|&number| doc.master_pages.for_page(doc.pages[number - 1].id)
            .is_some_and(|assignment| assignment.master != draft.master));
        let enabled = !pages.is_empty() && (draft.replace || !has_conflict);
        let (event, (apply, cancel)) = show_modal(ui.ctx(), "dtool.master.range", text("panel.master.apply_range"),
            ClosePolicy::CancelOnly, enabled, |ui| {
                ui.set_min_width(300.0);
                ui.label(master_name(master));
                ui.label(text("panel.master.range_label"));
                ui.add(egui::TextEdit::singleline(&mut draft.pages)
                    .desired_width(300.0)
                    .hint_text("1–3, 5"));
                if pages.is_empty() {
                    ui.weak(text("panel.master.range_invalid"));
                } else if has_conflict && !draft.replace {
                    ui.weak(text("panel.master.range_conflict"));
                }
                ui.checkbox(&mut draft.replace, text("panel.master.replace"));
                ui.add_enabled(draft.replace, egui::Checkbox::new(&mut draft.keep_previous, text("panel.master.replace_keep")));
                ui.horizontal(|ui| {
                    let apply = ui.add_enabled(enabled, egui::Button::new(text("panel.master.apply"))).clicked();
                    let cancel = ui.button(text("button.cancel")).clicked();
                    (apply, cancel)
                }).inner
            });
        if enabled && (apply || event == DialogResult::Confirm) {
            let ids: Vec<_> = parse_page_ranges(&draft.pages, doc.pages.len()).ok()?
                .into_iter().map(|number| doc.pages[number - 1].id).collect();
            if ids.is_empty() || (!draft.replace && ids.iter().any(|&id| doc.master_pages.for_page(id)
                .is_some_and(|assignment| assignment.master != draft.master)))
            {
                return None;
            }
            let draft = self.master_range.take()?;
            return Some(Command::with_params("master.apply", json!({
                "master_id": draft.master, "page_ids": ids, "replace": draft.replace,
                "keep_previous": draft.replace && draft.keep_previous
            })));
        }
        if cancel || event == DialogResult::Cancel {
            self.master_range = None;
        }
        None
    }
}

fn fold_settings(ui: &mut egui::Ui, draft: &mut PageSettingsDraft, unit: &str, units: dtool_core::document::Units, dpi: f32) {
    let mut enabled = draft.fold.is_some();
    if ui.checkbox(&mut enabled, text("panel.pages.fold")).changed() {
        draft.fold = enabled.then(|| PageFold::equal(FoldAxis::Vertical));
    }
    let Some(fold) = &mut draft.fold else { return };
    ui.horizontal(|ui| {
        if ui.button(text("panel.pages.fold.a4")).clicked() {
            let factor = dtool_core::units::per_inch(units, dpi) / 25.4;
            draft.size = [297.0 * factor, 210.0 * factor];
        }
        if ui.button(text("panel.pages.fold.equal")).clicked() {
            *fold = PageFold::equal(fold.axis);
        }
    });
    ui.horizontal(|ui| {
        ui.label(text("panel.pages.fold.direction"));
        ui.selectable_value(&mut fold.axis, FoldAxis::Vertical, text("panel.pages.fold.vertical"));
        ui.selectable_value(&mut fold.axis, FoldAxis::Horizontal, text("panel.pages.fold.horizontal"));
    });
    let Some(mut positions) = fold.positions(draft.size) else { return };
    let extent = draft.size[fold.axis.index()];
    let min_gap = extent * f32::EPSILON * 2.0;
    let mut changed = false;
    egui::Grid::new("page.fold.positions").show(ui, |ui| {
        let limits = [(min_gap, positions[1] - min_gap), (positions[0] + min_gap, extent - min_gap)];
        for ((label, position), (min, max)) in ["panel.pages.fold.first", "panel.pages.fold.second"]
            .into_iter().zip(&mut positions).zip(limits)
        {
            ui.label(text(label));
            changed |= ui.add(egui::DragValue::new(position).range(min..=max.max(min)).clamp_existing_to_range(false)
                .speed(extent / 500.0).max_decimals(3).suffix(format!(" {unit}"))).changed();
            ui.end_row();
        }
    });
    if changed && let Some(next) = PageFold::from_positions(fold.axis, positions, draft.size) {
        *fold = next;
    }
    let [first, second] = fold.positions(draft.size).unwrap_or(positions);
    let lengths = [first, second - first, extent - second];
    ui.weak(format!("{}  {:.2} / {:.2} / {:.2} {unit}",
        text("panel.pages.fold.panel_dimensions"), lengths[0], lengths[1], lengths[2]));
}

fn move_page(id: PageId, number: usize) -> Command {
    Command::with_params("page.move", json!({"id": id, "number": number}))
}

fn navigate(session: &Session, bounds: [f32; 4], view_size: [f32; 2]) -> Option<Command> {
    if !view_size.iter().all(|side| side.is_finite() && *side > 1.0) {
        return None;
    }
    let mut centered = session.view.clone();
    center_region(&mut centered, bounds, view_size);
    let delta = [centered.scroll[0] - session.view.scroll[0], centered.scroll[1] - session.view.scroll[1]];
    Some(Command::with_params("view.pan", json!({"delta": delta})))
}
