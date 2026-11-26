use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};
use std::time::Duration;

use dtool_core::update_check::{DownloadedInstaller, ReleaseUpdate};
use dtool_strings::text;

use crate::dialog_chrome::{ClosePolicy, DialogResult, options_group, options_scroll, show_modal};

struct Download {
    result: Receiver<Result<DownloadedInstaller, ()>>,
    percent: Arc<AtomicU8>,
}

#[derive(Default)]
pub(crate) struct UpdateIndicator {
    pending: Option<Receiver<Result<Option<ReleaseUpdate>, ()>>>,
    available: Option<Arc<ReleaseUpdate>>,
    check_failed: bool,
    notice_until: Option<f64>,
    open: bool,
    download: Option<Download>,
    installer: Option<DownloadedInstaller>,
    error: Option<&'static str>,
}

impl UpdateIndicator {
    pub(crate) fn start(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() {
            return;
        }
        self.check_failed = false;
        self.error = None;
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        self.pending = Some(receiver);
        if std::thread::Builder::new()
            .name("dtool-update-check".into())
            .spawn(move || {
                let _ = sender.send(dtool_core::update_check::newer_release(env!(
                    "CARGO_PKG_VERSION"
                )));
                ctx.request_repaint();
            })
            .is_err()
        {
            self.pending = None;
            self.check_failed = true;
        }
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        icons: &mut crate::icon_cache::IconCache,
        home_open: bool,
    ) {
        let now = ui.input(|input| input.time);
        if let Some(pending) = &self.pending {
            match pending.try_recv() {
                Ok(Ok(available)) => {
                    self.available = available.map(Arc::new);
                    self.notice_until = self.available.as_ref().map(|_| now + 5.0);
                    self.pending = None;
                }
                Ok(Err(())) | Err(TryRecvError::Disconnected) => {
                    self.check_failed = true;
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let label = self.status_label();
        let colour = if self.available.is_some() {
            egui::Color32::from_rgb(15, 169, 134)
        } else {
            dtool_design::icons::current_icon_colour()
        };
        let Some(texture) = icons.texture(
            ui.ctx(),
            "app-update",
            colour,
            dtool_design::icon_texture_px(ui),
        ) else {
            return;
        };
        let button = dtool_design::icon_image_button(
            ui,
            &texture,
            &dtool_design::ControlState::default().tooltip(&label),
        );
        button
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &label));
        // The startup document picker blocks the header's regular egui responses.
        let clicked_over_home = home_open
            && !self.open
            && ui.input(|input| {
                input.pointer.button_clicked(egui::PointerButton::Primary)
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|point| button.rect.contains(point))
            });
        if button.clicked() || clicked_over_home {
            self.open = true;
            self.notice_until = None;
        }
        if let Some(until) = self.notice_until.filter(|until| *until > now && !self.open) {
            let corner = button.rect.right_bottom() + egui::vec2(0.0, 6.0);
            egui::Area::new(egui::Id::new("dtool.update.notice"))
                .order(egui::Order::Tooltip)
                .interactable(false)
                .pivot(egui::Align2::RIGHT_TOP)
                .fixed_pos(corner)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(egui::Margin::symmetric(10, 7))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(&label).strong());
                        });
                });
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(until - now));
        }
    }

    fn status_label(&self) -> String {
        if self.pending.is_some() {
            text("app.update.checking").to_owned()
        } else if self.check_failed {
            text("app.update.unavailable").to_owned()
        } else if let Some(release) = &self.available {
            text("app.update.available").replace("{version}", &release.version)
        } else {
            text("app.update.current").to_owned()
        }
    }

    fn start_download(&mut self, release: Arc<ReleaseUpdate>, ctx: egui::Context) {
        let (sender, result) = mpsc::channel();
        let percent = Arc::new(AtomicU8::new(0));
        let worker_percent = Arc::clone(&percent);
        self.error = None;
        let job = std::thread::Builder::new()
            .name("dtool-update-download".into())
            .spawn(move || {
                let installer = release.download(|received, total| {
                    let value = (received.saturating_mul(100) / total).min(100) as u8;
                    if worker_percent.swap(value, Ordering::Relaxed) != value {
                        ctx.request_repaint();
                    }
                });
                let _ = sender.send(installer);
                ctx.request_repaint();
            });
        if job.is_ok() {
            self.download = Some(Download { result, percent });
        } else {
            self.error = Some("app.update.download_error");
        }
    }

    /// Returns true when the normal save-and-close flow should begin.
    pub(crate) fn show_dialog(&mut self, ctx: &egui::Context) -> bool {
        if let Some(download) = &self.download {
            match download.result.try_recv() {
                Ok(Ok(installer)) => {
                    self.download = None;
                    self.installer = Some(installer);
                    self.open = false;
                    return true;
                }
                Ok(Err(())) | Err(TryRecvError::Disconnected) => {
                    self.download = None;
                    self.error = Some("app.update.download_error");
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if !self.open {
            return false;
        }
        let release = self.available.clone();
        let checking = self.pending.is_some();
        let progress = self
            .download
            .as_ref()
            .map(|job| job.percent.load(Ordering::Relaxed));
        let policy = if progress.is_some() {
            ClosePolicy::JobRunning
        } else {
            ClosePolicy::CancelOnly
        };
        let has_installer = release.as_ref().is_some_and(|release| release.can_install());
        let idle = !checking && progress.is_none();
        let (result, (action, close)) = show_modal(
            ctx,
            "dtool.update.dialog",
            text("app.update.title"),
            policy,
            false,
            |ui| {
                let viewport = ui.ctx().content_rect();
                let width = viewport.width().min(600.0) - 36.0;
                let inset = 2.0 * (dtool_design::space("sm") + dtool_design::stroke_width("thin"));
                ui.set_width(width);
                dtool_design::dialog_section(ui, |ui| {
                    ui.set_width(width - inset);
                    ui.horizontal_wrapped(|ui| {
                        if checking {
                            ui.spinner();
                        }
                        ui.strong(self.status_label());
                    });
                    ui.label(text("app.update.installed").replace("{version}", env!("CARGO_PKG_VERSION")));
                });
                if let Some(release) = &release {
                    options_group(ui, &text("app.update.version").replace("{version}", &release.version));
                    let height = (viewport.height() - 230.0).clamp(108.0, 320.0);
                    dtool_design::dialog_section(ui, |ui| options_scroll(ui, "update.release_notes", height, |ui| {
                        if release.notes.trim().is_empty() {
                            ui.label(text("app.update.no_notes"));
                        }
                        for line in release.notes.lines() {
                            let line = line.trim_end();
                            if let Some(heading) = line.strip_prefix("## ") {
                                ui.label(egui::RichText::new(release_note_text(heading)).strong());
                            } else if let Some(item) = line.strip_prefix("- ") {
                                ui.label(format!("• {}", release_note_text(item)));
                            } else {
                                ui.label(release_note_text(line));
                            }
                        }
                    }));
                }
                if let Some(progress) = progress {
                    ui.add(egui::ProgressBar::new(f32::from(progress) / 100.0).show_percentage());
                    ui.label(text("app.update.downloading"));
                } else if let Some(error) = self.error {
                    ui.colored_label(ui.visuals().error_fg_color, text(error));
                } else if release.is_some() && !checking && !self.check_failed {
                    ui.label(text(if has_installer {
                        "app.update.before_install"
                    } else {
                        "app.update.no_installer"
                    }));
                }
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = text(if has_installer { "app.update.install" } else { "app.update.check_again" });
                    let action = ui.add_enabled_ui(idle, |ui| {
                        dtool_design::primary_button(ui, label, egui::vec2(96.0, 25.0))
                    }).inner.clicked();
                    let close = ui
                        .add_enabled(
                            progress.is_none(),
                            egui::Button::new(text(if release.is_some() { "app.update.later" } else { "common.close" })),
                        )
                        .clicked();
                    (action, close)
                })
                .inner
            },
        );
        if action {
            if let Some(release) = release.filter(|_| has_installer) {
                self.start_download(release, ctx.clone());
            } else {
                self.start(ctx);
            }
        } else if close || result == DialogResult::Cancel {
            self.open = false;
            self.error = None;
        }
        false
    }

    pub(crate) fn finish_exit(&mut self, approved: bool) -> bool {
        let Some(installer) = self.installer.take() else {
            return true;
        };
        if !approved {
            return false;
        }
        if installer.launch().is_err() {
            self.error = Some("app.update.launch_error");
            self.open = true;
            return false;
        }
        true
    }

    pub(crate) fn dialog_open(&self) -> bool {
        self.open
    }
}

fn release_note_text(mut line: &str) -> String {
    let mut text = String::new();
    while let Some((label, link)) = line.split_once("](https://") {
        let Some((prefix, title)) = label.rsplit_once('[') else {
            break;
        };
        let Some((_, suffix)) = link.split_once(')') else {
            break;
        };
        text.push_str(prefix);
        text.push_str(title);
        line = suffix;
    }
    text.push_str(line);
    text
}
