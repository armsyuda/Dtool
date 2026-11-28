use dtool_core::command::Session;
use dtool_core::view_state::ViewState;
use dtool_core::ObjectId;
use dtool_tools::dtool_text::spell_live::{squiggle, story_underlines, LiveSpell, Underline, SETTLE};
use dtool_tools::frame_text::{flow_frame_quad, FrameTextTool};
use egui::{Response, Ui};

use crate::canvas::CanvasView;
use dtool_core::{doc_to_view, view_to_doc};

thread_local! {
    static TOOL: std::cell::RefCell<FrameTextTool> = std::cell::RefCell::new(FrameTextTool::new());
    static LIVE_SPELL: std::cell::RefCell<Option<LiveSpell>> = const { std::cell::RefCell::new(None) };
    static SPELL_PENDING: std::cell::RefCell<std::collections::HashSet<dtool_core::StoryId>> = std::cell::RefCell::new(std::collections::HashSet::new());
}

fn with_live_spell<R>(f: impl FnOnce(&mut LiveSpell) -> R) -> R {
    LIVE_SPELL.with(|cell| f(cell.borrow_mut().get_or_insert_with(LiveSpell::en_us)))
}

pub fn note_spell_edit(session: &Session) {
    let Some(story) = session.text_edit.as_ref().map(|edit| edit.story) else { return };
    with_live_spell(|live| live.note_edit(story, std::time::Instant::now()));
    SPELL_PENDING.with(|pending| pending.borrow_mut().insert(story));
}

pub fn spell_pending(session: &Session) -> bool {
    session.text_edit.as_ref().is_some_and(|edit| SPELL_PENDING.with(|pending| pending.borrow().contains(&edit.story)) || with_live_spell(|live| live.pending(edit.story)))
}

pub fn spell_underlines(session: &Session) -> Vec<(ObjectId, Underline)> {
    let Some(story) = session.text_edit.as_ref().map(|edit| edit.story) else { return Vec::new() };
    let marks = with_live_spell(|live| {
        let pending = SPELL_PENDING.with(|pending| pending.borrow().contains(&story)) || live.pending(story);
        if pending {
            live.tick(session, std::time::Instant::now());
            if !live.is_stale(session, story) {
                SPELL_PENDING.with(|pending| pending.borrow_mut().remove(&story));
            }
        } else if live.is_stale(session, story) {
            live.refresh(session, story);
        }
        live.marks(story).to_vec()
    });
    if marks.is_empty() {
        return Vec::new();
    }
    let lib = dtool_tools::dtool_text::FontLibrary::shared();
    story_underlines(&lib, &session.document, story, &marks)
}

pub fn set_grammar_enabled(enabled: bool) {
    with_live_spell(|live| live.set_grammar(enabled));
    // A settings change invalidates the cached check, including edit debounce state.
    SPELL_PENDING.with(|pending| pending.borrow_mut().clear());
}

pub fn grammar_underlines(session: &Session) -> Vec<(ObjectId, Underline)> {
    let Some(story) = session.text_edit.as_ref().map(|edit| edit.story) else { return Vec::new() };
    let marks = with_live_spell(|live| live.issue_marks(story));
    if marks.is_empty() { return Vec::new(); }
    let lib = dtool_tools::dtool_text::FontLibrary::shared();
    story_underlines(&lib, &session.document, story, &marks)
}

pub fn spell_at_caret(session: &Session) -> Option<(dtool_core::StoryId, dtool_tools::dtool_text::spelling::Misspelling, Vec<String>)> {
    let edit = session.text_edit.as_ref()?;
    let (story, caret) = (edit.story, edit.caret);
    with_live_spell(|live| {
        if live.is_stale(session, story) {
            live.refresh(session, story);
        }
        live.suggestions_at(story, caret).or_else(|| caret.checked_sub(1).and_then(|index| live.suggestions_at(story, index)))
    })
    .map(|(mark, list)| (story, mark, list))
}

pub fn refresh_spelling(session: &Session, ignored: Option<&str>) {
    let Some(story) = session.text_edit.as_ref().map(|edit| edit.story) else { return };
    with_live_spell(|live| {
        if let Some(word) = ignored {
            live.ignore(session, word);
        }
        live.refresh(session, story);
    });
}

fn paint_spelling(ui: &Ui, session: &Session, rect: egui::Rect, view: &ViewState) {
    let lines = spell_underlines(session);
    let grammar = grammar_underlines(session);
    if spell_pending(session) { ui.ctx().request_repaint_after(SETTLE); }
    let painter = ui.painter_at(rect);
    let amplitude = dtool_design::size("icon.size.sm") * 0.1;
    for (lines, colour) in [(lines, ui.visuals().error_fg_color), (grammar, ui.visuals().hyperlink_color)] {
        let stroke = egui::Stroke::new(dtool_design::stroke_width("md"), colour);
        for (_, line) in lines {
            let a = screen(rect, view, [line.x0, line.y]); let b = screen(rect, view, [line.x1, line.y]);
            let on_screen = Underline { start: line.start, end: line.end, x0: a.x, x1: b.x, y: a.y + amplitude };
            let points: Vec<egui::Pos2> = squiggle(&on_screen, amplitude, amplitude * 2.5).into_iter().map(|[x, y]| egui::pos2(x, y)).collect();
            painter.add(egui::Shape::line(points, stroke));
        }
    }
}

fn screen(rect: egui::Rect, view: &ViewState, point: [f32; 2]) -> egui::Pos2 {
    let v = doc_to_view(view, point);
    egui::pos2(rect.min.x + v[0], rect.min.y + v[1])
}

fn port_mark(painter: &egui::Painter, at: egui::Pos2, side: f32, color: egui::Color32, tip: egui::Color32, filled: bool) {
    let mark = egui::Rect::from_center_size(at, egui::vec2(side, side));
    let stroke = egui::Stroke::new(dtool_design::stroke_width("md"), color);
    if filled {
        painter.rect_filled(mark, 0.0, color);
    } else {
        painter.rect_stroke(mark, 0.0, stroke, egui::StrokeKind::Inside);
    }
    let inner = mark.shrink(side * 0.25);
    let tip = if filled { tip } else { color };
    painter.add(egui::Shape::convex_polygon(vec![inner.left_top(), inner.right_center(), inner.left_bottom()], tip, egui::Stroke::NONE));
}

fn paint_flow(ui: &Ui, session: &Session, rect: egui::Rect, view: &ViewState, tool: &mut FrameTextTool, target: Option<ObjectId>) {
    if !session.has_document {
        return;
    }
    let marks = tool.flow_marks(session).clone();
    let painter = ui.painter_at(rect);
    let accent = dtool_design::color("accent");
    let overflow = ui.visuals().error_fg_color;
    let side = dtool_design::size("icon.size.sm") * 0.75;
    let tip = ui.visuals().extreme_bg_color;
    let line = egui::Stroke::new(dtool_design::stroke_width("md"), accent);
    if let Some(from) = tool.linking() {
        for id in [Some(from), target].into_iter().flatten() {
            if let Some(object) = session.document.get_object(id) {
                let corners = flow_frame_quad(&session.document, object).map(|p| screen(rect, view, p));
                painter.add(egui::Shape::convex_polygon(corners.to_vec(), accent.gamma_multiply(0.25), line));
            }
        }
    }
    for (from, to) in &marks.links {
        let (Some(a), Some(b)) = (session.document.get_object(*from), session.document.get_object(*to)) else {
            continue;
        };
        let start = screen(rect, view, flow_frame_quad(&session.document, a)[2]);
        let end = screen(rect, view, flow_frame_quad(&session.document, b)[0]);
        painter.line_segment([start, end], line);
        port_mark(&painter, start, side, accent, tip, true);
        port_mark(&painter, end, side, accent, tip, true);
    }
    for id in &marks.overset {
        if let Some(object) = session.document.get_object(*id) {
            let out = flow_frame_quad(&session.document, object)[2];
            port_mark(&painter, screen(rect, view, out), side, overflow, tip, true);
        }
    }
    crate::canvas::longdoc_marks::paint(&painter, session, rect, view, ui.visuals().text_color());
}

pub fn paint_overlay(ui: &Ui, session: &Session, rect: egui::Rect, view: &ViewState) {
    TOOL.with(|tool| paint_flow(ui, session, rect, view, &mut tool.borrow_mut(), None));
    if !is_composing() && session.text_composing.is_none() {
        paint_spelling(ui, session, rect, view);
    }
    let doc = &session.document;
    if let Some((id, path)) = session.text_edit.as_ref().and_then(|edit| edit.object)
        .and_then(|id| Some((id, doc.get_object(id)?.path_text_data()?.path)))
        .and_then(|(id, path)| Some((id, doc.get_object(path)?)))
    {
        let painter = ui.painter_at(rect);
        let stroke = egui::Stroke::new(1.0, ui.visuals().selection.bg_fill.gamma_multiply(0.55));
        for (points, closed) in dtool_core::scene::text_path_guide(doc, id, path) {
            let points = points.into_iter().map(|point| screen(rect, view, point)).collect();
            painter.add(if closed { egui::Shape::closed_line(points, stroke) } else { egui::Shape::line(points, stroke) });
        }
    }
}

pub fn paint_caret(ui: &Ui, session: &Session, response: &Response, view: &ViewState) {
    let Some(edit) = session.text_edit.as_ref().filter(|edit| edit.caret == edit.anchor) else { return; };
    if ui.memory(|memory| memory.focused().is_some_and(|id| id != response.id))
        || is_composing() || session.text_composing.is_some()
    {
        return;
    }
    let line = caret_line(session).map(|ends| ends.map(|point| screen(response.rect, view, point)));
    let Some(points) = line else { return; };
    let key = response.id.with("text-caret-blink");
    let stamp = (edit.story.0, edit.caret, session.document.version, points);
    let now = ui.input(|input| input.time);
    let since = ui.data_mut(|data| {
        let state = data.get_temp_mut_or_insert_with(key, || (stamp, now));
        if state.0 != stamp { *state = (stamp, now); }
        state.1
    });
    let phase = (now - since).max(0.0) % 1.0;
    let edge = if phase < 0.5 { 0.5 } else { 1.0 };
    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(edge - phase));
    if phase < 0.5 {
        let painter = ui.painter_at(response.rect);
        painter.line_segment(points, egui::Stroke::new(3.0, egui::Color32::WHITE));
        painter.line_segment(points, egui::Stroke::new(1.0, egui::Color32::BLACK));
    }
}

#[derive(Clone, Copy)]
pub(crate) enum TextRangeMark {
    Selection(egui::Color32),
    Composition(egui::Stroke),
}

impl TextRangeMark {
    pub(crate) fn paint(self, painter: &egui::Painter, points: [egui::Pos2; 4]) {
        match self {
            Self::Selection(fill) => {
                painter.add(egui::Shape::convex_polygon(points.to_vec(), fill, egui::Stroke::NONE));
            }
            Self::Composition(stroke) => {
                painter.line_segment([points[3], points[2]], stroke);
            }
        }
    }
}

pub fn paint_text_selection(ui: &Ui, session: &Session, response: &Response, view: &ViewState) {
    if is_composing() || session.text_composing.is_some() { return; }
    let accent = ui.visuals().selection.bg_fill;
    let alpha = if ui.memory(|memory| memory.focused().is_some_and(|id| id != response.id)) { 54 } else { 112 };
    let fill = egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha);
    paint_text_range(ui, session, response.rect, view, TextRangeMark::Selection(fill));
}

pub(crate) fn paint_text_range(
    ui: &Ui, session: &Session, rect: egui::Rect, view: &ViewState, mark: TextRangeMark,
) {
    use dtool_tools::dtool_text as text;
    let Some(edit) = session.text_edit.as_ref().filter(|edit| edit.caret != edit.anchor) else { return; };
    if edit.object.and_then(|id| session.document.get_object(id))
        .is_some_and(|object| matches!(object.kind, dtool_core::ObjectKind::Table(_))) {
        let mark = match mark {
            TextRangeMark::Selection(_) => TextRangeMark::Selection(ui.visuals().selection.bg_fill.gamma_multiply(0.45)),
            other => other,
        };
        crate::table::canvas::paint_text_range(ui, rect, session, view, mark);
        return;
    }
    let doc = &session.document;
    let Some(core) = doc.stories.iter().find(|story| story.id == edit.story) else { return; };
    let selected = edit.caret.min(edit.anchor)..edit.caret.max(edit.anchor);
    let painter = ui.painter_at(rect);
    let draw = |id, quad: [[f32; 2]; 4]| {
        let points = dtool_core::commands::warp::live_warp_points(doc, id, &quad).and_then(|points| points.try_into().ok()).unwrap_or(quad);
        mark.paint(&painter, points.map(|point| screen(rect, view, point)));
    };
    let library = text::FontLibrary::shared();
    let drawn = |id| dtool_core::scene::is_drawn_with_flags(doc, &view.canvas_flags, id);
    let object = edit.object.and_then(|id| doc.get_object(id))
        .or_else(|| doc.objects.iter().find(|object| object.story_id() == Some(edit.story)));
    if let Some(object) = object.filter(|object| object.is_path_text()) {
        if !drawn(object.id) { return; }
        let points = text::path_text::path_text_insertions(&library, doc, object);
        for edge in points.windows(2) {
            let [(start, [a, b]), (end, [c, d])] = [edge[0], edge[1]];
            if end == start + 1 && selected.contains(&start) {
                draw(object.id, [a, c, d, b]);
            }
        }
        return;
    }
    let story = text::bridge::story_in_document(core, doc);
    let default_size = dtool_core::text_cmd::DEFAULT_SIZE_PT * dtool_core::units::units_per_point(doc.units, doc.dpi);
    let paint_lines = |object: &dtool_core::object::Object, lines: &[text::story::Line]| {
        let matrix = dtool_core::geometry::text_matrix(object);
        let at = |x, y| dtool_core::geometry::apply_matrix(&matrix, [object.pos[0] + x, object.pos[1] + y]);
        for line in lines.iter().filter(|line| selected.start < line.range.end && line.range.start < selected.end) {
            let mut stops: Vec<usize> = line.body_glyphs().iter().map(|glyph| glyph.cluster).collect();
            stops.push(line.range.end);
            stops.sort_unstable();
            stops.dedup();
            let mut pen = 0.0;
            for cluster in line.body_glyphs().chunk_by(|a, b| a.cluster == b.cluster) {
                let first = &cluster[0];
                let start = first.cluster;
                let x = pen + first.offset[0];
                let advance = cluster.iter().map(|glyph| glyph.advance).sum::<f32>();
                pen += advance;
                let Some(&end) = stops.get(stops.partition_point(|stop| *stop <= start)) else { continue; };
                let low = selected.start.max(start);
                let high = selected.end.min(end);
                if low >= high || advance.abs() <= f32::EPSILON { continue; }
                let magnitude = advance / (end - start) as f32;
                let left = x + (low - start) as f32 * magnitude;
                let right = x + (high - start) as f32 * magnitude;
                let size = story.chars.iter().rev().find(|run| run.range.start <= start && start < run.range.end)
                    .map_or(default_size, |run| run.style.size_pt);
                let top = line.baseline - size * 0.85;
                let bottom = line.baseline + size * 0.25;
                draw(object.id, [at(left, top), at(right, top), at(right, bottom), at(left, bottom)]);
            }
        }
    };
    if let Some(object) = object.filter(|object| matches!(object.kind, dtool_core::object::ObjectKind::ArtText { .. })) {
        if drawn(object.id) { paint_lines(object, &text::layout_story(&library, &story, None)); }
    } else if let Some(flow) = text::frame::layout_doc_flow(&library, doc, edit.story) {
        for (id, lines) in flow.frames.iter().zip(&flow.layout.per_frame) {
            if let Some(frame) = doc.get_object(*id).filter(|frame| drawn(frame.id)) {
                paint_lines(frame, lines);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct FramePointerGesture {
    press: egui::Pos2,
    frame: Option<ObjectId>,
}

pub fn handle(ui: &mut Ui, session: &mut Session, response: &Response, view: &ViewState) {
    let rect = response.rect;
    let selection_drag = response.id.with("frame-text-selection-drag");
    let active = FrameTextTool::is_active(&session.active_tool);
    if !active {
        ui.data_mut(|data| data.remove::<FramePointerGesture>(selection_drag));
        commit_pending(session);
        TOOL.with(|tool| {
            let mut tool = tool.borrow_mut();
            tool.cancel();
            paint_flow(ui, session, rect, view, &mut tool, None);
        });
        return;
    }
    text_input(ui, session, response, view);
    let (down, up) = crate::input::primary_button_positions(ui);
    let down = down.filter(|point| response.interact_rect.contains(*point));
    if let Some(press) = down {
        ui.data_mut(|data| data.insert_temp(selection_drag, FramePointerGesture { press, frame: None }));
    }
    let pointer = up.or_else(|| response.hover_pos()).or_else(|| ui.input(|i| i.pointer.latest_pos()));
    let Some(pointer) = pointer else {
        TOOL.with(|tool| paint_flow(ui, session, rect, view, &mut tool.borrow_mut(), None));
        return;
    };
    let local = [pointer.x - rect.min.x, pointer.y - rect.min.y];
    let doc_point = view_to_doc(view, local);
    let tolerance = (view_to_doc(view, [local[0] + dtool_design::size("icon.size.sm") * 0.5, local[1]])[0] - doc_point[0]).abs();
    TOOL.with(|cell| {
        let mut tool = cell.borrow_mut();
        tool.hover(session, doc_point);
        let port = if tool.linking().is_none() { tool.out_port_at(&session.document, doc_point, tolerance) } else { None };
        let target = tool.linking().and_then(|_| tool.link_target(&session.document, doc_point));
        paint_flow(ui, session, rect, view, &mut tool, target);
        if response.hovered() && !tool.get_status_text().is_empty() {
            dtool_design::canvas_text_hint(ui,[pointer.x,pointer.y],dtool_strings::text(tool.get_status_text()));
        }
        let gesture = ui.data(|data| data.get_temp::<FramePointerGesture>(selection_drag));
        let moved = gesture.is_some_and(|gesture| gesture.press.distance(pointer) >= 4.0);
        if let Some(mut gesture) = gesture {
            if gesture.frame.is_none() && moved && tool.linking().is_none() {
                let start = view_to_doc(view, [gesture.press.x - rect.min.x, gesture.press.y - rect.min.y]);
                if let Some(id) = dtool_tools::frame_text::hit_frame_id(&session.document, start)
                    .filter(|id| dtool_core::scene::is_drawn_with_flags(&session.document, &view.canvas_flags, *id)
                        && tool.out_port_at(&session.document, start, tolerance).is_none())
                {
                    if let Some((story, caret)) = session.document.get_object(id).and_then(|object| object.story_id())
                        .zip(dtool_tools::frame_text::caret_in_frame(&session.document, id, start))
                    {
                        let cmd = dtool_protocol::Command::with_params("text.select", serde_json::json!({
                            "id": id.0, "story": story.0, "range": { "start": caret, "end": caret }
                        }));
                        if session.run(&cmd).is_ok() {
                            tool.cancel();
                            response.request_focus();
                            gesture.frame = Some(id);
                        }
                    }
                }
            }
            let released = up.is_some() || !ui.input(|i| i.pointer.primary_down());
            ui.data_mut(|data| {
                if released {
                    data.remove::<FramePointerGesture>(selection_drag);
                } else {
                    data.insert_temp(selection_drag, gesture);
                }
            });
            if let Some(id) = gesture.frame {
                if !released || response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                }
                let target = dtool_tools::frame_text::hit_frame_id(&session.document, doc_point)
                    .filter(|target| dtool_core::scene::is_drawn_with_flags(&session.document, &view.canvas_flags, *target)
                        && session.document.get_object(*target).and_then(|object| object.story_id())
                            == session.text_edit.as_ref().map(|edit| edit.story))
                    .unwrap_or(id);
                if let Some(caret) = dtool_tools::frame_text::caret_in_frame(&session.document, target, doc_point) {
                    extend_selection(session, caret);
                }
                return;
            }
        }
        if let Some(press) = down {
            tool.begin_pointer(view_to_doc(view, [press.x - rect.min.x, press.y - rect.min.y]));
        } else if gesture.is_none() && response.drag_started() {
            tool.begin_pointer(doc_point);
        }
        if moved || response.dragged() {
            tool.move_pointer(doc_point);
            if let Some((start, current)) = tool.drag_corners() {
                let a = doc_to_view(view, start);
                let b = doc_to_view(view, current);
                let preview = egui::Rect::from_two_pos(
                    egui::pos2(rect.min.x + a[0], rect.min.y + a[1]),
                    egui::pos2(rect.min.x + b[0], rect.min.y + b[1]),
                );
                dtool_design::canvas_frame_outline(ui,preview);
            }
        }
        if response.hovered() || response.is_pointer_button_down_on() {
            let cursor = if tool.linking().is_some() {
                egui::CursorIcon::Alias
            } else if tool.drag_corners().is_some() {
                egui::CursorIcon::Crosshair
            } else if port.is_some() {
                egui::CursorIcon::Alias
            } else if !tool.get_status_text().is_empty()
                || dtool_tools::frame_text::hit_frame_id(&session.document, doc_point).is_some_and(|id| {
                    dtool_core::scene::is_drawn_with_flags(&session.document, &view.canvas_flags, id)
                })
            {
                egui::CursorIcon::Text
            } else {
                egui::CursorIcon::Crosshair
            };
            ui.ctx().set_cursor_icon(cursor);
        }
        let raw_finished = gesture.is_some() && up.is_some();
        let clicked = response.clicked() || (raw_finished && !moved);
        let finished = raw_finished || response.drag_stopped() || clicked || response.double_clicked();
        if finished && tool.linking().is_some() {
            if clicked && tool.drag_corners().is_none() {
                tool.begin_pointer(doc_point);
            }
            let keep = ui.input(|i| i.modifiers.ctrl);
            if tool.finish_link(session, doc_point, keep).is_ok() {
                response.request_focus();
            }
        } else if finished && clicked && !response.double_clicked() && tool.drag_corners().is_none() && port.is_some() {
            if let Some(from) = port {
                tool.begin_link(from);
            }
        } else if finished {
            if clicked && !response.drag_started() && !response.double_clicked() && tool.drag_corners().is_none() {
                tool.begin_pointer(doc_point);
            }
            let hit=if tool.drag_corners().is_none() {dtool_tools::frame_text::hit_frame_id(&session.document,doc_point)} else {None};
            let caret=hit.and_then(|id|dtool_tools::frame_text::caret_in_frame(&session.document,id,doc_point).map(|caret|(id,caret)));
            let result=tool.finish(session,doc_point,response.double_clicked());
            if result.is_ok() {
                response.request_focus();
                if let Some((id,caret))=caret {
                    if let Some(story)=session.document.get_object(id).and_then(|o|o.story_id()) {
                        let command=dtool_protocol::Command::with_params("text.select",serde_json::json!({"id":id.0,"story":story.0,"range":{"start":caret,"end":caret}}));
                        let _=session.run(&command);
                    }
                }
            }
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if tool.linking().is_some() {
                tool.cancel_link();
            } else {
                tool.cancel();
                let cmd = dtool_protocol::Command::new("text.edit.end");
                let _ = session.run(&cmd);
            }
        }
    });
}

pub(crate) fn extend_selection(session: &mut Session, caret: usize) {
    if session.text_edit.as_ref().is_some_and(|edit| edit.caret != caret) {
        let cmd = dtool_protocol::Command::with_params("text.edit.caret", serde_json::json!({ "to": caret, "extend": true }));
        let _ = session.run(&cmd);
    }
}

pub fn caret_screen_rect(session: &Session, view: &ViewState, canvas: egui::Rect) -> Option<egui::Rect> {
    caret_screen_rect_at(session, view, canvas, session.text_edit.as_ref()?.caret)
}

fn caret_screen_rect_at(session: &Session, view: &ViewState, canvas: egui::Rect, at: usize) -> Option<egui::Rect> {
    let [a, b] = caret_line_at(session, at)?;
    let rect = egui::Rect::from_two_pos(screen(canvas, view, a), screen(canvas, view, b));
    Some(egui::Rect::from_min_size(rect.min, rect.size().max(egui::Vec2::splat(1.0))))
}

fn edited_text_object(session: &Session) -> Option<&dtool_core::Object> {
    let edit = session.text_edit.as_ref()?;
    edit.object.and_then(|id| session.document.get_object(id)).or_else(|| {
        session.document.objects.iter().find(|object| object.story_id() == Some(edit.story))
    })
}

fn caret_line(session: &Session) -> Option<[[f32; 2]; 2]> {
    caret_line_at(session, session.text_edit.as_ref()?.caret)
}

fn caret_line_at(session: &Session, at: usize) -> Option<[[f32; 2]; 2]> {
    if let Some(ends) = crate::table::canvas::caret_line_at(session, at) {
        return Some(ends);
    }
    let object = edited_text_object(session)?;
    let [x, y] = object.pos;
    let (target, ends) = if let Some(ends) = path_caret(session, at) {
        (object, ends)
    } else if let Some(ends) = art_caret_line(session, at) {
        (object, ends)
    } else if let Some((frame, ends)) = caret_layout_line(session, at) {
        (session.document.get_object(frame).unwrap_or(object), ends)
    } else {
        (object, [[x, y], [x, y + (object.size[1].abs() / object.text_scale[1]).max(12.0)]])
    };
    let ends = if target.is_path_text() {
        ends
    } else {
        let matrix = dtool_core::geometry::text_matrix(target);
        ends.map(|point| dtool_core::geometry::apply_matrix(&matrix, point))
    };
    let warped = dtool_core::commands::warp::live_warp_points(&session.document, target.id, &ends);
    Some(warped.and_then(|points| points.try_into().ok()).unwrap_or(ends))
}

fn path_caret(session: &Session, at: usize) -> Option<[[f32; 2]; 2]> {
    use dtool_tools::dtool_text::{path_text::path_text_insertions, FontLibrary};
    let edit = session.text_edit.as_ref()?;
    let object = session.document.get_object(edit.object?)?;
    if !object.is_path_text() { return None; }
    path_text_insertions(&FontLibrary::shared(), &session.document, object).into_iter()
        .min_by_key(|(index, _)| index.abs_diff(at)).map(|(_, line)| line)
}

pub(crate) fn path_text_at(scene: &dtool_core::scene::Scene, session: &Session, point: [f32; 2]) -> Option<(dtool_core::ObjectId, dtool_core::StoryId, usize)> {
    use dtool_core::scene::Prim;
    let tolerance = 5.0 / session.view.zoom.max(1e-4);
    scene.prims.iter().rev().find_map(|prim| {
        let Prim::Text { object: id, .. } = prim else { return None; };
        let object = session.document.get_object(*id)?;
        let data = object.path_text_data()?;
        if object.locked || !object.pickable { return None; }
        let (index, distance) = closest_path_insertion(&session.document, object, point)?;
        (distance <= tolerance).then_some((*id, data.story, index))
    })
}

pub(crate) fn closest_path_insertion(
    doc: &dtool_core::Document, object: &dtool_core::Object, point: [f32; 2],
) -> Option<(usize, f32)> {
    use dtool_core::scene_hit::segment_distance;
    use dtool_tools::dtool_text::{path_text::path_text_insertions, FontLibrary};
    let insertions = path_text_insertions(&FontLibrary::shared(), doc, object);
    let points: Vec<_> = insertions.iter().flat_map(|(_, ends)| *ends).collect();
    let mapped = dtool_core::commands::warp::live_warp_points(doc, object.id, &points).unwrap_or(points);
    insertions.into_iter().zip(mapped.chunks_exact(2))
        .map(|((index, _), ends)| (index, segment_distance(point, ends[0], ends[1])))
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
}

fn composition_insertion(edit: &dtool_core::command::TextEditState) -> usize {
    // Committing a composition replaces the selection from its first character.
    edit.anchor.min(edit.caret)
}

pub fn publish_caret_ime(ctx: &egui::Context, session: &Session, view: &ViewState, canvas: egui::Rect) {
    let Some(edit) = session.text_edit.as_ref() else { return; };
    let at = if is_composing() || session.text_composing.is_some() {
        composition_insertion(edit)
    } else {
        edit.caret
    };
    if let Some(rect) = caret_screen_rect_at(session, view, canvas, at) {
        crate::input::publish_ime_caret(ctx, rect);
    }
}

pub fn bind_canvas(view: &mut CanvasView, ui: &mut Ui, session: &mut Session, response: &Response, state: &ViewState) {
    let _ = view;
    handle(ui, session, response, state);
}

thread_local! {
    static COMPOSE: std::cell::RefCell<crate::input::ComposeMachine> = std::cell::RefCell::new(crate::input::ComposeMachine::new());
    static ROUTE: std::cell::Cell<crate::input::EditRoute> = const { std::cell::Cell::new(crate::input::EditRoute::Session) };
}

pub fn set_edit_route(route: crate::input::EditRoute) {
    ROUTE.with(|cell| cell.set(route));
}

pub fn edit_route() -> crate::input::EditRoute {
    ROUTE.with(|cell| cell.get())
}

pub fn preedit_text() -> Option<String> {
    COMPOSE.with(|cell| cell.borrow().preedit().map(str::to_string))
}

pub fn is_composing() -> bool {
    COMPOSE.with(|cell| cell.borrow().is_composing())
}

pub fn text_entry_active(session: &Session) -> bool {
    session.text_edit.is_some() && FrameTextTool::is_active(&session.active_tool)
}

pub fn feed_text_input(session: &mut Session, events: &[egui::Event]) -> bool {
    if session.text_edit.is_none() {
        COMPOSE.with(|cell| cell.borrow_mut().clear());
        return false;
    }
    let vertical = edited_text_object(session).is_some_and(dtool_core::Object::is_vertical_art_text);
    let actions = COMPOSE.with(|cell| cell.borrow_mut().feed_oriented(events, vertical));
    let mut changed = crate::input::apply_to_session(session, &actions, edit_route());
    if !is_composing() {
        for event in events {
            if let egui::Event::Paste(text) = event {
                changed |= crate::text_edit::paste::paste_at_caret(session, text);
            }
        }
    }
    if changed {
        note_spell_edit(session);
    }
    changed
}

pub fn commit_pending(session: &mut Session) -> bool {
    let Some(text) = COMPOSE.with(|cell| cell.borrow_mut().take_pending()) else {
        return false;
    };
    crate::input::apply_to_session(session, &[crate::input::EditAction::Commit(text)], edit_route())
}

pub fn finish_text_input(session: &mut Session) -> bool {
    let changed = commit_pending(session);
    if session.text_edit.is_some() {
        let _ = session.run(&dtool_protocol::Command::new("text.edit.end"));
    }
    changed
}

pub(crate) fn text_input(ui: &mut Ui, session: &mut Session, response: &Response, view: &ViewState) {
    let keyboard = ui.ctx().memory(|m| m.focused().map_or(true, |id| id == response.id));
    if session.text_edit.is_none() || !keyboard {
        return;
    }
    let events = ui.input(|i| i.events.clone());
    let before = composition_text(session);
    let mut start = 0;
    if edited_text_object(session).is_some_and(|object| matches!(object.kind, dtool_core::ObjectKind::Table(_))) {
        for (index, event) in events.iter().enumerate() {
            let egui::Event::Key { key: egui::Key::Tab, pressed: true, modifiers, .. } = event else { continue; };
            if modifiers.command || modifiers.ctrl || modifiers.alt { continue; }
            feed_text_input(session, &events[start..index]);
            if crate::table::canvas::step_edit_cell(session, modifiers.shift).unwrap_or(false) {
                ui.ctx().request_repaint();
            }
            start = index + 1;
        }
    }
    feed_text_input(session, &events[start..]);
    if before != composition_text(session) {
        ui.ctx().request_repaint();
    }
    if response.clicked() || response.drag_started() {
        commit_pending(session);
    }
    let filter = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, ..Default::default() };
    ui.memory_mut(|m| m.set_focus_lock_filter(response.id, filter));
    publish_caret_ime(ui.ctx(), session, view, response.rect);
}

fn caret_layout_line(session: &Session, at: usize) -> Option<(ObjectId, [[f32; 2]; 2])> {
    let edit = session.text_edit.as_ref()?;
    let mut offset = 0usize;
    let mut found = false;
    for story in &session.document.stories {
        if story.id == edit.story {
            found = true;
            break;
        }
        offset += story.text.chars().count() + 1;
    }
    if !found {
        return None;
    }
    let point = dtool_core::longdoc::marks::caret_point(&session.document, offset + at)?;
    Some((point.frame, [point.point, [point.point[0], point.point[1] + point.line_height]]))
}

pub(crate) fn composition_text(session: &Session) -> Option<String> {
    if edit_route() == crate::input::EditRoute::Core {
        session.text_composing.clone()
    } else {
        preedit_text()
    }
}

fn art_caret_line(session: &Session, at: usize) -> Option<[[f32; 2]; 2]> {
    use dtool_tools::dtool_text as text;
    let edit = session.text_edit.as_ref()?;
    let object = edited_text_object(session)?;
    if !matches!(object.kind, dtool_core::object::ObjectKind::ArtText { .. }) {
        return None;
    }
    let core = session.document.stories.iter().find(|story| story.id == edit.story)?;
    let story = text::bridge::story_in_document(core, &session.document);
    let lines = text::layout_story(&text::FontLibrary::shared(), &story, None);
    let size = dtool_core::text_cmd::size_pt_of(core, dtool_core::text_cmd::DEFAULT_SIZE_PT)
        * dtool_core::units::units_per_point(session.document.units, session.document.dpi);
    let line = lines.iter().find(|line| at >= line.range.start && at <= line.range.end).or_else(|| lines.last());
    let point = line.map_or(object.pos, |line| {
        let x = text::frame::caret_x(line, at);
        let top = if line.range.start == 0 { 0.0 } else { line.baseline - size * 0.8 };
        [object.pos[0] + x, object.pos[1] + top]
    });
    Some([point, [point[0], point[1] + size * 1.2]])
}
