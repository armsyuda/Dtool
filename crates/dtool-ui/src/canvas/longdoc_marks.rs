use dtool_core::command::Session;
use dtool_core::longdoc::marks::{layout_marks, MarkKind};
use dtool_tools::dtool_text::notes_layout::{layout_notes, note_line_height};
use dtool_core::view_state::ViewState;

use dtool_core::doc_to_view;

fn screen(rect: egui::Rect, view: &ViewState, point: [f32; 2]) -> egui::Pos2 {
    let v = doc_to_view(view, point);
    egui::pos2(rect.min.x + v[0], rect.min.y + v[1])
}

fn area(rect: egui::Rect, view: &ViewState, r: [f32; 4]) -> egui::Rect {
    egui::Rect::from_two_pos(screen(rect, view, [r[0], r[1]]), screen(rect, view, [r[2], r[3]]))
}

pub fn paint(painter: &egui::Painter, session: &Session, rect: egui::Rect, view: &ViewState, ink: egui::Color32) {
    let doc = &session.document;
    if doc.notes.notes.is_empty() && !(doc.index.show_marks && !doc.index.marks.is_empty()) {
        return;
    }
    let line_height = note_line_height(doc);
    let scale = (screen(rect, view, [0.0, line_height]) - screen(rect, view, [0.0, 0.0])).length() / line_height;
    let accent = dtool_design::color("accent");
    let edge = egui::Stroke::new(dtool_design::stroke_width("md"), accent.gamma_multiply(0.5));
    for note in layout_notes(doc, line_height) {
        let box_rect = area(rect, view, note.rect);
        painter.rect_stroke(box_rect, 0.0, edge, egui::StrokeKind::Inside);
        let font = egui::FontId::proportional((line_height * 0.8 * scale).max(4.0));
        for (index, line) in note.lines.iter().enumerate() {
            let at = box_rect.left_top() + egui::vec2(0.0, index as f32 * line_height * scale);
            painter.text(at, egui::Align2::LEFT_TOP, line, font.clone(), ink);
        }
    }
    for mark in layout_marks(doc) {
        let mark_rect = area(rect, view, mark.rect);
        match mark.kind {
            MarkKind::NoteRef => {
                let font = egui::FontId::proportional((line_height * 0.6 * scale).max(4.0));
                painter.text(mark_rect.left_top(), egui::Align2::LEFT_TOP, mark.label, font, ink);
            }
            MarkKind::NoteMark | MarkKind::IndexMark => {
                painter.rect_filled(mark_rect, 0.0, accent);
            }
        }
    }
}
