use dtool_export::raster::{options_schema, ExportError, ExportOptions, RenderIntent, Resample};
use dtool_export::{ExportArea, ExportFormat};

fn rejected(options: ExportOptions) -> String {
    match options.validate() {
        Err(ExportError::Unavailable(field)) => field,
        other => panic!("expected EXPORT_OPTION_UNAVAILABLE, got {other:?}"),
    }
}

#[test]
fn defaults_validate() {
    assert!(ExportOptions::default().validate().is_ok());
    assert_eq!(ExportOptions::default().unapplied_field(), None);
}

#[test]
fn options_the_writers_do_not_apply_are_rejected_not_ignored() {
    let base = ExportOptions::default();
    for resample in [Resample::Nearest, Resample::Bilinear, Resample::Lanczos] {
        assert_eq!(ExportOptions { resample, ..base.clone() }.unapplied_field(), None, "{resample:?} applies to raster");
        assert_eq!(ExportOptions { resample, format: ExportFormat::Pdf, ..base.clone() }.unapplied_field(), Some("resample"), "{resample:?} is meaningless for PDF");
    }
    assert_eq!(rejected(ExportOptions { intent: RenderIntent::Saturation, ..base.clone() }), "intent");
    assert_eq!(rejected(ExportOptions { skip_hidden: false, ..base.clone() }), "skip_hidden");
    assert_eq!(rejected(ExportOptions { include_bleed: true, area: ExportArea::Rect([0.0, 0.0, 10.0, 10.0]), ..base.clone() }), "include_bleed");
    let tiff = ExportOptions { format: ExportFormat::Tiff, ..base.clone() };
    assert_eq!(tiff.clone().unapplied_field(), None);
    assert_eq!(ExportOptions { icc: Some("sRGB".into()), ..tiff.clone() }.unapplied_field(), Some("icc"));
    assert_eq!(ExportOptions { embed_icc: true, ..tiff.clone() }.unapplied_field(), Some("embed_icc"));
    assert_eq!(ExportOptions { embed_metadata: true, ..tiff }.unapplied_field(), Some("embed_metadata"));
}

#[test]
fn icc_metadata_and_bleed_are_applied_for_png_jpeg_webp() {
    for format in [ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Webp] {
        let base = ExportOptions::for_format(format);
        for options in [
            ExportOptions { icc: Some("sRGB".into()), ..base.clone() },
            ExportOptions { embed_icc: true, ..base.clone() },
            ExportOptions { icc: Some("sRGB".into()), intent: RenderIntent::Saturation, ..base.clone() },
            ExportOptions { embed_metadata: true, ..base.clone() },
            ExportOptions { include_bleed: true, ..base.clone() },
        ] {
            assert_eq!(options.unapplied_field(), None, "{format:?} {options:?}");
        }
    }
}

#[test]
fn schema_only_advertises_applied_raster_fields() {
    let schema = options_schema();
    let png = schema.iter().find(|entry| entry.format == "png").expect("png schema");
    for field in ["area", "width", "height", "scale", "dpi", "transparent", "matte"] {
        assert!(png.fields.iter().any(|f| f == field), "png schema lists {field}");
    }
    for field in ["skip_hidden"] {
        assert!(schema.iter().all(|entry| entry.fields.iter().all(|f| f != field)), "{field} is not applied and must not be advertised");
    }
    for format in ["png", "jpeg", "webp"] {
        let entry = schema.iter().find(|entry| entry.format == format).unwrap();
        for field in ["icc", "embed_icc", "intent", "embed_metadata", "include_bleed", "resample"] {
            assert!(entry.fields.iter().any(|f| f == field), "{format} advertises {field}");
        }
    }
    for format in ["tiff", "gif"] {
        let entry = schema.iter().find(|entry| entry.format == format).unwrap();
        assert!(entry.fields.iter().all(|f| !["icc", "embed_icc", "embed_metadata", "resample"].contains(&f.as_str())), "{format}");
    }
}
