use std::cell::{Cell, RefCell};
use std::ptr::null_mut;
use std::sync::{Arc, OnceLock, Weak};

use dtool_core::clipboard::{AppClipboard, ClipboardPayload};
use dtool_io::{ImportOptions, PageSel};
use windows_sys::Win32::System::{
    DataExchange::{CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable,
        OpenClipboard, RegisterClipboardFormatW},
    Memory::{GlobalLock, GlobalSize, GlobalUnlock},
    Ole::CF_UNICODETEXT,
};

use super::{accept_svg_text, accept_vector, looks_like_svg};

const MAX_BYTES: usize = 32 * 1024 * 1024;

thread_local! {
    static IMPORTED: RefCell<Option<(u32, Weak<ClipboardPayload>)>> = const { RefCell::new(None) };
    static TEXT_IS_SVG: Cell<Option<(u32, bool)>> = const { Cell::new(None) };
}

fn pdf_format() -> u32 {
    static FORMAT: OnceLock<u32> = OnceLock::new();
    *FORMAT.get_or_init(|| unsafe { RegisterClipboardFormatW(windows_sys::w!("Portable Document Format")) })
}

fn svg_formats() -> [u32; 3] {
    static FORMATS: OnceLock<[u32; 3]> = OnceLock::new();
    *FORMATS.get_or_init(|| unsafe {
        [windows_sys::w!("image/svg+xml"), windows_sys::w!("Scalable Vector Graphics"),
            windows_sys::w!("image/x-inkscape-svg")].map(|name| RegisterClipboardFormatW(name))
    })
}

fn has_format(format: u32) -> bool {
    format != 0 && unsafe { IsClipboardFormatAvailable(format) != 0 }
}

pub(super) fn available() -> bool {
    if has_registered_format() { return true; }
    if !has_format(u32::from(CF_UNICODETEXT)) { return false; }
    let sequence = unsafe { GetClipboardSequenceNumber() };
    if let Some((saved, value)) = TEXT_IS_SVG.with(Cell::get) && sequence != 0 && sequence == saved {
        return value;
    }
    let Some((sequence, text)) = read_text() else { return false; };
    let value = looks_like_svg(&text);
    TEXT_IS_SVG.with(|cache| cache.set(Some((sequence, value))));
    value
}

pub(super) fn has_registered_format() -> bool {
    svg_formats().into_iter().any(has_format) || has_format(pdf_format())
}

struct OpenedClipboard;

impl Drop for OpenedClipboard {
    fn drop(&mut self) { unsafe { CloseClipboard(); } }
}

fn read(format: u32) -> Option<(u32, Vec<u8>)> {
    if !has_format(format) { return None; }
    if unsafe { OpenClipboard(null_mut()) } == 0 { return None; }
    let _opened = OpenedClipboard;
    let handle = unsafe { GetClipboardData(format) };
    if handle.is_null() { return None; }
    let size = unsafe { GlobalSize(handle) };
    if size == 0 || size > MAX_BYTES { return None; }
    let data = unsafe { GlobalLock(handle) }.cast::<u8>();
    if data.is_null() { return None; }
    let bytes = unsafe { std::slice::from_raw_parts(data, size) }.to_vec();
    unsafe { GlobalUnlock(handle); }
    Some((unsafe { GetClipboardSequenceNumber() }, bytes))
}

fn read_text() -> Option<(u32, String)> {
    let (sequence, bytes) = read(u32::from(CF_UNICODETEXT))?;
    let units: Vec<_> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).take_while(|&unit| unit != 0).collect();
    Some((sequence, String::from_utf16(&units).ok()?))
}

fn read_vector(clipboard: &AppClipboard) -> Option<u32> {
    for format in svg_formats() {
        let Some((sequence, bytes)) = read(format) else { continue; };
        let content = bytes.split(|&byte| byte == 0).next().and_then(|bytes| std::str::from_utf8(bytes).ok());
        if content.is_some_and(|text| accept_svg_text(clipboard, text)) { return Some(sequence); }
    }
    if let Some((sequence, text)) = read_text() && accept_svg_text(clipboard, &text) {
        return Some(sequence);
    }
    let (sequence, bytes) = read(pdf_format())?;
    let opts = ImportOptions { pages: PageSel::Range(1, 1), favour_editable_text: true, ..Default::default() };
    let (document, _) = dtool_io::import_pdf::import_pdf_bytes(&bytes, &opts).ok()?;
    accept_vector(clipboard, &document).then_some(sequence)
}

pub(super) fn refresh(clipboard: &AppClipboard) -> bool {
    let sequence = unsafe { GetClipboardSequenceNumber() };
    if sequence != 0 && IMPORTED.with(|imported| imported.borrow().as_ref().is_some_and(|(saved, payload)| {
        *saved == sequence && payload.upgrade().is_some_and(|payload| Arc::ptr_eq(&payload, &clipboard.get_payload()))
    })) {
        return true;
    }
    let Some(sequence) = read_vector(clipboard) else { return false; };
    IMPORTED.with(|imported| *imported.borrow_mut() = Some((sequence, Arc::downgrade(&clipboard.get_payload()))));
    true
}
