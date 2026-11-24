use std::ffi::c_void;
use std::mem::MaybeUninit;

use dtool_ui::tablet_input::{PenSample, push};
use eframe::egui::TouchPhase;
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::Input::Pointer::{
    GetPointerPenInfo, POINTER_FLAG_INCONTACT, POINTER_PEN_INFO,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MSG, PEN_MASK_PRESSURE, PEN_MASK_TILT_X, PEN_MASK_TILT_Y, WM_POINTERDOWN, WM_POINTERUP,
    WM_POINTERUPDATE,
};

pub fn observe_message(message: *const c_void) -> bool {
    // Winit passes a valid MSG for the duration of its message hook.
    let Some(message) = (unsafe { message.cast::<MSG>().as_ref() }) else {
        return false;
    };
    let phase = match message.message {
        WM_POINTERDOWN => TouchPhase::Start,
        WM_POINTERUPDATE => TouchPhase::Move,
        WM_POINTERUP => TouchPhase::End,
        _ => return false,
    };
    let id = (message.wParam & 0xffff) as u32;
    let mut pen = MaybeUninit::<POINTER_PEN_INFO>::uninit();
    // The pen state belongs to this message and must be read before Windows retrieves the next one.
    if unsafe { GetPointerPenInfo(id, pen.as_mut_ptr()) } == 0 {
        return false;
    }
    let pen = unsafe { pen.assume_init() };
    if phase == TouchPhase::Move && pen.pointerInfo.pointerFlags & POINTER_FLAG_INCONTACT == 0 {
        return false;
    }
    let mut client = pen.pointerInfo.ptPixelLocation;
    if unsafe { ScreenToClient(message.hwnd, &mut client) } == 0 {
        return false;
    }
    let axis = |mask, angle: i32| {
        if pen.penMask & mask != 0 {
            (angle as f32 / 90.0).clamp(-1.0, 1.0)
        } else {
            0.0
        }
    };
    push(PenSample {
        id: u64::from(id),
        phase,
        client_px: [client.x, client.y],
        pressure: (pen.penMask & PEN_MASK_PRESSURE != 0)
            .then(|| (pen.pressure as f32 / 1024.0).clamp(0.0, 1.0)),
        tilt: [
            axis(PEN_MASK_TILT_X, pen.tiltX),
            axis(PEN_MASK_TILT_Y, pen.tiltY),
        ],
        time_ms: pen.pointerInfo.dwTime,
    });
    false
}
