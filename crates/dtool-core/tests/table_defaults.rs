use dtool_core::{Document, ObjectKind};
use dtool_core::table::model::{TableNode, InitialAdvance, VerticalAlign};
use dtool_core::table::resolve::resolve_cell;
use serde_json::json;

fn legacy_table() -> serde_json::Value {
    json!({"alloc":{},"col_widths_pt":[40.0,40.0],"row_heights_pt":[20.0],
        "cells":[{"id":7,"row":0,"col":0,"row_span":1,"col_span":1,"merged_into":null,"story":1},
                 {"id":12,"row":0,"col":1,"row_span":1,"col_span":1,"merged_into":null,"story":2}],
        "header_rows":0,"footer_rows":0})
}

#[test]
fn legacy_table_without_format_fields_resolves_every_cell_to_factory_defaults() {
    let table: TableNode = serde_json::from_value(legacy_table()).expect("legacy table loads");
    let document = Document::default();
    for cell in &table.cells {
        let format = resolve_cell(&document, &table, cell);
        assert!(format.fill.is_none());
        assert!(format.edges.iter().all(Option::is_none));
        assert_eq!([format.insets.left,format.insets.top,format.insets.right,format.insets.bottom],[0.0;4]);
        assert_eq!(format.vertical_align, VerticalAlign::Top);
        assert_eq!(format.initial_advance, InitialAdvance::Default);
        assert_eq!(format.minimum_pt, 0.0);
    }
}

#[test]
fn legacy_allocator_without_high_water_never_reuses_existing_cell_ids() {
    let mut table: TableNode = serde_json::from_value(legacy_table()).expect("legacy allocator loads");
    let largest = table.cells.iter().map(|cell|cell.id.0).max().unwrap();
    let first = table.alloc.next().0;
    let second = table.alloc.next().0;
    assert!(first > largest);
    assert!(second > first);
}

#[test]
fn document_without_table_formats_loads_empty_and_retains_legacy_table() {
    let mut wire = serde_json::to_value(Document::default()).unwrap();
    wire.as_object_mut().unwrap().remove("table_formats");
    wire.as_object_mut().unwrap().remove("default_table_format");
    let mut object = serde_json::to_value(dtool_core::Object::default()).unwrap();
    object["id"] = json!(1);
    object["kind"] = json!({"Table":legacy_table()});
    wire["objects"] = json!([object]);
    let document: Document = serde_json::from_value(wire).expect("legacy document loads");
    assert!(document.table_formats.is_empty());
    assert!(document.default_table_format.is_none());
    assert!(matches!(document.objects[0].kind,ObjectKind::Table(_)));
}
