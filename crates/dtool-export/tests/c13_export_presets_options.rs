use dtool_core::commands::export_presets::{merged_options, FORMATS};
use dtool_export::raster::ExportOptions;
use dtool_export::ExportFormat;
use serde_json::{json, Value};

#[test]
fn preset_options_merge_into_export_options_and_unknown_keys_are_ignored_not_rejected() {
    let base = ExportOptions::for_format(ExportFormat::Png);
    let saved = json!({ "dpi": 144.0, "include_bleed": true, "embed_metadata": false, "future_key": 1 });
    let merged = merged_options(serde_json::to_value(&base).unwrap(), saved.as_object().unwrap());
    assert_eq!(merged["future_key"], json!(1));
    let options: ExportOptions = serde_json::from_value(merged).expect("export options");
    let back: Value = serde_json::to_value(&options).unwrap();
    assert_eq!(back["dpi"], json!(144.0));
    assert_eq!(options.format, ExportFormat::Png);
}

fn every_format() -> Vec<ExportFormat> {
    let all = vec![ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Webp, ExportFormat::Tiff, ExportFormat::Gif, ExportFormat::Pdf, ExportFormat::Svg, ExportFormat::Eps, ExportFormat::Wmf];
    for format in &all {
        match format {
            ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Webp | ExportFormat::Tiff | ExportFormat::Gif | ExportFormat::Pdf | ExportFormat::Svg | ExportFormat::Eps | ExportFormat::Wmf => {}
        }
    }
    all
}

#[test]
fn merged_options_deserialize_into_export_options_for_every_format_in_any_case() {
    let formats = every_format();
    assert_eq!(formats.len(), FORMATS.len());
    let base = serde_json::to_value(ExportOptions::for_format(ExportFormat::Png)).unwrap();
    for format in formats {
        let canonical = serde_json::to_value(format).unwrap();
        let name = canonical.as_str().expect("unit variant serializes as a string").to_string();
        assert!(FORMATS.contains(&name.as_str()), "{name}");
        for input in [name.clone(), name.to_lowercase(), name.to_uppercase()] {
            let saved = json!({ "format": input, "dpi": 150.0 });
            let merged = merged_options(base.clone(), saved.as_object().unwrap());
            assert_eq!(merged["format"], canonical, "{input}");
            let options: ExportOptions = serde_json::from_value(merged).unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(options.format, format, "{input}");
        }
        let lower_base = json!({ "format": name.to_lowercase() });
        let options: ExportOptions = serde_json::from_value(merged_options(lower_base, &serde_json::Map::new())).unwrap();
        assert_eq!(options.format, format);
    }
}

#[test]
fn unknown_saved_format_falls_back_to_the_base_format() {
    let base = serde_json::to_value(ExportOptions::for_format(ExportFormat::Jpeg)).unwrap();
    let saved = json!({ "format": "bmp", "quality": 55 });
    let options: ExportOptions = serde_json::from_value(merged_options(base, saved.as_object().unwrap())).unwrap();
    assert_eq!((options.format, options.quality), (ExportFormat::Jpeg, 55));
}
