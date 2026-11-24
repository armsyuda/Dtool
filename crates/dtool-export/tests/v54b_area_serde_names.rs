use dtool_core::ObjectId;
use dtool_export::raster::ExportOptions;
use dtool_export::{ExportArea, ExportFormat, ExportRequest};
use serde_json::json;

fn old_names() -> Vec<(serde_json::Value, ExportArea)> {
    vec![
        (json!("WholeDocument"), ExportArea::WholeDocument),
        (json!("SelectionWithBackground"), ExportArea::SelectionWithBackground),
        (json!("SelectionWithoutBackground"), ExportArea::SelectionWithoutBackground),
        (json!({"Artboard": 2}), ExportArea::Artboard(ObjectId(2))),
        (json!({"Rect": [1.0, 2.0, 3.0, 4.0]}), ExportArea::Rect([1.0, 2.0, 3.0, 4.0])),
    ]
}

#[test]
fn v54b_old_area_names_deserialize() {
    for (value, area) in old_names() {
        assert_eq!(serde_json::from_value::<ExportArea>(value.clone()).expect("old area name"), area, "{value}");
    }
}

#[test]
fn v54b_old_area_names_survive_export_options_patch() {
    let base = ExportOptions::for_format(ExportFormat::Png);
    for (value, area) in old_names() {
        let merged = base.merge_json(&json!({"area": value})).expect("ui.export.set area patch");
        assert_eq!(merged.area, area);
        let again: ExportOptions = serde_json::from_value(serde_json::to_value(&merged).unwrap()).unwrap();
        assert_eq!(again.area, area);
    }
}

#[test]
fn v54b_old_request_json_deserializes_and_round_trips() {
    let request: ExportRequest = serde_json::from_value(json!({
        "format": "Pdf",
        "area": {"Artboard": 2},
        "pages": null,
        "path": "out.pdf",
        "preset": null,
        "options": {}
    }))
    .expect("old request json");
    assert_eq!(request.area, ExportArea::Artboard(ObjectId(2)));
    let back: ExportRequest = serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap();
    assert_eq!(back.area, request.area);
    assert_eq!(back.format, ExportFormat::Pdf);
}