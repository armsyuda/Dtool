use dtool_export::raster::{
    encode, export_available, format_available, options_schema, Deferred, ExportOptions, E06_HELD_FORMATS,
    E06_OFFERED_FORMATS, EXPOSED_RASTER_FORMATS,
};
use dtool_export::ExportFormat;
use image::RgbaImage;

fn source() -> RgbaImage {
    RgbaImage::from_fn(16, 12, |x, y| image::Rgba([(x * 15) as u8, (y * 20) as u8, 90, 255]))
}

#[test]
fn offer_set_equals_e06_and_held_are_not_offered() {
    assert_eq!(
        E06_OFFERED_FORMATS,
        &[
            ExportFormat::Png,
            ExportFormat::Jpeg,
            ExportFormat::Pdf,
            ExportFormat::Svg,
            ExportFormat::Webp,
            ExportFormat::Eps,
        ]
    );
    assert_eq!(E06_OFFERED_FORMATS, dtool_export::batch::E06_OFFERED_FORMATS);
    for format in E06_OFFERED_FORMATS {
        assert!(export_available(*format));
    }
    for format in E06_HELD_FORMATS {
        assert!(!export_available(*format));
        assert!(!format_available(*format));
    }
    assert_eq!(EXPOSED_RASTER_FORMATS, [ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Webp]);
    let schema = options_schema();
    for name in ["png", "jpeg", "pdf", "svg", "webp", "eps"] {
        let entry = schema.iter().find(|entry| entry.format == name).unwrap();
        assert!(entry.available);
    }
    for name in ["tiff", "gif", "wmf"] {
        let entry = schema.iter().find(|entry| entry.format == name).unwrap();
        assert!(!entry.available);
        assert_eq!(entry.deferred, Some(Deferred::Planned));
    }
    assert!(schema.iter().all(|entry| entry.format != "canva"));
}

#[test]
fn png_still_encodes() {
    let bytes = encode(&source(), &ExportOptions::for_format(ExportFormat::Png)).unwrap();
    assert_eq!(&bytes[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
}
