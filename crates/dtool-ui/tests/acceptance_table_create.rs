use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::table::ids::{CellId, CellIdAllocator};
use dtool_core::table::model::TableNode;
use dtool_core::Session;
use serde_json::{json, Value};

fn session() -> Session { Session::new(Document::new([800.0, 600.0], Units::Px, 96.0, 1)) }

fn create(session: &mut Session, rows: u64, cols: u64, header: u64, footer: u64) -> Value {
    session.run_named("table.create", json!({"rows": rows, "cols": cols, "header_rows": header, "footer_rows": footer})).unwrap()["table"].clone()
}

fn place(session: &mut Session, table: Value) -> u64 {
    let node: TableNode = serde_json::from_value(table).unwrap();
    let mut object = Object::default();
    object.id = ObjectId(1);
    object.kind = ObjectKind::Table(node);
    session.document.objects.push(object);
    1
}

fn table_of(session: &Session) -> &TableNode {
    match &session.document.objects[0].kind { ObjectKind::Table(t) => t, _ => panic!("expected table") }
}

#[test]
fn test_table_create_drag_and_command_match() {
    let mut s = session();
    let table = create(&mut s, 3, 2, 1, 0);
    assert_eq!(table["header_rows"], json!(1));
    assert_eq!(table["footer_rows"], json!(0));
    assert_eq!(table["cells"].as_array().unwrap().len(), 6);
}

#[test]
fn test_table_header_footer_stored() {
    let mut s = session();
    let table = create(&mut s, 5, 3, 2, 1);
    place(&mut s, table);
    assert_eq!(table_of(&s).header_rows, 2);
    assert_eq!(table_of(&s).footer_rows, 1);
}

#[test]
fn test_table_create_rejects_out_of_range() {
    let mut s = session();
    assert!(s.run_named("table.create", json!({"rows": 0, "cols": 2})).is_err());
    assert!(s.run_named("table.create", json!({"rows": 2, "cols": 1001})).is_err());
}

#[test]
fn test_legacy_id_hydration() {
    let legacy_ids: Vec<CellId> = (1..=9).map(CellId).collect();
    let mut allocator = CellIdAllocator::hydrate(&legacy_ids).unwrap();
    assert_eq!(allocator.high_water, 9);
    assert_eq!(allocator.next().0, 10);
    assert!(CellIdAllocator::hydrate(&[CellId(1), CellId(1)]).is_err());
}

#[test]
fn test_table_create_with_legacy_hydration() {
    let mut s = session();
    let table = create(&mut s, 2, 2, 1, 0);
    let id = place(&mut s, table);
    assert_eq!(table_of(&s).alloc.high_water, 4);
    s.run_named("table.row.insert", json!({"object_id": id, "row": 0})).unwrap();
    assert_eq!(table_of(&s).alloc.high_water, 6);
    assert_eq!(table_of(&s).row_heights_pt.len(), 3);
    assert_unique_ids(&s);
}

fn assert_unique_ids(session: &Session) {
    let cells: Vec<u64> = table_of(session).cells.iter().map(|cell| cell.id.0).collect();
    let unique_cells: std::collections::BTreeSet<u64> = cells.iter().copied().collect();
    assert_eq!(unique_cells.len(), cells.len());
    assert!(cells.iter().all(|id| *id >= 1 && *id <= table_of(session).alloc.high_water));
    let objects: Vec<u64> = session.document.objects.iter().map(|object| object.id.0).collect();
    let unique_objects: std::collections::BTreeSet<u64> = objects.iter().copied().collect();
    assert_eq!(unique_objects.len(), objects.len());
    let stories: Vec<u64> = table_of(session).cells.iter().map(|cell| cell.story.0).collect();
    let unique_stories: std::collections::BTreeSet<u64> = stories.iter().copied().collect();
    assert_eq!(unique_stories.len(), stories.len());
}

#[test]
fn test_drag_create_matches_command_create() {
    let mut a = session();
    let mut b = session();
    let t1 = create(&mut a, 4, 3, 1, 1);
    place(&mut a, t1.clone());
    assert_unique_ids(&a);
    let t2 = create(&mut b, 4, 3, 1, 1);
    for key in ["header_rows", "footer_rows", "col_widths_pt", "row_heights_pt"] {
        assert_eq!(t1[key], t2[key]);
    }
}


#[test]
fn table_select_all_formats_every_cell_and_previews_as_one_undo() {
    use dtool_core::table::{resolve_cell, selection};
    use dtool_core::{Command, edit_session::EditEnd};
    let mut shell = dtool_ui::shell::AppShell::new();
    shell.workspace.new_document("Table formatting");
    let session = shell.workspace.session_mut().unwrap();
    let id = ObjectId(session.run_named("table.create", json!({
        "rows": 3, "cols": 3, "bounds": [150.0, 90.0], "pos": [0.0, 0.0],
    })).unwrap()["id"].as_u64().unwrap());
    session.active_tool = "tool.table".into();
    shell.select_all();

    let session = shell.workspace.session_mut().unwrap();
    let selected = selection::current(session);
    assert_eq!(selected.table, Some(id));
    assert_eq!(selected.cells.len(), 9);
    let paragraph = dtool_ui::panels::paragraph::apply_patch(session, json!({"align": "Center", "indent_left": 3.0}));
    assert!(paragraph.is_ok(), "{paragraph:?}");
    for story in &session.document.stories {
        let style = dtool_core::text_cmd::query_para_style(&story.paras, 0, 0);
        assert_eq!(style["align"], "Center");
        assert_eq!(style["indent_left"], 3.0);
    }
    let depth = session.undo.applied_steps();
    let fill = dtool_core::paint::Paint::Solid { colour: dtool_core::paint::Colour::new(20, 88, 180, 255) };
    for (left, width) in [(2.0, 0.5), (4.0, 2.0)] {
        let command = Command::with_params(Command::TABLE_CELL_FORMAT_SET, json!({
            "id": id, "cells": selected.cells,
            "fill": fill,
            "inset_patch": {"left": left, "top": 3.0, "right": 2.0, "bottom": 1.0, "linked": false},
            "vertical_align": "Center",
            "borders": {"selection_mask": 63, "edge_patch": {"width_pt": width}}
        }));
        session.preview("table.panel.format", &command).unwrap();
        let ObjectKind::Table(table) = &session.document.get_object(id).unwrap().kind else {
            panic!("expected a table");
        };
        for cell in &table.cells {
            let format = resolve_cell(&session.document, table, cell);
            assert_eq!([format.insets.left, format.insets.top, format.insets.right, format.insets.bottom], [left, 3.0, 2.0, 1.0]);
            assert_eq!(format.vertical_align, dtool_core::table::model::VerticalAlign::Center);
            assert_eq!(format.fill.as_ref(), Some(&fill));
            assert!(format.edges.iter().flatten().any(|edge| edge.width_pt == width));
        }
    }
    assert_eq!(session.undo.applied_steps(), depth);
    session.end_edit(EditEnd::Commit).unwrap();
    assert_eq!(session.undo.applied_steps(), depth + 1);
    session.run(&Command::new(dtool_core::table::merge::MERGE)).unwrap();
    assert_eq!(selection::current(session).cells.len(), 1);
    session.run(&Command::new(dtool_core::table::merge::UNMERGE)).unwrap();
    assert_eq!(selection::current(session).cells.len(), 9);
}
