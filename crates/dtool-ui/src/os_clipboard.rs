use dtool_core::clipboard::{os_text_for_payload, take_os_text, ClipboardKind};
use dtool_core::Session;

#[cfg(windows)]
mod native_vector;

pub const PASTE_SPECIAL_MENU: &str = "edit.paste.paste_special";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OsClipboardEvents {
    pub paste: Option<String>,
    pub paste_image: bool,
    pub copy: bool,
    pub cut: bool,
    pub key_paste: bool,
    pub key_copy: bool,
    pub key_cut: bool,
}

impl OsClipboardEvents {
    pub fn read(input: &egui::InputState) -> Self {
        if input.modifiers.shift || input.modifiers.alt {
            return Self::default();
        }
        let command = input.modifiers.command || input.modifiers.ctrl;
        Self {
            paste: input.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            }),
            paste_image: input.events.iter().any(|event| matches!(event, egui::Event::PasteImage(_))),
            copy: input.events.iter().any(|event| matches!(event, egui::Event::Copy)),
            cut: input.events.iter().any(|event| matches!(event, egui::Event::Cut)),
            key_paste: command && input.key_pressed(egui::Key::V),
            key_copy: command && input.key_pressed(egui::Key::C),
            key_cut: command && input.key_pressed(egui::Key::X),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.paste.is_none() && !self.paste_image && !self.copy && !self.cut
    }
}

// When the pointer release and OS paste arrive together, let the canvas place
// the text caret before deciding whether the paste belongs to text or objects.
pub(crate) fn take_paste_after_click(input: &mut egui::InputState, canvas: egui::Rect) -> Option<String> {
    use egui::{Event, Key, PointerButton};
    if input.modifiers.shift || input.modifiers.alt {
        return None;
    }
    let release = input.events.iter().rposition(|event| matches!(event,
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, .. } if canvas.contains(*pos)
    ))?;
    let index = input.events.iter().position(|event| matches!(event, Event::Paste(_)))?;
    if index <= release {
        return None;
    }
    let Event::Paste(text) = input.events.remove(index) else { return None };
    input.events.retain(|event| !matches!(event,
        Event::Key { key: Key::V, pressed: true, modifiers, .. }
            if (modifiers.command || modifiers.ctrl) && !modifiers.shift && !modifiers.alt
    ));
    Some(text)
}

pub fn before_shortcuts(session: &mut Session, events: &OsClipboardEvents) -> Option<&'static str> {
    if session.text_edit.is_some() {
        return None;
    }
    if (events.paste_image || events.key_paste || events.paste.is_some()) && refresh_native_graphic(&session.clipboard) {
        return (!events.key_paste).then_some("edit.paste");
    }
    if let Some(text) = events.paste.as_deref() {
        if !accept_svg_text(&session.clipboard, text, None) { take_os_text(session, text); }
        if events.key_paste || session.clipboard.get_payload().kind == ClipboardKind::Empty {
            return None;
        }
        return Some("edit.paste");
    }
    if events.cut && !events.key_cut {
        return Some("edit.cut");
    }
    if events.copy && !events.key_copy {
        return Some("edit.copy");
    }
    None
}

pub fn after_shortcuts(session: &Session, events: &OsClipboardEvents) -> Option<String> {
    if !(events.copy || events.cut) || session.text_edit.is_some() {
        return None;
    }
    Some(os_text_for_payload(&session.clipboard.get_payload()))
}

pub const MAX_CLIPBOARD_IMAGE_BYTES: usize = dtool_core::clipboard::flattened::MAX_IMAGE_BYTES;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl NativeImage {
    pub fn validate(&self) -> Result<(), dtool_core::CommandError> {
        let length = (self.width as usize).checked_mul(self.height as usize).and_then(|n| n.checked_mul(4));
        if self.width == 0 || self.height == 0 || length != Some(self.rgba.len()) || self.rgba.len() > MAX_CLIPBOARD_IMAGE_BYTES {
            return Err(dtool_core::CommandError::new("CLIPBOARD_IMAGE_INVALID"));
        }
        Ok(())
    }

    fn fingerprint(&self) -> String {
        format!("{}:{}:{}", self.width, self.height, dtool_core::resources::resource_content_hash(&self.rgba))
    }
}

thread_local! {
    static COPIED_IMAGE: std::cell::RefCell<Option<(String, std::sync::Weak<dtool_core::clipboard::ClipboardPayload>)>> = const { std::cell::RefCell::new(None) };
}

pub fn accept_native_image(clipboard: &dtool_core::clipboard::AppClipboard, image: NativeImage) -> Result<bool, dtool_core::CommandError> {
    image.validate()?;
    let payload = clipboard.get_payload();
    let same = COPIED_IMAGE.with(|cache| cache.borrow().as_ref().is_some_and(|(hash, previous)| *hash == image.fingerprint() && previous.upgrade().is_some_and(|previous| std::sync::Arc::ptr_eq(&previous, &payload))));
    if same { return Ok(false); }
    let pixels = dtool_export::raster::RgbaImage::from_raw(image.width, image.height, image.rgba.clone()).ok_or_else(|| dtool_core::CommandError::new("CLIPBOARD_IMAGE_INVALID"))?;
    let bytes = dtool_export::raster::encode(&pixels, &dtool_export::raster::ExportOptions::default()).map_err(|e| e.to_command_error())?;
    let mut doc = dtool_core::Document::new([image.width as f32, image.height as f32], dtool_core::document::Units::Px, 72.0, 1);
    let name = dtool_strings::text("clipboard.image").to_string();
    let resource = doc.add_image_resource(dtool_core::resources::ImageResource { name: name.clone(), format: dtool_core::resources::ImageFormat::Png, pixel_size: [image.width, image.height], dpi: 72.0, data: Some(bytes), ..Default::default() });
    let id = doc.create_object(dtool_core::Object { name, kind: dtool_core::ObjectKind::Image, pos: [0.0, 0.0], size: [image.width as f32, image.height as f32], raster_data: Some(image.rgba.clone()), raster_size: Some((image.width, image.height)), ..Default::default() });
    doc.place_image(id, resource)?;
    doc.pixel_layers.insert(id, dtool_core::pixel::PixelLayer::from_rgba(image.width, image.height, &image.rgba));
    let bundle = dtool_core::clipboard::collect_bundle(&doc, &[id]);
    clipboard.share().set_bundle(bundle);
    Ok(true)
}

pub fn refresh_native_image(clipboard: &dtool_core::clipboard::AppClipboard) -> bool {
    if std::env::var_os("DTOOL_DISABLE_OS_CLIPBOARD").is_some() { return false; }
    let Some(image) = arboard::Clipboard::new().ok().and_then(|mut clipboard| clipboard.get_image().ok()) else { return false };
    let (Ok(width), Ok(height)) = (u32::try_from(image.width), u32::try_from(image.height)) else { return false };
    accept_native_image(clipboard, NativeImage { width, height, rgba: image.bytes.into_owned() }).is_ok()
}

pub(crate) fn refresh_native_graphic(clipboard: &dtool_core::clipboard::AppClipboard) -> bool {
    if std::env::var_os("DTOOL_DISABLE_OS_CLIPBOARD").is_some() { return false; }
    #[cfg(windows)]
    if native_vector::refresh(clipboard) { return true; }
    refresh_native_image(clipboard)
}

pub(crate) fn native_graphic_available() -> bool {
    if std::env::var_os("DTOOL_DISABLE_OS_CLIPBOARD").is_some() { return false; }
    #[cfg(windows)]
    if native_vector::available() { return true; }
    native_image_available()
}

#[cfg(windows)]
pub fn native_vector_format_available() -> bool {
    std::env::var_os("DTOOL_DISABLE_OS_CLIPBOARD").is_none() && native_vector::has_registered_format()
}

fn looks_like_svg(text: &str) -> bool {
    let text = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    text.starts_with('<') && text.contains("<svg") && text.contains("http://www.w3.org/2000/svg")
}

fn accept_svg_text(clipboard: &dtool_core::clipboard::AppClipboard, text: &str, companion_pdf: Option<&[u8]>) -> bool {
    if !looks_like_svg(text) { return false; }
    let payload = clipboard.get_payload();
    if payload.kind == ClipboardKind::Objects && os_text_for_payload(&payload) == text.replace("\r\n", "\n") {
        return false;
    }
    let Ok((document, _)) = dtool_io::import_svg::import_clipboard_svg(text, companion_pdf) else { return false; };
    accept_vector(clipboard, &document)
}

fn accept_vector(clipboard: &dtool_core::clipboard::AppClipboard, document: &dtool_core::Document) -> bool {
    let roots: Vec<_> = document.objects.iter().filter(|object| object.parent_id.is_none()).map(|object| object.id).collect();
    if roots.is_empty() { return false; }
    clipboard.share().set_imported_objects(document, &roots);
    true
}

#[cfg(windows)]
fn native_image_available() -> bool {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::{DataExchange::{IsClipboardFormatAvailable, RegisterClipboardFormatW}, Ole::CF_DIBV5};

    if std::env::var_os("DTOOL_DISABLE_OS_CLIPBOARD").is_some() {
        return false;
    }
    static PNG_FORMAT: OnceLock<u32> = OnceLock::new();
    let png = *PNG_FORMAT.get_or_init(|| unsafe { RegisterClipboardFormatW(windows_sys::w!("PNG")) });
    unsafe { (png != 0 && IsClipboardFormatAvailable(png) != 0) || IsClipboardFormatAvailable(u32::from(CF_DIBV5)) != 0 }
}

#[cfg(not(windows))]
fn native_image_available() -> bool {
    false
}

pub(crate) fn can_create_document(clipboard: &dtool_core::clipboard::AppClipboard) -> bool {
    let payload = clipboard.get_payload();
    match payload.kind {
        ClipboardKind::Objects => payload.bundle.is_some() || payload.objects.as_ref().is_some_and(|objects| !objects.is_empty()),
        ClipboardKind::Text => payload.text.as_ref().is_some_and(|text| !text.is_empty()),
        _ => false,
    }
}

#[cfg(windows)]
pub(crate) fn write_native_image(_ctx: &egui::Context, image: NativeImage) -> Result<(), dtool_core::CommandError> {
    if std::env::var_os("DTOOL_DISABLE_OS_CLIPBOARD").is_some() {
        return Ok(());
    }
    // Keep the rendered RGBA bytes intact so a same-app paste can reuse the editable objects.
    let image = arboard::ImageData {
        width: image.width as usize,
        height: image.height as usize,
        bytes: std::borrow::Cow::Owned(image.rgba),
    };
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_image(image))
        .map_err(|_| dtool_core::CommandError::new("CLIPBOARD_WRITE_FAILED"))
}

#[cfg(not(windows))]
pub(crate) fn write_native_image(ctx: &egui::Context, image: NativeImage) -> Result<(), dtool_core::CommandError> {
    ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &image.rgba));
    Ok(())
}

pub fn image_for_copy(session: &Session) -> Result<Option<NativeImage>, dtool_core::CommandError> {
    Ok(prepare_graphic_copy(session, false)?.and_then(|graphic| graphic.image))
}

struct GraphicCopy {
    image: Option<NativeImage>,
    pdf: Option<Vec<u8>>,
}

pub(crate) fn publish_graphic_copy(ctx: &egui::Context, session: &Session) -> Result<bool, dtool_core::CommandError> {
    if std::env::var_os("DTOOL_DISABLE_OS_CLIPBOARD").is_some() { return Ok(true); }
    let Some(graphic) = prepare_graphic_copy(session, cfg!(windows))? else { return Ok(false); };
    if let Some(pdf) = graphic.pdf {
        #[cfg(windows)]
        if native_vector::publish(&session.clipboard, &pdf, graphic.image.as_ref()) {
            return Ok(true);
        }
        #[cfg(not(windows))]
        let _ = pdf;
        if !os_text_for_payload(&session.clipboard.get_payload()).is_empty() { return Ok(false); }
    }
    let image = graphic.image.ok_or_else(|| dtool_core::CommandError::new("CLIPBOARD_WRITE_FAILED"))?;
    write_native_image(ctx, image)?;
    Ok(true)
}

fn prepare_graphic_copy(session: &Session, vector: bool) -> Result<Option<GraphicCopy>, dtool_core::CommandError> {
    use dtool_core::ObjectKind;
    use dtool_export::raster::{self, ExportOptions};
    use dtool_export::{ExportArea, ExportFormat};

    let payload = session.clipboard.get_payload();
    let has_text = !os_text_for_payload(&payload).is_empty();
    if payload.kind != ClipboardKind::Objects || !vector && has_text { return Ok(None); }
    if let Some(png) = &payload.image_png {
        let pixels = raster::decode(png, ExportFormat::Png).map_err(|e| e.to_command_error())?;
        return Ok(Some(GraphicCopy { image: Some(remember_copy(pixels, &payload)?), pdf: None }));
    }
    let Some(bundle) = dtool_core::clipboard::payload_bundle(&payload) else { return Ok(None) };
    let mut doc = dtool_core::Document::new([1.0, 1.0], session.document.units, session.document.dpi, 1);
    let roots = dtool_core::clipboard::paste_bundle(&mut doc, &bundle, None, [0.0, 0.0]);
    let Some(mut bounds) = dtool_core::command::selection_bounds(&doc, &roots) else { return Ok(None) };
    let vector = vector && doc.objects.iter().any(|object| {
        dtool_core::scene::is_drawn(&doc, object.id)
            && !matches!(object.kind, ObjectKind::Image | ObjectKind::PictureFrame | ObjectKind::Group | ObjectKind::Artboard)
    });
    let bounds = if vector {
        dtool_export::selection_output_region(&doc, &roots).map(|(x, y, w, h)| [x, y, w, h])
    } else {
        dtool_core::command::selection_bounds(&doc, &roots)
    };
    let Some(bounds) = bounds else { return Ok(None) };
    let options = ExportOptions { area: ExportArea::Rect(bounds), ..Default::default() };
    let pdf = if vector {
        let options = ExportOptions {
            format: ExportFormat::Pdf, pdf_outline_text: false, ..options.clone()
        };
        raster::encode_output(&doc, &options, std::path::Path::new("")).ok()
            .map(|output| output.into_bytes()).filter(|bytes| bytes.len() <= MAX_CLIPBOARD_IMAGE_BYTES)
    } else {
        None
    };
    if pdf.is_none() && has_text { return Ok(None); }
    let image = match raster::render_preview(&doc, &options).map_err(|e| e.to_command_error())
        .and_then(|pixels| remember_copy(pixels, &payload))
    {
        Ok(image) => Some(image),
        Err(_) if pdf.is_some() => None,
        Err(error) => return Err(error),
    };
    Ok(Some(GraphicCopy { image, pdf }))
}

fn remember_copy(pixels: dtool_export::raster::RgbaImage, payload: &std::sync::Arc<dtool_core::clipboard::ClipboardPayload>) -> Result<NativeImage, dtool_core::CommandError> {
    let image = NativeImage { width: pixels.width(), height: pixels.height(), rgba: pixels.into_raw() };
    image.validate()?;
    COPIED_IMAGE.with(|cache| *cache.borrow_mut() = Some((image.fingerprint(), std::sync::Arc::downgrade(payload))));
    Ok(image)
}
