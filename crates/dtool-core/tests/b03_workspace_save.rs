
#[path = "../../dtool-cli/src/harness.rs"]
#[allow(dead_code)]
mod actual_owned_profile;

mod codex_workspace_save_acceptance {
    use super::actual_owned_profile::TestProfile;
    use dtool_core::{Command, ContextId, Scope, Workspace};
    use serde_json::json;

    fn fixture() -> (TestProfile, Workspace, ContextId) {
        let profile = TestProfile::new();
        let mut workspace = Workspace::default();
        let id = workspace.new_document("save-fixture");
        (profile, workspace, id)
    }

    fn command(workspace: &mut Workspace, id: ContextId, name: &str, params: serde_json::Value) -> dtool_core::CommandResult {
        let (target, result) = workspace.run(Scope::of(id), Command { name: name.into(), params });
        assert_eq!(target, Some(id));
        result
    }

    #[test]
    fn save_uses_context_path_and_clears_dirty() {
        let (profile, mut workspace, id) = fixture();
        let path = profile.path().join("saved.dtool");
        command(&mut workspace, id, "doc.save_as", json!({ "path": path })).unwrap();
        workspace.get_mut(id).unwrap().session.document.version += 1;
        assert!(workspace.get(id).unwrap().is_modified());
        command(&mut workspace, id, "doc.save", json!({})).unwrap();
        let context = workspace.get(id).unwrap();
        assert_eq!(context.path.as_deref(), Some(path.as_path()));
        assert_eq!(context.saved_version, context.session.document.version);
        assert!(!context.is_modified());
        let file: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(file["version"], context.saved_version);
    }

    #[test]
    fn save_as_changes_only_the_target_context_path_and_keeps_old_file() {
        let (profile, mut workspace, id) = fixture();
        let old = profile.path().join("old.dtool");
        let new = profile.path().join("new.dtool");
        command(&mut workspace, id, "doc.save_as", json!({ "path": old })).unwrap();
        let old_bytes = std::fs::read(&old).unwrap();
        workspace.get_mut(id).unwrap().session.document.version += 1;
        command(&mut workspace, id, "doc.save_as", json!({ "path": new })).unwrap();
        assert_eq!(workspace.get(id).unwrap().path.as_deref(), Some(new.as_path()));
        assert!(!workspace.get(id).unwrap().is_modified());
        assert_eq!(std::fs::read(old).unwrap(), old_bytes);
        assert!(new.is_file());
    }

    #[test]
    fn two_contexts_cannot_own_one_path() {
        let (profile, mut workspace, first) = fixture();
        let path = profile.path().join("owned.dtool");
        command(&mut workspace, first, "doc.save_as", json!({ "path": path })).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let second = workspace.new_document("other-context");
        let error = command(&mut workspace, second, "doc.save_as", json!({ "path": path })).unwrap_err();
        assert_eq!(error.code, "INVALID_PARAMS");
        assert!(workspace.get(second).unwrap().path.is_none());
        assert_eq!(workspace.get(first).unwrap().path.as_deref(), Some(path.as_path()));
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn failed_write_preserves_previous_file_path_and_dirty_state() {
        let (profile, mut workspace, id) = fixture();
        let path = profile.path().join("protected.dtool");
        command(&mut workspace, id, "doc.save_as", json!({ "path": path })).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let saved_version = workspace.get(id).unwrap().saved_version;
        workspace.get_mut(id).unwrap().session.document.version += 1;
        let mut temporary = path.as_os_str().to_os_string(); temporary.push(".tmp");
        std::fs::create_dir(std::path::PathBuf::from(temporary)).unwrap();
        assert!(command(&mut workspace, id, "doc.save", json!({})).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(workspace.get(id).unwrap().path.as_deref(), Some(path.as_path()));
        assert_eq!(workspace.get(id).unwrap().saved_version, saved_version);
        assert!(workspace.get(id).unwrap().is_modified());
    }
}
