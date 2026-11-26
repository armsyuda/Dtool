use dtool_core::command::Session;
use dtool_core::object::ObjectId;
use dtool_core::resources::{ImageFormat,ResourceLocation,ResourceRow,ResourceStatus};
use dtool_core::text_link::TextLinkRow;
use dtool_core::Command;
use dtool_strings::text;
use serde_json::{json,Value};
use std::collections::HashSet;
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
}

#[derive(Default)]
struct Pending {
    commands:Vec<Command>,
    locate:Option<Vec<ObjectId>>,
    replace:Option<u64>,
    #[cfg(windows)]
    pick_relink: Option<u64>,
    #[cfg(windows)]
    browse_folder: Option<FolderField>,
    #[cfg(windows)]
    reveal:Option<u64>,
    close:bool,
}

#[cfg(windows)]
enum FolderField {
    Destination,
    Relink,
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
    let row_height = ui.spacing().interact_size.y;
    let table_height = (ui.available_height() - 156.0).clamp(100.0, 420.0);
    egui::ScrollArea::vertical().id_salt(id).max_height(table_height)
        .auto_shrink([false, false])
        .show_rows(ui, row_height, count, |ui, range| {
            for index in range {
                ui.push_id(index, |ui| {
                    let frame = egui::Frame::new().fill(if index % 2 == 0 {
                        ui.visuals().faint_bg_color
                    } else {
                        egui::Color32::TRANSPARENT
                    });
                    frame.show(ui, |ui| {
                        ui.horizontal(|ui| row(ui, index, &widths));
                    });
                });
            }
        });
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
        ui.add(egui::Label::new(label).truncate()).on_hover_text(hover);
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
        let images=dtool_core::resources::resource_rows(&session.document,doc_dir);
        let texts=dtool_core::text_link::text_link_rows(&session.document);
        let mut pending=Pending::default();
        let screen = ctx.content_rect();
        let default_width = (screen.width() - 40.0).clamp(360.0, 800.0);
        let default_height = (screen.height() - 96.0).clamp(320.0, 490.0);
        let mut open = self.open;
        egui::Window::new(text("dialog.resource_manager.title"))
            .collapsible(false)
            .open(&mut open)
            .default_size(egui::vec2(default_width, default_height))
            .min_width(default_width.min(460.0))
            .default_pos(egui::pos2(screen.center().x - default_width * 0.5, screen.top() + 68.0))
            .resizable(true)
            .show(ctx,|ui| {
            dtool_design::dialog_frame(ui,|ui| {
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
                if self.active_tab==0 {self.image_tab(ui,&images,doc_dir,&mut pending);} else {self.text_tab(ui,&texts,&mut pending);}
                if let Some(message)=&self.error {ui.label(message);}
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if dtool_design::button(ui,text("button.close")).clicked() {pending.close=true;}
                });
                if dtool_design::dialog_keys(ui)==Some(dtool_design::DialogKey::Cancel) {pending.close=true;}
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
        if let Some(field) = pending.browse_folder {
            let title = text(match field { FolderField::Destination => "rm.select_destination", FolderField::Relink => "rm.select_folder" });
            match crate::dialogs::pick_folder_path(title) {
                Ok(Some(path)) => {
                    let input = match field { FolderField::Destination => &mut self.folder, FolderField::Relink => &mut self.relink_path };
                    *input = path.to_string_lossy().into_owned();
                    self.error = None;
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

    fn image_tab(&mut self,ui:&mut egui::Ui,rows:&[ResourceRow],doc_dir:Option<&Path>,pending:&mut Pending) {
        dtool_design::dialog_section(ui, |ui| resource_table(ui, "resource_images", [
            ("rm.name", 3.1), ("rm.status", 1.2), ("rm.locations", 2.3),
            ("rm.file_size", 1.2), ("rm.uses", 0.8),
        ], rows.len(), |ui, index, widths| {
            let row = &rows[index];
            let mut checked = self.selected.contains(&row.id.0);
            let (toggled, clicked) = table_cell(ui, widths[0], |ui| {
                let toggled = dtool_design::toggle(ui, &mut checked, "").on_hover_text(&row.name).changed();
                let clicked = dtool_design::with_icon_selection(ui, |ui| {
                    ui.add(egui::Button::selectable(checked, &row.name).truncate())
                }).on_hover_text(&row.name).clicked();
                (toggled, clicked)
            });
            if toggled || clicked { self.last_located = None; }
            if toggled {
                self.selection_anchor = Some(row.id.0);
                if checked { self.selected.insert(row.id.0); } else { self.selected.remove(&row.id.0); }
            } else if clicked {
                let modifiers = ui.input(|input| input.modifiers);
                if modifiers.shift {
                    let anchor = rows.iter().position(|item| Some(item.id.0) == self.selection_anchor).unwrap_or(index);
                    if !modifiers.command { self.selected.clear(); }
                    self.selected.extend(rows[anchor.min(index)..=anchor.max(index)].iter().map(|item| item.id.0));
                } else {
                    self.selection_anchor = Some(row.id.0);
                    if !modifiers.command { self.selected.clear(); }
                    if !self.selected.remove(&row.id.0) { self.selected.insert(row.id.0); }
                }
            }
            table_status(ui, widths[1], row.status);
            let locations = row.locations.iter().map(location_label).collect::<Vec<_>>().join(", ");
            let locations = if locations.is_empty() { "—" } else { &locations };
            table_label(ui, widths[2], locations, locations);
            let size = row.file_size_bytes.map(file_size_label).unwrap_or_else(|| "—".into());
            let bytes = row.file_size_bytes.map(|bytes| format!("{bytes} B")).unwrap_or_default();
            table_label(ui, widths[3], size, bytes);
            let uses = row.uses.len().to_string();
            table_label(ui, widths[4], &uses, &uses);
        }));
        ui.add_space(dtool_design::space("md"));
        let selected: Vec<_> = rows.iter().filter(|row| self.selected.contains(&row.id.0)).collect();
        let single = match selected.as_slice() { [row] => Some(*row), _ => None };
        if let Some(row) = single {
            let placed_size = placed_size_label(row.placed_size);
            let details = [
                (TYPE_KEY, format_label(row.format).to_owned()),
                ("rm.original_size", format!("{} × {}", row.original_size[0], row.original_size[1])),
                ("rm.original_dpi", format!("{:.1}", row.original_dpi)),
                ("rm.placed_size", if placed_size.is_empty() { "—".into() } else { placed_size }),
                ("rm.placed_dpi", row.placed_size.map_or_else(|| "—".into(), |_| format!("{:.1}", row.placed_dpi))),
                ("rm.colour_space", row.colour_space.clone()),
            ];
            dtool_design::dialog_section(ui, |ui| {
                dtool_design::panel_columns(ui, details.len(), 154.0, |ui, index| {
                    let (key, value) = &details[index];
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(text(key));
                        ui.label(value);
                    });
                });
            });
        }
        let available = |row: &ResourceRow| matches!(row.status, ResourceStatus::Linked | ResourceStatus::Modified);
        let dir=dir_param(doc_dir);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(selected.iter().any(|row| !row.uses.is_empty()), egui::Button::new(text("rm.locate"))).clicked() {
                pending.locate = Some(selected.iter().flat_map(|row| row.uses.iter().copied()).collect());
            }
            ui.add_enabled_ui(single.is_some(), |ui| {
                if ui.add_enabled(cfg!(windows) || !self.relink_path.trim().is_empty(),
                    egui::Button::new(text("rm.replace"))).clicked() {
                    pending.replace = single.map(|row| row.id.0);
                }
                let (update, embed) = ui.add_enabled_ui(single.is_some_and(available), |ui| {
                    (dtool_design::button(ui, text("rm.update")).clicked(), dtool_design::button(ui, text("rm.embed")).clicked())
                }).inner;
                if let Some(row) = single {
                    if update {pending.commands.push(Command::with_params("resource.update",json!({"id":row.id.0,"doc_dir":dir})));}
                    if embed {pending.commands.push(Command::with_params("resource.embed",json!({"id":row.id.0,"doc_dir":dir})));}
                }
            });
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            resource_path_field(ui, text("rm.folder"), &mut self.folder);
            #[cfg(windows)]
            if dtool_design::button(ui, text("pref.browse")).on_hover_text(text("rm.select_destination")).clicked() {
                pending.browse_folder = Some(FolderField::Destination);
            }
            let embedded = single.filter(|row| row.status == ResourceStatus::Embedded);
            if ui.add_enabled(embedded.is_some(), egui::Button::new(text("rm.make_linked"))).clicked() {
                if let Some(row) = embedded {
                    pending.commands.push(Command::with_params("resource.make_linked", json!({"id":row.id.0,"folder":self.folder,"doc_dir":dir})));
                }
            }
            let can_collect = !selected.is_empty() && selected.iter().all(|row| available(row)) && !self.folder.trim().is_empty();
            if ui.add_enabled(can_collect, egui::Button::new(text("rm.collect"))).clicked() {
                let ids: Vec<_> = selected.iter().map(|row| row.id.0).collect();
                pending.commands.push(Command::with_params("resource.collect", json!({"ids":ids,"folder":self.folder,"doc_dir":dir})));
            }
            #[cfg(windows)]
            if ui.add_enabled(single.is_some_and(available), egui::Button::new(text("rm.reveal"))).clicked() {
                pending.reveal = single.map(|row| row.id.0);
            }
        });
        ui.horizontal_wrapped(|ui| {
            let multiple = selected.len() > 1;
            resource_path_field(ui, text(if multiple { "rm.folder" } else { "rm.path" }), &mut self.relink_path);
            #[cfg(windows)]
            if multiple && dtool_design::button(ui, text("pref.browse")).on_hover_text(text("rm.select_folder")).clicked() {
                pending.browse_folder = Some(FolderField::Relink);
            }
            let has_path = !self.relink_path.trim().is_empty();
            let can_relink = !selected.is_empty() && selected.iter().all(|row| row.status != ResourceStatus::Embedded)
                && (has_path || (cfg!(windows) && single.is_some()));
            let label = if multiple { "rm.relink_selected" } else { "rm.relink" };
            if ui.add_enabled(can_relink, egui::Button::new(text(label))).clicked() {
                if has_path {
                    let ids: Vec<_> = selected.iter().map(|row| row.id.0).collect();
                    pending.commands.push(Command::with_params("resource.relink",
                        json!({"ids":ids,"path":self.relink_path,"doc_dir":dir})));
                } else {
                    #[cfg(windows)]
                    { pending.pick_relink = single.map(|row| row.id.0); }
                }
            }
        });
        let modified=rows.iter().any(|row|row.status==ResourceStatus::Modified);
        if ui.add_enabled_ui(modified,|ui|dtool_design::button(ui,text("rm.update_all"))).inner.clicked() {
            pending.commands.push(Command::with_params("resource.update_all",json!({"doc_dir":dir})));
        }
    }

    fn text_tab(&mut self,ui:&mut egui::Ui,rows:&[TextLinkRow],pending:&mut Pending) {
        dtool_design::dialog_section(ui, |ui| resource_table(ui, "resource_texts", [
            ("rm.name", 2.8), ("rm.status", 1.5), ("rm.frames", 0.9), ("rm.path", 4.0),
        ], rows.len(), |ui, index, widths| {
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
