use dtool_core::blend::BlendMode;
use dtool_core::document::{Document, Units};
use dtool_core::layer_style::{default_effect, EffectKind, EffectParams, LayerEffect};
use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_export::pdf::{export_pdf, ExportArea, ExportControl, ExportRequest, PdfOptions};
use serde_json::json;

fn shadow() -> LayerEffect {
    let mut effect = default_effect(EffectKind::OuterShadow);
    effect.opacity = 1.0;
    effect.blend = BlendMode::Normal;
    effect.params = EffectParams::Shadow { radius: 3.0, offset: 12.0, angle: 135.0, intensity: 0.0, colour: Colour::new(0, 0, 255, 255), knock_out: true };
    effect
}

fn doc(effects: bool, dpi: Option<f32>) -> Document {
    let mut doc = Document::new([100.0, 100.0], Units::Pt, 72.0, 1);
    doc.objects.push(Object { id: ObjectId(1), name: "o".into(), pos: [20.0, 20.0], size: [40.0, 40.0], fill: Paint::Solid { colour: Colour::new(255, 0, 0, 255) }, effects: if effects { vec![shadow()] } else { vec![] }, ..Object::default() });
    doc.next_object_id = 2;
    doc.effect_raster_dpi = dpi;
    doc
}

fn png(doc: &Document) -> image::RgbaImage {
    image::load_from_memory(&dtool_export::export_png(doc, None).expect("png")).unwrap().to_rgba8()
}

fn blue(img: &image::RgbaImage) -> usize {
    img.pixels().filter(|p| p[2] > 150 && p[0] < 80 && p[3] > 100).count()
}

fn pdf(doc: &Document, name: &str) -> (Vec<u8>, Vec<String>) {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let req = ExportRequest { format: dtool_export::raster::parse_format("pdf").unwrap(), area: ExportArea::WholeDocument, pages: None, path: path.clone(), preset: None, options: json!({}) };
    let receipt = export_pdf(doc, &req, &PdfOptions::default(), &ExportControl::new()).expect("pdf export");
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    (bytes, receipt.warnings)
}

fn images(bytes: &[u8]) -> usize {
    ["/Subtype /Image", "/Subtype/Image"].iter().map(|n| bytes.windows(n.len()).filter(|w| *w == n.as_bytes()).count()).sum()
}

#[test]
fn png_export_draws_effects_and_honours_effect_raster_dpi() {
    assert_eq!(blue(&png(&doc(false, None))), 0);
    let full = png(&doc(true, None));
    assert!(blue(&full) > 100, "shadow exported");
    let coarse = png(&doc(true, Some(36.0)));
    assert!(blue(&coarse) > 100);
    assert_ne!(full.as_raw(), coarse.as_raw(), "effects rendered at 36 dpi differ from full resolution");
    assert_eq!(full.get_pixel(40, 40), coarse.get_pixel(40, 40), "object body unchanged");
}

#[test]
fn pdf_rasterises_effects_at_effect_raster_dpi() {
    let (plain, warnings) = pdf(&doc(false, None), "b50_plain.pdf");
    assert_eq!(images(&plain), 0, "no raster for plain vector object");
    assert!(warnings.is_empty(), "{warnings:?}");
    let (low, warnings) = pdf(&doc(true, Some(72.0)), "b50_low.pdf");
    assert!(images(&low) > 0, "effected object embedded as image");
    assert!(warnings.iter().any(|w| w == "warning.pdf.effects_rasterised"), "{warnings:?}");
    let (high, _) = pdf(&doc(true, Some(600.0)), "b50_high.pdf");
    assert!(images(&high) > 0);
    assert!(high.len() > low.len(), "higher effect dpi embeds more pixels: {} vs {}", high.len(), low.len());
}