use dtool_core::longdoc::anchors::{Hyperlink, LinkTarget};
use dtool_core::longdoc::sections::{NumberStyle, Section};
use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_core::Document;
use dtool_design::{button, dropdown, number_field, panel_frame, panel_header, toggle};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::json;
use std::cell::RefCell;

const STYLES: [(&str, NumberStyle); 7] = [
    ("1, 2, 3", NumberStyle::Arabic),
    ("i, ii, iii", NumberStyle::RomanLower),
    ("I, II, III", NumberStyle::RomanUpper),
    ("a, b, c", NumberStyle::AlphaLower),
    ("A, B, C", NumberStyle::AlphaUpper),
    ("\u{ac00}, \u{b098}, \u{b2e4}", NumberStyle::Hangul),
    ("\u{4e00}, \u{4e8c}, \u{4e09}", NumberStyle::HanjaNumber),
];

const LINK_TYPES: [&str; 4] = ["hyperlink.url", "hyperlink.anchor", "hyperlink.page", "hyperlink.email"];
const XREF_FORMATS: [&str; 4] = ["page", "anchor_text", "caption", "page_n"];

fn link_type_index(target: &LinkTarget) -> usize {
    match target {
        LinkTarget::Url(_) => 0,
        LinkTarget::Anchor(_) => 1,
        LinkTarget::Page(_) => 2,
        LinkTarget::Email(_) => 3,
    }
}

fn link_target(kind: usize, value: &str) -> LinkTarget {
    match kind {
        1 => LinkTarget::Anchor(value.to_string()),
        2 => LinkTarget::Page(value.trim().parse().unwrap_or(1)),
        3 => LinkTarget::Email(value.to_string()),
        _ => LinkTarget::Url(value.to_string()),
    }
}

fn link_text(target: &LinkTarget) -> String {
    match target {
        LinkTarget::Url(s) | LinkTarget::Anchor(s) | LinkTarget::Email(s) => s.clone(),
        LinkTarget::Page(p) => p.to_string(),
    }
}

#[derive(Default)]
pub struct SectionManagerState {
    pub open: bool,
    pub selected_index: Option<usize>,
}

pub fn show_section_manager(
    ui: &mut egui::Ui,
    doc: &Document,
    state: &mut SectionManagerState,
    on_change: &dyn Fn(usize, Section),
    on_delete: &dyn Fn(usize),
) {
    if !state.open {
        return;
    }
    panel_frame(ui, |ui| {
        let bounds = ui.ctx().content_rect();
        let width = (bounds.width() - 60.0).clamp(164.0, 540.0);
        ui.set_width(width);
        panel_header(ui, text("dialog.section_manager.title"));
        let sections = doc.sections();
        let page_count = doc.page_count();
        state.selected_index = state.selected_index.filter(|&index| index < sections.len())
            .or_else(|| (!sections.is_empty()).then_some(0));
        let height = (bounds.height() - 124.0).clamp(86.0, 450.0);
        dtool_design::dialog_options_scroll(ui, "section.manager.options", height, |ui| {
            let side_by_side = dtool_design::panel_column_count(ui, ui.available_width(), 2, 220.0) == 2;
            let list_height = if side_by_side { height - 22.0 } else { (height * 0.45).clamp(42.0, 118.0) };
            dtool_design::panel_columns(ui, 2, 220.0, |ui, column| {
                if column == 0 {
                    dtool_design::dialog_section(ui, |ui| {
                        dtool_design::dialog_options_scroll(ui, "section.manager.list", list_height, |ui| {
                            for (i, section) in sections.iter().enumerate() {
                                let end_page = sections.get(i + 1).map_or(page_count, |next| next.start_page.saturating_sub(1));
                                let caption = format!("{}. {}  {}\u{2013}{}", i + 1, section.name, section.start_page, end_page);
                                let choice = egui::Button::selectable(state.selected_index == Some(i), &caption)
                                    .min_size(egui::vec2(ui.available_width(), 0.0)).truncate();
                                if dtool_design::with_icon_selection(ui, |ui| ui.add(choice).on_hover_text(&caption)).clicked() {
                                    state.selected_index = Some(i);
                                }
                            }
                        });
                    });
                } else if let Some(i) = state.selected_index {
                    dtool_design::dialog_section(ui, |ui| {
                        let mut section = sections[i].clone();
                        let first = if i == 0 { 1 } else { sections[i - 1].start_page + 1 };
                        let last = sections.get(i + 1).map_or(page_count, |next| next.start_page - 1);
                        let mut start = section.start_page as f64;
                        let spec = dtool_design::NumericSpec::new("").range(first as f64..=last.max(first) as f64)
                            .decimals(0).width(96.0).enabled(i > 0);
                        dtool_design::studio_row(ui, text("section.start_on"), |ui, label| {
                            let id = ui.make_persistent_id(("section.start", i));
                            if dtool_design::numeric_value(ui, id, &mut start, &spec).labelled_by(label).changed() {
                                section.start_page = start as usize;
                            }
                        });

                        let mut continue_numbers = section.restart_at.is_none();
                        if toggle(ui, &mut continue_numbers, text("section.continue")).changed() {
                            section.restart_at = if continue_numbers { None } else { Some(1) };
                        }
                        if let Some(number) = section.restart_at {
                            let mut number = f64::from(number);
                            let spec = dtool_design::NumericSpec::new("").range(1.0..=f64::from(u32::MAX))
                                .decimals(0).width(96.0);
                            dtool_design::studio_row(ui, text("section.restart"), |ui, label| {
                                let id = ui.make_persistent_id(("section.restart", i));
                                if dtool_design::numeric_value(ui, id, &mut number, &spec).labelled_by(label).changed() {
                                    section.restart_at = Some(number as u32);
                                }
                            });
                        }

                        for (key, value) in [("section.name", &mut section.name), ("section.prefix", &mut section.prefix)] {
                            dtool_design::studio_row(ui, text(key), |ui, label| {
                                let id = ui.make_persistent_id((key, i));
                                ui.add(egui::TextEdit::singleline(value).id(id)
                                    .desired_width(ui.available_width())).labelled_by(label);
                            });
                        }
                        dtool_design::studio_row(ui, text("section.number_style"), |ui, caption| {
                            let options = STYLES.map(|(label, _)| label);
                            let mut selected = STYLES.iter().position(|(_, style)| *style == section.style).unwrap_or(0);
                            if dropdown(ui, &format!("section_style_{i}"), &mut selected, &options).labelled_by(caption).changed() {
                                section.style = STYLES[selected].1.clone();
                            }
                        });
                        toggle(ui, &mut section.include_on_export, text("section.include_on_export"));
                        if section != sections[i] {
                            on_change(i, section);
                        }
                    });
                }
            });
        });
        ui.horizontal(|ui| {
            let start_page = sections.last().map_or(1, |section| section.start_page + 1);
            if dtool_design::button_with(ui, "+", &dtool_design::ControlState::enabled(start_page <= page_count)).clicked() {
                on_change(sections.len(), Section {
                    start_page,
                    name: String::new(),
                    restart_at: None,
                    prefix: String::new(),
                    style: NumberStyle::Arabic,
                    include_on_export: true,
                });
                state.selected_index = Some(sections.len());
            }
            if dtool_design::button_with(ui, text("button.delete"),
                &dtool_design::ControlState::enabled(state.selected_index.is_some_and(|index| index > 0))).clicked()
                && let Some(index) = state.selected_index
            {
                on_delete(index);
                state.selected_index = Some(index.saturating_sub(1));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button(ui, text("button.close")).clicked() {
                    state.open = false;
                }
            });
        });
    });

}

#[derive(Default)]
pub struct InsertAnchorState {
    pub open: bool,
    pub name: String,
}

pub fn show_insert_anchor_dialog(ui: &mut egui::Ui, state: &mut InsertAnchorState, on_confirm: &dyn Fn(String)) {
    if !state.open {
        return;
    }
    panel_frame(ui, |ui| {
        panel_header(ui, text("dialog.insert_anchor.title"));
        ui.horizontal(|ui| {
            ui.label(text("anchor.name"));
            ui.text_edit_singleline(&mut state.name);
        });
        ui.horizontal(|ui| {
            if button(ui, text("button.ok")).clicked() && !state.name.trim().is_empty() {
                on_confirm(state.name.trim().to_string());
                state.open = false;
                state.name.clear();
            }
            if button(ui, text("button.cancel")).clicked() {
                state.open = false;
                state.name.clear();
            }
        });
    });
}

#[derive(Default)]
pub struct MergeDocumentState {
    pub open: bool,
    pub path: String,
    pub insert_after: usize,
}

pub fn show_merge_document_dialog(ui: &mut egui::Ui, doc: &Document, state: &mut MergeDocumentState, on_confirm: &dyn Fn(String, usize)) {
    if !state.open {
        return;
    }
    panel_frame(ui, |ui| {
        panel_header(ui, text("dialog.merge_document.title"));
        let mut val = state.insert_after as f64;
        if number_field(ui, &mut val, text("merge.insert_after")).changed() {
            state.insert_after = (val.max(0.0) as usize).min(doc.page_count());
        }
        ui.text_edit_singleline(&mut state.path);
        ui.horizontal(|ui| {
            if button(ui, text("button.ok")).clicked() && !state.path.trim().is_empty() {
                on_confirm(state.path.trim().to_string(), state.insert_after);
                *state = MergeDocumentState::default();
            }
            if button(ui, text("button.cancel")).clicked() {
                *state = MergeDocumentState::default();
            }
        });
    });
}

pub struct InsertXrefState {
    pub open: bool,
    pub anchor: String,
    pub format: String,
}

impl Default for InsertXrefState {
    fn default() -> Self {
        Self {
            open: false,
            anchor: String::new(),
            format: XREF_FORMATS[0].to_string(),
        }
    }
}

pub fn show_insert_xref_dialog(ui: &mut egui::Ui, doc: &Document, state: &mut InsertXrefState, on_confirm: &dyn Fn(String, String)) {
    if !state.open {
        return;
    }
    panel_frame(ui, |ui| {
        panel_header(ui, text("dialog.insert_xref.title"));
        ui.horizontal(|ui| {
            ui.label(text("xref.anchor"));
            let names: Vec<&str> = doc.anchors().iter().map(|a| a.name.as_str()).collect();
            let mut selected = names.iter().position(|n| *n == state.anchor).unwrap_or(0);
            if dropdown(ui, "xref_anchor", &mut selected, &names).changed() || state.anchor.is_empty() {
                state.anchor = names.get(selected).map(|n| n.to_string()).unwrap_or_default();
            }
        });
        ui.horizontal(|ui| {
            ui.label(text("xref.format"));
            let mut selected = XREF_FORMATS.iter().position(|f| *f == state.format).unwrap_or(0);
            if dropdown(ui, "xref_format", &mut selected, &XREF_FORMATS).changed() {
                state.format = XREF_FORMATS[selected].to_string();
            }
        });
        ui.horizontal(|ui| {
            if button(ui, text("button.ok")).clicked() && !state.anchor.is_empty() {
                on_confirm(state.anchor.clone(), state.format.clone());
                *state = InsertXrefState::default();
            }
            if button(ui, text("button.cancel")).clicked() {
                *state = InsertXrefState::default();
            }
        });
    });
}

#[derive(Default)]
pub struct InsertHyperlinkState {
    pub open: bool,
    pub link_type: usize,
    pub target: String,
    pub range_start: usize,
    pub range_end: usize,
}

fn hyperlink_fields(ui: &mut egui::Ui, id: &str, link_type: &mut usize, target: &mut String, start: &mut usize, end: &mut usize) {
    ui.horizontal(|ui| {
        ui.label(text("hyperlink.type"));
        let labels: Vec<&str> = LINK_TYPES.iter().map(|k| text(k)).collect();
        dropdown(ui, id, link_type, &labels);
    });
    ui.horizontal(|ui| {
        ui.label(text(LINK_TYPES.get(*link_type).copied().unwrap_or("hyperlink.target")));
        ui.text_edit_singleline(target);
    });
    let mut start_val = *start as f64;
    if number_field(ui, &mut start_val, text("hyperlink.range_start")).changed() {
        *start = start_val.max(0.0) as usize;
    }
    let mut end_val = *end as f64;
    if number_field(ui, &mut end_val, text("hyperlink.range_end")).changed() {
        *end = end_val.max(0.0) as usize;
    }
}

pub fn show_insert_hyperlink_dialog(ui: &mut egui::Ui, state: &mut InsertHyperlinkState, on_confirm: &dyn Fn(Option<(usize, usize)>, LinkTarget)) {
    if !state.open {
        return;
    }
    panel_frame(ui, |ui| {
        panel_header(ui, text("dialog.hyperlink.title"));
        hyperlink_fields(ui, "hyperlink_insert_type", &mut state.link_type, &mut state.target, &mut state.range_start, &mut state.range_end);
        ui.horizontal(|ui| {
            if button(ui, text("button.ok")).clicked() && !state.target.trim().is_empty() {
                let range = (state.range_end > state.range_start).then_some((state.range_start, state.range_end));
                on_confirm(range, link_target(state.link_type, state.target.trim()));
                *state = InsertHyperlinkState::default();
            }
            if button(ui, text("button.cancel")).clicked() {
                *state = InsertHyperlinkState::default();
            }
        });
    });
}

#[derive(Default)]
pub struct EditHyperlinkState {
    pub open: bool,
    pub index: usize,
    pub link_type: usize,
    pub target: String,
    pub range_start: usize,
    pub range_end: usize,
}

impl EditHyperlinkState {
    pub fn open_for(&mut self, index: usize, link: &Hyperlink) {
        *self = Self {
            open: true,
            index,
            link_type: link_type_index(&link.target),
            target: link_text(&link.target),
            range_start: link.range.0,
            range_end: link.range.1,
        };
    }
}

pub fn show_edit_hyperlink_dialog(ui: &mut egui::Ui, state: &mut EditHyperlinkState, on_confirm: &dyn Fn(usize, (usize, usize), LinkTarget)) {
    if !state.open {
        return;
    }
    panel_frame(ui, |ui| {
        panel_header(ui, text("dialog.hyperlink.edit.title"));
        hyperlink_fields(ui, "hyperlink_edit_type", &mut state.link_type, &mut state.target, &mut state.range_start, &mut state.range_end);
        ui.horizontal(|ui| {
            if button(ui, text("button.ok")).clicked() && state.range_end >= state.range_start {
                on_confirm(state.index, (state.range_start, state.range_end), link_target(state.link_type, state.target.trim()));
                state.open = false;
            }
            if button(ui, text("button.cancel")).clicked() {
                state.open = false;
            }
        });
    });
}

#[derive(Default)]
pub struct LongdocDialogs {
    pub section_manager: SectionManagerState,
    pub insert_anchor: InsertAnchorState,
    pub merge_document: MergeDocumentState,
    pub insert_xref: InsertXrefState,
    pub insert_hyperlink: InsertHyperlinkState,
    pub edit_hyperlink: EditHyperlinkState,
    pub index_mark: IndexMarkState,
    pub index_topic: IndexTopicState,
}

impl LongdocDialogs {
    pub fn open_edit_hyperlink(&mut self, index: usize, link: &Hyperlink) {
        self.edit_hyperlink.open_for(index, link);
    }

    pub fn any_open(&self) -> bool {
        self.section_manager.open || self.insert_anchor.open || self.merge_document.open || self.insert_xref.open || self.insert_hyperlink.open || self.edit_hyperlink.open || self.index_mark.open || self.index_topic.open
    }

    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        let mut pending: Vec<Command> = Vec::new();
        {
            let Some(doc) = workspace.session().and_then(|s| s.document()) else { return; };
            ui.horizontal_wrapped(|ui| {
                if button(ui, text("dialog.section_manager.title")).clicked() {
                    self.section_manager.open = true;
                }
                if button(ui, text("dialog.insert_anchor.title")).clicked() {
                    self.insert_anchor.open = true;
                }
                if button(ui, text("dialog.hyperlink.title")).clicked() {
                    self.insert_hyperlink.open = true;
                }
                if !doc.anchors().is_empty() && button(ui, text("dialog.insert_xref.title")).clicked() {
                    self.insert_xref.open = true;
                }
                if !doc.cross_refs().is_empty() && button(ui, text("xref.update_all")).clicked() {
                    pending.push(Command::with_params("xref.update_all", json!({})));
                }
                if button(ui, text("dialog.merge_document.title")).clicked() {
                    self.merge_document.open = true;
                }
            });
        }
        for command in pending {
            let _ = workspace.run(Scope::active(), command);
        }
        self.show_open(ui, workspace);
    }

    pub fn show_open(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        let pending: RefCell<Vec<Command>> = RefCell::new(Vec::new());
        if index_mark_shortcut_pressed(ui.ctx()) {
            self.index_mark.open = true;
        }
        {
            let Some(doc) = workspace.session().and_then(|s| s.document()) else { return; };
            show_section_manager(
                ui,
                doc,
                &mut self.section_manager,
                &|index, section| pending.borrow_mut().push(Command::with_params("section.set", json!({ "index": index, "section": section }))),
                &|index| pending.borrow_mut().push(Command::with_params("section.delete", json!({ "index": index }))),
            );
            show_insert_anchor_dialog(ui, &mut self.insert_anchor, &|name| {
                pending.borrow_mut().push(Command::with_params("anchor.insert", json!({ "name": name })))
            });
            show_merge_document_dialog(ui, doc, &mut self.merge_document, &|path, at| {
                pending.borrow_mut().push(Command::with_params("doc.merge", json!({ "path": path, "at": at })))
            });
            show_insert_xref_dialog(ui, doc, &mut self.insert_xref, &|anchor, format| {
                pending.borrow_mut().push(Command::with_params("xref.insert", json!({ "anchor": anchor, "format": format })))
            });
            show_insert_hyperlink_dialog(ui, &mut self.insert_hyperlink, &|range, target| {
                let params = match range {
                    Some((start, end)) => json!({ "target": target, "start": start, "end": end }),
                    None => json!({ "target": target }),
                };
                pending.borrow_mut().push(Command::with_params("hyperlink.insert", params))
            });
            show_edit_hyperlink_dialog(ui, &mut self.edit_hyperlink, &|index, range, target| {
                pending.borrow_mut().push(Command::with_params(
                    "hyperlink.update",
                    json!({ "index": index, "start": range.0, "end": range.1, "target": target }),
                ))
            });
            show_index_mark_dialog(ui, doc, &mut self.index_mark, &|topic| {
                pending.borrow_mut().push(Command::with_params("index.mark.insert", json!({ "topic": topic })))
            });
            show_index_topic_dialog(ui, doc, &mut self.index_topic, &|name, parent, see, see_also| {
                pending.borrow_mut().push(Command::with_params(
                    "index.topic.add",
                    json!({ "name": name, "parent": parent, "see": see, "see_also": see_also }),
                ))
            });
        }
        for command in pending.into_inner() {
            let _ = workspace.run(Scope::active(), command);
        }
    }
}

#[derive(Default)]
pub struct IndexMarkState {
    pub open: bool,
    pub topic: String,
    pub pick: usize,
}

pub fn show_index_mark_dialog(ui: &mut egui::Ui, doc: &Document, state: &mut IndexMarkState, on_confirm: &dyn Fn(String)) {
    if !state.open {
        return;
    }
    panel_frame(ui, |ui| {
        panel_header(ui, text("index.insert_mark"));
        let paths = dtool_core::longdoc::index::topic_paths(&doc.index.topics);
        if !paths.is_empty() {
            let labels: Vec<&str> = paths.iter().map(String::as_str).collect();
            if state.pick >= labels.len() {
                state.pick = 0;
            }
            let before = state.pick;
            dropdown(ui, "index_mark_topic", &mut state.pick, &labels);
            if before != state.pick || state.topic.is_empty() {
                state.topic = paths[state.pick].clone();
            }
        }
        ui.text_edit_singleline(&mut state.topic);
        ui.horizontal(|ui| {
            if button(ui, text("button.ok")).clicked() && !state.topic.trim().is_empty() {
                on_confirm(state.topic.trim().to_string());
                state.open = false;
            }
            if button(ui, text("button.cancel")).clicked() {
                state.open = false;
            }
        });
    });
}

#[derive(Default)]
pub struct IndexTopicState {
    pub open: bool,
    pub name: String,
    pub parent: usize,
    pub see: String,
    pub see_also: String,
}

pub fn show_index_topic_dialog(ui: &mut egui::Ui, doc: &Document, state: &mut IndexTopicState, on_confirm: &dyn Fn(String, String, String, String)) {
    if !state.open {
        return;
    }
    panel_frame(ui, |ui| {
        panel_header(ui, text("index.add_topic"));
        let mut parents = vec![String::new()];
        parents.extend(dtool_core::longdoc::index::topic_paths(&doc.index.topics));
        let labels: Vec<&str> = parents.iter().map(String::as_str).collect();
        if state.parent >= labels.len() {
            state.parent = 0;
        }
        dropdown(ui, "index_topic_parent", &mut state.parent, &labels);
        ui.text_edit_singleline(&mut state.name);
        ui.horizontal(|ui| {
            ui.label(text("index.see"));
            ui.text_edit_singleline(&mut state.see);
        });
        ui.horizontal(|ui| {
            ui.label(text("index.see_also"));
            ui.text_edit_singleline(&mut state.see_also);
        });
        ui.horizontal(|ui| {
            if button(ui, text("button.ok")).clicked() && !state.name.trim().is_empty() {
                on_confirm(state.name.trim().to_string(), parents[state.parent].clone(), state.see.clone(), state.see_also.clone());
                state.open = false;
                state.name.clear();
                state.see.clear();
                state.see_also.clear();
            }
            if button(ui, text("button.cancel")).clicked() {
                state.open = false;
            }
        });
    });
}

pub fn index_mark_shortcut_pressed(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        i.modifiers.ctrl && i.modifiers.alt && i.modifiers.shift && (i.key_pressed(egui::Key::OpenBracket) || i.key_pressed(egui::Key::OpenCurlyBracket))
    })
}
