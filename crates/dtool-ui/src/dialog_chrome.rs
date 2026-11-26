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

pub fn begin_modal_frame(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| data.remove_temp::<bool>(egui::Id::new(MODAL_FLAG))).unwrap_or(false)
}

pub fn modal_open(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(egui::Id::new(MODAL_FLAG))).unwrap_or(false)
}

pub fn backdrop_color() -> egui::Color32 {
    dtool_design::color("dialog.backdrop")
}

pub(crate) fn options_scroll<R>(ui: &mut egui::Ui, id: &'static str, height: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        let mut scroll = egui::style::ScrollStyle::solid();
        scroll.bar_width = 11.0;
        scroll.handle_min_length = 28.0;
        scroll.bar_outer_margin = 2.0;
        scroll.foreground_color = true;
        ui.style_mut().spacing.scroll = scroll;
        egui::ScrollArea::vertical().id_salt(id).max_height(height).auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, add).inner
    }).inner
}

pub(crate) fn options_group(ui: &mut egui::Ui, title: &str) {
    ui.add_space(dtool_design::space("xs"));
    options_divider(ui);
    ui.add_space(dtool_design::space("xs"));
    ui.strong(title);
}

pub(crate) fn options_divider(ui: &mut egui::Ui) {
    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(line.x_range(), line.center().y, egui::Stroke::new(1.0, dtool_design::color("border")));
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

#[derive(Debug, Clone, Default)]
pub struct DialogChrome {
    frames: u32,
}

impl DialogChrome {
    pub fn reset(&mut self) {
        self.frames = 0;
    }

    pub fn frames(&self) -> u32 {
        self.frames
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
        let (rect, inner, backdrop_clicked, popup_open) = if mode == DialogMode::Modal {
            ctx.data_mut(|data| data.insert_temp(egui::Id::new(MODAL_FLAG), true));
            let shown = egui::Modal::new(egui::Id::new(id))
                .backdrop_color(backdrop_color())
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    dialog_frame(ui, |ui| {
                        panel_header(ui, title);
                        add(ui)
                    })
                    .inner
                });
            (shown.response.rect, shown.inner, shown.backdrop_response.clicked(), shown.any_popup_open)
        } else {
            let shown = egui::Area::new(egui::Id::new(id))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    dialog_frame(ui, |ui| {
                        panel_header(ui, title);
                        add(ui)
                    })
                    .inner
                });
            (shown.response.rect, shown.inner, false, false)
        };
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
}
