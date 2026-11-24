use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(Default)]
pub(crate) struct UpdateIndicator {
    pending: Option<Receiver<Result<Option<String>, ()>>>,
    available: Option<String>,
    failed: bool,
}

impl UpdateIndicator {
    pub(crate) fn start(&mut self, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        self.pending = Some(receiver);
        if std::thread::Builder::new().name("dtool-update-check".into()).spawn(move || {
            let _ = sender.send(dtool_core::update_check::newer_release(env!("CARGO_PKG_VERSION")));
            ctx.request_repaint();
        }).is_err() {
            self.pending = None;
            self.failed = true;
        }
    }

    pub(crate) fn show(&mut self, ui: &mut egui::Ui, icons: &mut crate::icon_cache::IconCache) {
        if let Some(pending) = &self.pending {
            match pending.try_recv() {
                Ok(Ok(available)) => {
                    self.available = available;
                    self.pending = None;
                }
                Ok(Err(())) | Err(TryRecvError::Disconnected) => {
                    self.failed = true;
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let text = dtool_strings::text;
        let label = if let Some(version) = &self.available {
            text("app.update.available").replace("{version}", version)
        } else if self.pending.is_some() {
            text("app.update.checking").to_owned()
        } else if self.failed {
            text("app.update.unavailable").to_owned()
        } else {
            text("app.update.current").to_owned()
        };
        let colour = if self.available.is_some() {
            egui::Color32::from_rgb(15, 169, 134)
        } else {
            dtool_design::icons::current_icon_colour()
        };
        let Some(texture) = icons.texture(ui.ctx(), "app-update", colour, dtool_design::icon_texture_px(ui)) else { return; };
        let button = dtool_design::icon_image_button(ui, &texture, &dtool_design::ControlState::default().tooltip(&label));
        button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &label));
        if button.clicked() && self.available.is_some() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(dtool_core::update_check::RELEASES_PAGE));
        }
    }
}
