use dtool_core::longdoc::notes::{NoteKind, NoteOptions, NoteRestart, SUPPORTED};
use dtool_core::longdoc::sections::NumberStyle;
use dtool_core::workspace::{Scope, Workspace as CoreWorkspace};
use dtool_design::{button, dropdown, number_field, panel_frame, panel_header, toggle};
use dtool_protocol::Command;
use dtool_strings::text;
use serde_json::json;
use std::collections::HashMap;

const KINDS: [(NoteKind, &str, &str); 3] = [
    (NoteKind::Footnote, "note.kind.footnote", "note.insert_footnote"),
    (NoteKind::Endnote, "note.kind.endnote", "note.insert_endnote"),
    (NoteKind::Sidenote, "note.kind.sidenote", "note.insert_sidenote"),
];

const STYLES: [(&str, NumberStyle); 7] = [
    ("1, 2, 3", NumberStyle::Arabic),
    ("i, ii, iii", NumberStyle::RomanLower),
    ("I, II, III", NumberStyle::RomanUpper),
    ("a, b, c", NumberStyle::AlphaLower),
    ("A, B, C", NumberStyle::AlphaUpper),
    ("\u{ac00}, \u{b098}, \u{b2e4}", NumberStyle::Hangul),
    ("\u{4e00}, \u{4e8c}, \u{4e09}", NumberStyle::HanjaNumber),
];

const RESTARTS: [(NoteRestart, &str); 3] = [
    (NoteRestart::Continuous, "note.restart.continuous"),
    (NoteRestart::Page, "note.restart.page"),
    (NoteRestart::Section, "note.restart.section"),
];

#[derive(Default)]
pub struct NotesPanel {
    pub kind: usize,
    pub convert_from: usize,
    pub convert_to: usize,
    bodies: HashMap<u64, String>,
}

impl NotesPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, workspace: &mut CoreWorkspace) {
        let mut pending: Vec<Command> = Vec::new();
        {
            let Some(doc) = workspace.session().and_then(|s| s.document()) else { return };
            panel_frame(ui, |ui| {
                panel_header(ui, text("menu.window.references.notes"));
                ui.horizontal_wrapped(|ui| {
                    for (kind, _, key) in KINDS.iter().filter(|(k, _, _)| SUPPORTED.contains(k)) {
                        if button(ui, text(key)).clicked() {
                            pending.push(Command::with_params("note.insert", json!({ "kind": kind.name() })));
                        }
                    }
                    if button(ui, text("note.go_body")).clicked() {
                        pending.push(Command::with_params("note.go", json!({ "to": "body" })));
                    }
                    if button(ui, text("note.go_reference")).clicked() {
                        pending.push(Command::with_params("note.go", json!({ "to": "reference" })));
                    }
                });
                let kind_labels: Vec<&str> = KINDS.iter().map(|(_, k, _)| text(k)).collect();
                ui.horizontal(|ui| {
                    ui.label(text("note.type"));
                    dropdown(ui, "notes_kind", &mut self.kind, &kind_labels);
                });
                let kind = KINDS.get(self.kind).map(|k| k.0).unwrap_or(NoteKind::Footnote);
                let current = doc.notes.options(kind).clone();
                let mut options: NoteOptions = current.clone();
                toggle(ui, &mut options.custom, text("note.custom"));
                ui.horizontal(|ui| {
                    ui.label(text("section.number_style"));
                    let labels: Vec<&str> = STYLES.iter().map(|(l, _)| *l).collect();
                    let mut selected = STYLES.iter().position(|(_, s)| *s == options.style).unwrap_or(0);
                    if dropdown(ui, "notes_style", &mut selected, &labels).changed() {
                        options.style = STYLES[selected].1.clone();
                    }
                });
                let mut start = options.start as f64;
                if number_field(ui, &mut start, text("note.start_at")).changed() {
                    options.start = start.max(1.0) as u32;
                }
                ui.horizontal(|ui| {
                    ui.label(text("note.restart"));
                    let labels: Vec<&str> = RESTARTS.iter().map(|(_, k)| text(k)).collect();
                    let mut selected = RESTARTS.iter().position(|(r, _)| *r == options.restart).unwrap_or(0);
                    if dropdown(ui, "notes_restart", &mut selected, &labels).changed() {
                        options.restart = RESTARTS[selected].0;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(text("note.format"));
                    ui.add(egui::TextEdit::singleline(&mut options.prefix).desired_width(40.0));
                    ui.label("1");
                    ui.add(egui::TextEdit::singleline(&mut options.suffix).desired_width(40.0));
                });
                if options != current {
                    pending.push(Command::with_params("note.options.set", json!({ "kind": kind.name(), "options": options })));
                }
                ui.horizontal(|ui| {
                    dropdown(ui, "notes_convert_from", &mut self.convert_from, &kind_labels);
                    ui.label("\u{2192}");
                    dropdown(ui, "notes_convert_to", &mut self.convert_to, &kind_labels);
                    if button(ui, text("note.convert")).clicked() && self.convert_from != self.convert_to {
                        if let (Some(from), Some(to)) = (KINDS.get(self.convert_from), KINDS.get(self.convert_to)) {
                            pending.push(Command::with_params("note.convert", json!({ "from": from.0.name(), "to": to.0.name() })));
                        }
                    }
                });
                ui.separator();
                for (id, _, label) in dtool_core::longdoc::notes::note_numbers(doc, kind) {
                    let Some(note) = doc.notes.notes.iter().find(|n| n.id == id) else { continue };
                    let body = self.bodies.entry(id).or_insert_with(|| note.body.clone());
                    ui.horizontal(|ui| {
                        ui.label(label);
                        let response = ui.text_edit_singleline(body);
                        if response.lost_focus() && *body != note.body {
                            pending.push(Command::with_params("note.set_body", json!({ "id": id, "body": body.clone() })));
                        }
                        if button(ui, text("button.delete")).clicked() {
                            pending.push(Command::with_params("note.delete", json!({ "id": id })));
                        }
                    });
                }
                self.bodies.retain(|id, _| doc.notes.notes.iter().any(|n| n.id == *id));
            });
        }
        for command in pending {
            let _ = workspace.run(Scope::active(), command);
        }
    }
}
