use std::cell::{Cell, RefCell};
use std::ptr::null_mut;
use std::sync::{Arc, OnceLock, Weak};

use dtool_core::clipboard::{AppClipboard, ClipboardPayload};
use dtool_io::{ImportOptions, PageSel};
use windows_sys::Win32::{
    Foundation::{GlobalFree, HGLOBAL, HWND},
    Graphics::Gdi::{BI_BITFIELDS, BITMAPV5HEADER, LCS_GM_IMAGES},
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardOwner, GetClipboardSequenceNumber,
            IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        Ole::{CF_DIBV5, CF_UNICODETEXT},
    },
    UI::Input::KeyboardAndMouse::GetActiveWindow,
};

use super::{NativeImage, accept_svg_text, accept_vector, looks_like_svg};

const MAX_BYTES: usize = 32 * 1024 * 1024;

thread_local! {
    static IMPORTED: RefCell<Option<(u32, Weak<ClipboardPayload>)>> = const { RefCell::new(None) };
    static PUBLISHED: RefCell<Option<(HWND, Weak<ClipboardPayload>)>> = const { RefCell::new(None) };
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

struct ClipboardBuffer(HGLOBAL);

impl Drop for ClipboardBuffer {
    fn drop(&mut self) { unsafe { GlobalFree(self.0); } }
}

impl ClipboardBuffer {
    fn filled(len: usize, fill: impl FnOnce(&mut [std::mem::MaybeUninit<u8>])) -> Option<Self> {
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, len) };
        if handle.is_null() { return None; }
        let buffer = Self(handle);
        let ptr = unsafe { GlobalLock(handle) }.cast::<std::mem::MaybeUninit<u8>>();
        if ptr.is_null() { return None; }
        // The handle has no other owners until SetClipboardData succeeds.
        fill(unsafe { std::slice::from_raw_parts_mut(ptr, len) });
        unsafe { GlobalUnlock(handle); }
        Some(buffer)
    }

    fn bytes(bytes: &[u8]) -> Option<Self> {
        Self::filled(bytes.len(), |target| {
            target.write_copy_of_slice(bytes);
        })
    }

    fn publish(self, format: u32) -> bool {
        let success = format != 0 && !unsafe { SetClipboardData(format, self.0) }.is_null();
        if success { std::mem::forget(self); }
        success
    }
}

fn dibv5(image: &NativeImage) -> Option<ClipboardBuffer> {
    let header_size = std::mem::size_of::<BITMAPV5HEADER>();
    let header = BITMAPV5HEADER {
        bV5Size: header_size as u32,
        bV5Width: i32::try_from(image.width).ok()?,
        bV5Height: i32::try_from(image.height).ok()?,
        bV5Planes: 1,
        bV5BitCount: 32,
        bV5Compression: BI_BITFIELDS,
        bV5SizeImage: u32::try_from(image.rgba.len()).ok()?,
        bV5RedMask: 0x00ff0000,
        bV5GreenMask: 0x0000ff00,
        bV5BlueMask: 0x000000ff,
        bV5AlphaMask: 0xff000000,
        bV5CSType: 0x7352_4742, // LCS_sRGB
        bV5Intent: LCS_GM_IMAGES as u32,
        ..Default::default()
    };
    let stride = image.width as usize * 4;
    ClipboardBuffer::filled(header_size + image.rgba.len(), |data| {
        let (bitmap_header, pixels) = data.split_at_mut(header_size);
        unsafe { bitmap_header.as_mut_ptr().cast::<BITMAPV5HEADER>().write_unaligned(header); }
        // Positive DIB heights use bottom-up rows, stored as BGRA.
        for (src, dst) in image.rgba.chunks_exact(stride).rev().zip(pixels.chunks_exact_mut(stride)) {
            for (rgba, bgra) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                bgra.write_copy_of_slice(&[rgba[2], rgba[1], rgba[0], rgba[3]]);
            }
        }
    })
}

pub(super) fn publish(clipboard: &AppClipboard, pdf: &[u8], image: Option<&NativeImage>) -> bool {
    use dtool_export::raster::{self, ExportOptions, RgbaImage};

    let format = pdf_format();
    let Some(pdf) = ClipboardBuffer::bytes(pdf).filter(|_| format != 0) else { return false; };
    let png = image.and_then(|image| {
        let pixels = RgbaImage::from_raw(image.width, image.height, image.rgba.clone())?;
        let bytes = raster::encode(&pixels, &ExportOptions::default()).ok()?;
        let format = unsafe { RegisterClipboardFormatW(windows_sys::w!("PNG")) };
        ClipboardBuffer::bytes(&bytes).map(|buffer| (format, buffer))
    });
    let dib = image.and_then(dibv5);
    let payload = clipboard.get_payload();
    let text = dtool_core::clipboard::os_text_for_payload(&payload);
    let text = (!text.is_empty()).then(|| {
        let bytes: Vec<u8> = text.replace("\r\n", "\n").replace('\n', "\r\n")
            .encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        ClipboardBuffer::bytes(&bytes)
    }).flatten();

    let owner = unsafe { GetActiveWindow() };
    if owner.is_null() || unsafe { OpenClipboard(owner) } == 0 { return false; }
    let _opened = OpenedClipboard;
    if unsafe { EmptyClipboard() } == 0 { return false; }
    let has_pdf = pdf.publish(format);
    let has_png = png.is_some_and(|(format, buffer)| buffer.publish(format));
    let has_dib = dib.is_some_and(|buffer| buffer.publish(u32::from(CF_DIBV5)));
    let has_text = text.is_some_and(|buffer| buffer.publish(u32::from(CF_UNICODETEXT)));
    if has_pdf {
        // A local paste must retain the original editable bundle, including native shape settings.
        PUBLISHED.with(|cached| *cached.borrow_mut() = Some((owner, Arc::downgrade(&payload))));
    }
    has_pdf || has_png || has_dib || has_text
}

fn read(format: u32) -> Option<(u32, Vec<u8>)> {
    let _opened = open(format)?;
    let bytes = read_open(format)?;
    Some((unsafe { GetClipboardSequenceNumber() }, bytes))
}

fn open(format: u32) -> Option<OpenedClipboard> {
    if !has_format(format) { return None; }
    if unsafe { OpenClipboard(null_mut()) } == 0 { return None; }
    Some(OpenedClipboard)
}

// The caller holds the clipboard open, so companion formats belong to the same copy.
fn read_open(format: u32) -> Option<Vec<u8>> {
    let handle = unsafe { GetClipboardData(format) };
    if handle.is_null() { return None; }
    let size = unsafe { GlobalSize(handle) };
    if size == 0 || size > MAX_BYTES { return None; }
    let data = unsafe { GlobalLock(handle) }.cast::<u8>();
    if data.is_null() { return None; }
    let bytes = unsafe { std::slice::from_raw_parts(data, size) }.to_vec();
    unsafe { GlobalUnlock(handle); }
    Some(bytes)
}

fn decode_text(bytes: &[u8]) -> Option<String> {
    let units: Vec<_> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).take_while(|&unit| unit != 0).collect();
    String::from_utf16(&units).ok()
}

fn read_text() -> Option<(u32, String)> {
    let (sequence, bytes) = read(u32::from(CF_UNICODETEXT))?;
    Some((sequence, decode_text(&bytes)?))
}

fn read_svg(clipboard: &AppClipboard, format: u32) -> Option<u32> {
    let (sequence, svg, pdf) = {
        let _opened = open(format)?;
        let mut bytes = read_open(format)?;
        let svg = if format == u32::from(CF_UNICODETEXT) {
            decode_text(&bytes)?
        } else {
            bytes.truncate(bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len()));
            String::from_utf8(bytes).ok()?
        };
        if !looks_like_svg(&svg) { return None; }
        let pdf = svg.contains(dtool_io::import_svg::AFFINITY_SVG_NAMESPACE)
            .then(|| read_open(pdf_format())).flatten();
        (unsafe { GetClipboardSequenceNumber() }, svg, pdf)
    };
    accept_svg_text(clipboard, &svg, pdf.as_deref()).then_some(sequence)
}

fn read_vector(clipboard: &AppClipboard) -> Option<u32> {
    for format in svg_formats().into_iter().chain([u32::from(CF_UNICODETEXT)]) {
        if let Some(sequence) = read_svg(clipboard, format) { return Some(sequence); }
    }
    let (sequence, bytes) = read(pdf_format())?;
    let opts = ImportOptions { pages: PageSel::Range(1, 1), favour_editable_text: true, ..Default::default() };
    let (document, _) = dtool_io::import_pdf::import_pdf_bytes(&bytes, &opts).ok()?;
    accept_vector(clipboard, &document).then_some(sequence)
}

pub(super) fn refresh(clipboard: &AppClipboard) -> bool {
    let owner = unsafe { GetClipboardOwner() };
    if PUBLISHED.with(|published| published.borrow().as_ref().is_some_and(|(saved, payload)| {
        *saved == owner && has_format(pdf_format())
            && payload.upgrade().is_some_and(|payload| Arc::ptr_eq(&payload, &clipboard.get_payload()))
    })) {
        return true;
    }
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
