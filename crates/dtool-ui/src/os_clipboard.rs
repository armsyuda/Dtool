use dtool_core::clipboard::{os_text_for_payload, take_os_text, ClipboardKind};
use dtool_core::Session;

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
    if (events.paste_image || events.key_paste || events.paste.is_some()) && refresh_native_image(&session.clipboard) {
        return (!events.key_paste).then_some("edit.paste");
    }
    if let Some(text) = events.paste.as_deref() {
        take_os_text(session, text);
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

pub const MAX_CLIPBOARD_IMAGE_BYTES: usize = 64 * 1024 * 1024;

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

#[cfg(windows)]
pub(crate) fn native_image_available() -> bool {
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
pub(crate) fn native_image_available() -> bool {
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
    let payload = session.clipboard.get_payload();
    if payload.kind != ClipboardKind::Objects || !os_text_for_payload(&payload).is_empty() { return Ok(None); }
    let Some(bundle) = dtool_core::clipboard::payload_bundle(&payload) else { return Ok(None) };
    let mut doc = dtool_core::Document::new([1.0, 1.0], session.document.units, session.document.dpi, 1);
    let roots = dtool_core::clipboard::paste_bundle(&mut doc, &bundle, None, [0.0, 0.0]);
    let Some(bounds) = dtool_core::command::selection_bounds(&doc, &roots) else { return Ok(None) };
    let options = dtool_export::raster::ExportOptions { area: dtool_export::ExportArea::Rect(bounds), transparent: true, ..Default::default() };
    let pixels = dtool_export::raster::render_preview(&doc, &options).map_err(|e| e.to_command_error())?;
    let image = NativeImage { width: pixels.width(), height: pixels.height(), rgba: pixels.into_raw() };
    image.validate()?;
    COPIED_IMAGE.with(|cache| *cache.borrow_mut() = Some((image.fingerprint(), std::sync::Arc::downgrade(&payload))));
    Ok(Some(image))
}
