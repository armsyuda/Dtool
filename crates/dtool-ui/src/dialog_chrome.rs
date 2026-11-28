use dtool_design::{dialog_frame, panel_header};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogMode {
    Modal,
    Nonmodal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosePolicy {
    Free,
    CancelOnly,
    JobRunning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogResult {
    Open,
    Confirm,
    Cancel,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChromeInput {
    pub escape: bool,
    pub enter: bool,
    pub click_outside: bool,
}

pub fn decide(input: ChromeInput, mode: DialogMode, policy: ClosePolicy, can_confirm: bool) -> DialogResult {
    if policy == ClosePolicy::JobRunning {
        return DialogResult::Open;
    }
    if input.escape {
        return DialogResult::Cancel;
    }
    if input.click_outside && mode == DialogMode::Modal && policy == ClosePolicy::Free {
        return DialogResult::Cancel;
    }
    if input.enter && can_confirm {
        return DialogResult::Confirm;
    }
    DialogResult::Open
}

pub const MODAL_FLAG: &str = "dtool.dialog_chrome.modal_open";

const FLOATING_DRAG: &str = "dtool.dialog_chrome.floating_drag";

pub fn begin_modal_frame(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| {
        data.remove_temp::<bool>(egui::Id::new(FLOATING_DRAG));
        data.remove_temp::<bool>(egui::Id::new(MODAL_FLAG)).unwrap_or(false)
    })
}

pub(crate) fn floating_drag_owns_pointer(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(egui::Id::new(FLOATING_DRAG))).unwrap_or(false)
}

pub fn modal_open(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(egui::Id::new(MODAL_FLAG))).unwrap_or(false)
}

pub fn backdrop_color() -> egui::Color32 {
    egui::Color32::TRANSPARENT
}

pub(crate) fn show_floating(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    add: impl FnOnce(&mut egui::Ui),
) {
    show_floating_at(ctx, id, title, None, add);
}

pub(crate) fn show_floating_at(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    initial_position: Option<egui::Pos2>,
    add: impl FnOnce(&mut egui::Ui),
) {
    let id = egui::Id::new(id);
    let key = id.with("floating_dialog_state");
    let mut chrome: DialogChrome = ctx.data(|data| data.get_temp(key)).unwrap_or(DialogChrome {
        position: initial_position,
        ..DialogChrome::default()
    });
    let shown = chrome.area(id).show(ctx, |ui| dialog_contents(ui, title, add));
    if ctx.memory(|memory| memory.allows_interaction(shown.response.layer_id))
        && !egui::Popup::is_any_open(ctx)
    {
        let was_dragging = chrome.drag_origin.is_some();
        let moved = chrome.move_by_title(ctx, shown.response.rect, shown.inner.1);
        if was_dragging || moved || chrome.drag_origin.is_some() {
            ctx.data_mut(|data| data.insert_temp(egui::Id::new(FLOATING_DRAG), true));
        }
    } else {
        chrome.drag_origin = None;
    }
    ctx.data_mut(|data| data.insert_temp(key, chrome));
}

fn dialog_contents<R>(
    ui: &mut egui::Ui,
    title: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> (R, egui::Rect) {
    dialog_frame(ui, |ui| {
        let title = panel_header(ui, title);
        let inner = add(ui);
        let mut title_rect = title.rect;
        title_rect.max.x = ui.min_rect().right();
        ui.interact(title_rect, title.id.with("drag"), egui::Sense::hover())
            .on_hover_cursor(egui::CursorIcon::Grab);
        (inner, title_rect)
    }).inner
}

pub(crate) fn options_scroll<R>(ui: &mut egui::Ui, id: &'static str, height: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    dtool_design::dialog_options_scroll(ui, id, height, add)
}

pub(crate) fn options_group(ui: &mut egui::Ui, title: &str) {
    ui.add_space(dtool_design::space("xs"));
    options_divider(ui);
    ui.add_space(dtool_design::space("xs"));
    ui.strong(title);
}

pub(crate) fn options_divider(ui: &mut egui::Ui) {
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(line.x_range(), line.center().y, egui::Stroke::new(dtool_design::stroke_width("thin"), dtool_design::color("dialog.separator")));
}

pub(crate) fn confirm_from_enter(
    ctx: &egui::Context,
    event: DialogResult,
    focus_before: Option<egui::Id>,
    submit_fields: &[egui::Id],
) -> bool {
    event == DialogResult::Confirm
        && focus_before.is_none_or(|field| submit_fields.contains(&field))
        && !egui::Popup::is_any_open(ctx)
}

pub(crate) fn confirm_cancel_buttons(
    ui: &mut egui::Ui,
    confirm_label: &str,
    enabled: bool,
) -> (bool, bool) {
    let size = egui::vec2(ui.available_width(), dtool_design::size("button.height"));
    ui.allocate_ui_with_layout(size, egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let cancel = dtool_design::button(ui, dtool_strings::text("button.cancel")).clicked();
        let confirm = dtool_design::button_with(
            ui, confirm_label, &dtool_design::ControlState::enabled(enabled),
        ).clicked();
        (confirm, cancel)
    }).inner
}

pub fn show_modal<R>(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    policy: ClosePolicy,
    can_confirm: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> (DialogResult, R) {
    let key = egui::Id::new(id).with("dtool.dialog_chrome.state");
    let mut chrome: DialogChrome = ctx.data_mut(|data| data.get_temp(key)).unwrap_or_default();
    let shown = chrome.show(ctx, id, title, DialogMode::Modal, policy, can_confirm, add);
    if shown.0 == DialogResult::Open {
        ctx.data_mut(|data| data.insert_temp(key, chrome));
    } else {
        ctx.data_mut(|data| data.remove_temp::<DialogChrome>(key));
    }
    shown
}

#[derive(Clone, Copy)]
struct ModalBounds {
    position: egui::Pos2,
    size: egui::Vec2,
    drag: Option<ModalDrag>,
}

#[derive(Clone, Copy)]
struct ModalDrag {
    pointer: egui::Pos2,
    position: egui::Pos2,
    size: egui::Vec2,
    resize: bool,
}

/// A modal with a title drag target and a dedicated bottom-right resize grip.
pub(crate) fn show_resizable_modal<R>(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    preferred: egui::Vec2,
    minimum: egui::Vec2,
    add: impl FnOnce(&mut egui::Ui, egui::Vec2) -> R,
) -> (DialogResult, R) {
    let id = egui::Id::new(id);
    let state_id = id.with("bounds");
    let viewport = ctx.content_rect().shrink(6.0);
    let frame_inset = 2.0 * (dtool_design::space("lg") + dtool_design::stroke_width("thin"));
    let maximum = (viewport.size() - egui::Vec2::splat(frame_inset)).max(egui::Vec2::splat(1.0));
    let minimum = minimum.min(maximum);
    let initial = preferred.min(maximum);
    let mut bounds = ctx.data(|data| data.get_temp::<ModalBounds>(state_id)).unwrap_or(ModalBounds {
        position: viewport.center() - (initial + egui::Vec2::splat(frame_inset)) * 0.5,
        size: initial,
        drag: None,
    });
    let displayed_size = bounds.size.clamp(minimum, maximum);
    let limit = viewport.max - displayed_size - egui::Vec2::splat(frame_inset);
    let position = bounds.position.clamp(viewport.min, limit.max(viewport.min));
    let area = egui::Area::new(id).kind(egui::UiKind::Modal).order(egui::Order::Foreground)
        .sense(egui::Sense::hover()).fixed_pos(position).constrain_to(viewport);
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(MODAL_FLAG), true));
    let modal = egui::Modal::new(id).area(area).backdrop_color(backdrop_color()).frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            dialog_frame(ui, |ui| {
                ui.set_width(displayed_size.x);
                ui.set_min_height(displayed_size.y);
                let (title_rect, close) = ui.horizontal(|ui| {
                    let gap = ui.spacing().item_spacing.x;
                    let close_width = dtool_design::size("control.hit_size");
                    let (rect, _) = ui.allocate_exact_size(egui::vec2((displayed_size.x - close_width - gap).max(1.0), close_width), egui::Sense::hover());
                    ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, title,
                        egui::TextStyle::Heading.resolve(ui.style()), ui.visuals().text_color());
                    let close = dtool_design::icon_button(ui, "×").on_hover_text(dtool_strings::text("button.cancel")).clicked();
                    (rect, close)
                }).inner;
                let (separator, _) = ui.allocate_exact_size(egui::vec2(displayed_size.x, 1.0), egui::Sense::hover());
                ui.painter().hline(separator.x_range(), separator.center().y, ui.visuals().widgets.noninteractive.bg_stroke);
                let bottom = title_rect.top() + displayed_size.y;
                let content_size = egui::vec2(displayed_size.x, (bottom - ui.cursor().top() - 16.0).max(1.0));
                let inner = add(ui, content_size);
                ui.interact(title_rect, id.with("title_drag"), egui::Sense::hover())
                    .on_hover_cursor(egui::CursorIcon::Grab);
                let anchor = egui::pos2(separator.right(), bottom);
                let grip = egui::Rect::from_min_max(anchor - egui::Vec2::splat(16.0), anchor);
                ui.interact(grip, id.with("resize"), egui::Sense::hover())
                    .on_hover_cursor(egui::CursorIcon::ResizeNwSe);
                let ink = ui.visuals().weak_text_color();
                for span in [6.0, 11.0] {
                    ui.painter().line_segment([anchor - egui::vec2(span, 1.0), anchor - egui::vec2(1.0, span)], egui::Stroke::new(1.0, ink));
                }
                (inner, close, title_rect, grip)
            }).inner
        });
    let (inner, close, title_rect, grip) = modal.inner;
    let original = bounds;
    // Use the ordered pointer events so a release in the same frame as motion still moves the frame.
    ctx.input(|input| {
        for event in &input.raw.events {
            match *event {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. }
                    if title_rect.contains(pos) || grip.contains(pos) => {
                    bounds.drag = Some(ModalDrag { pointer: pos, position, size: displayed_size, resize: grip.contains(pos) });
                }
                egui::Event::PointerMoved(pos) | egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, .. } => {
                    if let Some(drag) = bounds.drag {
                        let delta = pos - drag.pointer;
                        if drag.resize {
                            let room = (viewport.max - drag.position - egui::Vec2::splat(frame_inset)).max(minimum);
                            bounds.position = drag.position;
                            bounds.size = (drag.size + delta).clamp(minimum, room);
                        } else {
                            bounds.position = (drag.position + delta).clamp(viewport.min, limit.max(viewport.min));
                        }
                    }
                    if matches!(event, egui::Event::PointerButton { .. }) { bounds.drag = None; }
                }
                egui::Event::PointerGone => bounds.drag = None,
                _ => {}
            }
        }
    });
    ctx.data_mut(|data| data.insert_temp(state_id, bounds));
    if bounds.position != original.position || bounds.size != original.size { ctx.request_repaint(); }
    let result = if close {
        DialogResult::Cancel
    } else if !modal.any_popup_open && !egui::Popup::is_any_open(ctx) {
        let (escape, enter) = ctx.input(|input| (input.key_pressed(egui::Key::Escape), input.key_pressed(egui::Key::Enter)));
        decide(ChromeInput { escape, enter, click_outside: false }, DialogMode::Modal, ClosePolicy::CancelOnly, true)
    } else {
        DialogResult::Open
    };
    (result, inner)
}

#[derive(Debug, Clone, Default)]
pub struct DialogChrome {
    frames: u32,
    position: Option<egui::Pos2>,
    drag_origin: Option<(egui::Pos2, egui::Pos2)>,
}

impl DialogChrome {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }

    fn area(&self, id: egui::Id) -> egui::Area {
        let area = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .sense(egui::Sense::hover());
        if let Some(position) = self.position {
            area.fixed_pos(position)
        } else {
            area.anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        }
    }

    pub fn show<R>(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        title: &str,
        mode: DialogMode,
        policy: ClosePolicy,
        can_confirm: bool,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> (DialogResult, R) {
        let id = egui::Id::new(id);
        let area = self.area(id);
        let (rect, (inner, title_rect), backdrop_clicked, popup_open, accepts_drag) = if mode == DialogMode::Modal {
            ctx.data_mut(|data| data.insert_temp(egui::Id::new(MODAL_FLAG), true));
            let shown = egui::Modal::new(id)
                .area(area.kind(egui::UiKind::Modal))
                .backdrop_color(backdrop_color())
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| dialog_contents(ui, title, add));
            (shown.response.rect, shown.inner, shown.backdrop_response.clicked(), shown.any_popup_open, shown.is_top_modal)
        } else {
            let shown = area.show(ctx, |ui| dialog_contents(ui, title, add));
            let is_top = ctx.memory(|memory| memory.allows_interaction(shown.response.layer_id));
            (shown.response.rect, shown.inner, false, false, is_top)
        };
        if accepts_drag && !popup_open {
            self.move_by_title(ctx, rect, title_rect);
        } else {
            self.drag_origin = None;
        }
        let first = self.frames == 0;
        self.frames = self.frames.saturating_add(1);
        if popup_open {
            return (DialogResult::Open, inner);
        }
        let (escape, enter, clicked_outside) = ctx.input(|input| {
            (
                input.key_pressed(egui::Key::Escape),
                input.key_pressed(egui::Key::Enter),
                backdrop_clicked || (input.pointer.primary_clicked() && input.pointer.interact_pos().is_some_and(|pos| !rect.contains(pos))),
            )
        });
        let input = ChromeInput { escape, enter, click_outside: clicked_outside && !first };
        (decide(input, mode, policy, can_confirm), inner)
    }

    fn move_by_title(&mut self, ctx: &egui::Context, rect: egui::Rect, title: egui::Rect) -> bool {
        let before = self.position;
        let viewport = ctx.content_rect();
        let limit = (viewport.max - rect.size()).max(viewport.min);
        ctx.input(|input| {
            for event in &input.raw.events {
                match *event {
                    egui::Event::PointerButton {pos, button: egui::PointerButton::Primary, pressed: true, ..}
                        if title.contains(pos) => self.drag_origin = Some((pos, rect.min)),
                    egui::Event::PointerMoved(pos)
                    | egui::Event::PointerButton {pos, button: egui::PointerButton::Primary, pressed: false, ..} => {
                        if let Some((start, origin)) = self.drag_origin {
                            if pos != start {
                                self.position = Some((origin + (pos - start)).clamp(viewport.min, limit));
                            }
                        }
                        if matches!(event, egui::Event::PointerButton { .. }) {
                            self.drag_origin = None;
                        }
                    }
                    egui::Event::PointerGone => self.drag_origin = None,
                    _ => {}
                }
            }
        });
        let moved = self.position != before;
        if moved { ctx.request_repaint(); }
        moved
    }
}
