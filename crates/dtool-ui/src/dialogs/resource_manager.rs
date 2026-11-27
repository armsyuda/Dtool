use dtool_core::command::Session;
use dtool_core::object::ObjectId;
use dtool_core::resources::{ImageFormat,ResourceLocation,ResourceRow,ResourceStatus};
use dtool_core::text_link::TextLinkRow;
use dtool_core::{Command, Document};
use dtool_strings::text;
use serde_json::{json,Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Default)]
pub struct ResourceManagerDialog {
    pub open:bool,
    pub active_tab:usize,
    selected: HashSet<u64>,
    selection_anchor: Option<u64>,
    last_located: Option<ObjectId>,
    pub selected_text:Option<u64>,
    pub folder:String,
    pub relink_path:String,
    pub error:Option<String>,
    pub owner:Option<dtool_core::workspace::ContextId>,
    thumbnails: HashMap<u64, Option<egui::TextureHandle>>,
    thumbnail_generation: Option<u64>,
}

#[derive(Default)]
struct Pending {
    commands:Vec<Command>,
    locate:Option<Vec<ObjectId>>,
    replace:Option<u64>,
    #[cfg(windows)]
    pick_relink: Option<u64>,
    #[cfg(windows)]
    browse_folder: Option<FolderAction>,
    #[cfg(windows)]
    reveal:Option<u64>,
    close:bool,
}

#[cfg(windows)]
enum FolderAction {
    MakeLinked(u64),
    Collect(Vec<u64>),
    Relink(Vec<u64>),
}

pub fn status_text(status:ResourceStatus)->&'static str {
    match status {
        ResourceStatus::Embedded=>"rm.embedded",
        ResourceStatus::Linked=>"rm.linked",
        ResourceStatus::Modified=>"rm.modified",
        ResourceStatus::Missing=>"rm.missing",
    }
}

pub const TYPE_KEY:&str="rm.type";

pub fn image_headers()->Vec<&'static str> {
    let mut keys=vec!["rm.name"];
    if dtool_strings::has(TYPE_KEY) {keys.push(TYPE_KEY);}
    keys.extend(["rm.status","rm.locations","rm.file_size","rm.original_size","rm.original_dpi","rm.placed_size","rm.placed_dpi","rm.colour_space","rm.uses"]);
    keys
}

pub fn format_label(format:ImageFormat)->&'static str {
    match format {
        ImageFormat::Png=>"PNG",
        ImageFormat::Jpeg=>"JPEG",
        ImageFormat::Gif=>"GIF",
        ImageFormat::Tiff=>"TIFF",
        ImageFormat::Bmp=>"BMP",
        ImageFormat::Tga=>"TGA",
        ImageFormat::Webp=>"WebP",
        ImageFormat::Heif=>"HEIF",
        ImageFormat::JpegXl=>"JPEG XL",
        ImageFormat::Exr=>"EXR",
        ImageFormat::Hdr=>"HDR",
    }
}

pub fn placed_size_label(size:Option<[f32;2]>)->String {
    match size {
        Some([w,h])=>format!("{:.1}x{:.1}",w,h),
        None=>String::new(),
    }
}

fn file_size_label(bytes: u64) -> String {
    for (unit, divisor) in [("GiB", 1 << 30), ("MiB", 1 << 20), ("KiB", 1 << 10)] {
        if bytes >= divisor {
            return format!("{:.1} {unit}", bytes as f64 / divisor as f64);
        }
    }
    format!("{bytes} B")
}

fn location_label(location: &ResourceLocation) -> String {
    match location {
        ResourceLocation::Page { label, .. } => label.clone(),
        ResourceLocation::Artboard { name, .. } if !name.is_empty() => name.clone(),
        ResourceLocation::Artboard { .. } => text("rm.unnamed_artboard").to_owned(),
        ResourceLocation::Master { name, .. } => format!("{} {name}", text("rm.master")),
        ResourceLocation::Pasteboard => text("rm.pasteboard").to_owned(),
    }
}

fn dir_param(doc_dir:Option<&Path>)->Value {
    doc_dir.map(|dir|Value::from(dir.display().to_string())).unwrap_or(Value::Null)
}

fn resource_table<const N: usize>(
    ui: &mut egui::Ui,
    id: &'static str,
    columns: [(&str, f32); N],
    count: usize,
    layout: TableLayout<'_>,
    mut row: impl FnMut(&mut egui::Ui, usize, &[f32; N]),
) {
    let gap = ui.spacing().item_spacing.x;
    let scroll_style = &ui.spacing().scroll;
    let scrollbar = scroll_style.bar_width + scroll_style.bar_inner_margin + scroll_style.bar_outer_margin;
    let width = (ui.available_width() - scrollbar - gap * (N - 1) as f32).max(0.0);
    let total: f32 = columns.iter().map(|(_, weight)| *weight).sum();
    let widths = columns.map(|(_, weight)| width * weight / total);
    ui.horizontal(|ui| {
        for ((key, _), width) in columns.into_iter().zip(widths) {
            let heading = text(key);
            table_label(ui, width, egui::RichText::new(heading).strong(), heading);
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().id_salt(id).max_height(layout.height)
        .auto_shrink([false, false])
        .show_rows(ui, layout.row_height, count, |ui, range| {
            for index in range {
                ui.push_id(index, |ui| {
                    let frame = egui::Frame::new().fill(if (layout.is_selected)(index) {
                        dtool_design::color("layers.selected")
                    } else if index % 2 == 0 {
                        ui.visuals().faint_bg_color
                    } else {
                        egui::Color32::TRANSPARENT
                    });
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.set_min_size(egui::vec2(width + gap * (N - 1) as f32, layout.row_height));
                            row(ui, index, &widths);
                        });
                    });
                });
            }
        });
}

struct TableLayout<'a> {
    height: f32,
    row_height: f32,
    is_selected: &'a dyn Fn(usize) -> bool,
}

fn table_cell<R>(ui: &mut egui::Ui, width: f32, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_width(width);
            contents(ui)
        }).inner
}

fn table_label(ui: &mut egui::Ui, width: f32, label: impl Into<egui::WidgetText>, hover: impl Into<egui::WidgetText>) {
    table_cell(ui, width, |ui| {
        ui.add(egui::Label::new(label).selectable(false).truncate()).on_hover_text(hover);
    });
}

fn table_status(ui: &mut egui::Ui, width: f32, status: ResourceStatus) {
    let value = text(status_text(status));
    let label = match status {
        ResourceStatus::Missing => egui::RichText::new(value).color(ui.visuals().error_fg_color),
        ResourceStatus::Modified => egui::RichText::new(value).color(ui.visuals().warn_fg_color),
        _ => egui::RichText::new(value),
    };
    table_label(ui, width, label, value);
}

#[cfg(not(windows))]
fn resource_path_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::TextEdit::singleline(value)
                .desired_width(156.0_f32.min(ui.available_width())),
        );
    });
}

impl ResourceManagerDialog {
    pub fn new()->Self {Self::default()}

    pub fn open(&mut self) {
        self.open=true;
        self.error=None;
        self.selected.clear();
        self.selection_anchor=None;
        self.last_located=None;
        self.selected_text=None;
        self.owner=None;
        self.thumbnails.clear();
        self.thumbnail_generation = None;
    }

    pub fn open_for(&mut self,owner:dtool_core::workspace::ContextId,session:&Session) {
        self.open();
        self.owner=Some(owner);
        self.selected.extend(session.selection.ids.iter()
            .filter_map(|&id| session.document.placed_resource(id))
            .map(|id| id.0));
        self.selected.extend(session.document.paint_image_uses().into_iter()
            .filter(|(_, uses)| uses.iter().any(|id| session.selection.ids.contains(id)))
            .map(|(id, _)| id.0));
        if !self.selected.is_empty() {
            self.active_tab=0;
            self.selection_anchor=session.document.resources.iter()
                .find(|resource| self.selected.contains(&resource.id.0))
                .map(|resource| resource.id.0);
        }
    }

    pub fn show(&mut self,ctx:&egui::Context,session:&mut Session,doc_dir:Option<&Path>) -> Option<ObjectId> {
        if !self.open {return None;}
        if self.thumbnail_generation != Some(session.generation) {
            self.thumbnails.clear();
            self.thumbnail_generation = Some(session.generation);
        }
        let images=dtool_core::resources::resource_rows(&session.document,doc_dir);
        let texts=dtool_core::text_link::text_link_rows(&session.document);
        let mut pending=Pending::default();
        let screen = ctx.content_rect();
        let available = (screen.size() - egui::vec2(24.0, 56.0)).max(egui::vec2(180.0, 96.0));
        let initial = egui::vec2(1120.0, 670.0).min(available);
        let mut open = self.open;
        egui::Window::new(text("dialog.resource_manager.title"))
            .collapsible(false)
            .open(&mut open)
            .default_size(initial)
            .min_size(egui::vec2(460.0, 320.0).min(available))
            .max_size(available)
            .default_pos(egui::pos2(screen.center().x - initial.x * 0.5, screen.top() + 68.0))
            .constrain_to(screen)
            .resizable(true)
            .show(ctx, |ui| {
                ui.scope(|ui| {
                    ui.set_width(ui.available_width().min(available.x));
                    ui.set_max_height(ui.available_height().min(available.y));
                    dtool_design::dialog_frame(ui, |ui| {
                        ui.horizontal(|ui| {
                            let width = (ui.available_width() - ui.spacing().item_spacing.x) * 0.5;
                            for (tab, label) in ["rm.images", "rm.text"].into_iter().enumerate() {
                                let selected = self.active_tab == tab;
                                let response = dtool_design::with_icon_selection(ui, |ui| {
                                    ui.add(egui::Button::selectable(selected, text(label)).min_size(egui::vec2(width, 24.0)))
                                });
                                if response.clicked() && !selected {
                                    self.active_tab = tab;
                                    self.last_located = None;
                                }
                            }
                        });
                        ui.add_space(dtool_design::space("md"));
                        let compact = ui.available_height() < 380.0 || ui.available_width() < 610.0;
                        let scroll_id = if self.active_tab == 0 { "resource_manager_images" } else { "resource_manager_text" };
                        let mut body = |ui: &mut egui::Ui| {
                            if self.active_tab == 0 {
                                self.image_tab(ui, &images, &session.document, compact);
                                if compact {
                                    ui.add_space(dtool_design::space("sm"));
                                    self.image_actions(ui, &images, doc_dir, &mut pending);
                                    self.image_details(ui, &images, &session.document);
                                }
                            } else {
                                self.text_tab(ui, &texts, &mut pending, compact);
                            }
                            if let Some(message) = &self.error { ui.label(message); }
                        };
                        if compact {
                            let footer = ui.spacing().interact_size.y + ui.spacing().item_spacing.y * 2.0 + 2.0;
                            let height = (ui.available_height() - footer).max(24.0);
                            dtool_design::dialog_options_scroll(ui, scroll_id, height, body);
                        } else {
                            body(ui);
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            let actions_width = (ui.available_width() - 90.0).max(0.0);
                            ui.allocate_ui_with_layout(egui::vec2(actions_width, 0.0),
                                egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    if !compact && self.active_tab == 0 {
                                        self.image_actions(ui, &images, doc_dir, &mut pending);
                                    }
                                });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if dtool_design::button(ui, text("button.close")).clicked() {
                                    pending.close = true;
                                }
                            });
                        });
                        if dtool_design::dialog_keys(ui)==Some(dtool_design::DialogKey::Cancel) {pending.close=true;}
                    });
                });
            });
        #[cfg(windows)]
        if let Some(id) = pending.reveal {
            let path = session.document.image_resource(dtool_core::resources::ResourceId(id))
                .and_then(|resource| resource.resolved_path(doc_dir))
                .and_then(|path| std::path::absolute(path).ok())
                .filter(|path| path.is_file());
            self.error = path.and_then(|path| {
                let mut argument = std::ffi::OsString::from("/select,");
                argument.push(path);
                std::process::Command::new("explorer.exe").arg(argument).spawn().ok()
            }).is_none().then(|| text("error.io_error").to_owned());
        }
        if let Some(id) = pending.replace {
            #[cfg(windows)]
            match crate::dialogs::pick_file_path_with_extensions(text("rm.replace"), text("rm.images"),
                &crate::place_tool::PLACE_IMAGE_EXTENSIONS, None)
            {
                Ok(Some(path)) => pending.commands.push(Command::with_params("resource.replace",
                    json!({"resource_id":id,"path":path}))),
                Ok(None) => {}
                Err(_) => self.error = Some(text("error.io_error").to_owned()),
            }
            #[cfg(not(windows))]
            pending.commands.push(Command::with_params("resource.replace",
                json!({"resource_id":id,"path":self.relink_path})));
        }
        #[cfg(windows)]
        if let Some(id) = pending.pick_relink {
            match crate::dialogs::pick_file_path_with_extensions(text("rm.relink"), text("rm.images"),
                &crate::place_tool::PLACE_IMAGE_EXTENSIONS, None)
            {
                Ok(Some(path)) => pending.commands.push(Command::with_params("resource.relink",
                    json!({"id":id,"path":path,"doc_dir":dir_param(doc_dir)}))),
                Ok(None) => {}
                Err(_) => self.error = Some(text("error.io_error").to_owned()),
            }
        }
        #[cfg(windows)]
        if let Some(action) = pending.browse_folder {
            let title = text(match action {
                FolderAction::Relink(_) => "rm.select_folder",
                _ => "rm.select_destination",
            });
            match crate::dialogs::pick_folder_path(title) {
                Ok(Some(path)) => {
                    let doc_dir = dir_param(doc_dir);
                    let command = match action {
                        FolderAction::MakeLinked(id) =>
                            Command::with_params("resource.make_linked", json!({"id":id,"folder":path,"doc_dir":doc_dir})),
                        FolderAction::Collect(ids) =>
                            Command::with_params("resource.collect", json!({"ids":ids,"folder":path,"doc_dir":doc_dir})),
                        FolderAction::Relink(ids) =>
                            Command::with_params("resource.relink", json!({"ids":ids,"path":path,"doc_dir":doc_dir})),
                    };
                    pending.commands.push(command);
                }
                Ok(None) => {}
                Err(_) => self.error = Some(text("error.io_error").to_owned()),
            }
        }
        let located = pending.locate.and_then(|mut ids| {
            ids.retain(|id| session.document.get_object(*id).is_some());
            ids.sort_by_key(|id| (dtool_core::data_merge::layout::object_page_index(&session.document, *id), id.0));
            ids.dedup();
            let first = ids.first().copied()?;
            let next = ids.iter().position(|id| Some(*id) == self.last_located)
                .and_then(|index| ids.get(index + 1)).copied().unwrap_or(first);
            match session.run(&Command::with_params("selection.set", json!({"ids":[next.0],"op":"Replace"}))) {
                Ok(_) => {
                    self.last_located = Some(next);
                    self.error = None;
                    Some(next)
                }
                Err(error) => {
                    self.error = Some(text(&error.message_key).to_owned());
                    None
                }
            }
        });
        for command in pending.commands {
            match session.run(&command) {
                Ok(_)=>self.error=None,
                Err(error)=>{self.error=Some(text(&error.message_key).to_string());break;}
            }
        }
        self.open = open && !pending.close;
        located
    }

    fn image_tab(&mut self, ui: &mut egui::Ui, rows: &[ResourceRow], document: &Document, compact: bool) {
        if compact {
            let height = (ui.spacing().interact_size.y + ui.spacing().item_spacing.y) * 4.0;
            self.image_list(ui, rows, document, height, [
                ("rm.name", 4.0), ("rm.status", 1.2), ("rm.locations", 1.5),
            ]);
        } else {
            let details_width = (ui.available_width() * 0.24).clamp(196.0, 278.0);
            let list_width = ui.available_width() - details_width - ui.spacing().item_spacing.x * 2.0;
            let height = (ui.available_height() - 120.0).clamp(220.0, 532.0);
            let panel_height = height + 36.0;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(list_width, panel_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(list_width);
                    self.image_list(ui, rows, document, height, [
                        ("rm.name", 4.5), ("rm.status", 0.9), ("rm.locations", 1.1),
                        ("rm.placement", 1.15), ("rm.file_size", 1.05), ("rm.placed_dpi", 0.95),
                        (TYPE_KEY, 0.8),
                    ]);
                });
                ui.allocate_ui_with_layout(egui::vec2(details_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(details_width);
                    self.image_details(ui, rows, document);
                });
            });
        }
    }

    fn image_list<const N: usize>(
        &mut self, ui: &mut egui::Ui, rows: &[ResourceRow], document: &Document, height: f32,
        columns: [(&str, f32); N],
    ) {
        let selected = self.selected.clone();
        dtool_design::dialog_section(ui, |ui| {
            resource_table(ui, "resource_images", columns, rows.len(), TableLayout {
                height,
                row_height: 38.0,
                is_selected: &|index| selected.contains(&rows[index].id.0),
            }, |ui, index, widths| {
                let row = &rows[index];
                let top = ui.cursor().min;
                for ((key, _), width) in columns.into_iter().zip(widths) {
                    match key {
                        "rm.name" => table_cell(ui, *width, |ui| {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 32.0), egui::Sense::hover());
                            self.paint_thumbnail(ui, document, row, rect);
                            ui.add(egui::Label::new(&row.name).selectable(false).truncate())
                                .on_hover_text(&row.name);
                        }),
                        "rm.status" if matches!(row.status, ResourceStatus::Missing | ResourceStatus::Modified) => {
                            table_status(ui, *width, row.status);
                        }
                        _ => {
                            let label = match key {
                                "rm.status" => text("rm.current").to_owned(),
                                "rm.locations" => row.locations.iter().map(location_label).collect::<Vec<_>>().join(", "),
                                "rm.placement" => document.image_resource(row.id)
                                    .map(|image| text(if image.embedded { "rm.embedded" } else { "rm.linked" }).to_owned())
                                    .unwrap_or_default(),
                                "rm.file_size" => row.file_size_bytes.map(file_size_label).unwrap_or_default(),
                                "rm.placed_dpi" if row.placed_size.is_some() => format!("{:.0} dpi", row.placed_dpi),
                                TYPE_KEY => format_label(row.format).to_owned(),
                                _ => String::new(),
                            };
                            let label = if label.is_empty() { "—" } else { &label };
                            table_label(ui, *width, label, label);
                        }
                    }
                }
                let rect = egui::Rect::from_min_size(top, egui::vec2(
                    widths.iter().sum::<f32>() + ui.spacing().item_spacing.x * (N - 1) as f32, 38.0,
                ));
                let response = ui.interact(rect, ui.id().with(row.id.0), egui::Sense::click());
                if response.hovered() || response.has_focus() {
                    ui.painter().rect_stroke(rect, 2.0, ui.visuals().widgets.hovered.bg_stroke, egui::StrokeKind::Inside);
                }
                if response.clicked() {
                    self.last_located = None;
                    let modifiers = ui.input(|input| input.events.iter().rev().find_map(|event| match event {
                        egui::Event::PointerButton { button: egui::PointerButton::Primary, pressed: false, modifiers, .. } =>
                            Some(*modifiers),
                        _ => None,
                    }).unwrap_or(input.modifiers));
                    if modifiers.shift {
                        let anchor = rows.iter().position(|item| Some(item.id.0) == self.selection_anchor).unwrap_or(index);
                        if !modifiers.command { self.selected.clear(); }
                        self.selected.extend(rows[anchor.min(index)..=anchor.max(index)].iter().map(|item| item.id.0));
                    } else {
                        self.selection_anchor = Some(row.id.0);
                        if modifiers.command {
                            if !self.selected.remove(&row.id.0) { self.selected.insert(row.id.0); }
                        } else {
                            self.selected.clear();
                            self.selected.insert(row.id.0);
                        }
                    }
                }
            });
        });
    }

    fn paint_thumbnail(&mut self, ui: &egui::Ui, document: &Document, row: &ResourceRow, rect: egui::Rect) {
        if !ui.is_rect_visible(rect) { return; }
        if !self.thumbnails.contains_key(&row.id.0) {
            if self.thumbnails.len() >= 128 { self.thumbnails.clear(); }
            // Imported raster data is already colour-converted and available even if its
            // linked source has moved. Sample the tiles without expanding the full image.
            let raster = row.uses.iter().find_map(|&id| {
                (document.placed_resource(id) == Some(row.id)).then(|| document.pixel_layers.get(id)).flatten()
            });
            let image = if let Some(raster) = raster.filter(|raster| raster.width > 0 && raster.height > 0) {
                let scale = (176.0 / raster.width.max(raster.height) as f32).min(1.0);
                let width = (raster.width as f32 * scale).round().max(1.0) as u32;
                let height = (raster.height as f32 * scale).round().max(1.0) as u32;
                let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                for y in 0..height {
                    for x in 0..width {
                        rgba.extend_from_slice(&raster.get(
                            (u64::from(2 * x + 1) * u64::from(raster.width) / u64::from(2 * width)) as u32,
                            (u64::from(2 * y + 1) * u64::from(raster.height) / u64::from(2 * height)) as u32,
                        ));
                    }
                }
                Some(egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba))
            } else {
                document.image_resource(row.id).and_then(|resource| resource.data.as_deref())
                    .and_then(|bytes| dtool_io::image_import::embedded_thumbnail(bytes, 176))
                    .map(|image| egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize], image.as_raw(),
                    ))
            };
            self.thumbnails.insert(row.id.0, image.map(|image| {
                ui.ctx().load_texture(format!("resource.{}", row.id.0), image, egui::TextureOptions::LINEAR)
            }));
        }
        ui.painter().rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
        if let Some(texture) = self.thumbnails.get(&row.id.0).and_then(Option::as_ref) {
            let size = texture.size_vec2();
            let fit = (rect.width() / size.x).min(rect.height() / size.y);
            ui.painter().image(texture.id(), egui::Rect::from_center_size(rect.center(), size * fit),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
    }

    fn image_details(&mut self, ui: &mut egui::Ui, rows: &[ResourceRow], document: &Document) {
        let mut selected = rows.iter().filter(|row| self.selected.contains(&row.id.0));
        let Some(row) = selected.next().filter(|_| selected.next().is_none()) else { return };
        ui.add_space(dtool_design::space("sm"));
        dtool_design::dialog_section(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(egui::RichText::new(&row.name).strong());
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 126.0), egui::Sense::hover());
            self.paint_thumbnail(ui, document, row, rect.shrink(4.0));
            let placed_size = placed_size_label(row.placed_size);
            let details = [
                (TYPE_KEY, format_label(row.format).to_owned()),
                ("rm.original_size", format!("{} × {} px", row.original_size[0], row.original_size[1])),
                ("rm.original_dpi", format!("{:.1}", row.original_dpi)),
                ("rm.placed_size", if placed_size.is_empty() { "—".into() } else { format!("{placed_size} pt") }),
                ("rm.placed_dpi", row.placed_size.map_or_else(|| "—".into(), |_| format!("{:.1}", row.placed_dpi))),
                ("rm.colour_space", row.colour_space.clone()),
                ("rm.icc_profile", row.icc.clone().unwrap_or_default()),
                ("rm.uses", row.uses.len().to_string()),
            ];
            egui::Grid::new("resource_image_details").num_columns(2).show(ui, |ui| {
                for (key, value) in details {
                    ui.weak(text(key));
                    let value = if value.is_empty() { "—" } else { &value };
                    ui.add(egui::Label::new(value).truncate()).on_hover_text(value);
                    ui.end_row();
                }
            });
        });
        ui.add_space(dtool_design::space("sm"));
    }

    fn image_actions(&mut self, ui: &mut egui::Ui, rows: &[ResourceRow], doc_dir: Option<&Path>, pending: &mut Pending) {
        let selected: Vec<_> = rows.iter().filter(|row| self.selected.contains(&row.id.0)).collect();
        let single = match selected.as_slice() { [row] => Some(*row), _ => None };
        let available = |row: &ResourceRow| matches!(row.status, ResourceStatus::Linked | ResourceStatus::Modified);
        let dir = dir_param(doc_dir);
        #[cfg(not(windows))]
        ui.horizontal_wrapped(|ui| {
            resource_path_field(ui, text("rm.folder"), &mut self.folder);
            resource_path_field(ui, text("rm.path"), &mut self.relink_path);
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(selected.iter().any(|row| !row.uses.is_empty()), egui::Button::new(text("rm.locate"))).clicked() {
                pending.locate = Some(selected.iter().flat_map(|row| row.uses.iter().copied()).collect());
            }
            let available_single = single.filter(|row| available(row));
            if ui.add_enabled(available_single.is_some(), egui::Button::new(text("rm.update"))).clicked()
                && let Some(row) = available_single {
                pending.commands.push(Command::with_params("resource.update", json!({"id":row.id.0,"doc_dir":dir})));
            }
            if ui.add_enabled(single.is_some() && (cfg!(windows) || !self.relink_path.trim().is_empty()),
                egui::Button::new(text("rm.replace"))).clicked() {
                pending.replace = single.map(|row| row.id.0);
            }
            let embedded = single.filter(|row| row.status == ResourceStatus::Embedded);
            let can_choose_folder = cfg!(windows) || !self.folder.trim().is_empty();
            if ui.add_enabled(embedded.is_some() && can_choose_folder, egui::Button::new(text("rm.make_linked"))).clicked()
                && let Some(row) = embedded {
                #[cfg(windows)]
                { pending.browse_folder = Some(FolderAction::MakeLinked(row.id.0)); }
                #[cfg(not(windows))]
                pending.commands.push(Command::with_params("resource.make_linked", json!({"id":row.id.0,"folder":self.folder,"doc_dir":dir})));
            }
            let can_collect = !selected.is_empty() && selected.iter().all(|row| available(row)) && can_choose_folder;
            if ui.add_enabled(can_collect, egui::Button::new(text("rm.collect"))).clicked() {
                let ids: Vec<_> = selected.iter().map(|row| row.id.0).collect();
                #[cfg(windows)]
                { pending.browse_folder = Some(FolderAction::Collect(ids)); }
                #[cfg(not(windows))]
                pending.commands.push(Command::with_params("resource.collect", json!({"ids":ids,"folder":self.folder,"doc_dir":dir})));
            }
            let can_relink = !selected.is_empty() && selected.iter().all(|row| row.status != ResourceStatus::Embedded)
                && (cfg!(windows) || !self.relink_path.trim().is_empty());
            let relink_label = if selected.len() > 1 { "rm.relink_selected" } else { "rm.relink" };
            if ui.add_enabled(can_relink, egui::Button::new(text(relink_label))).clicked() {
                #[cfg(windows)]
                if let Some(row) = single {
                    pending.pick_relink = Some(row.id.0);
                } else {
                    pending.browse_folder = Some(FolderAction::Relink(selected.iter().map(|row| row.id.0).collect()));
                }
                #[cfg(not(windows))]
                {
                    let ids: Vec<_> = selected.iter().map(|row| row.id.0).collect();
                    pending.commands.push(Command::with_params("resource.relink", json!({"ids":ids,"path":self.relink_path,"doc_dir":dir})));
                }
            }
            if ui.add_enabled(available_single.is_some(), egui::Button::new(text("rm.embed"))).clicked()
                && let Some(row) = available_single {
                pending.commands.push(Command::with_params("resource.embed", json!({"id":row.id.0,"doc_dir":dir})));
            }
            #[cfg(windows)]
            if ui.add_enabled(available_single.is_some(), egui::Button::new(text("rm.reveal"))).clicked() {
                pending.reveal = available_single.map(|row| row.id.0);
            }
            let modified = rows.iter().any(|row| row.status == ResourceStatus::Modified);
            if ui.add_enabled(modified, egui::Button::new(text("rm.update_all"))).clicked() {
                pending.commands.push(Command::with_params("resource.update_all", json!({"doc_dir":dir})));
            }
        });
    }

    fn text_tab(&mut self,ui:&mut egui::Ui,rows:&[TextLinkRow],pending:&mut Pending,compact:bool) {
        let row_height = ui.spacing().interact_size.y;
        let height = if compact {
            (row_height + ui.spacing().item_spacing.y) * rows.len().clamp(2, 4) as f32
        } else {
            (ui.available_height() - 90.0).clamp(100.0, 490.0)
        };
        let selected = self.selected_text;
        dtool_design::dialog_section(ui, |ui| resource_table(ui, "resource_texts", [
            ("rm.name", 2.8), ("rm.status", 1.5), ("rm.frames", 0.9), ("rm.path", 4.0),
        ], rows.len(), TableLayout {
            height, row_height, is_selected: &|index| selected == Some(rows[index].story.0),
        }, |ui, index, widths| {
            let row = &rows[index];
            let clicked = table_cell(ui, widths[0], |ui| {
                dtool_design::with_icon_selection(ui, |ui| {
                    ui.add(egui::Button::selectable(self.selected_text == Some(row.story.0), &row.name).truncate())
                }).on_hover_text(&row.name).clicked()
            });
            if clicked {
                self.selected_text=Some(row.story.0);
                self.last_located=None;
            }
            table_status(ui, widths[1], row.status);
            let frames = row.frames.len().to_string();
            table_label(ui, widths[2], &frames, &frames);
            let path = row.path.to_string_lossy();
            table_label(ui, widths[3], path.as_ref(), path.as_ref());
        }));
        ui.add_space(dtool_design::space("md"));
        let selected=self.selected_text.and_then(|story|rows.iter().find(|row|row.story.0==story));
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(selected.is_some(),|ui| {
                let locate = dtool_design::button(ui,text("rm.locate")).clicked();
                let update = dtool_design::button(ui,text("rm.update")).clicked();
                if let Some(row) = selected {
                    if locate {pending.locate=Some(row.frames.clone());}
                    if update {pending.commands.push(Command::with_params("text.import.update",json!({"story":row.story.0})));}
                }
            });
            let modified:Vec<u64>=rows.iter().filter(|row|row.status==ResourceStatus::Modified).map(|row|row.story.0).collect();
            if ui.add_enabled_ui(!modified.is_empty(),|ui|dtool_design::button(ui,text("rm.update_all"))).inner.clicked() {
                pending.commands.extend(modified.into_iter().map(|story|Command::with_params("text.import.update",json!({"story":story}))));
            }
        });
    }
}
