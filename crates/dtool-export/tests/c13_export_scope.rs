use dtool_export::raster::{encode, format_available, options_schema, parse_format, Deferred, ExportError, ExportOptions, EXPOSED_RASTER_FORMATS};
use dtool_export::webp::{encode_rgba, is_lossy_webp, lossy_encoder_available, WebpEncodeError};
use dtool_export::ExportFormat;
use image::RgbaImage;

fn source() -> RgbaImage {
    RgbaImage::from_fn(16, 12, |x, y| image::Rgba([(x * 15) as u8, (y * 20) as u8, 90, 255]))
}

#[test]
fn only_png_jpeg_webp_are_offered_as_raster_exports() {
    assert_eq!(EXPOSED_RASTER_FORMATS, [ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Webp]);
    for format in [ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Webp] {
        assert!(format_available(format));
    }
    for format in [ExportFormat::Tiff, ExportFormat::Gif, ExportFormat::Wmf] {
        assert!(!format_available(format));
    }
    let schema = options_schema();
    for name in ["tiff", "gif", "wmf"] {
        let entry = schema.iter().find(|entry| entry.format == name).unwrap();
        assert!(!entry.available);
        assert_eq!(entry.deferred, Some(Deferred::Planned));
    }
    for name in ["tga", "jxl", "exr", "hdr"] {
        assert!(schema.iter().all(|entry| entry.format != name));
        assert!(parse_format(name).is_err());
    }
}

#[test]
fn held_formats_are_rejected_before_encoding() {
    for format in [ExportFormat::Tiff, ExportFormat::Gif] {
        let error = encode(&source(), &ExportOptions::for_format(format)).unwrap_err();
        assert!(matches!(error, ExportError::Unavailable(_)));
    }
}

#[test]
fn webp_lossless_is_vp8l_and_lossy_uses_the_registered_libwebp_encoder() {
    let lossless = encode(&source(), &ExportOptions::for_format(ExportFormat::Webp)).unwrap();
    assert_eq!(&lossless[0..4], b"RIFF");
    assert!(!is_lossy_webp(&lossless));
    let lossy = ExportOptions { lossless: false, quality: 60, ..ExportOptions::for_format(ExportFormat::Webp) };
    let lossy_bytes = encode(&source(), &lossy).unwrap();
    assert!(is_lossy_webp(&lossy_bytes));
    assert!(lossy_encoder_available());
    let raw = source().into_raw();
    assert!(is_lossy_webp(&encode_rgba(16, 12, &raw, false, 60).unwrap()));
    assert_eq!(WebpEncodeError::LossyUnavailable.to_command_error().code, "NOT_IMPLEMENTED");
}