use std::collections::{HashMap, HashSet};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

use dtool_core::commands::masters::{MasterPlacement, ScaleMode, master_name, unused_master_spreads};
use dtool_core::document::PageId;
use dtool_core::page_fold::{FoldAxis, PageFold};
use dtool_core::ObjectId;
use dtool_strings::text;
use dtool_core::scene::{Scene, build_scene, rasterize_scene};
use dtool_core::view::center_region;
use dtool_core::workspace::{ContextId, Workspace};
use dtool_core::{CanvasFlags, Command, Session};
use egui::{Color32, ColorImage, TextureHandle, TextureOptions, vec2};
use serde_json::json;

mod master_create;

type PageRevision = (Option<dtool_core::ContextId>, u64, u64, u16);

#[derive(Clone, Copy, Default, PartialEq)]
enum ThumbnailSize {
    Small,
    #[default]
    Medium,
    Large,
    ExtraLarge,
}

impl ThumbnailSize {
    const OPTIONS: [(Self, &str); 4] = [
        (Self::Small, "panel.pages.thumbnail_small"),
        (Self::Medium, "panel.pages.thumbnail_medium"),
        (Self::Large, "panel.pages.thumbnail_large"),
        (Self::ExtraLarge, "panel.pages.thumbnail_extra_large"),
    ];

    fn height(self, width: f32, viewport_height: f32) -> f32 {
        let medium = (width * 0.52).clamp(92.0, 160.0);
        let visible_preview = (viewport_height - 34.0).max(medium);
        match self {
            Self::Small => (width * 0.32).clamp(48.0, 72.0),
            Self::Medium => medium,
            Self::Large => (width * 0.76).clamp(112.0, 224.0).min(visible_preview),
            Self::ExtraLarge => (width * 1.4).clamp(136.0, 384.0).min(visible_preview),
        }
    }

    fn width_limit(self) -> f32 {
        match self {
            Self::Small => 76.0,
            Self::Medium => 132.0,
            Self::Large => 196.0,
            Self::ExtraLarge => f32::INFINITY,
        }
    }

    fn resolution(self) -> u16 {
        match self {
            Self::Small | Self::Medium => 144,
            Self::Large => 224,
            Self::ExtraLarge => 384,
        }
    }
}

#[derive(Clone, Copy)]
struct FollowedPageRow {
    page: PageId,
    document_version: u64,
    top: f32,
    height: f32,
    stride: f32,
    viewport_height: f32,
}
const TEXT_THUMBNAIL_DELAY: Duration = Duration::from_millis(200);

type PageThumbnailJob = super::ThumbnailJob<PageRevision, Vec<(PageId, ColorImage)>>;

struct MasterDraft {
    id: ObjectId,
    name: String,
    size: [f32; 2],
    initial_size: [f32; 2],
}

#[derive(Clone, Copy, PartialEq)]
enum MasterConform {
    None,
    Size,
    Spread,
}

struct MasterRangeDraft {
    master: ObjectId,
    pages: String,
    replace: bool,
    add: bool,
    keep_previous: bool,
    conform: MasterConform,
    source_page: Option<usize>,
    placement: MasterPlacement,
    target_group: Option<ObjectId>,
}

struct MasterBaseDraft {
    target: ObjectId,
    base: Option<ObjectId>,
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
    count_field: egui::Id,
}

struct PageTransferDraft {
    pages: String,
    order: Vec<PageId>,
    target: f64,
    after: bool,
    copy: bool,
    field: egui::Id,
    destination: Option<ContextId>,
    destination_order: Vec<PageId>,
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
    master_thumbnails: HashMap<ObjectId, TextureHandle>,
    icons: crate::icon_cache::IconCache,
    rename: Option<(PageId, String)>,
    selected_pages: HashSet<PageId>,
    selection_anchor: Option<PageId>,
    drag: Option<(PageId, egui::Pos2)>,
    master_drag: Option<(ObjectId, egui::Pos2)>,
    add_pages_requested: Option<PageId>,
    delete_pages: Vec<PageId>,
    page_settings: Option<PageSettingsDraft>,
    spread_settings: Option<SpreadDraft>,
    page_transfer: Option<PageTransferDraft>,
    pages_closed: bool,
    thumbnail_size: ThumbnailSize,
    thumbnail_row_layout: Option<(ThumbnailSize, f32)>,
    resized_thumbnail_scroll: Option<f32>,
    follow_view: bool,
    hide_applied_master_tags: bool,
    last_followed_row: Option<FollowedPageRow>,
    section_manager_requested: bool,
    masters_open: bool,
    selected_master: Option<ObjectId>,
    canvas_master_selection: Option<ObjectId>,
    delete_master: Option<ObjectId>,
    delete_unused_masters: bool,
    master_draft: Option<MasterDraft>,
    master_create: Option<master_create::Draft>,
    master_range: Option<MasterRangeDraft>,
    master_base: Option<MasterBaseDraft>,
}

impl PagesPanel {
    fn thumbnail_size_for(&self, doc: &dtool_core::Document) -> ThumbnailSize {
        if doc.layout_artboard_origins.is_some() { self.thumbnail_size } else { ThumbnailSize::Medium }
    }

    pub fn show_master_tab(&mut self, masters: bool) {
        self.masters_open = masters;
        if !masters {
            self.pages_closed = false;
        }
    }

    pub(crate) fn take_section_manager_request(&mut self) -> bool {
        std::mem::take(&mut self.section_manager_requested)
    }

    pub(crate) fn take_add_pages_request(&mut self) -> Option<PageId> {
        self.add_pages_requested.take()
    }

    pub fn select_master(&mut self, id: ObjectId) {
        self.selected_master = Some(id);
        self.masters_open = true;
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
        self.background_thumbnails = after_text_edit || !session.document.pixel_layers.is_empty()
                    || session.document.objects().len() >= 100;
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
                (doc.pages[index].id, page_thumbnail_image(&doc, &mut scene, index, bounds, f32::from(revision.3)))
            }).collect()
        });
        if let Ok(job) = worker {
            self.thumbnail_job = Some(job);
        } else {
            self.background_thumbnails = false;
            ctx.request_repaint();
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &Workspace, view_size: [f32; 2]) -> Option<Command> {
        let Some(session) = workspace.session() else {
            self.revision = None;
            self.pending_text_thumbnails = None;
            self.background_thumbnails = false;
            self.rename = None;
            self.selected_pages.clear();
            self.selection_anchor = None;
            self.drag = None;
            self.master_drag = None;
            self.delete_pages.clear();
            self.page_settings = None;
            self.spread_settings = None;
            self.page_transfer = None;
            self.last_followed_row = None;
            self.thumbnail_row_layout = None;
            self.resized_thumbnail_scroll = None;
            self.master_draft = None;
            self.master_create = None;
            self.master_range = None;
            self.master_base = None;
            self.thumbnails.clear();
            self.master_thumbnails.clear();
            self.selected_master = None;
            self.canvas_master_selection = None;
            self.delete_master = None;
            self.delete_unused_masters = false;
            ui.weak(dtool_strings::text("app.no_document"));
            return None;
        };
        let thumbnail_size = self.thumbnail_size_for(&session.document);
        let revision = (session.context, session.artwork_generation(), session.document.version, thumbnail_size.resolution());
        if self.revision.map(|old| old.0) != Some(session.context) {
            self.rename = None;
            self.selected_pages.clear();
            self.selection_anchor = None;
            self.drag = None;
            self.master_drag = None;
            self.delete_pages.clear();
            self.page_settings = None;
            self.spread_settings = None;
            self.page_transfer = None;
            self.last_followed_row = None;
            self.thumbnail_row_layout = None;
            self.resized_thumbnail_scroll = None;
            self.master_draft = None;
            self.master_create = None;
            self.master_range = None;
            self.master_base = None;
            self.selected_master = None;
            self.canvas_master_selection = None;
            self.delete_master = None;
            self.delete_unused_masters = false;
        }
        if self.revision != Some(revision) {
            self.master_thumbnails.clear();
        }
        let thumbnails_deferred = self.prepare_thumbnails(ui.ctx(), revision, session);
        self.collect_thumbnails(ui.ctx(), revision);

        let (pressed, released) = crate::input::primary_button_positions(ui);
        let down = ui.input(|input| input.pointer.primary_down());
        let cancelled = ui.input(|input| !input.focused || input.key_pressed(egui::Key::Escape))
            || crate::dialog_chrome::modal_open(ui.ctx()) || egui::Popup::is_any_open(ui.ctx());
        if pressed.is_some() || (!down && released.is_none()) || cancelled {
            self.master_drag = None;
        }

        let master = self.show_masters(ui, session, view_size, pressed.filter(|_| !cancelled), released);
        let pages = self.show_document_pages(ui, workspace, session, view_size, thumbnails_deferred);
        if released.is_some() {
            self.master_drag = None;
        }
        master.or(pages)
    }

    fn show_document_pages(
        &mut self, ui: &mut egui::Ui, workspace: &Workspace, session: &Session,
        view_size: [f32; 2], thumbnails_deferred: bool,
    ) -> Option<Command> {
        let doc = &session.document;
        let thumbnail_size = self.thumbnail_size_for(doc);
        let revision = (session.context, session.artwork_generation(), doc.version, thumbnail_size.resolution());
        let can_navigate = view_size.iter().all(|side| side.is_finite() && *side > 1.0);
        let current = dtool_core::commands::pages::viewed_page(session);
        self.selected_pages.retain(|id| doc.pages.iter().any(|page| page.id == *id));
        if self.rename.as_ref().is_some_and(|(id, _)| !doc.pages.iter().any(|page| page.id == *id)) {
            self.rename = None;
        }
        let targets: Vec<_> = doc.pages.iter().filter(|page| self.selected_pages.contains(&page.id))
            .map(|page| page.id).collect();
        let targets = if targets.is_empty() { current.into_iter().collect() } else { targets };
        let mut open = !self.pages_closed;
        let ([sections, add, duplicate, delete], header) = section_header(ui, &mut self.icons, &mut open, text("panel.pages"), [
            ("para-list-number", "dialog.section_manager.title", true),
            ("file-new", "panel.pages.add", true),
            ("edit-duplicate", "panel.pages.duplicate", !targets.is_empty()),
            ("layer-delete", "panel.pages.delete", !targets.is_empty() && targets.len() < doc.pages.len()),
        ]);
        self.section_manager_requested |= sections;
        self.pages_closed = !open;
        let mut command = if add {
            self.pages_closed = false;
            Some(Command::with_params("page.add", targets.last().map_or_else(|| json!({}), |id| json!({"after_id": id}))))
        } else if duplicate {
            Some(Command::with_params("page.duplicate", json!({"ids": targets})))
        } else {
            if delete {
                self.delete_pages = targets;
            }
            None
        };
        if doc.layout_artboard_origins.is_some() {
            header.context_menu(|ui| {
                ui.menu_button(text("panel.pages.thumbnail_size"), |ui| {
                    for (size, label) in ThumbnailSize::OPTIONS {
                        if dtool_design::choice_value(ui, &mut self.thumbnail_size, size, text(label)).clicked() {
                            ui.close();
                            ui.ctx().request_repaint();
                        }
                    }
                });
                if dtool_design::toggle(ui, &mut self.follow_view, text("panel.pages.follow_view")).changed() {
                    self.last_followed_row = None;
                    ui.close();
                }
                let mut show_tags = !self.hide_applied_master_tags;
                if dtool_design::toggle(ui, &mut show_tags, text("panel.pages.show_master_tags")).changed() {
                    self.hide_applied_master_tags = !show_tags;
                    ui.close();
                }
                command = page_move_menu(ui, doc).or(command.take());
            });
        }
        if self.pages_closed {
            self.drag = None;
            return self.show_page_delete(ui, doc).or(command);
        }
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

        let viewport_height = ui.available_rect_before_wrap().intersect(ui.clip_rect()).height();
        let preview_height = thumbnail_size.height(ui.available_width(), viewport_height);
        let thumb_box_height = preview_height + 4.0;
        let row_height = preview_height + 30.0;
        let facing = doc.setup.facing_pages;
        let show_master_tags = !self.hide_applied_master_tags && doc.master_pages.has_tag_colours();
        let right_cover = dtool_core::data_merge::layout::has_right_cover(doc);
        let row_count = if doc.pages.is_empty() { 0 } else { spreads.len() };
        let scroll_salt = "pages.document_thumbnails";
        let mut scroll = egui::ScrollArea::vertical().id_salt(scroll_salt);
        if let Some(target) = self.resized_thumbnail_scroll.take() {
            scroll = scroll.vertical_scroll_offset(target);
        }
        let stride = row_height + ui.spacing().item_spacing.y;
        let previous_layout = self.thumbnail_row_layout.replace((thumbnail_size, stride));
        let size_change = previous_layout.filter(|(size, old_stride)| *size != thumbnail_size && *old_stride != stride);
        let id = ui.make_persistent_id(egui::IdSalt::new(scroll_salt));
        let scroll_offset = || egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |state| state.clamped_offset().y);
        let mut resized_scroll = size_change.map(|(_, old_stride)| scroll_offset() * stride / old_stride);
        if self.follow_view && doc.layout_artboard_origins.is_some() && self.drag.is_none() && self.master_drag.is_none() {
            let height = viewport_height;
            let previous = self.last_followed_row;
            if current.is_none() {
                self.last_followed_row = None;
            } else if let Some(page) = current
                && previous.is_none_or(|row| row.page != page || row.document_version != doc.version
                    || row.height != row_height || row.stride != stride || row.viewport_height != height)
                && let Some(index) = doc.pages.iter().position(|item| item.id == page)
                && let Some(row) = spreads.iter().position(|spread| spread.contains(&index))
            {
                let offset = scroll_offset();
                let top = row as f32 * stride;
                // Keep a followed row visible after the dock changes size, without undoing a manual scroll away from it.
                if previous.is_none_or(|row| row.page != page
                    || (row.top < offset + row.viewport_height && offset < row.top + row.height))
                {
                    let candidate = resized_scroll.unwrap_or(offset);
                    let row_center = top + (row_height - height) * 0.5;
                    let target = if height < row_height {
                        row_center
                    } else {
                        let bottom = (top + stride - height).min(top);
                        if previous.is_none_or(|row| row.page != page) && !(bottom..=top).contains(&candidate) {
                            row_center
                        } else {
                            candidate.clamp(bottom, top)
                        }
                    };
                    if size_change.is_some() {
                        resized_scroll = Some(target);
                    } else if target != offset {
                        scroll = scroll.vertical_scroll_offset(target);
                    }
                }
                self.last_followed_row = Some(FollowedPageRow {
                    page, document_version: doc.version, top, height: row_height, stride, viewport_height: height,
                });
            }
        }
        if let Some(target) = resized_scroll {
            // Apply after egui has recorded the new bounds for its scroll physics.
            self.resized_thumbnail_scroll = Some(target);
            ui.ctx().request_repaint();
        }
        scroll.show_rows(ui, row_height, row_count, |ui, rows| {
            let mut drop_targets = Vec::new();
            let mut spread_rows = Vec::new();
            let mut vacancy_targets = Vec::new();
            let mut vacancy_command = None;
            let rects = dtool_core::scene::page_rects(doc);
            let mut show_page = |ui: &mut egui::Ui, index: usize, spread: &std::ops::Range<usize>,
                                 cell_width: f32, columns: usize, align: egui::Align,
                                 scale: f32, top_inset: f32| {
                let page = &doc.pages[index];
                let [x, y, width, height] = rects[index];
                if ![x, y, width, height].iter().all(|n| n.is_finite()) || width <= 0.0 || height <= 0.0 {
                    ui.allocate_space(vec2(cell_width, row_height));
                    return;
                }
                visible.push(page.id);
                if !thumbnails_deferred && self.thumbnails.get(&page.id).is_none_or(|(stamp, _)| *stamp != revision) {
                    let bounds = [x, y, width, height];
                    if self.background_thumbnails {
                        thumbnail_requests.push((index, bounds));
                    } else {
                        let texture = render_page_thumbnail(ui.ctx(), doc, &mut scene, index, bounds, f32::from(revision.3));
                        self.thumbnails.insert(page.id, (revision, texture));
                    }
                }
                let texture = self.thumbnails.get(&page.id).map(|(_, handle)| handle);

                let number = dtool_core::longdoc::sections::section_page_label(doc, index);
                let label = if page.name.is_empty() {
                    dtool_strings::format("panel.pages.number", &[("number", &number)])
                } else {
                    format!("{number} · {}", page.name)
                };
                let thumbnail_label = if cell_width < 58.0 { &number } else { &label };
                let selected = if self.selected_pages.is_empty() {
                    current == Some(page.id)
                } else {
                    self.selected_pages.contains(&page.id)
                };
                let thumb_size = vec2(width * scale, height * scale);
                let row = ui.allocate_ui_with_layout(
                    vec2(cell_width, row_height),
                    egui::Layout::top_down(align),
                    |ui| {
                        ui.set_max_width(cell_width);
                        let (rect, _) = ui.allocate_exact_size(vec2(thumb_size.x, thumb_box_height), egui::Sense::hover());
                        let thumb = egui::Rect::from_min_size(rect.min + vec2(0.0, top_inset), thumb_size);
                        ui.painter().rect_filled(thumb, 0.0, Color32::WHITE);
                        if let Some(texture) = texture {
                            ui.painter().image(
                                texture.id(), thumb,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE,
                            );
                        }
                        let own = page.tag_colour.as_deref().and_then(dtool_design::tokens::parse_hex);
                        if show_master_tags {
                            let applied: Vec<_> = doc.master_pages.on_page(page.id)
                                .filter_map(|item| doc.master_pages.tag_colour(item.master).and_then(dtool_design::tokens::parse_hex))
                                .collect();
                            draw_thumbnail_tags(ui, thumb, &applied, true);
                        }
                        if let Some(colour) = own {
                            draw_thumbnail_tags(ui, thumb, &[colour], false);
                        }
                        if selected {
                            thumbnail_outline(ui, thumb);
                        }
                        ui.scope(|ui| {
                            let ink = ui.visuals().text_color();
                            let selection = &mut ui.visuals_mut().selection;
                            selection.bg_fill = dtool_design::color("layers.selected");
                            selection.stroke.color = ink;
                            ui.add(egui::Button::selectable(selected, thumbnail_label).truncate().sense(egui::Sense::hover()));
                        });
                    },
                );
                let response = ui.interact(row.response.rect, ui.id().with(("page", page.id.0)), egui::Sense::click_and_drag())
                    .on_hover_ui(|ui| {
                        ui.label(&label);
                        if page.spread_group.is_some() && spread.len() > 1 {
                            let first = dtool_core::longdoc::sections::section_page_label(doc, spread.start);
                            let last = dtool_core::longdoc::sections::section_page_label(doc, spread.end - 1);
                            ui.label(format!("{first}–{last}"));
                        }
                        for item in doc.master_pages.on_page(page.id) {
                            if let Some(root) = doc.get_object(item.master) {
                                let name = format!("{} · {}", text("panel.master"), master_name(root));
                                ui.label(application_label(&name, item.stack_slot));
                            }
                        }
                    });
                let hitbox = response.interact_rect.intersect(ui.clip_rect());
                drop_targets.push((index, response.rect, hitbox, columns > 1));
                if let Some(point) = pressed.filter(|point| response.enabled() && hitbox.contains(*point)
                    && ui.ctx().layer_id_at(*point) == Some(response.layer_id)
                    && !egui::Popup::is_any_open(ui.ctx()))
                {
                    self.drag = Some((page.id, point));
                }
                response.context_menu(|ui| {
                    let group = doc.layout_artboard_origins.is_some()
                        && self.selected_pages.contains(&page.id) && self.selected_pages.len() > 1;
                    let command_ids = || {
                        if group {
                            doc.pages.iter().filter(|page| self.selected_pages.contains(&page.id)).map(|page| page.id).collect()
                        } else {
                            vec![page.id]
                        }
                    };
                    if doc.layout_artboard_origins.is_some() {
                        ui.menu_button(text("panel.pages.tag_colour"), |ui| {
                            let first = page.tag_colour.as_deref();
                            let same = !group || doc.pages.iter().filter(|item| self.selected_pages.contains(&item.id))
                                .all(|item| item.tag_colour.as_deref() == first);
                            if let Some(colour) = super::layers::tag_colour_picker(ui, same.then_some(first)) {
                                command = Some(Command::with_params("page.tag.set", json!({"ids": command_ids(), "colour": colour})));
                            }
                        });
                    }
                    if doc.layout_artboard_origins.is_some()
                        && !(index == 0 && dtool_core::data_merge::layout::has_right_cover(doc))
                        && ui.button(text("panel.pages.spread_settings")).clicked()
                    {
                        let range = spread.clone();
                        let custom = dtool_core::commands::pages::spreads::is_custom(doc, &range);
                        let start = if custom { range.start } else { index };
                        let count_field = ui.id().with(("spread.page_count", doc.pages[start].id));
                        dtool_design::numeric_cancel_edit(ui.ctx(), count_field);
                        self.spread_settings = Some(SpreadDraft {
                            start: doc.pages[start].id,
                            count: range.end - start,
                            custom,
                            count_field,
                        });
                        ui.close();
                    }
                    if doc.layout_artboard_origins.is_some() {
                        command = page_move_menu(ui, doc).or(command.take());
                        ui.separator();
                    }
                    let settings = egui::Button::new(text("panel.pages.settings"));
                    if ui.add_enabled(dtool_core::commands::pages::can_resize(doc, page.id), settings).clicked() {
                        let dimension_fields = ["width", "height"].map(|axis| ui.id().with(("page.settings.size", page.id, axis)));
                        for id in dimension_fields {
                            dtool_design::numeric_cancel_edit(ui.ctx(), id);
                            dtool_design::numeric_cancel_edit(ui.ctx(), id.with("fold"));
                        }
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
                    if doc.layout_artboard_origins.is_some() && ui.button(text("panel.pages.add_dialog")).clicked() {
                        self.add_pages_requested = Some(page.id);
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
                    let duplicate_label = if group { "panel.pages.duplicate_selected" } else { "panel.pages.duplicate" };
                    if ui.button(text(duplicate_label)).clicked() {
                        command = Some(Command::with_params("page.duplicate", json!({"ids": command_ids()})));
                        ui.close();
                    }
                    if doc.layout_artboard_origins.is_some() && ui.button(text("panel.pages.transfer")).clicked() {
                        let field = ui.make_persistent_id("pages.transfer.target");
                        dtool_design::numeric_cancel_edit(ui.ctx(), field);
                        let pages = doc.pages.iter().enumerate()
                            .filter(|(_, item)| if group { self.selected_pages.contains(&item.id) } else { item.id == page.id })
                            .map(|(position, _)| (position + 1).to_string()).collect::<Vec<_>>().join(", ");
                        self.page_transfer = Some(PageTransferDraft {
                            pages, order: doc.pages.iter().map(|page| page.id).collect(),
                            target: (index + 1) as f64, after: true, copy: true, field,
                            destination: None, destination_order: Vec::new(),
                        });
                        ui.close();
                    }
                    let delete_count = if group { self.selected_pages.len() } else { 1 };
                    let delete_label = if group { "panel.pages.delete_selected" } else { "panel.pages.delete" };
                    if ui.add_enabled(doc.pages.len() > delete_count, egui::Button::new(text(delete_label))).clicked() {
                        self.delete_pages = command_ids();
                        ui.close();
                    }
                    if let Some(current) = doc.master_pages.for_page(page.id) {
                        let current_master = doc.master_pages.spread_id(current.master);
                        if doc.master_pages.spread_roots().any(|id| id != current_master && doc.get_object(id).is_some()) {
                            ui.menu_button(text("panel.master.replace"), |ui| {
                                for master in doc.master_pages.spread_roots().filter(|&id| id != current_master).filter_map(|id| doc.get_object(id)) {
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
                            for master in doc.master_pages.spread_roots().filter_map(|id| doc.get_object(id)) {
                                if ui.button(master_name(master)).clicked() {
                                    command = Some(Command::with_params("master.apply", json!({"master_id": master.id, "page_id": page.id})));
                                    ui.close();
                                }
                            }
                        });
                    }
                    if doc.master_pages.for_page(page.id).is_some() {
                        ui.menu_button(text("panel.master.add_application"), |ui| {
                            for root in doc.master_pages.spread_roots().filter_map(|id| doc.get_object(id)) {
                                if ui.button(master_name(root)).clicked() {
                                    command = Some(Command::with_params("master.apply", json!({
                                        "master_id": root.id, "page_ids": command_ids(), "add": true
                                    })));
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button(text("panel.master.application_layers"), |ui| {
                            for binding in doc.master_pages.on_page(page.id) {
                                let Some(group_id) = binding.group else { continue; };
                                let Some(root) = doc.get_object(binding.master) else { continue; };
                                let name = application_label(master_name(root), binding.stack_slot);
                                ui.menu_button(name, |ui| {
                                    if ui.button(text("panel.master.placement_options")).clicked() {
                                        self.master_range = Some(MasterRangeDraft {
                                            master: doc.master_pages.spread_id(binding.master),
                                            pages: (index + 1).to_string(), replace: false, add: false,
                                            keep_previous: false, conform: MasterConform::None, source_page: None,
                                            placement: binding.placement, target_group: Some(group_id),
                                        });
                                        ui.close();
                                    }
                                    for (name, label) in [
                                        ("master.detach", "panel.master.detach"),
                                        ("master.remove", "panel.master.remove_application"),
                                    ] {
                                        if ui.button(text(label)).clicked() {
                                            command = Some(Command::with_params(name, json!({
                                                "page_id": page.id, "group_id": group_id
                                            })));
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        });
                    }
                    if let Some(&master) = doc.master_pages.roots.first()
                        && ui.button(text("panel.master.apply_options")).clicked()
                    {
                        let targets = command_ids();
                        let pages = doc.pages.iter().enumerate()
                            .filter(|(_, page)| targets.contains(&page.id))
                            .map(|(index, _)| (index + 1).to_string()).collect::<Vec<_>>().join(", ");
                        self.master_range = Some(MasterRangeDraft {
                            master: doc.master_pages.for_page(page.id).map_or(master, |item| doc.master_pages.spread_id(item.master)),
                            pages, replace: doc.master_pages.on_page(page.id).count() <= 1, add: false,
                            keep_previous: false, conform: MasterConform::None, source_page: None,
                            placement: doc.master_pages.for_page(page.id).map_or_else(MasterPlacement::default, |assignment| assignment.placement),
                            target_group: None,
                        });
                        ui.close();
                    }
                });
                if response.clicked() {
                    let modifiers = if doc.layout_artboard_origins.is_some() {
                        drop_modifiers(ui)
                    } else {
                        egui::Modifiers::NONE
                    };
                    if modifiers.shift {
                        let anchor = self.selection_anchor.or(current)
                            .and_then(|id| doc.pages.iter().position(|page| page.id == id))
                            .unwrap_or(index);
                        if !modifiers.command {
                            self.selected_pages.clear();
                        }
                        self.selected_pages.extend(doc.pages[anchor.min(index)..=anchor.max(index)].iter().map(|page| page.id));
                        self.selection_anchor = Some(doc.pages[anchor].id);
                    } else if modifiers.command {
                        if self.selected_pages.is_empty()
                            && let Some(page) = current.or(self.selection_anchor)
                        {
                            self.selected_pages.insert(page);
                        }
                        if !self.selected_pages.remove(&page.id) {
                            self.selected_pages.insert(page.id);
                        }
                        self.selection_anchor = Some(page.id);
                    } else {
                        self.selected_pages.clear();
                        self.selection_anchor = Some(page.id);
                        if can_navigate {
                            chosen = Some(page.id);
                        }
                    }
                }
            };
            for row in rows {
                let spread = &spreads[row];
                let fixed = dtool_core::commands::pages::spreads::fixed_spread(doc, spread);
                let columns = fixed.map_or(spread.len(), |spread| spread.slots.len())
                    .max(if facing { 2 } else { 1 });
                let spine = dtool_core::data_merge::layout::spread_spine(doc, spread);
                let gap = 5.0_f32.min(ui.available_width() / (2 * columns) as f32);
                let width = ui.available_width();
                let left = ui.cursor().left();
                let half = (width - gap) * 0.5;
                let spine_x = if facing {
                    left + width * 0.5
                } else {
                    left + (width + gap) * spine as f32 / columns as f32 - gap * 0.5
                };
                let cell = |column| {
                    let (width, align) = if !facing {
                        ((width - gap * (columns - 1) as f32) / columns as f32, egui::Align::Center)
                    } else if column < spine {
                        ((half - gap * (spine - 1) as f32) / spine as f32, egui::Align::Max)
                    } else {
                        let trailing = columns - spine;
                        ((half - gap * (trailing - 1) as f32) / trailing as f32, egui::Align::Min)
                    };
                    (width.max(1.0), align)
                };
                let leading_blank = usize::from(right_cover && row == 0 && spread.len() == 1);
                let page_indices: HashMap<_, _> =
                    spread.clone().map(|index| (doc.pages[index].id, index)).collect();
                let column_page = |column: usize| {
                    if let Some(fixed) = fixed {
                        fixed.slots.get(column).and_then(|slot| slot.page)
                            .and_then(|id| page_indices.get(&id).copied())
                    } else {
                        column.checked_sub(leading_blank).filter(|offset| *offset < spread.len())
                            .map(|offset| spread.start + offset)
                    }
                };
                let size = |column: usize| {
                    if let Some(index) = column_page(column) {
                        let [_, _, w, h] = rects[index];
                        [w, h]
                    } else if let Some(slot) = fixed.and_then(|spread| spread.slots.get(column)) {
                        slot.size
                    } else {
                        let [_, _, w, h] = rects[spread.start];
                        [w, h]
                    }
                };
                let mut scale = f32::INFINITY;
                let mut tallest = 0.0_f32;
                for column in 0..columns {
                    let [w, h] = size(column);
                    if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 {
                        let (cell_width, _) = cell(column);
                        let max_width = (cell_width - 6.0).clamp(1.0, thumbnail_size.width_limit());
                        scale = scale.min(max_width / w).min(preview_height / h);
                        tallest = tallest.max(h);
                    }
                }
                let scale = if scale.is_finite() { scale } else { 1.0 };
                let top_inset = (thumb_box_height - tallest * scale) * 0.5;
                let response = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for column in 0..columns {
                        let (cell_width, align) = cell(column);
                        if let Some(index) = column_page(column) {
                            show_page(ui, index, spread, cell_width, columns, align, scale, top_inset);
                        } else {
                            let [width, height] = size(column);
                            let thumb_size = vec2(width * scale, height * scale);
                            let vacant_cell = ui.allocate_ui_with_layout(vec2(cell_width, row_height),
                                egui::Layout::top_down(align), |ui| {
                                    ui.set_max_width(cell_width);
                                    ui.allocate_exact_size(vec2(thumb_size.x, thumb_box_height), egui::Sense::hover()).0
                                });
                            let thumb = egui::Rect::from_min_size(vacant_cell.inner.min + vec2(0.0, top_inset), thumb_size);
                            let fillable = fixed.is_some_and(|spread| column < spread.slots.len()
                                && !(spread.cover && column == 0));
                            let structural = facing && (leading_blank == 1 || row + 1 == row_count
                                && !dtool_core::commands::pages::spreads::is_custom(doc, spread));
                            if doc.layout_artboard_origins.is_some() && (fillable || structural) {
                                ui.painter().rect_stroke(thumb, 0.0, egui::Stroke::new(1.0,
                                    ui.visuals().weak_text_color().gamma_multiply(0.65)), egui::StrokeKind::Inside);
                            }
                            if fillable {
                                let anchor = doc.pages[spread.start].id;
                                vacancy_targets.push((vacant_cell.response.rect, thumb, anchor, column + 1));
                                let response = ui.interact(vacant_cell.response.rect, ui.id().with(("empty_page", anchor.0, column)),
                                    egui::Sense::click()).on_hover_text(text("panel.pages.vacancy"));
                                let add = || Command::with_params("page.add",
                                    json!({"vacancy": {"anchor_id": anchor, "number": column + 1}}));
                                if response.double_clicked() {
                                    vacancy_command = Some(add());
                                }
                                response.context_menu(|ui| {
                                    if ui.button(text("panel.pages.fill_vacancy")).clicked() {
                                        vacancy_command = Some(add());
                                        ui.close();
                                    }
                                });
                            }
                        }
                    }
                }).response;
                spread_rows.push((spread.clone(), response.rect, spine_x, gap));
            }
            command = vacancy_command.or(command.take());
            let pointer = released.or_else(|| ui.input(|input| input.pointer.interact_pos()));
            if command.is_none() && let Some(point) = pointer
                && let Some((source, start)) = self.master_drag
                && start.distance(point) >= 4.0 && doc.master_pages.roots.contains(&source)
                && ui.clip_rect().contains(point)
            {
                if ui.input(|input| input.modifiers.ctrl) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
                }
                if released.is_none() {
                    scroll_drag_at_edge(ui, point.y);
                }
                let edge_height = dtool_design::space("sm");
                let insert = spread_rows.iter().find_map(|(range, rect, _, _)| {
                    let in_row = rect.expand2(vec2(0.0, edge_height)).contains(point);
                    if !in_row { return None; }
                    if (point.y - rect.top()).abs() <= edge_height {
                        return Some((range.start, false, rect.top(), rect.x_range()));
                    }
                    if (point.y - rect.bottom()).abs() <= edge_height {
                        return Some((range.end - 1, true, rect.bottom(), rect.x_range()));
                    }
                    None
                });
                if let Some((index, after, y, x)) = insert {
                    ui.painter().hline(x, y, ui.visuals().selection.stroke);
                    if released.is_some() {
                        let anchor = if after { "after_id" } else { "before_id" };
                        command = Some(Command::with_params("page.add", json!({
                            (anchor): doc.pages[index].id, "spread": "new",
                            "master_id": source, "conform_layout": true
                        })));
                    }
                } else if let Some((_, thumb, anchor, number)) = vacancy_targets.iter()
                    .find(|(hit, _, _, _)| hit.contains(point))
                {
                    thumbnail_outline(ui, *thumb);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
                    if released.is_some() {
                        command = Some(Command::with_params("page.add", json!({
                            "vacancy": {"anchor_id": anchor, "number": number},
                            "master_id": source
                        })));
                    }
                } else if let Some((spread, rect, _, _)) = spread_rows.iter().find(|(spread, rect, spine_x, gap)| {
                    spread.len() > 1 && rect.y_range().contains(point.y)
                        && (point.x - spine_x).abs() <= gap * 0.5 + dtool_design::space("sm")
                }) {
                    thumbnail_outline(ui, rect.shrink(3.0));
                    if released.is_some() {
                        let ids: Vec<_> = doc.pages[spread.clone()].iter().map(|page| page.id).collect();
                        command = Some(master_drop(ui, source, ids));
                    }
                } else if let Some(&(index, rect, _, _)) = drop_targets.iter().find(|(_, _, hitbox, _)| hitbox.contains(point)) {
                    let page = doc.pages[index].id;
                    let group = self.selected_pages.contains(&page);
                    let selected = |id| id == page || (group && self.selected_pages.contains(&id));
                    thumbnail_outline(ui, rect.shrink(3.0));
                    if released.is_some() {
                        let ids: Vec<_> = doc.pages.iter().map(|page| page.id).filter(|&id| selected(id)).collect();
                        command = Some(master_drop(ui, source, ids));
                    }
                }
            }
            if let Some((id, start)) = self.drag
                && let Some(point) = pointer.filter(|point| start.distance(*point) >= 4.0 && ui.clip_rect().contains(*point))
            {
                if released.is_none() {
                    scroll_drag_at_edge(ui, point.y);
                }
                let group_drag = self.selected_pages.contains(&id);
                let moving: Vec<_> = doc.pages.iter().enumerate().filter(|(_, page)| {
                    page.id == id || (group_drag && self.selected_pages.contains(&page.id))
                }).collect();
                let ids = || moving.iter().map(|(_, page)| page.id).collect::<Vec<_>>();
                let copy = doc.layout_artboard_origins.is_some() && drop_modifiers(ui).command;
                let fixed = doc.layout_artboard_origins.is_some() && doc.fixed_spreads.is_some();
                let edge = dtool_design::space("sm");
                let edge_target = fixed.then(|| spread_rows.iter().find_map(|(range, row, _, _)| {
                    let after = (point.y - row.bottom()).abs() <= edge;
                    if !row.x_range().contains(point.x) || (!after && (point.y - row.top()).abs() > edge)
                        || (!after && right_cover && range.start == 0) {
                        return None;
                    }
                    let index = if copy {
                        if after { range.end - 1 } else { range.start }
                    } else {
                        range.clone().find(|&index| !moving.iter().any(|(from, _)| *from == index))?
                    };
                    Some((doc.pages[index].id, after, *row))
                })).flatten();
                if let Some((anchor, after, row)) = edge_target {
                    ui.painter().hline(row.x_range(), if after { row.bottom() } else { row.top() }, ui.visuals().selection.stroke);
                    ui.ctx().set_cursor_icon(if copy { egui::CursorIcon::Copy } else { egui::CursorIcon::Grabbing });
                    if released.is_some() && command.is_none() {
                        command = Some(if copy {
                            copy_pages(&ids(), anchor, after)
                        } else {
                            Command::with_params("page.move", json!({"ids": ids(),
                                "destination": {"anchor_id": anchor, "after": after, "new_spread": true}}))
                        });
                    }
                } else if let Some((_, thumb, anchor, number)) = vacancy_targets.iter().find(|(hit, _, _, _)| hit.contains(point)) {
                    if dtool_core::commands::pages::spreads::can_fill(doc, *anchor, *number, moving.len()) {
                        thumbnail_outline(ui, *thumb);
                        ui.ctx().set_cursor_icon(if copy { egui::CursorIcon::Copy } else { egui::CursorIcon::Grabbing });
                        if released.is_some() && command.is_none() {
                            command = Some(Command::with_params(if copy { "page.duplicate" } else { "page.move" },
                                json!({"ids": ids(), "vacancy": {"anchor_id": anchor, "number": number}})));
                        }
                    }
                } else if let Some((index, rect, _, horizontal)) = drop_targets.iter().find(|(_, _, hitbox, horizontal)| {
                    let expansion = if *horizontal { vec2(2.0, 0.0) } else { vec2(0.0, 2.0) };
                    hitbox.expand2(expansion).contains(point)
                }) {
                    let after = if *horizontal { point.x >= rect.center().x } else { point.y >= rect.center().y };
                    let insertion = index + usize::from(after);
                    let to = insertion - moving.partition_point(|(from, _)| *from < insertion);
                    let can_extend = fixed && !(right_cover && *index == 0)
                        && !moving.iter().any(|(from, _)| from == index);
                    if copy || can_extend || (!fixed && !moving.iter().map(|(from, _)| *from).eq(to..to + moving.len())) {
                        let stroke = ui.visuals().selection.stroke;
                        if *horizontal {
                            ui.painter().vline(if after { rect.right() } else { rect.left() }, rect.y_range(), stroke);
                        } else {
                            ui.painter().hline(rect.x_range(), if after { rect.bottom() } else { rect.top() }, stroke);
                        }
                        ui.ctx().set_cursor_icon(if copy { egui::CursorIcon::Copy } else { egui::CursorIcon::Grabbing });
                        if released.is_some() && command.is_none() {
                            command = Some(if copy {
                                copy_pages(&ids(), doc.pages[*index].id, after)
                            } else if fixed {
                                Command::with_params("page.move", json!({"ids": ids(),
                                    "destination": {"anchor_id": doc.pages[*index].id, "after": after, "new_spread": false}}))
                            } else {
                                Command::with_params("page.move", json!({"ids": ids(), "number": to + 1}))
                            });
                            if !group_drag {
                                self.selected_pages.clear();
                                self.selection_anchor = Some(id);
                            }
                        }
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
        if let Some(command) = self.show_page_transfer(ui, workspace, doc) {
            return Some(command);
        }
        if let Some(delete) = self.show_page_delete(ui, doc) {
            return Some(delete);
        }
        if command.is_some() {
            return command;
        }
        Some(Command::with_params("page.navigate", json!({"id": chosen?})))
    }

    fn show_page_delete(&mut self, ui: &mut egui::Ui, doc: &dtool_core::Document) -> Option<Command> {
        if !self.delete_pages.is_empty() {
            use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
            let indices: Vec<_> = doc.pages.iter().enumerate()
                .filter_map(|(index, page)| self.delete_pages.contains(&page.id).then_some(index))
                .collect();
            if indices.len() == self.delete_pages.len() && indices.len() < doc.pages.len() {
                let (title, message) = if let [index] = indices.as_slice() {
                    ("panel.pages.delete",
                        dtool_strings::format("panel.pages.delete_confirm", &[("number", &(index + 1).to_string())]))
                } else {
                    ("panel.pages.delete_selected",
                        dtool_strings::format("panel.pages.delete_selected_confirm", &[("count", &indices.len().to_string())]))
                };
                let (event, (remove, cancel)) = show_modal(ui.ctx(), "dtool.page.delete",
                    text(title), ClosePolicy::CancelOnly, false, |ui| {
                        ui.label(&message);
                        ui.horizontal_wrapped(|ui| {
                            let remove = ui.button(text(title)).clicked();
                            let cancel = ui.button(text("panel.master.cancel")).clicked();
                            (remove, cancel)
                        }).inner
                    });
                if remove {
                    let ids = std::mem::take(&mut self.delete_pages);
                    return Some(Command::with_params("page.delete", json!({"ids": ids})));
                }
                if cancel || event == DialogResult::Cancel {
                    self.delete_pages.clear();
                }
            } else {
                self.delete_pages.clear();
            }
        }
        None
    }

    fn show_page_transfer(&mut self, ui: &mut egui::Ui, workspace: &Workspace, doc: &dtool_core::Document) -> Option<Command> {
        use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};

        let draft = self.page_transfer.as_mut()?;
        if doc.pages.is_empty() || !doc.pages.iter().map(|page| page.id).eq(draft.order.iter().copied()) {
            self.page_transfer = None;
            return None;
        }
        let destinations: Vec<_> = workspace.contexts().iter().filter(|context| {
            Some(context.id) != workspace.active()
                && context.session.document.layout_artboard_origins.is_some()
                && !context.session.document.pages.is_empty()
        }).map(|context| {
            let duplicates = workspace.contexts().iter().filter(|peer| peer.title == context.title).take(2).count();
            let label = if duplicates > 1 { format!("{} ({})", context.title, context.id.0) } else { context.title.clone() };
            (context, label)
        }).collect();
        let focused = ui.memory(|memory| memory.focused());
        let mut range_field = None;
        let (event, (accept, cancel, target, sources)) = show_modal(ui.ctx(), "dtool.page.transfer",
            text("panel.pages.transfer"), ClosePolicy::CancelOnly, true, |ui| {
                let width = (ui.ctx().content_rect().width() - 48.0).clamp(180.0, 400.0);
                ui.set_width(width);
                let mut valid = true;
                let mut settings = |ui: &mut egui::Ui| {
                    dtool_design::dialog_section(ui, |ui| {
                        let caption = ui.label(text("panel.pages.transfer_source"));
                        let id = ui.make_persistent_id("page.transfer.range");
                        range_field = Some(id);
                        ui.add(egui::TextEdit::singleline(&mut draft.pages).id(id).hint_text("1–3, 5")
                            .desired_width(ui.available_width())).labelled_by(caption.id);
                        transfer_choices(ui, &mut draft.copy, ["panel.pages.transfer_move", "panel.pages.transfer_copy"]);
                        if !destinations.is_empty() || draft.destination.is_some() {
                            let fallback = if draft.destination.is_none() { "panel.pages.transfer_same_document" }
                                else { "panel.pages.transfer_changed_destination" };
                            let title = destinations.iter().find(|(context, _)| Some(context.id) == draft.destination)
                                .map_or_else(|| text(fallback), |(_, title)| title.as_str());
                            let caption = ui.label(text("panel.pages.transfer_document"));
                            let combo_width = ui.available_width().min(280.0);
                            let combo = egui::ComboBox::from_id_salt("page.transfer.document")
                                .width(combo_width)
                                .truncate()
                                .selected_text(title)
                                .show_ui(ui, |ui| dtool_design::with_icon_selection(ui, |ui| {
                                    if ui.selectable_label(draft.destination.is_none(), text("panel.pages.transfer_same_document")).clicked() {
                                        draft.destination = None;
                                        draft.destination_order.clear();
                                        draft.target = draft.target.clamp(1.0, doc.pages.len() as f64);
                                        dtool_design::numeric_cancel_edit(ui.ctx(), draft.field);
                                    }
                                    for (context, name) in &destinations {
                                        let choice = egui::Button::selectable(draft.destination == Some(context.id), name).wrap();
                                        if ui.add(choice).clicked() {
                                            draft.destination = Some(context.id);
                                            draft.destination_order = context.session.document.pages.iter().map(|page| page.id).collect();
                                            draft.target = draft.destination_order.len() as f64;
                                            dtool_design::numeric_cancel_edit(ui.ctx(), draft.field);
                                        }
                                    }
                            }));
                            combo.response.labelled_by(caption.id).on_hover_text(title);
                        }
                        let target_count = if draft.destination.is_some() {
                            draft.destination_order.len().max(1)
                        } else {
                            doc.pages.len()
                        };
                        let caption = ui.label(text("panel.pages.transfer_target"));
                        let spec = dtool_design::NumericSpec::new("")
                            .range(1.0..=target_count as f64).decimals(0).width(96.0);
                        dtool_design::numeric_value(ui, draft.field, &mut draft.target, &spec).labelled_by(caption.id);
                        let input = dtool_design::numeric_state(ui.ctx(), draft.field);
                        if input.edited {
                            let parsed = input.buffer.as_deref().and_then(|value| spec.parse_text(value));
                            valid = parsed.is_some();
                            if let Some(value) = parsed { draft.target = value; }
                        }
                        transfer_choices(ui, &mut draft.after, ["panel.pages.transfer_before", "panel.pages.transfer_after"]);
                    });
                    let sources = dtool_core::longdoc::helpers::parse_page_ranges(&draft.pages, doc.pages.len())
                        .unwrap_or_default();
                    let current_destination = draft.destination.and_then(|id| destinations.iter()
                        .find(|(context, _)| context.id == id).map(|(context, _)| &context.session.document));
                    let destination_valid = draft.destination.is_none() || current_destination.is_some_and(|destination| {
                        destination.pages.iter().map(|page| page.id).eq(draft.destination_order.iter().copied())
                    });
                    let target_doc = current_destination.unwrap_or(doc);
                    let target = (draft.target.round() as usize).checked_sub(1).filter(|_| valid)
                        .filter(|&index| index < target_doc.pages.len()
                            && (draft.copy || draft.destination.is_some() || !sources.contains(&(index + 1))));
                    let last_page_move = draft.destination.is_some() && !draft.copy && sources.len() == doc.pages.len();
                    if sources.is_empty() {
                        ui.weak(text("panel.pages.transfer_invalid_source"));
                    } else if !destination_valid {
                        ui.weak(text("panel.pages.transfer_changed_destination"));
                    } else if last_page_move {
                        ui.weak(text("error.last_page"));
                    } else if target.is_none() && valid && !draft.copy {
                        ui.weak(text("panel.pages.transfer_selected_target"));
                    }
                    (target.filter(|_| destination_valid && !last_page_move), sources)
                };
                let available_height = ui.ctx().content_rect().height() - 104.0;
                let (target, sources) = if available_height < 325.0 {
                    dtool_design::dialog_options_scroll(ui, "page.transfer.options", available_height.max(90.0), settings)
                } else {
                    settings(ui)
                };
                let label = if draft.copy { "panel.pages.transfer_copy" } else { "panel.pages.transfer_move" };
                let (accept, cancel) = crate::dialog_chrome::confirm_cancel_buttons(
                    ui, text(label), target.is_some() && !sources.is_empty(),
                );
                (accept, cancel, target, sources)
            });
        if cancel || event == DialogResult::Cancel {
            self.page_transfer = None;
            return None;
        }
        let confirm = crate::dialog_chrome::confirm_from_enter(ui.ctx(), event, focused, range_field.as_slice());
        if let Some(target) = target && !sources.is_empty() && (accept || confirm) {
            let draft = self.page_transfer.take()?;
            let ids: Vec<_> = sources.iter().map(|&number| doc.pages[number - 1].id).collect();
            if let Some(destination) = draft.destination {
                let key = if draft.after { "after_id" } else { "before_id" };
                let command = if draft.copy { "page.copy_to_document" } else { "page.move_to_document" };
                return Some(Command::with_params(command,
                    json!({"ids": ids, "destination_context": destination, (key): draft.destination_order[target]})));
            }
            if draft.copy {
                return Some(copy_pages(&ids, doc.pages[target].id, draft.after));
            }
            let insertion = target + usize::from(draft.after);
            let removed_before = sources.partition_point(|&number| number <= insertion);
            return Some(Command::with_params("page.move", json!({"ids": ids, "number": insertion - removed_before + 1})));
        }
        None
    }

    fn show_spread_settings(&mut self, ui: &egui::Ui, doc: &dtool_core::Document) -> Option<Command> {
        use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};

        let draft = self.spread_settings.as_mut()?;
        let Some(start) = doc.pages.iter().position(|page| page.id == draft.start) else {
            self.spread_settings = None;
            return None;
        };
        let focused = ui.memory(|memory| memory.focused());
        let count_id = draft.count_field;
        let (event, (accept, cancel, valid)) = show_modal(ui.ctx(), "dtool.spread.settings",
            text("panel.pages.spread_settings"), ClosePolicy::CancelOnly, true, |ui| {
                let number = dtool_core::longdoc::sections::section_page_label(doc, start);
                ui.label(dtool_strings::format("panel.pages.number", &[("number", &number)]));
                let available = ui.ctx().content_rect().width() - 48.0;
                ui.set_width(available.clamp(180.0, 400.0));
                let mut valid = true;
                dtool_design::dialog_section(ui, |ui| {
                    dtool_design::panel_columns(ui, 2, 156.0, |ui, column| {
                        if column == 0 {
                            if dtool_design::toggle(ui, &mut draft.custom, text("panel.pages.spread_custom")).changed() {
                                dtool_design::numeric_cancel_edit(ui.ctx(), count_id);
                            }
                        } else {
                            let caption = ui.label(text("dialog.new_document.pages"));
                            let mut count = draft.count as f64;
                            let spec = dtool_design::NumericSpec::new("")
                                .range(1.0..=(doc.pages.len() - start) as f64)
                                .decimals(0)
                                .width(96.0)
                                .enabled(draft.custom);
                            dtool_design::numeric_value(ui, count_id, &mut count, &spec).labelled_by(caption.id);
                            if draft.custom {
                                let input = dtool_design::numeric_state(ui.ctx(), count_id);
                                if input.edited {
                                    let parsed = input.buffer.as_deref().and_then(|text| spec.parse_text(text));
                                    valid = parsed.is_some();
                                    if let Some(parsed) = parsed { count = parsed; }
                                }
                            }
                            draft.count = count.round() as usize;
                        }
                    });
                });
                let (accept, cancel) = crate::dialog_chrome::confirm_cancel_buttons(ui, text("common.ok"), valid);
                (accept, cancel, valid)
            });
        if cancel || event == DialogResult::Cancel {
            dtool_design::numeric_cancel_edit(ui.ctx(), count_id);
            self.spread_settings = None;
            return None;
        }
        let confirm = crate::dialog_chrome::confirm_from_enter(ui.ctx(), event, focused, &[]);
        if valid && (accept || confirm) {
            dtool_design::numeric_cancel_edit(ui.ctx(), count_id);
            let draft = self.spread_settings.take()?;
            let count = draft.custom.then_some(draft.count);
            return Some(Command::with_params("page.spread.set", json!({"id": draft.start, "count": count})));
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
        let focused = ui.memory(|memory| memory.focused());
        let dimensions_valid = |size: [f32; 2]| size.iter().all(|side| (0.01..=1_000_000.0).contains(side));
        let (event, (accept, cancel, valid)) = show_modal(ui.ctx(), "dtool.page.settings",
            text("panel.pages.settings"), ClosePolicy::CancelOnly, can_resize, |ui| {
                let width = (ui.ctx().content_rect().width() - 48.0).clamp(180.0, 430.0);
                ui.set_width(width);
                let settings = |ui: &mut egui::Ui| {
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
                    dtool_design::dialog_section(ui, |ui| {
                        dtool_design::panel_columns(ui, 3, 122.0, |ui, column| {
                            if column == 0 {
                                let caption = ui.label(text("doc_setup.size_preset"));
                                let preset = egui::ComboBox::from_id_salt("page.settings.preset")
                                    .width(ui.available_width().min(132.0))
                                    .truncate()
                                    .selected_text(selected.unwrap_or_else(|| text("doc_setup.size_custom")))
                                    .show_ui(ui, |ui| dtool_design::with_icon_selection(ui, |ui| {
                                        for name in presets {
                                            if ui.selectable_label(selected == Some(name), name).clicked()
                                                && let Some(size) = dimensions(name)
                                            {
                                                draft.size = size;
                                                draft.invalid_dimensions = [false; 2];
                                                for id in ids { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
                                            }
                                        }
                                    }));
                                preset.response.labelled_by(caption.id);
                                return;
                            }
                            let axis = column - 1;
                            let label = ["doc_setup.page_width", "doc_setup.page_height"][axis];
                            let caption = ui.label(text(label));
                            let spec = dtool_design::NumericSpec::new("").width(116.0).suffix(unit);
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
                        });
                    });
                    let invalid_input = draft.invalid_dimensions.contains(&true);
                    ui.horizontal_wrapped(|ui| dtool_design::with_icon_selection(ui, |ui| {
                        ui.label(text("doc_setup.orientation"));
                        let landscape = draft.size[0] > draft.size[1];
                        for (label, direction) in [("doc_setup.portrait", false), ("doc_setup.landscape", true)] {
                            let button = egui::Button::selectable(direction == landscape, text(label));
                            if ui.add_enabled(!invalid_input && draft.size[0] != draft.size[1], button).clicked() && direction != landscape {
                                draft.size.swap(0, 1);
                                for id in ids { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
                            }
                        }
                    }));
                    if dimensions_valid(draft.size) {
                        let (area, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 80.0), egui::Sense::hover());
                        crate::dialogs::paint_page_preview(ui, area.center(), draft.size[0], draft.size[1], 66.0);
                    }
                    ui.separator();
                    let fold_valid = dtool_design::dialog_section(ui, |ui| {
                        fold_settings(ui, draft, unit, doc.units, doc.dpi)
                    }).inner;
                    ui.separator();
                    let size_valid = !invalid_input && dimensions_valid(draft.size);
                    let valid = size_valid && fold_valid;
                    if !size_valid { ui.label(text("panel.pages.invalid_size")); }
                    valid
                };
                let height = (ui.ctx().content_rect().height() - 104.0).clamp(90.0, 385.0);
                let valid = dtool_design::dialog_options_scroll(ui, "page.settings.options", height, settings);
                let (accept, cancel) = crate::dialog_chrome::confirm_cancel_buttons(ui, text("common.ok"), can_resize && valid);
                (accept, cancel, valid)
            });
        let cancelled = cancel || event == DialogResult::Cancel;
        let confirm = crate::dialog_chrome::confirm_from_enter(ui.ctx(), event, focused, &[]);
        if cancelled || (can_resize && valid && (accept || confirm)) {
            for id in ids.into_iter().chain(ids.map(|id| id.with("fold"))) {
                dtool_design::numeric_cancel_edit(ui.ctx(), id);
            }
            let draft = self.page_settings.take()?;
            if cancelled { return None; }
            let fold = draft.fold.and_then(|fold| fold.positions(draft.size)
                .map(|positions| json!({"axis": fold.axis, "positions": positions})));
            return Some(Command::with_params("page.resize", json!({"id": draft.id, "size": draft.size, "fold": fold})));
        }
        None
    }

    fn show_masters(
        &mut self, ui: &mut egui::Ui, session: &Session, view_size: [f32; 2],
        pressed: Option<egui::Pos2>, released: Option<egui::Pos2>,
    ) -> Option<Command> {
        let doc = &session.document;
        let mut chosen = None;
        let mut command = None;
        let canvas_master = session.selection.ids.iter().copied().find(|id| doc.master_pages.roots.contains(id));
        if canvas_master != self.canvas_master_selection {
            self.canvas_master_selection = canvas_master;
            if let Some(id) = canvas_master {
                self.select_master(id);
            }
        }
        let selected = self.selected_master.or_else(|| crate::menus::pages::viewed_master(session))
            .filter(|id| doc.master_pages.roots.contains(id)).map(|id| doc.master_pages.spread_id(id));
        let ([add, duplicate, delete], header) = section_header(ui, &mut self.icons, &mut self.masters_open, text("panel.master.section"), [
            ("file-new", "panel.master.add", doc.layout_artboard_origins.is_some()),
            ("edit-duplicate", "panel.master.duplicate", selected.is_some()),
            ("layer-delete", "panel.master.delete", selected.is_some()),
        ]);
        if doc.layout_artboard_origins.is_some() {
            header.context_menu(|ui| {
                let available = !unused_master_spreads(doc).is_empty();
                if ui.add_enabled_ui(available, |ui| dtool_design::menu_item(ui, text("panel.master.delete_unused"))).inner.clicked() {
                    self.delete_unused_masters = true;
                    ui.close();
                }
            });
        }
        if add {
            self.masters_open = true;
            self.master_create = Some(master_create::Draft::new(doc));
        }
        if duplicate && let Some(id) = selected {
            return Some(Command::with_params("master.duplicate", json!({"id": id})));
        }
        if delete {
            self.delete_master = selected;
        }
        let mut scene = None;
        let roots: Vec<_> = doc.master_pages.spread_roots().collect();
        if self.masters_open && !roots.is_empty() {
            let max_height = (ui.available_height() * 0.5).max(80.0);
            let thumbnail_size = self.thumbnail_size_for(doc);
            let preview_height = thumbnail_size.height(ui.available_width(), max_height);
            egui::ScrollArea::vertical().id_salt("pages.master_thumbnails").max_height(max_height)
                .show_rows(ui, preview_height + 30.0, roots.len(), |ui, rows| {
            for index in rows {
                let id = roots[index];
                let Some(root) = doc.get_object(id) else { continue };
                let master_pages = doc.master_pages.spread_pages(&id);
                let page_rects: Vec<_> = master_pages.iter().filter_map(|&id| doc.get_object(id))
                    .map(dtool_core::geometry::object_world_bounds).collect();
                let Some(bounds) = dtool_core::commands::align::union_rects(&page_rects) else { continue };
                let scale = ((ui.available_width() - 12.0).clamp(1.0, thumbnail_size.width_limit()) / bounds[2])
                    .min(preview_height / bounds[3]);
                let size = vec2(bounds[2] * scale, bounds[3] * scale);
                let is_selected = selected == Some(id);
                let row = ui.vertical_centered(|ui| {
                    if size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0 {
                        let (area, _) = ui.allocate_exact_size(vec2(size.x, preview_height + 4.0), egui::Sense::hover());
                        let preview = egui::Rect::from_center_size(area.center(), size);
                        let preview_rect = |[x, y, w, h]: [f32; 4]| egui::Rect::from_min_size(
                            preview.min + vec2(x - bounds[0], y - bounds[1]) * scale, vec2(w, h) * scale);
                        for &rect in &page_rects {
                            ui.painter().rect_filled(preview_rect(rect), 0.0, Color32::WHITE);
                        }
                        if let Some(texture) = self.master_thumbnails.get(&id) {
                            ui.painter().image(texture.id(), preview,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
                        } else {
                            let mut scoped = scene.get_or_insert_with(|| build_scene(doc, &CanvasFlags::default())).clone();
                            let members = master_pages.iter().flat_map(|&id| doc.subtree_ids(id)).collect();
                            dtool_export::retain_page_scene(&mut scoped, &[], &members);
                            let pixels = [(size.x * 1.5).round().max(1.0) as u32, (size.y * 1.5).round().max(1.0) as u32];
                            let rgba = rasterize_scene(doc, &scoped, bounds, pixels[0], pixels[1], None);
                            let image = ColorImage::from_rgba_unmultiplied(pixels.map(|value| value as usize), &rgba);
                            let texture = ui.ctx().load_texture(format!("master-thumbnail-{}", id.0), image, TextureOptions::LINEAR);
                            ui.painter().image(texture.id(), preview,
                                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
                            self.master_thumbnails.insert(id, texture);
                        }
                        for (index, &rect) in page_rects.iter().enumerate().skip(1) {
                            let page = preview_rect(rect);
                            let key = if doc.master_pages.spread(id).is_some_and(|spread| spread.spine == index) {
                                "accent"
                            } else {
                                "dialog.bg"
                            };
                            ui.painter().vline(page.left(), page.y_range(), egui::Stroke::new(1.0, dtool_design::color(key)));
                        }
                        if let Some(colour) = doc.master_pages.tag_colour(id).and_then(dtool_design::tokens::parse_hex) {
                            draw_thumbnail_tags(ui, preview, &[colour], false);
                        }
                        if is_selected {
                            thumbnail_outline(ui, preview);
                        }
                    }
                    ui.add(egui::Label::new(master_name(root)).truncate());
                });
                let response = ui.interact(row.response.rect, ui.id().with(("master.thumbnail", id.0)), egui::Sense::click_and_drag())
                    .on_hover_ui(|ui| {
                        ui.label(master_name(root));
                        if let Some(base) = doc.master_pages.for_master(id).and_then(|item| doc.get_object(item.inherited.master)) {
                            ui.label(format!("{} · {}", text("panel.master.base"), master_name(base)));
                        }
                    });
                let hitbox = response.interact_rect.intersect(ui.clip_rect());
                if let Some(start) = pressed.filter(|&point| response.enabled() && hitbox.contains(point)
                    && ui.ctx().layer_id_at(point) == Some(response.layer_id))
                {
                    self.master_drag = Some((id, start));
                }
                let pointer = released.or_else(|| ui.input(|input| input.pointer.interact_pos()));
                if let Some((source, start)) = self.master_drag
                    && pointer.is_some_and(|point| hitbox.contains(point) && start.distance(point) >= 4.0)
                    && doc.master_pages.can_set_spread_base(id, source)
                {
                    thumbnail_outline(ui, row.response.rect.shrink(4.0));
                    if released.is_some() {
                        self.master_drag = None;
                        self.master_base = Some(MasterBaseDraft { target: id, base: Some(source), keep_previous: false });
                    }
                }
                if response.clicked() {
                    self.selected_master = Some(id);
                    chosen = Some(bounds);
                }
                response.context_menu(|ui| {
                    ui.menu_button(text("panel.pages.tag_colour"), |ui| {
                        if let Some(colour) = super::layers::tag_colour_picker(ui, Some(doc.master_pages.tag_colour(id))) {
                            command = Some(Command::with_params("master.tag.set", json!({"id": id, "colour": colour})));
                        }
                    });
                    if ui.button(text("panel.master.apply_range")).clicked() {
                        self.master_range = Some(MasterRangeDraft {
                            master: root.id,
                            pages: dtool_core::commands::pages::viewed_page(session)
                                .and_then(|id| doc.pages.iter().position(|page| page.id == id))
                                .map_or_else(|| "1".to_owned(), |index| (index + 1).to_string()),
                            replace: false,
                            add: false,
                            keep_previous: true,
                            conform: MasterConform::None,
                            source_page: None,
                            placement: MasterPlacement::default(),
                            target_group: None,
                        });
                        ui.close();
                    }
                    if ui.button(text("panel.master.options")).clicked() {
                        self.master_draft = Some(MasterDraft {
                            id: root.id,
                            name: master_name(root).to_owned(),
                            size: root.size,
                            initial_size: root.size,
                        });
                        ui.close();
                    }
                    if ui.button(text("panel.master.set_base")).clicked() {
                        self.master_base = Some(MasterBaseDraft {
                            target: id,
                            base: doc.master_pages.for_master(id).map(|item| doc.master_pages.spread_id(item.inherited.master)),
                            keep_previous: false,
                        });
                        ui.close();
                    }
                });
            }
            });
        }
        if self.delete_unused_masters {
            use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
            let unused = unused_master_spreads(doc);
            let available = !unused.is_empty();
            let (event, (remove, cancel)) = show_modal(ui.ctx(), "dtool.master.delete_unused",
                text("panel.master.delete_unused"), ClosePolicy::CancelOnly, false, |ui| {
                    ui.set_width((ui.ctx().content_rect().width() - 44.0).clamp(140.0, 300.0));
                    dtool_design::dialog_section(ui, |ui| {
                        ui.label(dtool_strings::format("panel.master.delete_unused_confirm",
                            &[("count", &unused.len().to_string())]));
                        egui::ScrollArea::vertical().max_height(88.0).auto_shrink([false, true]).show(ui, |ui| {
                            for id in &unused {
                                if let Some(root) = doc.get_object(*id) {
                                    let name = master_name(root);
                                    ui.add(egui::Label::new(name).truncate()).on_hover_text(name);
                                }
                            }
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        let remove = dtool_design::button_with(ui, text("button.delete"), &dtool_design::ControlState::enabled(available)).clicked();
                        let cancel = dtool_design::button(ui, text("button.cancel")).clicked();
                        (remove, cancel)
                    }).inner
                });
            if cancel || event == DialogResult::Cancel || remove {
                self.delete_unused_masters = false;
            }
            if remove {
                if self.selected_master.is_some_and(|id| unused.contains(&doc.master_pages.spread_id(id))) {
                    self.selected_master = None;
                }
                return Some(Command::with_params("master.delete_unused", json!({})));
            }
        }
        if let Some(id) = self.delete_master {
            use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
            if let Some(root) = doc.get_object(id).filter(|_| doc.master_pages.roots.contains(&id)) {
                let name = master_name(root);
                let (event, (remove, cancel)) = show_modal(ui.ctx(), "dtool.master.delete", text("panel.master.delete"),
                    ClosePolicy::CancelOnly, false, |ui| {
                        ui.label(dtool_strings::format("panel.master.delete_confirm", &[("name", name)]));
                        ui.horizontal_wrapped(|ui| {
                            let remove = dtool_design::button(ui, text("panel.master.delete")).clicked();
                            let cancel = dtool_design::button(ui, text("button.cancel")).clicked();
                            (remove, cancel)
                        }).inner
                    });
                if remove {
                    self.delete_master = None;
                    self.selected_master = None;
                    return Some(Command::with_params("master.delete", json!({"id": id})));
                }
                if cancel || event == DialogResult::Cancel {
                    self.delete_master = None;
                }
            } else {
                self.delete_master = None;
            }
        }
        if let Some(command) = self.show_master_range(ui, doc) {
            return Some(command);
        }
        if let Some(command) = self.show_master_base(ui, doc) {
            return Some(command);
        }
        if let Some(draft) = &mut self.master_create {
            match draft.show(ui.ctx(), doc) {
                Some(crate::dialog_chrome::DialogResult::Confirm) => {
                    let draft = self.master_create.take()?;
                    return Some(draft.command());
                }
                Some(crate::dialog_chrome::DialogResult::Cancel) => self.master_create = None,
                _ => {}
            }
        }
        if let Some(draft) = &mut self.master_draft {
            use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
            let enabled = !draft.name.trim().is_empty() && draft.size.iter().all(|n| n.is_finite() && *n > 0.0);
            let (event, (save, cancel)) = show_modal(ui.ctx(), "dtool.master.options", text("panel.master.options"),
                ClosePolicy::CancelOnly, enabled, |ui| {
                    let width = (ui.ctx().content_rect().width() - 44.0).clamp(160.0, 340.0);
                    ui.set_width(width);
                    dtool_design::dialog_section(ui, |ui| {
                        let name = ui.label(text("panel.master.name"));
                        ui.add(egui::TextEdit::singleline(&mut draft.name).char_limit(128)
                            .desired_width(ui.available_width())).labelled_by(name.id);
                        ui.add_space(dtool_design::space("xs"));
                        let unit = crate::captions::units_identity(doc.units);
                        dtool_design::panel_columns(ui, 2, 116.0, |ui, axis| {
                            let label = ["panel.master.width", "panel.master.height"][axis];
                            let caption = ui.label(text(label));
                            let id = ui.make_persistent_id(label);
                            let spec = dtool_design::NumericSpec::new("")
                                .range(0.01..=1_000_000.0).suffix(unit);
                            let mut value = f64::from(draft.size[axis]);
                            dtool_design::numeric_value(ui, id, &mut value, &spec).labelled_by(caption.id);
                            draft.size[axis] = value as f32;
                        });
                    });
                    ui.horizontal_wrapped(|ui| {
                        let ready = !draft.name.trim().is_empty() && draft.size.iter().all(|n| n.is_finite() && *n > 0.0);
                        let save = dtool_design::button_with(ui, text("common.ok"), &dtool_design::ControlState::enabled(ready)).clicked();
                        let cancel = dtool_design::button(ui, text("panel.master.cancel")).clicked();
                        (save, cancel)
                    }).inner
                });
            if save || event == DialogResult::Confirm {
                let draft = self.master_draft.take()?;
                let mut params = json!({"id": draft.id, "name": draft.name});
                if draft.size != draft.initial_size {
                    params["size"] = json!(draft.size);
                }
                return Some(Command::with_params("master.update", params));
            }
            if cancel || event == DialogResult::Cancel {
                self.master_draft = None;
            }
        }
        command.or_else(|| navigate(session, chosen?, view_size))
    }

    fn show_master_base(&mut self, ui: &mut egui::Ui, doc: &dtool_core::Document) -> Option<Command> {
        use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};

        let draft = self.master_base.as_mut()?;
        let Some(target) = doc.get_object(draft.target).filter(|root| doc.master_pages.roots.contains(&root.id)) else {
            self.master_base = None;
            return None;
        };
        let previous = doc.master_pages.for_master(draft.target).map(|item| doc.master_pages.spread_id(item.inherited.master));
        let focused = ui.memory(|memory| memory.focused());
        let (event, (apply, cancel, ready)) = show_modal(ui.ctx(), "dtool.master.base",
            text("panel.master.set_base"), ClosePolicy::CancelOnly, true, |ui| {
                let width = (ui.ctx().content_rect().width() - 44.0).clamp(160.0, 340.0);
                ui.set_width(width);
                master_dialog_settings(ui, "master.base.settings", width, 160.0, |ui| {
                    ui.label(master_name(target));
                    dtool_design::dialog_section(ui, |ui| {
                        let caption = ui.label(text("panel.master.base"));
                        let selected = draft.base.and_then(|id| doc.get_object(id)).map_or_else(
                            || text("panel.master.base_none"), master_name,
                        );
                        let combo_width = (ui.ctx().content_rect().width() - 56.0).clamp(140.0, 280.0);
                        let combo = egui::ComboBox::from_id_salt("panel.master.base")
                            .width(combo_width)
                            .truncate()
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                dtool_design::choice_value(ui, &mut draft.base, None, text("panel.master.base_none"));
                                for candidate in doc.master_pages.spread_roots() {
                                    if doc.master_pages.can_set_spread_base(draft.target, candidate)
                                        && let Some(root) = doc.get_object(candidate)
                                    {
                                        let name = master_name(root);
                                        let choice = egui::Button::selectable(draft.base == Some(candidate), name).truncate();
                                        let response = dtool_design::with_icon_selection(ui, |ui| ui.add(choice).on_hover_text(name));
                                        if response.clicked() {
                                            draft.base = Some(candidate);
                                        }
                                    }
                                }
                            });
                        combo.response.labelled_by(caption.id).on_hover_text(selected);
                        if previous.is_some() && draft.base.is_some() && draft.base != previous {
                            dtool_design::toggle(ui, &mut draft.keep_previous, text("panel.master.replace_keep"));
                        }
                    });
                });
                let ready = draft.base != previous && draft.base.is_none_or(|id| doc.master_pages.can_set_spread_base(draft.target, id));
                let (apply, cancel) = crate::dialog_chrome::confirm_cancel_buttons(ui, text("common.ok"), ready);
                (apply, cancel, ready)
            });
        if cancel || event == DialogResult::Cancel {
            self.master_base = None;
            return None;
        }
        if ready && (apply || crate::dialog_chrome::confirm_from_enter(ui.ctx(), event, focused, &[])) {
            let draft = self.master_base.take()?;
            return Some(Command::with_params("master.base.set", json!({
                "id": draft.target, "master_id": draft.base, "keep_previous": draft.keep_previous
            })));
        }
        None
    }

    fn show_master_range(&mut self, ui: &mut egui::Ui, doc: &dtool_core::Document) -> Option<Command> {
        use crate::dialog_chrome::{ClosePolicy, DialogResult, show_modal};
        use dtool_core::longdoc::helpers::parse_page_ranges;

        let draft = self.master_range.as_mut()?;
        if let Some(group) = draft.target_group {
            let Some(binding) = doc.master_pages.for_group(group) else {
                self.master_range = None;
                return None;
            };
            let name = doc.get_object(binding.master).map_or(text("panel.master"), master_name);
            let name = application_label(name, binding.stack_slot);
            let focused = ui.memory(|memory| memory.focused());
            let (result, (apply, cancel)) = show_modal(ui.ctx(), "dtool.master.placement", text("panel.master.placement_options"),
                ClosePolicy::CancelOnly, true, |ui| {
                    let width = (ui.ctx().content_rect().width() - 44.0).clamp(160.0, 340.0);
                    ui.set_width(width);
                    ui.add(egui::Label::new(name.as_str()).truncate()).on_hover_text(&name);
                    master_dialog_settings(ui, "master.placement.settings", width, 190.0, |ui| {
                        master_placement_controls(ui, &mut draft.placement);
                    });
                    crate::dialog_chrome::confirm_cancel_buttons(ui, text("panel.master.apply"), true)
                });
            if cancel || result == DialogResult::Cancel {
                self.master_range = None;
                return None;
            }
            if apply || crate::dialog_chrome::confirm_from_enter(ui.ctx(), result, focused, &[]) {
                let placement = self.master_range.take()?.placement;
                return Some(Command::with_params("master.apply", json!({
                    "master_id": binding.master, "page_id": binding.page, "group_id": group, "placement": placement
                })));
            }
            return None;
        }
        let masters: Vec<_> = doc.master_pages.spread_roots().filter_map(|id| doc.get_object(id)).collect();
        let Some(mut selected) = masters.iter().position(|master| master.id == draft.master) else {
            self.master_range = None;
            return None;
        };
        let names: Vec<_> = masters.iter().map(|master| master_name(master)).collect();
        let rects = dtool_core::scene::page_rects(doc);
        let status = |pages: &[usize], source, slot, conform| {
            let targets: Vec<_> = pages.iter().map(|&number| (number - 1, doc.pages[number - 1].id)).collect();
            if conform == MasterConform::Spread {
                return match dtool_core::commands::masters::preview_spread_conform(doc, source, &targets) {
                    Ok(conflict) => (conflict, false),
                    Err(_) => (false, true),
                };
            }
            let Ok(sources) = dtool_core::commands::masters::application_sources(doc, source, &targets, slot) else {
                return (true, true);
            };
            let mut conflict = false;
            let mut invalid_size = false;
            for (&(index, page), source) in targets.iter().zip(sources) {
                conflict |= doc.master_pages.for_page(page).is_some_and(|item| item.master != source);
                let [_, _, w, h] = rects[index];
                invalid_size |= doc.get_object(source).is_none_or(|root| !dtool_core::commands::pages::valid_page_size(root.size)
                    || ([w, h] != root.size && !dtool_core::commands::pages::can_resize(doc, page)));
            }
            (conflict, invalid_size)
        };
        let pages = parse_page_ranges(&draft.pages, doc.pages.len()).unwrap_or_default();
        let (has_conflict, invalid_size) = status(&pages, draft.master, draft.source_page, draft.conform);
        let invalid_size = draft.conform != MasterConform::None && invalid_size;
        let enabled = !pages.is_empty() && (draft.add || draft.replace || !has_conflict) && !invalid_size;
        let focused = ui.memory(|memory| memory.focused());
        let mut range_field = None;
        let (event, (apply, cancel)) = show_modal(ui.ctx(), "dtool.master.range", text("panel.master.apply_options"),
            ClosePolicy::CancelOnly, enabled, |ui| {
                let width = (ui.ctx().content_rect().width() - 44.0).clamp(160.0, 340.0);
                ui.set_width(width);
                let settings = |ui: &mut egui::Ui| {
                    dtool_design::dialog_section(ui, |ui| {
                        ui.label(text("panel.master"));
                        if dtool_design::dropdown(ui, "master.apply.source", &mut selected, &names).changed() {
                            draft.master = masters[selected].id;
                            draft.source_page = None;
                        }
                        let page_count = doc.master_pages.spread_pages(&draft.master).len();
                        if page_count > 1 && draft.conform != MasterConform::Spread {
                            ui.label(text("panel.master.source_page"));
                            let labels: Vec<_> = std::iter::once(text("panel.master.align_spine").to_owned())
                                .chain((1..=page_count).map(|page| page.to_string())).collect();
                            let labels: Vec<_> = labels.iter().map(String::as_str).collect();
                            let mut slot = draft.source_page.map_or(0, |index| index + 1);
                            if dtool_design::dropdown(ui, "master.apply.source_page", &mut slot, &labels).changed() {
                                draft.source_page = slot.checked_sub(1);
                            }
                        }
                        let caption = ui.label(text("panel.master.range_label"));
                        let id = ui.make_persistent_id("master.apply.range");
                        range_field = Some(id);
                        ui.add(egui::TextEdit::singleline(&mut draft.pages)
                            .id(id).desired_width(ui.available_width())
                            .hint_text("1–3, 5")).labelled_by(caption.id);
                    });
                    if pages.is_empty() {
                        ui.weak(text("panel.master.range_invalid"));
                    } else if has_conflict && !draft.add && !draft.replace {
                        ui.weak(text("panel.master.range_conflict"));
                    } else if invalid_size {
                        ui.weak(text("panel.master.conform_unavailable"));
                    }
                    dtool_design::dialog_section(ui, |ui| {
                        ui.label(text("panel.master.conform"));
                        let modes = [MasterConform::None, MasterConform::Size, MasterConform::Spread];
                        let labels = ["panel.master.conform_none", "panel.master.conform_size", "panel.master.conform_spread"]
                            .map(text);
                        let mut choice = modes.iter().position(|mode| *mode == draft.conform).unwrap_or(0);
                        if dtool_design::dropdown(ui, "master.apply.conform", &mut choice, &labels).changed() {
                            draft.conform = modes[choice];
                            if draft.conform == MasterConform::Spread {
                                draft.source_page = None;
                                draft.add = false;
                            }
                        }
                        if draft.conform == MasterConform::Spread {
                            ui.weak(text("panel.master.conform_spread_scope"));
                        }
                    });
                    master_placement_controls(ui, &mut draft.placement);
                    if dtool_design::toggle_with(ui, &mut draft.add, text("panel.master.add_application"),
                        &dtool_design::ControlState::enabled(draft.conform != MasterConform::Spread)).changed() && draft.add {
                        draft.replace = false;
                    }
                    dtool_design::panel_columns(ui, 2, 145.0, |ui, option| match option {
                        0 => { dtool_design::toggle_with(ui, &mut draft.replace, text("panel.master.replace"),
                            &dtool_design::ControlState::enabled(!draft.add)); }
                        _ => { dtool_design::toggle_with(ui, &mut draft.keep_previous, text("panel.master.replace_keep"),
                            &dtool_design::ControlState::enabled(draft.replace && !draft.add)); }
                    });
                };
                master_dialog_settings(ui, "master.apply.settings", width, 476.0, settings);
                crate::dialog_chrome::confirm_cancel_buttons(ui, text("panel.master.apply"), enabled)
            });
        if cancel || event == DialogResult::Cancel {
            self.master_range = None;
            return None;
        }
        let confirm = crate::dialog_chrome::confirm_from_enter(ui.ctx(), event, focused, range_field.as_slice());
        if enabled && (apply || confirm) {
            let pages = parse_page_ranges(&draft.pages, doc.pages.len()).ok()?;
            let (conflict, invalid_size) = status(&pages, draft.master, draft.source_page, draft.conform);
            if pages.is_empty() || (conflict && !draft.add && !draft.replace)
                || (draft.conform != MasterConform::None && invalid_size) {
                return None;
            }
            let ids: Vec<_> = pages.into_iter().map(|number| doc.pages[number - 1].id).collect();
            let draft = self.master_range.take()?;
            let mut params = json!({
                "master_id": draft.master, "page_ids": ids, "replace": draft.replace, "add": draft.add,
                "keep_previous": draft.replace && draft.keep_previous,
                "conform_layout": draft.conform == MasterConform::Size,
                "conform_spread": draft.conform == MasterConform::Spread,
                "placement": draft.placement
            });
            if let Some(slot) = draft.source_page {
                params["source_page"] = json!(slot + 1);
            }
            return Some(Command::with_params("master.apply", params));
        }
        None
    }
}

pub(super) fn application_label(name: &str, slot: u32) -> String {
    if slot == 0 {
        name.to_owned()
    } else {
        format!("{} · {name}", u64::from(slot) + 1)
    }
}

fn drop_modifiers(ui: &egui::Ui) -> egui::Modifiers {
    ui.input(|input| {
        input.events.iter().rev().find_map(|event| match event {
            egui::Event::PointerButton {
                button: egui::PointerButton::Primary, pressed: false, modifiers, ..
            } => Some(*modifiers),
            _ => None,
        }).unwrap_or(input.modifiers)
    })
}

fn scroll_drag_at_edge(ui: &mut egui::Ui, pointer_y: f32) {
    let clip = ui.clip_rect();
    let depth = (clip.top() + 24.0 - pointer_y).max(0.0) - (pointer_y - (clip.bottom() - 24.0)).max(0.0);
    if depth != 0.0 {
        let elapsed = ui.input(|input| input.stable_dt.min(0.05));
        ui.scroll_with_delta_animation(vec2(0.0, depth * 60.0 * elapsed), egui::style::ScrollAnimation::none());
        ui.ctx().request_repaint();
    }
}

fn master_drop(ui: &egui::Ui, source: ObjectId, ids: Vec<PageId>) -> Command {
    let add = drop_modifiers(ui).ctrl;
    Command::with_params("master.apply", json!({
        "master_id": source, "page_ids": ids, "replace": !add, "add": add
    }))
}

fn master_dialog_settings(ui: &mut egui::Ui, id: &'static str, width: f32, content_height: f32, settings: impl FnOnce(&mut egui::Ui)) {
    let available_height = ui.ctx().content_rect().height() - 104.0;
    if available_height < content_height || width < 260.0 {
        dtool_design::dialog_options_scroll(ui, id, available_height.clamp(90.0, content_height), settings);
    } else {
        settings(ui);
    }
}

fn master_placement_controls(ui: &mut egui::Ui, placement: &mut MasterPlacement) {
    use dtool_core::transform::Anchor;
    const MODES: [ScaleMode; 4] = [ScaleMode::None, ScaleMode::Stretch, ScaleMode::Fit, ScaleMode::Fill];
    const ANCHORS: [Anchor; 9] = [
        Anchor::TopLeft, Anchor::Top, Anchor::TopRight, Anchor::Left, Anchor::Centre, Anchor::Right,
        Anchor::BottomLeft, Anchor::Bottom, Anchor::BottomRight,
    ];
    let modes = ["panel.master.scale.none", "panel.master.scale.stretch", "panel.master.scale.fit", "panel.master.scale.fill"].map(text);
    let anchors = [
        "frame.anchor.top_left", "frame.anchor.top", "frame.anchor.top_right",
        "frame.anchor.left", "frame.anchor.center", "frame.anchor.right",
        "frame.anchor.bottom_left", "frame.anchor.bottom", "frame.anchor.bottom_right",
    ].map(text);
    dtool_design::dialog_section(ui, |ui| {
        dtool_design::panel_columns(ui, 2, 132.0, |ui, column| {
            if column == 0 {
                ui.label(text("panel.master.scale"));
                let mut index = MODES.iter().position(|mode| *mode == placement.scale).unwrap_or_default();
                if dtool_design::dropdown(ui, "master.placement.scale", &mut index, &modes).changed() {
                    placement.scale = MODES[index];
                }
            } else {
                ui.label(text("frame.anchor"));
                let mut index = ANCHORS.iter().position(|anchor| *anchor == placement.anchor).unwrap_or_default();
                if dtool_design::dropdown(ui, "master.placement.anchor", &mut index, &anchors).changed() {
                    placement.anchor = ANCHORS[index];
                }
            }
        });
        dtool_design::toggle_with(ui, &mut placement.scale_strokes, text("panel.master.scale_strokes"),
            &dtool_design::ControlState::enabled(placement.scale != ScaleMode::None));
    });
}

fn fold_settings(ui: &mut egui::Ui, draft: &mut PageSettingsDraft, unit: &str, units: dtool_core::document::Units, dpi: f32) -> bool {
    let field_ids = draft.dimension_fields.map(|id| id.with("fold"));
    let mut enabled = draft.fold.is_some();
    if dtool_design::toggle(ui, &mut enabled, text("panel.pages.fold")).changed() {
        draft.fold = enabled.then(|| PageFold::equal(FoldAxis::Vertical));
        for id in field_ids {
            dtool_design::numeric_cancel_edit(ui.ctx(), id);
        }
    }
    let Some(fold) = &mut draft.fold else { return true };
    ui.horizontal_wrapped(|ui| {
        if dtool_design::button(ui, text("panel.pages.fold.a4")).clicked() {
            let factor = dtool_core::units::per_inch(units, dpi) / 25.4;
            draft.size = [297.0 * factor, 210.0 * factor];
            for id in field_ids { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
        }
        if dtool_design::button(ui, text("panel.pages.fold.equal")).clicked() {
            *fold = PageFold::equal(fold.axis);
            for id in field_ids { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(text("panel.pages.fold.direction"));
        let vertical = dtool_design::choice_value(ui, &mut fold.axis, FoldAxis::Vertical, text("panel.pages.fold.vertical"));
        let horizontal = dtool_design::choice_value(ui, &mut fold.axis, FoldAxis::Horizontal, text("panel.pages.fold.horizontal"));
        if vertical.changed() || horizontal.changed() {
            for id in field_ids { dtool_design::numeric_cancel_edit(ui.ctx(), id); }
        }
    });
    let Some(mut positions) = fold.positions(draft.size) else { return false };
    let extent = draft.size[fold.axis.index()];
    let min_gap = extent * f32::EPSILON * 2.0;
    let mut valid = true;
    let limits = [(min_gap, positions[1] - min_gap), (positions[0] + min_gap, extent - min_gap)];
    dtool_design::panel_columns(ui, 2, 128.0, |ui, index| {
        let label = ["panel.pages.fold.first", "panel.pages.fold.second"][index];
        let caption = ui.label(text(label));
        let (min, max) = limits[index];
        let spec = dtool_design::NumericSpec::new("")
            .range(f64::from(min)..=f64::from(max.max(min)))
            .step(f64::from(extent) / 500.0)
            .decimals(3)
            .width(116.0)
            .suffix(unit);
        let mut value = f64::from(positions[index]);
        dtool_design::numeric_value(ui, field_ids[index], &mut value, &spec).labelled_by(caption.id);
        let input = dtool_design::numeric_state(ui.ctx(), field_ids[index]);
        if input.edited {
            let parsed = input.buffer.as_deref().and_then(|text| spec.parse_text(text));
            valid &= parsed.is_some();
            if let Some(parsed) = parsed { value = parsed; }
        }
        positions[index] = value as f32;
    });
    if valid && fold.positions(draft.size) != Some(positions) {
        if let Some(next) = PageFold::from_positions(fold.axis, positions, draft.size) {
            *fold = next;
        } else {
            valid = false;
        }
    }
    let [first, second] = fold.positions(draft.size).unwrap_or(positions);
    let lengths = [first, second - first, extent - second];
    ui.weak(format!("{}  {:.2} / {:.2} / {:.2} {unit}",
        text("panel.pages.fold.panel_dimensions"), lengths[0], lengths[1], lengths[2]));
    valid
}

fn thumbnail_outline(ui: &egui::Ui, thumbnail: egui::Rect) {
    let color = dtool_design::theme::widget_token("focus").unwrap_or(ui.visuals().text_color());
    ui.painter().rect_stroke(thumbnail.expand(2.0), 2.0,
        egui::Stroke::new(2.0, color), egui::StrokeKind::Outside);
}

fn draw_thumbnail_tags(ui: &egui::Ui, preview: egui::Rect, colours: &[Color32], top: bool) {
    if colours.is_empty() {
        return;
    }
    let height = (preview.height() * 0.2).min(3.0);
    let y = if top { preview.top() } else { preview.bottom() - height };
    let width = preview.width() / colours.len() as f32;
    for (index, &colour) in colours.iter().enumerate() {
        let x = preview.left() + index as f32 * width;
        ui.painter().rect_filled(egui::Rect::from_min_size(egui::pos2(x, y), vec2(width, height)), 0.0, colour);
    }
}

fn page_move_menu(ui: &mut egui::Ui, doc: &dtool_core::Document) -> Option<Command> {
    use dtool_core::commands::masters::PageMoveMasterPolicy;
    let mut enabled = doc.fixed_spreads.is_none();
    if dtool_design::toggle(ui, &mut enabled, text("panel.pages.reflow")).changed() {
        ui.close();
        return Some(Command::with_params("page.reflow.set", json!({"enabled": enabled})));
    }
    let mut anchor = doc.page_move_anchor_to_spine;
    if dtool_design::toggle(ui, &mut anchor, text("panel.pages.anchor_to_spine")).changed() {
        ui.close();
        return Some(Command::with_params("page.move_options.set", json!({"anchor_to_spine": anchor})));
    }
    let mut policy = doc.page_move_master_policy;
    ui.menu_button(text("panel.pages.move_master_policy"), |ui| {
        for (choice, label) in [
            (PageMoveMasterPolicy::Keep, "panel.pages.master_policy_keep"),
            (PageMoveMasterPolicy::MoveContent, "panel.pages.master_policy_move_content"),
            (PageMoveMasterPolicy::Reapply, "panel.pages.master_policy_reapply"),
        ] {
            if dtool_design::choice_value(ui, &mut policy, choice, text(label)).clicked() {
                ui.close();
            }
        }
    });
    (policy != doc.page_move_master_policy)
        .then(|| Command::with_params("page.move_options.set", json!({"master_policy": policy})))
}

fn section_header<const N: usize>(
    ui: &mut egui::Ui, icons: &mut crate::icon_cache::IconCache, open: &mut bool,
    title: &str, actions: [(&str, &str, bool); N],
) -> ([bool; N], egui::Response) {
    egui::Frame::new().fill(dtool_design::color("bars"))
        .inner_margin(egui::Margin::symmetric(2, 1)).show(ui, |ui| {
            let width = ui.available_width();
            let gap = dtool_design::space("xs");
            let arrow = if *open { "▾" } else { "▸" };
            let ink = ui.visuals().text_color();
            let font = egui::TextStyle::Button.resolve(ui.style());
            let caption = format!("{arrow} {title}");
            let title_text = ui.painter().layout_no_wrap(caption.clone(), font.clone(), ink);
            let button_width = dtool_design::size("control.icon.width");
            let room_for_gaps = width - N as f32 * button_width - title_text.size().x - 6.0;
            let stacked = room_for_gaps < 0.0;
            let title_text = if stacked && title_text.size().x + 6.0 > width {
                ui.painter().layout(caption, font, ink, (width - 6.0).max(1.0))
            } else {
                title_text
            };
            let per_row = if stacked { ((width / button_width) as usize).clamp(1, N) } else { N };
            let spaces = per_row.saturating_sub(1);
            let button_gap = if stacked {
                ((width - per_row as f32 * button_width) / spaces.max(1) as f32).clamp(0.0, gap)
            } else {
                (room_for_gaps / N as f32).min(gap)
            };
            let controls_width = per_row as f32 * button_width + spaces as f32 * button_gap;
            let button_height = ui.spacing().interact_size.y.max(dtool_design::size("control.icon.height"));
            let title_height = button_height.max(title_text.size().y + 2.0);
            let controls_height = if stacked { N.div_ceil(per_row) as f32 * button_height } else { 0.0 };
            let (row, _) = ui.allocate_exact_size(vec2(width, title_height + controls_height), egui::Sense::hover());
            let title_right = if stacked { row.right() } else { row.right() - controls_width - button_gap };
            let title_rect = egui::Rect::from_min_max(row.min, egui::pos2(title_right, row.top() + title_height));
            let response = ui.interact(title_rect, ui.id().with(title), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, *open, title));
            let painter = ui.painter();
            if response.hovered() {
                painter.rect_filled(title_rect, 0.0, dtool_design::color("button.bg_hover"));
            }
            if response.has_focus() {
                painter.rect_stroke(title_rect, 0.0, ui.visuals().widgets.active.bg_stroke, egui::StrokeKind::Inside);
            }
            painter.hline(row.x_range(), row.bottom(), egui::Stroke::new(
                dtool_design::stroke_width("thin"), dtool_design::color("studio.divider"),
            ));
            painter.with_clip_rect(title_rect)
                .galley(egui::pos2(title_rect.left() + 4.0, title_rect.center().y - title_text.size().y * 0.5), title_text, ink);
            if response.clicked() {
                response.request_focus();
                *open = !*open;
            }
            let mut clicks = [false; N];
            for (line, items) in actions.chunks(per_row).enumerate() {
                let top = if stacked { title_rect.bottom() + line as f32 * button_height } else { row.top() };
                let line_width = items.len() as f32 * button_width + items.len().saturating_sub(1) as f32 * button_gap;
                let controls = egui::Rect::from_min_max(egui::pos2(row.right() - line_width, top),
                    egui::pos2(row.right(), top + if stacked { button_height } else { title_height }));
                let mut controls_ui = ui.new_child(egui::UiBuilder::new().max_rect(controls)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)));
                controls_ui.spacing_mut().item_spacing.x = button_gap;
                controls_ui.spacing_mut().button_padding.x = gap;
                controls_ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                for (index, &(icon, label, enabled)) in items.iter().enumerate() {
                    clicks[line * per_row + index] = controls_ui.add_enabled_ui(enabled, |ui| {
                        icons.studio_button(ui, icon, text(label), false).clicked()
                    }).inner;
                }
            }
            (clicks, response)
        }).inner
}

fn move_page(id: PageId, number: usize) -> Command {
    Command::with_params("page.move", json!({"id": id, "number": number}))
}

fn transfer_choices(ui: &mut egui::Ui, selected: &mut bool, labels: [&str; 2]) {
    ui.horizontal_wrapped(|ui| {
        for (value, key) in [false, true].into_iter().zip(labels) {
            dtool_design::choice_value(ui, selected, value, text(key));
        }
    });
}

fn copy_pages(ids: &[PageId], target: PageId, after: bool) -> Command {
    let key = if after { "after_id" } else { "before_id" };
    Command::with_params("page.duplicate", json!({"ids": ids, (key): target}))
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
