use std::cell::Cell;
use std::ffi::c_void;

use eframe::egui::{Event, RawInput};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_INSERT, VK_MENU, VK_SHIFT, VK_V};
use windows_sys::Win32::UI::WindowsAndMessaging::{MSG, WM_KEYDOWN};

thread_local! {
    static VECTOR_PASTE: Cell<bool> = const { Cell::new(false) };
}

pub fn observe_message(message: *const c_void) {
    // Winit borrows a valid MSG for the duration of this hook.
    let Some(message) = (unsafe { message.cast::<MSG>().as_ref() }) else { return; };
    if message.message != WM_KEYDOWN { return; }
    let key = message.wParam as u16;
    if key != VK_V && key != VK_INSERT { return; }
    let held = |key| unsafe { GetKeyState(i32::from(key)) < 0 };
    let (ctrl, shift) = (held(VK_CONTROL), held(VK_SHIFT));
    if !held(VK_MENU) && (key == VK_V && ctrl && !shift || key == VK_INSERT && shift && !ctrl)
        && dtool_ui::os_clipboard::native_vector_format_available()
    {
        VECTOR_PASTE.with(|pending| pending.set(true));
    }
}

pub fn forward_vector_paste(input: &mut RawInput, canvas_focused: bool) {
    let pending = VECTOR_PASTE.with(|pending| pending.replace(false));
    // egui handles ordinary text and image shortcuts itself. A registered vector-only format
    // produces no event, so dispatch that shortcut through the canvas paste path.
    if pending && canvas_focused && input.focused
        && !input.events.iter().any(|event| matches!(event, Event::Paste(_) | Event::PasteImage(_)))
    {
        input.events.push(Event::Paste(String::new()));
    }
}
