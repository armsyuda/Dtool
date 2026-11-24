use dtool_core::workspace::{ContextId, SharedServices, Workspace};
use std::sync::Arc;

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("dtool_workspace_open_{}_{}", std::process::id(), nanos));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn new_document_twice_creates_two_tabs_with_ids_1_and_2() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);

    let id1 = workspace.new_document("Doc 1");
    let id2 = workspace.new_document("Doc 2");

    assert_eq!(id1, ContextId(1));
    assert_eq!(id2, ContextId(2));
    assert_eq!(workspace.contexts().len(), 2);
    assert_eq!(workspace.active(), Some(ContextId(2)));
}

#[test]
fn open_path_same_file_twice_keeps_one_tab() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.dtool");
    std::fs::write(&file_path, serde_json::to_string(&dtool_core::document::Document::default()).unwrap()).unwrap();

    let id1 = workspace.open_path(&file_path).unwrap();
    let id2 = workspace.open_path(&file_path).unwrap();

    assert_eq!(id1, id2);
    assert_eq!(workspace.contexts().len(), 1);
}

#[test]
fn ids_are_not_reused_after_close() {
    let services = Arc::new(SharedServices::default());
    let mut workspace = Workspace::new(services);

    let id1 = workspace.new_document("Doc 1");
    let id2 = workspace.new_document("Doc 2");

    assert_eq!(id1, ContextId(1));
    assert_eq!(id2, ContextId(2));

    workspace.close_with(id1, dtool_core::workspace::CloseAnswer::No).unwrap();

    let id3 = workspace.new_document("Doc 3");
    assert_eq!(id3, ContextId(3));
}
