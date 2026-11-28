#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use dtool_core::prefs::Prefs;
use dtool_core::workspace::HostWake;
use dtool_strings::text;
use dtool_ui::fonts::install_korean_font;
use dtool_ui::settings::UiSettings;
use dtool_ui::shell::AppShell;
use std::sync::Arc;

#[cfg(windows)]
mod windows_ink;
#[cfg(windows)]
mod windows_clipboard;

const HOST_DRAIN_BUDGET: usize = 32;

struct App {
    shell: AppShell,
    mcp: dtool_mcp::http::LiveMcp,
    wake: HostWake,
}

impl App {
    fn drain_host(&mut self, ctx: &eframe::egui::Context) {
        let mcp = &self.mcp;
        let applied = self.shell.drain_host(ctx, || mcp.next_request(), HOST_DRAIN_BUDGET);
        mcp.publish(&self.shell.workspace);
        if applied == HOST_DRAIN_BUDGET {
            ctx.request_repaint();
        }
    }

    fn sync_mcp(&mut self, ctx: &eframe::egui::Context) {
        let enabled = self.shell.workspace.ai_connection().enabled();
        let endpoint = dtool_mcp::handoff::default_endpoint_path();
        if self.mcp.sync(enabled, &mut self.shell.workspace, self.wake.clone(), mcp_config(), endpoint.as_deref()) {
            ctx.request_repaint();
        }
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &eframe::egui::Context, input: &mut eframe::egui::RawInput) {
        self.shell.intercept_pie_input(ctx, input);
        #[cfg(windows)]
        windows_clipboard::forward_vector_paste(input, self.shell.accepts_vector_paste_shortcut(ctx));
    }

    fn logic(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        self.shell.handle_window_close(ctx);
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dpi = display_dpi(frame)
            .map(|dpi| dpi / ctx.pixels_per_point())
            .unwrap_or(96.0);
        self.shell.workspace.set_display_dpi(Some(dpi));
        #[cfg(windows)]
        if ctx.input(|input| !input.raw.dropped_files.is_empty()) {
            self.shell.file_drop_position = native_drop_position(frame, ctx.pixels_per_point());
        }
        self.drain_host(&ctx);
        self.shell.show(ui);
        #[cfg(windows)]
        resize_client_window(&ctx);
        self.sync_mcp(&ctx);
    }

    fn on_exit(&mut self) {
        self.shell.workspace.remember_open_views();
        let path = UiSettings::default_path();
        let _ = self.shell.settings.save(&path);
        self.mcp.stop(&mut self.shell.workspace, dtool_mcp::http::STOP_WAIT);
    }
}

#[cfg(windows)]
fn native_drop_position(frame: &eframe::Frame, pixels_per_point: f32) -> Option<eframe::egui::Pos2> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let RawWindowHandle::Win32(window) = frame.window_handle().ok()?.as_raw() else { return None; };
    let mut point = POINT::default();
    // OLE file drags can suppress egui's pointer motion events.
    if unsafe { GetCursorPos(&mut point) == 0 || ScreenToClient(window.hwnd.get() as _, &mut point) == 0 } {
        return None;
    }
    Some(eframe::egui::pos2(point.x as f32, point.y as f32) / pixels_per_point)
}

#[cfg(windows)]
fn display_dpi(frame: &eframe::Frame) -> Option<f32> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_RAW_DPI};

    let RawWindowHandle::Win32(window) = frame.window_handle().ok()?.as_raw() else {
        return None;
    };
    let mut x = 0;
    let mut y = 0;
    // The borrowed window handle belongs to this frame and the outputs live
    // through the synchronous Windows calls.
    let status = unsafe {
        let monitor = MonitorFromWindow(window.hwnd.get() as _, MONITOR_DEFAULTTONEAREST);
        GetDpiForMonitor(monitor, MDT_RAW_DPI, &mut x, &mut y)
    };
    (status >= 0 && x > 0 && y > 0).then(|| (x as f32 * y as f32).sqrt())
}

#[cfg(not(windows))]
fn display_dpi(_: &eframe::Frame) -> Option<f32> {
    None
}

fn load_prefs(settings: &UiSettings) -> Prefs {
    let path = Prefs::default_path();
    let first_run = !path.exists();
    let mut prefs = Prefs::load(&path);
    let _ = dtool_core::colour::sync_app_prefs(&prefs);
    if first_run && !settings.allow_ai {
        prefs.mcp_enabled = false;
        let _ = prefs.save(&path);
    }
    prefs
}

fn mcp_config() -> dtool_mcp::Config {
    dtool_mcp::Config { allow_dirs: dtool_mcp::permission::default_allow_dirs(), ..Default::default() }
}

#[cfg(windows)]
fn resize_client_window(ctx: &eframe::egui::Context) {
    use eframe::egui::{CursorIcon, ResizeDirection, ViewportCommand};

    let Some((point, pressed)) = ctx.input(|input| {
        if input.viewport().maximized.unwrap_or(false) || input.viewport().fullscreen.unwrap_or(false) {
            return None;
        }
        input.pointer.hover_pos().map(|point| (point, input.pointer.primary_pressed()))
    }) else { return; };
    let rect = ctx.viewport_rect();
    let margin = 4.0;
    let x = if point.x < rect.left() + margin { -1 } else if point.x > rect.right() - margin { 1 } else { 0 };
    let y = if point.y < rect.top() + margin { -1 } else if point.y > rect.bottom() - margin { 1 } else { 0 };
    let (edge, cursor) = match (x, y) {
        (-1, -1) => (ResizeDirection::NorthWest, CursorIcon::ResizeNwSe),
        (1, 1) => (ResizeDirection::SouthEast, CursorIcon::ResizeNwSe),
        (-1, 1) => (ResizeDirection::SouthWest, CursorIcon::ResizeNeSw),
        (1, -1) => (ResizeDirection::NorthEast, CursorIcon::ResizeNeSw),
        (-1, _) => (ResizeDirection::West, CursorIcon::ResizeHorizontal),
        (1, _) => (ResizeDirection::East, CursorIcon::ResizeHorizontal),
        (_, -1) => (ResizeDirection::North, CursorIcon::ResizeVertical),
        (_, 1) => (ResizeDirection::South, CursorIcon::ResizeVertical),
        _ => return,
    };
    ctx.set_cursor_icon(cursor);
    if pressed {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(edge));
    }
}

fn main() -> eframe::Result<()> {
    dtool_render::preview::install();
    dtool_io::install_import_commands();
    dtool_io::brush_file::install();
    let settings = UiSettings::load(&UiSettings::default_path());
    let prefs = load_prefs(&settings);
    let mut shell = AppShell::new();
    shell.client_title_bar = cfg!(windows);
    shell.native_picture_frame_pickers = cfg!(windows);
    shell.settings = settings;
    shell.workspace.ai_connection().set_from_gui(prefs.mcp_enabled);
    #[cfg(windows)]
    let use_windows_ink = prefs.tablet_input == "windows_ink";
    shell.prefs = prefs;
    if shell.prefs.show_home_on_startup { shell.open_new_document_dialog(); }

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(text("app.title"))
        .with_decorations(!shell.client_title_bar)
        .with_inner_size([1600.0, 1000.0])
        .with_min_inner_size([1024.0, 640.0]);
    if let Some(bytes) = dtool_design::app_icon() {
        if let Ok(icon) = eframe::icon_data::from_png_bytes(bytes) {
            viewport = viewport.with_icon(icon);
        }
    }
    let options = eframe::NativeOptions {
        viewport,
        #[cfg(windows)]
        event_loop_builder: Some({
            Box::new(move |builder: &mut winit::event_loop::EventLoopBuilder<_>| {
                use winit::platform::windows::EventLoopBuilderExtWindows;
                builder.with_msg_hook(move |message| {
                    windows_clipboard::observe_message(message);
                    use_windows_ink && windows_ink::observe_message(message)
                });
            }) as eframe::EventLoopBuilderHook
        }),
        ..Default::default()
    };

    eframe::run_native(
        text("app.title"),
        options,
        Box::new(move |cc| {
            install_korean_font(&cc.egui_ctx);
            if let Ok(mut dialog) = dtool_ui::shell::export_dialog().lock() {
                dialog.enable_background_pdf_preview(cc.egui_ctx.clone());
            }
            let wake_ctx = cc.egui_ctx.clone();
            let wake: HostWake = Arc::new(move || wake_ctx.request_repaint());
            let mut app = App { shell, mcp: dtool_mcp::http::LiveMcp::new(), wake };
            app.shell.start_release_check(&cc.egui_ctx);
            app.sync_mcp(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}
