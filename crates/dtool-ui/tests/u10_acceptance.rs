use dtool_core::document::{Document, Story, StoryId};
use dtool_core::object::{Object, ObjectKind};
use dtool_core::table::geometry::table_grid;
use dtool_core::table::model::{CellFormat, CellId, CellIdAllocator, FrameFormatMask, TableCell, TableFrameFormat, TableNode};
use dtool_core::table::query::table_query;
use dtool_core::workspace::{CloseAnswer, ContextId};
use dtool_core::{Command, Scope, Workspace};
use dtool_ui::context_bar::{apply_edit_with_view, ContextBarModel, ContextEdit, ControlState, OPTION_SET};
use dtool_ui::table::snapshot::TableSnapshots;
use dtool_ui::table::view::{TableView, ERROR_KEY};
use serde_json::{json, Value};

const VIEW: [f32; 2] = [1024.0, 768.0];

fn create_table(workspace: &mut Workspace, rows: u32, cols: u32) -> u64 {
    let (_, created) = workspace.run(Scope::active(), Command::with_params("table.create", json!({ "rows": rows, "cols": cols, "insert": true })));
    let created = created.expect("table.create");
    created.get("id").or_else(|| created.get("object_id")).and_then(Value::as_u64).expect("table id")
}

fn stamped(mut query: Value, generation: u64, version: u64) -> Value {
    query["generation"] = json!(generation);
    query["document_version"] = json!(version);
    query
}

fn cell(id: u64, col: u32, col_span: u32, merged_into: Option<u64>) -> TableCell {
    TableCell { id: CellId(id), row: 0, col, row_span: 1, col_span, merged_into: merged_into.map(CellId), story: StoryId(id), local: CellFormat::default(), mask: Default::default() }
}

fn merged_document() -> (Document, dtool_core::ObjectId) {
    let mut doc = Document::default();
    for id in 1..=3 {
        doc.stories.push(Story { id: StoryId(id), text: format!("s{id}"), ..Story::default() });
    }
    let table = TableNode {
        alloc: CellIdAllocator { high_water: 3 },
        col_widths_pt: vec![40.0, 20.0, 30.0],
        row_heights_pt: vec![25.0],
        cells: vec![cell(1, 0, 1, None), cell(2, 1, 2, None), cell(3, 2, 1, Some(2))],
        header_rows: 0,
        footer_rows: 0,
        format: None,
        frame_local: TableFrameFormat::default(),
        frame_mask: FrameFormatMask::default(),
        text_defaults: None,
    };
    let id = doc.create_object(Object { kind: ObjectKind::Table(table), size: [90.0, 25.0], ..Object::default() });
    (doc, id)
}

#[test]
fn u10_accept_snapshot_view_matches_core_grid() {
    let mut workspace = Workspace::default();
    let context = workspace.new_document("grid");
    let table = create_table(&mut workspace, 2, 3);
    let mut snapshots = TableSnapshots::new();
    assert!(snapshots.refresh(&workspace));
    let session = &workspace.get(context).expect("context").session;
    let view = snapshots.views(context).expect("views").views.get(&table).expect("table view").as_ref().expect("view ok");
    let node = match &session.document.objects.iter().find(|o| o.id.0 == table).expect("object").kind {
        ObjectKind::Table(t) => t,
        _ => panic!("not a table"),
    };
    let grid = table_grid(node);
    assert_eq!(view.rects.len(), 6);
    assert_eq!(view.rects.len(), grid.rects.len());
    for (a, b) in view.rects.iter().zip(grid.rects.iter()) {
        assert_eq!((a.id, a.row, a.col, a.row_span, a.col_span), (b.id, b.row, b.col, b.row_span, b.col_span));
        for i in 0..4 {
            assert!((a.rect[i] - b.rect[i]).abs() < 1e-3, "cell {} rect {:?} vs {:?}", a.id, a.rect, b.rect);
        }
    }
    assert!(view.merged_into.is_empty());
    assert_eq!((view.generation, view.document_version), (session.generation, session.document.version));
}

#[test]
fn u10_accept_snapshot_one_refresh_per_document_change() {
    let mut workspace = Workspace::default();
    let context = workspace.new_document("refresh");
    let first = create_table(&mut workspace, 1, 2);
    let mut snapshots = TableSnapshots::new();
    assert!(snapshots.refresh(&workspace));
    let count = snapshots.refresh_count();
    assert!(!snapshots.refresh(&workspace));
    assert!(!snapshots.refresh(&workspace));
    assert_eq!(snapshots.refresh_count(), count);
    let second = create_table(&mut workspace, 2, 2);
    assert!(snapshots.refresh(&workspace));
    assert!(!snapshots.refresh(&workspace));
    assert_eq!(snapshots.refresh_count(), count + 1);
    let views = &snapshots.views(context).expect("views").views;
    assert!(views.get(&first).expect("first").is_ok());
    assert!(views.get(&second).expect("second").is_ok());
}

#[test]
fn u10_accept_snapshot_keyed_per_context() {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let table_a = create_table(&mut workspace, 1, 1);
    let b = workspace.new_document("b");
    let table_b = create_table(&mut workspace, 3, 1);
    let mut snapshots = TableSnapshots::new();
    workspace.activate(a).expect("activate a");
    assert!(snapshots.refresh(&workspace));
    workspace.activate(b).expect("activate b");
    assert!(snapshots.refresh(&workspace));
    let view_a = snapshots.views(a).expect("a").views.get(&table_a).expect("table a").as_ref().expect("ok a");
    let view_b = snapshots.views(b).expect("b").views.get(&table_b).expect("table b").as_ref().expect("ok b");
    assert_eq!(view_a.rects.len(), 1);
    assert_eq!(view_b.rects.len(), 3);
    workspace.activate(a).expect("back to a");
    assert!(!snapshots.refresh(&workspace));
    assert!(workspace.close_with(b, CloseAnswer::No).expect("close b"));
    snapshots.refresh(&workspace);
    assert!(snapshots.views(b).is_none());
    assert!(snapshots.views(a).is_some());
}

#[test]
fn u10_accept_view_parses_merged_cells_from_core_query() {
    let (doc, id) = merged_document();
    let query = stamped(table_query(&doc, id, None), 4, doc.version);
    let view = TableView::from_query(&query).expect("merged view");
    assert_eq!(view.merged_into, vec![(3, 2)]);
    assert_eq!(view.rects.len(), 2);
    let owner = view.rects.iter().find(|r| r.id == 2).expect("owner rect");
    assert_eq!((owner.col, owner.col_span), (1, 2));
    for (got, want) in owner.rect.iter().zip([40.0, 0.0, 90.0, 25.0]) {
        assert!((got - want).abs() < 1e-3, "{:?}", owner.rect);
    }
}

#[test]
fn u10_accept_view_rejects_tampered_core_query() {
    let (doc, id) = merged_document();
    let good = stamped(table_query(&doc, id, None), 1, doc.version);
    assert!(TableView::from_query(&good).is_ok());
    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("document_version");
    let mut negative = good.clone();
    negative["col_widths_pt"][1] = json!(-5.0);
    let mut wide = good.clone();
    wide["cells"][1]["col_span"] = json!(3);
    let mut orphan = good.clone();
    orphan["cells"][2]["merged_into"] = json!("c1");
    for (name, query) in [("missing", missing), ("negative", negative), ("wide", wide), ("orphan", orphan)] {
        assert_eq!(TableView::from_query(&query).err().as_deref(), Some(ERROR_KEY), "{name}");
    }
}

#[test]
fn u10_accept_zoom_fit_view_size_in_workspace_session() {
    let mut workspace = Workspace::default();
    workspace.new_document("zoom");
    let session = workspace.session_mut().expect("session");
    session.run_named("ui.tool.set", json!({ "id": "tool.zoom" })).expect("tool");
    session.run_named("view.zoom.set", json!({ "zoom": 5.0 })).expect("zoom");
    assert_eq!(ContextBarModel::current(session).item("zoom.fit").expect("fit").state, ControlState::Disabled);
    assert_eq!(ContextBarModel::current(session).with_view_size(session, VIEW).item("zoom.fit").expect("fit").state, ControlState::Enabled);
    let page = session.document.pages.first().expect("page").size;
    let expected = dtool_core::view::fit_zoom(page, VIEW, session.view.rotation_deg);
    apply_edit_with_view(session, "tool.zoom", &ContextEdit { id: "zoom.fit".into(), value: json!(null) }, VIEW).expect("fit");
    assert!((session.view.zoom - expected).abs() < 1e-4, "{} vs {expected}", session.view.zoom);
}

#[test]
fn u10_accept_zoom_fit_option_set_over_command_path() {
    let mut workspace = Workspace::default();
    let context: ContextId = workspace.new_document("mcp");
    workspace.run(Scope::active(), Command::with_params("ui.tool.set", json!({ "id": "tool.zoom" }))).1.expect("tool");
    workspace.run(Scope::active(), Command::with_params("view.zoom.set", json!({ "zoom": 6.0 }))).1.expect("zoom");
    let (_, bare) = workspace.run(Scope::active(), Command::with_params(OPTION_SET, json!({ "tool": "tool.zoom", "id": "zoom.fit" })));
    assert!(bare.is_err());
    let (_, sized) = workspace.run(Scope::active(), Command::with_params(OPTION_SET, json!({ "tool": "tool.zoom", "id": "zoom.fit", "params": { "view_size": VIEW } })));
    sized.expect("option set with view_size");
    let session = &workspace.get(context).expect("context").session;
    let page = session.document.pages.first().expect("page").size;
    let expected = dtool_core::view::fit_zoom(page, VIEW, session.view.rotation_deg);
    assert!((session.view.zoom - expected).abs() < 1e-4, "{} vs {expected}", session.view.zoom);
}