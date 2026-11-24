use dtool_core::document::Units;
use dtool_core::workspace::{Scope, Workspace};
use dtool_protocol::{Command, CommandResult};
use dtool_ui::shell::install_export_host;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

fn call(workspace: &mut Workspace, name: &str, params: Value) -> CommandResult {
    workspace.run(Scope::active(), Command::with_params(name, params)).1
}

fn new_doc(workspace: &mut Workspace, units: Units, width: f64, height: f64, dpi: f64) {
    call(workspace, "doc.new", json!({ "width": width, "height": height, "units": units, "dpi": dpi, "pages": 1 })).expect("doc.new");
    let session = workspace.session_mut().unwrap();
    session.document.create_object(dtool_core::object::Object { name: "box".into(), pos: [-2.0, -2.0], size: [30.0, 20.0], ..Default::default() });
}

fn out(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c13_b10_real_{}_{name}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

fn export(workspace: &mut Workspace, path: &PathBuf, options: Value) -> Value {
    let mut item = options;
    item["path"] = json!(path);
    call(workspace, "export.job.run", json!({ "items": [item], "overwrite": "replace" })).expect("export.job.run")
}

fn written(workspace: &mut Workspace, name: &str, options: Value) -> Vec<u8> {
    let path = out(name);
    let ran = export(workspace, &path, options);
    assert!(ran["written"].as_array().is_some_and(|items| items.len() == 1), "{name}: {ran}");
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    bytes
}

fn refused(workspace: &mut Workspace, name: &str, options: Value) {
    let path = out(name);
    let mut item = options;
    item["path"] = json!(path);
    let result = call(workspace, "export.job.run", json!({ "items": [item], "overwrite": "replace" }));
    let refused = match &result {
        Err(error) => error.code == "EXPORT_OPTION_UNAVAILABLE",
        Ok(value) => value["written"].as_array().is_some_and(|items| items.is_empty()) && value["failed"].as_array().is_some_and(|items| items.iter().any(|item| item["code"] == "EXPORT_OPTION_UNAVAILABLE")),
    };
    assert!(refused, "{name}: {result:?}");
    assert!(!path.exists(), "{name} wrote a file");
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn png_chunk<'a>(bytes: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    let mut at = 8;
    while at + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        if &bytes[at + 4..at + 8] == kind {
            return bytes.get(at + 8..at + 8 + len);
        }
        at += 12 + len;
    }
    None
}

fn format_of(bytes: &[u8]) -> dtool_export::ExportFormat {
    if bytes.starts_with(b"\x89PNG") {
        dtool_export::ExportFormat::Png
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        dtool_export::ExportFormat::Jpeg
    } else {
        dtool_export::ExportFormat::Webp
    }
}

fn dims(bytes: &[u8]) -> (u32, u32) {
    let image = dtool_export::raster::decode(bytes, format_of(bytes)).expect("decodes");
    (image.width(), image.height())
}

fn jpeg_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut at = 2;
    while at + 4 <= bytes.len() && bytes[at] == 0xFF {
        let len = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        let data = &bytes[at + 4..at + 2 + len];
        if bytes[at + 1] == 0xE2 && data.starts_with(b"ICC_PROFILE\0") {
            return Some(data[14..].to_vec());
        }
        at += 2 + len;
    }
    None
}

fn riff_chunk(bytes: &[u8], kind: &[u8; 4]) -> Option<Vec<u8>> {
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == kind {
            return bytes.get(at + 8..at + 8 + len).map(<[u8]>::to_vec);
        }
        at += 8 + len + (len % 2);
    }
    None
}

#[test]
fn c13_b10_new_documents_get_3mm_bleed_and_raster_export_honours_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let _ = install_export_host();
    let mut workspace = Workspace::default();
    new_doc(&mut workspace, Units::Mm, 210.0, 297.0, 300.0);
    let bleed = workspace.session().unwrap().document.setup.bleed;
    assert!(bleed.enabled && [bleed.left, bleed.right, bleed.top, bleed.bottom].iter().all(|v| (*v - 3.0).abs() < 1e-4), "{bleed:?}");
    new_doc(&mut workspace, Units::Px, 200.0, 100.0, 72.0);
    let bleed = workspace.session().unwrap().document.setup.bleed;
    let px = 3.0 / 25.4 * 72.0;
    assert!([bleed.left, bleed.right, bleed.top, bleed.bottom].iter().all(|v| (*v - px).abs() < 1e-3), "{bleed:?}");
    let mut old = Workspace::default();
    old.new_document("existing");
    assert_eq!(old.session().unwrap().document.setup.bleed.left, 0.0, "existing documents keep their bleed");
    assert_eq!(dims(&written(&mut workspace, "trim.png", json!({ "format": "png" }))), (200, 100));
    let with_bleed = (((200.0 + 2.0 * px) as f32).round() as u32, ((100.0 + 2.0 * px) as f32).round() as u32);
    assert_eq!(dims(&written(&mut workspace, "bleed.png", json!({ "format": "png", "include_bleed": true }))), with_bleed);
    let double = (((200.0 + 2.0 * px) * 2.0) as f32).round() as u32;
    assert_eq!(dims(&written(&mut workspace, "bleed144.png", json!({ "format": "png", "include_bleed": true, "dpi": 144.0 }))).0, double);
    assert_eq!(dims(&written(&mut workspace, "bleed.jpg", json!({ "format": "jpeg", "include_bleed": true }))), with_bleed);
    refused(&mut workspace, "bleed_area.png", json!({ "format": "png", "include_bleed": true, "area": { "Rect": [0.0, 0.0, 10.0, 10.0] } }));
}

#[test]
fn c13_b10_raster_export_embeds_icc_profile() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let _ = install_export_host();
    let mut workspace = Workspace::default();
    new_doc(&mut workspace, Units::Px, 64.0, 32.0, 72.0);
    let srgb = dtool_core::colour::services().read().unwrap().profiles.find(dtool_core::colour::ProfileStore::SRGB).unwrap().bytes.to_vec();
    let plain = written(&mut workspace, "plain.png", json!({ "format": "png" }));
    assert!(png_chunk(&plain, b"iCCP").is_none());
    let png = written(&mut workspace, "icc.png", json!({ "format": "png", "embed_icc": true }));
    let iccp = png_chunk(&png, b"iCCP").expect("PNG iCCP chunk");
    let name_end = iccp.iter().position(|b| *b == 0).expect("profile name");
    assert!(name_end > 0 && iccp[name_end + 1] == 0 && iccp[name_end + 2] == 0x78, "iCCP name + zlib profile");
    assert_eq!(dims(&png), (64, 32));
    let jpeg = written(&mut workspace, "icc.jpg", json!({ "format": "jpeg", "embed_icc": true }));
    assert!(contains(&jpeg, b"ICC_PROFILE\0"), "JPEG APP2 ICC_PROFILE");
    assert_eq!(jpeg_icc(&jpeg), Some(srgb.clone()), "JPEG profile round-trips");
    assert_eq!(dims(&jpeg), (64, 32));
    let webp = written(&mut workspace, "icc.webp", json!({ "format": "webp", "embed_icc": true }));
    assert!(contains(&webp, b"VP8X") && contains(&webp, b"ICCP"), "WebP VP8X + ICCP");
    assert_eq!(riff_chunk(&webp, b"ICCP"), Some(srgb), "WebP profile round-trips");
    assert_eq!(dims(&webp), (64, 32));
    refused(&mut workspace, "cmyk_icc.png", json!({ "format": "png", "embed_icc": true, "icc": dtool_core::colour::ProfileStore::FOGRA39L }));
    refused(&mut workspace, "intent_only.png", json!({ "format": "png", "intent": "Saturation" }));
}

#[test]
fn c13_b10_raster_export_embeds_document_metadata() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let _ = install_export_host();
    let mut workspace = Workspace::default();
    new_doc(&mut workspace, Units::Px, 40.0, 20.0, 72.0);
    call(&mut workspace, "doc.metadata.set", json!({ "title": "포스터 Title", "author": "Hyoung Wook Park", "copyright": "(c) 2026", "keywords": "print, bleed" })).expect("doc.metadata.set");
    let plain = written(&mut workspace, "plain_meta.png", json!({ "format": "png" }));
    assert!(!contains(&plain, "포스터".as_bytes()));
    let png = written(&mut workspace, "meta.png", json!({ "format": "png", "embed_metadata": true }));
    let title = png_chunk(&png, b"iTXt").expect("PNG iTXt");
    assert!(title.starts_with(b"Title\0") && contains(title, "포스터 Title".as_bytes()), "PNG Title iTXt");
    assert!(contains(&png, b"XML:com.adobe.xmp") && contains(&png, b"Hyoung Wook Park"), "PNG XMP");
    assert_eq!(dims(&png), (40, 20));
    let jpeg = written(&mut workspace, "meta.jpg", json!({ "format": "jpeg", "embed_metadata": true, "embed_icc": true }));
    assert!(contains(&jpeg, b"http://ns.adobe.com/xap/1.0/\0") && contains(&jpeg, "포스터 Title".as_bytes()) && contains(&jpeg, b"ICC_PROFILE\0"), "JPEG APP1 XMP + ICC");
    assert_eq!(dims(&jpeg), (40, 20));
    let webp = written(&mut workspace, "meta.webp", json!({ "format": "webp", "embed_metadata": true }));
    assert!(contains(&webp, b"XMP ") && contains(&webp, b"Hyoung Wook Park"), "WebP XMP chunk");
    assert_eq!(dims(&webp), (40, 20));
}

fn pixels(bytes: &[u8]) -> dtool_export::raster::RgbaImage {
    dtool_export::raster::decode(bytes, format_of(bytes)).expect("decodes")
}

fn mixed_columns(image: &dtool_export::raster::RgbaImage) -> usize {
    (0..image.width()).filter(|x| {
        let [r, g, b, _] = image.get_pixel(*x, image.height() / 2).0;
        let v = [r, g, b];
        v != [0, 0, 0] && v != [255, 255, 255]
    }).count()
}

#[test]
fn c13_b10_raster_resample_filters_change_the_upscaled_pixels() {
    let _guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let _ = install_export_host();
    let mut workspace = Workspace::default();
    call(&mut workspace, "doc.new", json!({ "width": 40.0, "height": 20.0, "units": Units::Px, "dpi": 72.0, "pages": 1 })).expect("doc.new");
    let session = workspace.session_mut().unwrap();
    let black = dtool_core::paint::Paint::Solid { colour: dtool_core::paint::Colour::new(0, 0, 0, 255) };
    let white = dtool_core::paint::Paint::Solid { colour: dtool_core::paint::Colour::new(255, 255, 255, 255) };
    session.document.create_object(dtool_core::object::Object { name: "white".into(), pos: [0.0, 0.0], size: [40.0, 20.0], fill: white, stroke: dtool_core::paint::StrokeStyle { paint: dtool_core::paint::Paint::None, ..Default::default() }, ..Default::default() });
    session.document.create_object(dtool_core::object::Object { name: "black".into(), pos: [0.0, 0.0], size: [20.0, 20.0], fill: black, stroke: dtool_core::paint::StrokeStyle { paint: dtool_core::paint::Paint::None, ..Default::default() }, ..Default::default() });
    let nearest = written(&mut workspace, "nearest.png", json!({ "format": "png", "transparent": false, "scale": 4.0, "resample": "Nearest" }));
    let bilinear = written(&mut workspace, "bilinear.png", json!({ "format": "png", "transparent": false, "scale": 4.0, "resample": "Bilinear" }));
    let lanczos = written(&mut workspace, "lanczos.png", json!({ "format": "png", "transparent": false, "dpi": 288.0, "resample": "Lanczos" }));
    let jpeg = written(&mut workspace, "nearest.jpg", json!({ "format": "jpeg", "width": 100, "resample": "Nearest" }));
    let (nearest, bilinear, lanczos) = (pixels(&nearest), pixels(&bilinear), pixels(&lanczos));
    assert_eq!((nearest.dimensions(), bilinear.dimensions(), lanczos.dimensions()), ((160, 80), (160, 80), (160, 80)), "dpi/scale set the output size");
    assert_eq!(dims(&jpeg), (100, 50), "width keeps the aspect");
    assert_eq!(mixed_columns(&nearest), 0, "nearest keeps hard native pixels");
    assert!(mixed_columns(&bilinear) >= 2, "bilinear blends across the edge: {}", mixed_columns(&bilinear));
    assert_ne!(bilinear.as_raw(), lanczos.as_raw(), "lanczos differs from bilinear");
    assert_eq!(nearest.get_pixel(79, 40).0[..3], [0, 0, 0]);
    assert_eq!(nearest.get_pixel(80, 40).0[..3], [255, 255, 255]);
    refused(&mut workspace, "resample.pdf", json!({ "format": "pdf", "resample": "Nearest" }));
}
