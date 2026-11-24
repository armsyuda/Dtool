use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use dtool_core::document::Document;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::table::model::CellId;
use dtool_core::table::query::{attach_layout_lines, table_query_with_overflows};
use dtool_core::workspace::{ContextId, Workspace};
use dtool_tools::dtool_text::{layout_table, FontLibrary};

use crate::shell::AppShell;
use crate::table::view::TableView;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotKey {
    pub generation: u64,
    pub document_version: u64,
    pub font_generation: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ContextTableViews {
    pub key: Option<SnapshotKey>,
    pub views: BTreeMap<u64, Result<TableView, String>>,
    pub selection: dtool_core::table::selection::TableSelection,
    pub overflows: BTreeMap<u64, Vec<u64>>,
}

pub fn layout_overflows(lib: &FontLibrary, document: &Document, object_id: ObjectId) -> HashMap<CellId, bool> {
    let Some(object) = document.objects.iter().find(|object| object.id == object_id) else {
        return HashMap::new();
    };
    let ObjectKind::Table(table) = &object.kind else {
        return HashMap::new();
    };
    let grid = dtool_core::table::geometry::table_grid(table);
    layout_table(lib, document, table, &grid).cells.into_iter().map(|cell| (cell.id, cell.overflow)).collect()
}

pub fn table_query_with_layout(lib: &FontLibrary, document: &Document, object_id: ObjectId, cell_filter: Option<CellId>) -> serde_json::Value {
    let Some(lines) = dtool_tools::dtool_text::table_layout::table_lines_with(lib, document, object_id) else {
        return table_query_with_overflows(document, object_id, cell_filter, None);
    };
    let overflows: HashMap<CellId, bool> = lines.cells.iter().map(|(id, cell)| (*id, cell.overflow)).collect();
    let mut query = table_query_with_overflows(document, object_id, cell_filter, Some(&overflows));
    attach_layout_lines(&mut query, &lines);
    query
}

fn overflowing_cells(query: &serde_json::Value) -> Vec<u64> {
    query["cells"]
        .as_array()
        .map(|cells| {
            cells
                .iter()
                .filter(|cell| cell["overflow"].as_bool() == Some(true))
                .filter_map(|cell| cell["id"].as_str()?.strip_prefix('c')?.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Default)]
pub struct TableSnapshots {
    contexts: HashMap<ContextId, ContextTableViews>,
    font_generation: u64,
    refreshes: u64,
    follow_font_signal: bool,
}

impl TableSnapshots {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn following_font_signal() -> Self {
        Self { follow_font_signal: true, ..Self::default() }
    }

    pub fn font_generation(&self) -> u64 {
        self.font_generation
    }

    pub fn set_font_generation(&mut self, generation: u64) {
        self.font_generation = generation;
    }

    pub fn sync_font_generation(&mut self) -> bool {
        let published = dtool_core::text_lines::font_generation();
        if published > self.font_generation {
            self.font_generation = published;
            true
        } else {
            false
        }
    }

    pub fn refresh_count(&self) -> u64 {
        self.refreshes
    }

    pub fn views(&self, id: ContextId) -> Option<&ContextTableViews> {
        self.contexts.get(&id)
    }

    pub fn refresh(&mut self, workspace: &Workspace) -> bool {
        if self.follow_font_signal {
            self.sync_font_generation();
        }
        let open: Vec<ContextId> = workspace.contexts().iter().map(|context| context.id).collect();
        self.contexts.retain(|id, _| open.contains(id));
        let Some(id) = workspace.active() else {
            return false;
        };
        let Some(context) = workspace.get(id) else {
            return false;
        };
        let session = &context.session;
        let key = SnapshotKey { generation: session.generation, document_version: session.document.version, font_generation: self.font_generation };
        let entry = self.contexts.entry(id).or_default();
        if entry.key == Some(key) {
            return false;
        }
        let lib = FontLibrary::shared();
        let mut overflows = BTreeMap::new();
        entry.views = session
            .document
            .objects
            .iter()
            .filter(|object| matches!(object.kind, ObjectKind::Table(_)))
            .map(|object| {
                let mut query = table_query_with_layout(&lib, &session.document, object.id, None);
                overflows.insert(object.id.0, overflowing_cells(&query));
                query["generation"] = serde_json::json!(session.generation);
                query["document_version"] = serde_json::json!(session.document.version);
                (object.id.0, TableView::from_query(&query))
            })
            .collect();
        entry.overflows = overflows;
        entry.selection = dtool_core::table::selection::current(session);
        entry.key = Some(key);
        self.refreshes += 1;
        true
    }
}

thread_local! {
    static TABLE_SNAPSHOTS: RefCell<TableSnapshots> = RefCell::new(TableSnapshots::default());
}

pub fn with_table_snapshots<R>(f: impl FnOnce(&mut TableSnapshots) -> R) -> R {
    TABLE_SNAPSHOTS.with(|snapshots| f(&mut snapshots.borrow_mut()))
}

impl AppShell {
    pub fn refresh_table_views(&mut self) -> bool {
        with_table_snapshots(|snapshots| {
            snapshots.set_font_generation(dtool_tools::dtool_text::fonts::font_generation());
            snapshots.refresh(&self.workspace)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtool_core::workspace::SharedServices;
    use dtool_protocol::Command;
    use serde_json::json;
    use std::sync::Arc;

    fn add_table(workspace: &mut Workspace) -> u64 {
        let (_, result) = workspace.run(dtool_core::Scope::active(), Command::with_params("table.create", json!({ "rows": 2, "cols": 2, "bounds": [100.0, 60.0], "insert": true })));
        result.expect("table.create")["object_id"].as_u64().expect("table id")
    }

    #[test]
    fn one_command_gives_exactly_one_refresh() {
        let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
        workspace.new_document("A");
        let table = add_table(&mut workspace);
        let mut snapshots = TableSnapshots::new();
        assert!(snapshots.refresh(&workspace));
        let before = snapshots.refresh_count();
        let (_, result) = workspace.run(dtool_core::Scope::active(), Command::with_params("table.row.insert", json!({ "object_id": table, "row": 1 })));
        result.expect("table.row.insert");
        assert!(snapshots.refresh(&workspace));
        assert!(!snapshots.refresh(&workspace));
        assert!(!snapshots.refresh(&workspace));
        assert_eq!(snapshots.refresh_count(), before + 1);
        let id = workspace.active().expect("active");
        let views = snapshots.views(id).expect("views");
        assert_eq!(views.views.len(), 1);
        assert!(views.views.values().all(Result::is_ok));
    }

    #[test]
    fn font_generation_change_drops_old_views() {
        let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
        workspace.new_document("A");
        add_table(&mut workspace);
        let mut snapshots = TableSnapshots::new();
        assert!(snapshots.refresh(&workspace));
        assert!(!snapshots.refresh(&workspace));
        snapshots.set_font_generation(snapshots.font_generation() + 1);
        assert!(snapshots.refresh(&workspace));
    }

    #[test]
    fn shell_refresh_follows_font_registry_generation() {
        let arial = std::path::Path::new("C:\\Windows\\Fonts\\arial.ttf");
        if !arial.exists() {
            eprintln!("skip: arial.ttf is not installed");
            return;
        }
        let _ = dtool_tools::dtool_text::FontLibrary::shared();
        let mut shell = AppShell::new();
        if shell.workspace.active().is_none() {
            shell.workspace.new_document("A");
        }
        add_table(&mut shell.workspace);
        shell.refresh_table_views();
        assert!(!shell.refresh_table_views());
        dtool_tools::dtool_text::FontLibrary::from_fixtures(&[arial]).expect("fixture");
        let generation = dtool_tools::dtool_text::fonts::font_generation();
        assert!(shell.refresh_table_views());
        assert!(!shell.refresh_table_views());
        let id = shell.workspace.active().expect("active");
        with_table_snapshots(|snapshots| {
            assert_eq!(snapshots.font_generation(), generation);
            assert_eq!(snapshots.views(id).expect("views").key.expect("key").font_generation, generation);
        });
    }

    #[test]
    fn producer_follows_font_change_and_skips_no_op() {
        let mut shell = AppShell::new();
        if shell.workspace.active().is_none() {
            shell.workspace.new_document("A");
        }
        add_table(&mut shell.workspace);
        shell.refresh_table_views();
        let before = with_table_snapshots(|snapshots| snapshots.font_generation());
        let same = dtool_tools::dtool_text::fonts::note_font_change(7, 7);
        assert_eq!(same, dtool_tools::dtool_text::fonts::font_generation());
        let bumped = dtool_tools::dtool_text::fonts::note_font_change(7, 8);
        assert!(bumped > before);
        assert!(shell.refresh_table_views());
        assert!(with_table_snapshots(|snapshots| snapshots.font_generation()) >= bumped);
    }

    #[test]
    fn snapshot_walks_grouped_table() {
        let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
        workspace.new_document("A");
        let first = add_table(&mut workspace);
        let second = add_table(&mut workspace);
        let third = add_table(&mut workspace);
        let (_, result) = workspace.run(dtool_core::Scope::active(), Command::with_params("node.group", json!({ "ids": [first, second] })));
        let group = result.expect("node.group")["group"].as_u64().expect("group id");
        let (_, result) = workspace.run(dtool_core::Scope::active(), Command::with_params("node.group", json!({ "ids": [group, third] })));
        result.expect("nested node.group");
        let id = workspace.active().expect("active");
        let document = &workspace.get(id).expect("context").session.document;
        assert!(document.get_object(dtool_core::ObjectId(first)).and_then(|object| object.parent_id).is_some());
        let mut snapshots = TableSnapshots::new();
        assert!(snapshots.refresh(&workspace));
        let views = snapshots.views(id).expect("views");
        for table in [first, second, third] {
            assert!(views.views.get(&table).is_some_and(Result::is_ok), "{table}");
        }
        assert_eq!(views.views.len(), 3);
    }

    #[test]
    fn refresh_follows_published_font_signal_without_shell() {
        let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
        workspace.new_document("A");
        add_table(&mut workspace);
        let mut snapshots = TableSnapshots::following_font_signal();
        assert!(snapshots.refresh(&workspace));
        let next = dtool_core::text_lines::font_generation() + 1;
        dtool_core::text_lines::publish_font_generation(next);
        assert!(snapshots.refresh(&workspace));
        assert!(snapshots.font_generation() >= next);
        let mut fixed = TableSnapshots::new();
        assert!(fixed.refresh(&workspace));
        dtool_core::text_lines::publish_font_generation(dtool_core::text_lines::font_generation() + 1);
        assert!(!fixed.refresh(&workspace));
        assert!(fixed.sync_font_generation());
        assert!(fixed.refresh(&workspace));
        let id = workspace.active().expect("active");
        assert_eq!(snapshots.views(id).expect("views").key.expect("key").font_generation, snapshots.font_generation());
    }

    #[test]
    fn switching_tabs_keeps_each_documents_views() {
        let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
        let a = workspace.new_document("A");
        workspace.activate(a).expect("activate a");
        add_table(&mut workspace);
        let b = workspace.new_document("B");
        workspace.activate(b).expect("activate b");
        add_table(&mut workspace);
        add_table(&mut workspace);
        let mut snapshots = TableSnapshots::new();
        assert!(snapshots.refresh(&workspace));
        workspace.activate(a).expect("back to a");
        assert!(snapshots.refresh(&workspace));
        let count = snapshots.refresh_count();
        workspace.activate(b).expect("back to b");
        assert!(!snapshots.refresh(&workspace));
        workspace.activate(a).expect("again a");
        assert!(!snapshots.refresh(&workspace));
        assert_eq!(snapshots.refresh_count(), count);
        assert_eq!(snapshots.views(a).expect("a views").views.len(), 1);
        assert_eq!(snapshots.views(b).expect("b views").views.len(), 2);
    }
}
