use std::{ffi::{OsString, c_void}, mem::{offset_of, size_of}, os::windows::ffi::{OsStrExt, OsStringExt}, path::{Path, PathBuf}};

use dtool_core::Document;
use dtool_core::colour::RenderingIntent;
use dtool_export::raster;
use dtool_protocol::CommandError;
use windows_sys::Win32::{
    Foundation::GlobalFree,
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BITMAPV5HEADER, DEVMODEW, DIB_RGB_COLORS, DMCOLLATE_FALSE,
        DMDUP_HORIZONTAL, DMDUP_VERTICAL, DM_COLLATE, DM_COPIES, DM_DUPLEX, DeleteDC, GetDeviceCaps,
        HALFTONE, HDC, HORZRES, LCS_GM_ABS_COLORIMETRIC, LCS_GM_BUSINESS, LCS_GM_GRAPHICS,
        IntersectClipRect, LCS_GM_IMAGES, LOGPIXELSX, LOGPIXELSY, PHYSICALHEIGHT, PHYSICALOFFSETX,
        PHYSICALOFFSETY, PHYSICALWIDTH, PatBlt, ResetDCW, RestoreDC, SRCCOPY, SaveDC, SetBrushOrgEx, SetStretchBltMode,
        StretchDIBits, VERTRES, WHITENESS,
    },
    Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage},
    System::Memory::{GlobalLock, GlobalSize, GlobalUnlock},
    UI::{
        ColorSystem::{GetICMProfileW, ICM_ON, LCS_sRGB, SetICMMode, SetICMProfileW},
        Controls::Dialogs::{
            CommDlgExtendedError, DEVNAMES, PD_NOWARNING, PD_PRINTSETUP, PD_RETURNDC, PD_RETURNDEFAULT,
            PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGW, PrintDlgW,
        },
        Input::KeyboardAndMouse::GetActiveWindow,
    },
};

use super::profiles::PageLayout;
use super::sheet::{Region, Sheet};
use super::vector::VectorFills;

fn print_error() -> CommandError {
    CommandError::new("PRINT_FAILED")
}

pub(super) const ICC_UNAVAILABLE: &str = "PRINT_ICC_UNAVAILABLE";

pub(super) struct Printer(PRINTDLGW);

impl Drop for Printer {
    fn drop(&mut self) {
        // PrintDlg transfers ownership of the device context and memory handles.
        unsafe {
            if !self.0.hDC.is_null() { DeleteDC(self.0.hDC); }
            if !self.0.hDevMode.is_null() { GlobalFree(self.0.hDevMode); }
            if !self.0.hDevNames.is_null() { GlobalFree(self.0.hDevNames); }
        }
    }
}

impl Printer {
    fn open(flags: u32) -> Result<Option<Self>, CommandError> {
        let mut printer = Self(PRINTDLGW {
            lStructSize: size_of::<PRINTDLGW>() as u32,
            hwndOwner: unsafe { GetActiveWindow() },
            Flags: flags | PD_RETURNDC | PD_USEDEVMODECOPIESANDCOLLATE | PD_NOWARNING,
            nCopies: 1,
            ..Default::default()
        });
        if unsafe { PrintDlgW(&mut printer.0) } == 0 {
            return if unsafe { CommDlgExtendedError() } == 0 { Ok(None) } else { Err(print_error()) };
        }
        if printer.0.hDC.is_null() {
            return Err(print_error());
        }
        Ok(Some(printer))
    }

    pub(super) fn default_printer() -> Option<Self> {
        Self::open(PD_RETURNDEFAULT).ok().flatten()
    }

    pub(super) fn choose() -> Result<Option<Self>, CommandError> {
        Self::open(PD_PRINTSETUP)
    }

    pub(super) fn name(&self) -> String {
        let handle = self.0.hDevNames;
        if handle.is_null() {
            return String::new();
        }
        // DEVNAMES uses UTF-16 offsets relative to the locked buffer.
        unsafe {
            let ptr = GlobalLock(handle);
            if ptr.is_null() {
                return String::new();
            }
            let size = GlobalSize(handle);
            let mut name = String::new();
            if size >= size_of::<DEVNAMES>() {
                let start = usize::from((*ptr.cast::<DEVNAMES>()).wDeviceOffset);
                let words = std::slice::from_raw_parts(ptr.cast::<u16>(), size / 2);
                if let Some(device) = words.get(start..) {
                    let end = device.iter().position(|&unit| unit == 0).unwrap_or(device.len());
                    name = String::from_utf16_lossy(&device[..end]);
                }
            }
            GlobalUnlock(handle);
            name
        }
    }

    pub(super) fn paper(&self) -> Option<Paper> {
        let cap = |index: u32| unsafe { GetDeviceCaps(self.0.hDC, index as i32) };
        let dpi = [cap(LOGPIXELSX), cap(LOGPIXELSY)];
        let printable = [cap(HORZRES), cap(VERTRES)];
        let size = [cap(PHYSICALWIDTH), cap(PHYSICALHEIGHT)];
        let offset = [cap(PHYSICALOFFSETX), cap(PHYSICALOFFSETY)];
        dpi.into_iter().chain(printable).chain(size).all(|value| value > 0)
            .then_some(Paper { dpi, printable, size, offset })
    }

    pub(super) fn colour_profile(&self) -> Option<PathBuf> {
        let mut path = vec![0; 260];
        let mut len = path.len() as u32;
        if unsafe { GetICMProfileW(self.0.hDC, &mut len, path.as_mut_ptr()) } == 0 {
            if len as usize <= path.len() { return None; }
            path.resize(len as usize, 0);
            if unsafe { GetICMProfileW(self.0.hDC, &mut len, path.as_mut_ptr()) } == 0 {
                return None;
            }
        }
        let len = path.iter().position(|&ch| ch == 0).unwrap_or(path.len());
        (len > 0).then(|| OsString::from_wide(&path[..len]).into())
    }

    pub(super) fn print(&self, doc: &Document, title: &str, sheets: &[Sheet], output: Output<'_>) -> Result<bool, CommandError> {
        let duplex = self.use_application_copies()?;
        let paper = self.paper().ok_or_else(print_error)?;
        spool(doc, title, self.0.hDC, paper, sheets, output, duplex)
    }

    fn use_application_copies(&self) -> Result<bool, CommandError> {
        let handle = self.0.hDevMode;
        if handle.is_null() {
            return Ok(false);
        }
        // Keep the driver's private settings attached to its DEVMODE while
        // updating only the copy and collation fields it says are supported.
        unsafe {
            let size = GlobalSize(handle);
            let required = offset_of!(DEVMODEW, dmCollate) + size_of::<i16>();
            if size < required { return Err(print_error()); }
            let mode = GlobalLock(handle).cast::<DEVMODEW>();
            if mode.is_null() { return Err(print_error()); }
            let result = (|| {
                let public_size = usize::from((*mode).dmSize);
                let extra_size = usize::from((*mode).dmDriverExtra);
                if public_size < required || public_size + extra_size > size {
                    return Err(print_error());
                }
                let fields = (*mode).dmFields;
                let duplex = fields & DM_DUPLEX != 0 && matches!((*mode).dmDuplex, DMDUP_VERTICAL | DMDUP_HORIZONTAL);
                let old_copies = (*mode).Anonymous1.Anonymous1.dmCopies;
                let old_collation = (*mode).dmCollate;
                let copies_changed = fields & DM_COPIES != 0 && old_copies != 1;
                let collate_changed = fields & DM_COLLATE != 0 && old_collation != DMCOLLATE_FALSE;
                if copies_changed { (*mode).Anonymous1.Anonymous1.dmCopies = 1; }
                if collate_changed { (*mode).dmCollate = DMCOLLATE_FALSE; }
                if (copies_changed || collate_changed) && ResetDCW(self.0.hDC, mode).is_null() {
                    (*mode).Anonymous1.Anonymous1.dmCopies = old_copies;
                    (*mode).dmCollate = old_collation;
                    return Err(print_error());
                }
                Ok(duplex)
            })();
            GlobalUnlock(handle);
            result
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Sizing {
    #[default]
    Actual,
    Fit,
    Scale(u32),
}

#[derive(Clone, Copy)]
pub(super) struct IccOutput<'a> {
    pub profile: Option<&'a Path>,
    pub intent: RenderingIntent,
}

pub(super) struct Output<'a> {
    pub sizing: Sizing,
    pub dpi: u32,
    pub clip_transparency: bool,
    pub prefer_vectors: bool,
    pub copies: usize,
    pub collate: bool,
    pub to_file: bool,
    pub icc: Option<IccOutput<'a>>,
    pub page_layout: PageLayout,
}

fn print_order(pages: usize, output: &Output<'_>, duplex: bool) -> Result<impl Iterator<Item = usize>, CommandError> {
    let sides_per_sheet = if duplex { 2 } else { 1 };
    let sides_per_copy = pages.checked_next_multiple_of(sides_per_sheet).ok_or_else(print_error)?;
    let sides_per_run = output.copies.checked_mul(sides_per_sheet).ok_or_else(print_error)?;
    let sides = sides_per_copy.checked_mul(output.copies).ok_or_else(print_error)?;
    Ok((0..sides).map(move |side| {
        if output.collate {
            side % sides_per_copy
        } else {
            (side / sides_per_run) * sides_per_sheet + side % sides_per_sheet
        }
    }).take(sides - usize::from(sides > 0 && pages != sides_per_copy)))
}

#[derive(Clone, Copy)]
pub(super) struct Paper {
    pub dpi: [i32; 2],
    pub printable: [i32; 2],
    pub size: [i32; 2],
    pub offset: [i32; 2],
}

#[derive(Clone, Copy)]
pub(super) struct PrintArea {
    pub origin: [i32; 2],
    pub size: [i32; 2],
}

impl Paper {
    pub(super) fn printable_area(self) -> PrintArea {
        PrintArea { origin: self.offset, size: self.printable }
    }

    pub(super) fn cell(self, layout: PageLayout, index: usize) -> PrintArea {
        let [columns, rows] = match layout {
            PageLayout::Single => return self.printable_area(),
            PageLayout::Four => [2, 2],
            PageLayout::Two if i64::from(self.size[0]) * i64::from(self.dpi[1])
                > i64::from(self.size[1]) * i64::from(self.dpi[0]) => [2, 1],
            PageLayout::Two => [1, 2],
        };
        let split = |axis: usize, part: i32, count: i32| {
            let total = i64::from(self.printable[axis]);
            // Use a 4 mm gap between cells, bounded for very small printable areas.
            let gap = ((f64::from(self.dpi[axis]) * 4.0 / 25.4).round() as i32)
                .min(self.printable[axis] / 4);
            let start = (total * i64::from(part) / i64::from(count)) as i32
                + if part > 0 { gap / 2 } else { 0 };
            let end = (total * i64::from(part + 1) / i64::from(count)) as i32
                - if part + 1 < count { gap - gap / 2 } else { 0 };
            (self.offset[axis] + start, end - start)
        };
        let (x, width) = split(0, index as i32 % columns, columns);
        let (y, height) = split(1, index as i32 / columns, rows);
        PrintArea { origin: [x, y], size: [width, height] }
    }

    // Device pixels measured from the physical sheet's top-left corner.
    pub(super) fn placement(self, doc: &Document, region: Region, sizing: Sizing, layout: PageLayout, index: usize) -> ([f64; 2], [f64; 2]) {
        let area = if layout == PageLayout::Single && sizing != Sizing::Fit {
            PrintArea { origin: [0, 0], size: self.size }
        } else {
            self.cell(layout, index)
        };
        let per_inch = f64::from(raster::document_unit_points(doc)) / 72.0;
        let mut size = [f64::from(region.2) * per_inch * f64::from(self.dpi[0]),
            f64::from(region.3) * per_inch * f64::from(self.dpi[1])];
        let scale = match sizing {
            Sizing::Actual => 1.0,
            Sizing::Scale(percent) => f64::from(percent) / 100.0,
            Sizing::Fit => (f64::from(area.size[0]) / size[0]).min(f64::from(area.size[1]) / size[1]),
        };
        size.iter_mut().for_each(|length| *length *= scale);
        let origin = [0, 1].map(|axis|
            f64::from(area.origin[axis]) + (f64::from(area.size[axis]) - size[axis]) / 2.0
        );
        (origin, size)
    }
}

fn enable_icc(dc: HDC, profile: Option<&[u16]>) -> Result<(), CommandError> {
    if let Some(path) = profile && unsafe { SetICMProfileW(dc, path.as_ptr()) } == 0 {
        return Err(CommandError::new(ICC_UNAVAILABLE));
    }
    (unsafe { SetICMMode(dc, ICM_ON) } != 0)
        .then_some(())
        .ok_or_else(|| CommandError::new(ICC_UNAVAILABLE))
}

fn spool(doc: &Document, title: &str, dc: HDC, paper: Paper, sheets: &[Sheet], output: Output<'_>, duplex: bool) -> Result<bool, CommandError> {
    let per_side = output.page_layout.per_side();
    let order = print_order(sheets.len().div_ceil(per_side), &output, duplex)?;
    let prefer_vectors = output.prefer_vectors && output.icc.is_none();
    let profile = output.icc.and_then(|icc| icc.profile)
        .map(|path| path.as_os_str().encode_wide().chain([0]).collect::<Vec<_>>());
    if output.icc.is_some() {
        // Validate before StartDoc, which can prompt for a destination file.
        let state = unsafe { SaveDC(dc) };
        if state == 0 { return Err(print_error()); }
        let validation = enable_icc(dc, profile.as_deref());
        if unsafe { RestoreDC(dc, state) } == 0 { return Err(print_error()); }
        validation?;
    }
    let job_name: Vec<u16> = title.encode_utf16().filter(|unit| *unit != 0).chain([0]).collect();
    // FILE: directs the print subsystem to request the output path for this job.
    let file_port = [b'F' as u16, b'I' as u16, b'L' as u16, b'E' as u16, b':' as u16, 0];
    let info = DOCINFOW {
        cbSize: size_of::<DOCINFOW>() as i32,
        lpszDocName: job_name.as_ptr(),
        lpszOutput: if output.to_file { file_port.as_ptr() } else { std::ptr::null() },
        ..Default::default()
    };
    if unsafe { StartDocW(dc, &info) } <= 0 {
        // Virtual printers can prompt for a filename during StartDoc.
        return if std::io::Error::last_os_error().raw_os_error() == Some(1223) { Ok(false) } else { Err(print_error()) };
    }
    let result = (|| {
        for index in order {
            let start = index * per_side;
            let side = sheets.get(start..(start + per_side).min(sheets.len())).unwrap_or_default();
            if unsafe { StartPage(dc) } <= 0 { return Err(print_error()); }
            let use_icc = output.icc.is_some() && !side.is_empty();
            let state = if use_icc {
                let state = unsafe { SaveDC(dc) };
                if state == 0 { return Err(print_error()); }
                Some(state)
            } else {
                None
            };
            let result: Result<bool, CommandError> = (|| {
                if use_icc {
                    // StartPage resets the ICM state.
                    enable_icc(dc, profile.as_deref())?;
                }
                let mut drawn = false;
                for (slot, &sheet) in side.iter().enumerate() {
                    let vectors = if prefer_vectors && sheet.can_print_fills_as_vectors() {
                        sheet.content.pages().and_then(|pages| VectorFills::prepare(doc, &pages))
                    } else {
                        None
                    };
                    drawn |= draw_page(dc, doc, title, paper, sheet, slot, &output, vectors.as_ref())?;
                }
                Ok(drawn)
            })();
            if state.is_some_and(|state| unsafe { RestoreDC(dc, state) } == 0) {
                return Err(print_error());
            }
            if !result? {
                // Preserve blank pages and the padding side of an odd duplex set.
                if unsafe { PatBlt(dc, 0, 0, 1, 1, WHITENESS) } == 0 { return Err(print_error()); }
            }
            if unsafe { EndPage(dc) } <= 0 { return Err(print_error()); }
        }
        (unsafe { EndDoc(dc) } > 0).then_some(()).ok_or_else(print_error)
    })();
    if result.is_err() {
        unsafe { AbortDoc(dc); }
    }
    result.map(|()| true)
}

fn draw_page(dc: HDC, doc: &Document, title: &str, paper: Paper, sheet: Sheet, slot: usize, output: &Output<'_>, vectors: Option<&VectorFills>) -> Result<bool, CommandError> {
    let (placement, destination) = paper.placement(doc, sheet.media, output.sizing, output.page_layout, slot);
    let cell = paper.cell(output.page_layout, slot);
    if let Some(vectors) = vectors.filter(|vectors| vectors.reference.is_none())
        && let Some(drawn) = vectors.draw(dc, sheet.media, paper, cell, placement, destination)?
    {
        return Ok(drawn);
    }
    if destination.iter().any(|value| !value.is_finite() || *value < 0.5 || *value >= f64::from(i32::MAX)) {
        return Err(print_error());
    }
    let mut size = destination.map(|length| length.round() as i32);
    let resolution = f64::from(output.dpi);
    let lengths = [0, 1].map(|axis| destination[axis] * resolution / f64::from(paper.dpi[axis]));
    let reduction = (31_000_000.0 / (lengths[0].max(1.0) * lengths[1].max(1.0))).sqrt().min(1.0);
    let mut raster = lengths.map(|length| (length * reduction).floor().max(1.0) as i32);
    if i64::from(raster[0]) * i64::from(raster[1]) > 32_000_000 {
        return Err(print_error());
    }
    let mut origin = [0, 1].map(|axis| (placement[axis] - f64::from(paper.offset[axis])).round() as i32);
    let mut pixels = sheet.render(doc, title, [raster[0] as usize, raster[1] as usize]).ok_or_else(print_error)?;
    let mut patch = None;
    let mut vector_drawn = false;
    if let Some(vectors) = vectors && let Some(reference) = &vectors.reference {
        let flags = dtool_core::CanvasFlags::default();
        let bounds = [sheet.media.0, sheet.media.1, sheet.media.2, sheet.media.3];
        let effect_scale = dtool_core::effect_raster::effect_scale(doc, raster[0] as f32 / bounds[2]);
        let reference = dtool_core::layer_style::raster::render_scene_region(
            doc, &flags, &[], reference, bounds, raster.map(|axis| axis as u32), effect_scale,
        );
        if reference.len() != pixels.len() { return Err(print_error()); }
        let raster_size = raster.map(|axis| axis as usize);
        let difference = difference_bounds(&pixels, &reference, raster_size);
        if difference != Some(([0, 0], raster_size))
            && let Some(drawn) = vectors.draw(dc, sheet.media, paper, cell, placement, destination)?
        {
            vector_drawn = drawn;
            if difference.is_none() { return Ok(drawn); }
            patch = difference;
        }
    }
    if patch.is_none() && output.clip_transparency {
        patch = transparent_bounds(&pixels, raster[0] as usize);
        if patch.is_none() {
            return Ok(false);
        }
    }
    if let Some((start, end)) = patch {
        crop_pixels(&mut pixels, raster.map(|axis| axis as usize), start, end);
        for axis in 0..2 {
            let scale = f64::from(size[axis]) / f64::from(raster[axis]);
            let start_offset = (start[axis] as f64 * scale).round() as i32;
            let end_offset = (end[axis] as f64 * scale).round() as i32;
            origin[axis] = origin[axis].saturating_add(start_offset);
            size[axis] = (end_offset - start_offset).max(1);
            raster[axis] = (end[axis] - start[axis]) as i32;
        }
        if (0..2).any(|axis| {
            let start = i64::from(cell.origin[axis]) - i64::from(paper.offset[axis]);
            let bitmap_start = i64::from(origin[axis]);
            bitmap_start >= start + i64::from(cell.size[axis]) || bitmap_start + i64::from(size[axis]) <= start
        }) {
            return Ok(vector_drawn);
        }
    }
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        for component in &mut pixel[..3] {
            *component = paper_component(*component, alpha);
        }
        pixel.swap(0, 2);
        pixel[3] = 0;
    }
    let bitmap = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: raster[0], biHeight: -raster[1], biPlanes: 1, biBitCount: 32,
            biCompression: BI_RGB, ..Default::default()
        },
        ..Default::default()
    };
    let managed_bitmap = output.icc.map(|icc| BITMAPV5HEADER {
        bV5Size: size_of::<BITMAPV5HEADER>() as u32,
        bV5Width: raster[0],
        bV5Height: -raster[1],
        bV5Planes: 1,
        bV5BitCount: 32,
        bV5Compression: BI_RGB,
        bV5CSType: LCS_sRGB as u32,
        bV5Intent: match icc.intent {
            RenderingIntent::Perceptual => LCS_GM_IMAGES,
            RenderingIntent::RelativeColorimetric => LCS_GM_GRAPHICS,
            RenderingIntent::Saturation => LCS_GM_BUSINESS,
            RenderingIntent::AbsoluteColorimetric => LCS_GM_ABS_COLORIMETRIC,
        } as u32,
        ..Default::default()
    });
    // GDI consumes the live pixel buffer synchronously. A negative height makes the DIB top-down.
    unsafe {
        let state = if output.page_layout != PageLayout::Single {
            let state = SaveDC(dc);
            if state == 0 { return Err(print_error()); }
            let [x, y] = [0, 1].map(|axis| cell.origin[axis] - paper.offset[axis]);
            if IntersectClipRect(dc, x, y, x + cell.size[0], y + cell.size[1]) == 0 {
                RestoreDC(dc, state);
                return Err(print_error());
            }
            Some(state)
        } else {
            None
        };
        SetStretchBltMode(dc, HALFTONE);
        SetBrushOrgEx(dc, 0, 0, std::ptr::null_mut());
        let header = managed_bitmap.as_ref().map_or(&raw const bitmap, |header| {
            (header as *const BITMAPV5HEADER).cast::<BITMAPINFO>()
        });
        let rows = StretchDIBits(dc, origin[0], origin[1], size[0], size[1], 0, 0, raster[0], raster[1],
            pixels.as_ptr().cast::<c_void>(), header, DIB_RGB_COLORS, SRCCOPY);
        if state.is_some_and(|state| RestoreDC(dc, state) == 0) || rows <= 0 {
            return Err(print_error());
        }
    }
    Ok(true)
}

fn paper_component(component: u8, alpha: u32) -> u8 {
    ((u32::from(component) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
}

fn difference_bounds(pixels: &[u8], reference: &[u8], [width, height]: [usize; 2]) -> Option<([usize; 2], [usize; 2])> {
    let mut start = [width, height];
    let mut end = [0, 0];
    for (index, (full, vector)) in pixels.chunks_exact(4).zip(reference.chunks_exact(4)).enumerate() {
        if full != vector && (0..3).any(|i| paper_component(full[i], u32::from(full[3]))
            != paper_component(vector[i], u32::from(vector[3])))
        {
            let x = index % width;
            let y = index / width;
            start[0] = start[0].min(x);
            start[1] = start[1].min(y);
            end[0] = end[0].max(x + 1);
            end[1] = y + 1;
        }
    }
    // Leave a two-pixel overlap so sampling at the patch edge uses the same background.
    (end[1] > 0).then(|| (start.map(|axis| axis.saturating_sub(2)), [(end[0] + 2).min(width), (end[1] + 2).min(height)]))
}

fn transparent_bounds(pixels: &[u8], width: usize) -> Option<([usize; 2], [usize; 2])> {
    let stride = width * 4;
    let height = pixels.len() / stride;
    let mut start = [width, height];
    let mut end = [0, 0];
    for (y, row) in pixels.chunks_exact(stride).enumerate() {
        let mut pixels = row.chunks_exact(4);
        if let Some(x) = pixels.clone().position(|pixel| pixel[3] != 0) {
            let right = pixels.rposition(|pixel| pixel[3] != 0).unwrap_or(x);
            start[0] = start[0].min(x);
            start[1] = start[1].min(y);
            end[0] = end[0].max(right + 1);
            end[1] = y + 1;
        }
    }
    (end[1] > 0).then_some((start, end))
}

fn crop_pixels(pixels: &mut Vec<u8>, size: [usize; 2], start: [usize; 2], end: [usize; 2]) {
    if start == [0, 0] && end == size { return; }
    let stride = size[0] * 4;
    let row_size = (end[0] - start[0]) * 4;
    for (row, y) in (start[1]..end[1]).enumerate() {
        let offset = y * stride + start[0] * 4;
        pixels.copy_within(offset..offset + row_size, row * row_size);
    }
    pixels.truncate(row_size * (end[1] - start[1]));
}
