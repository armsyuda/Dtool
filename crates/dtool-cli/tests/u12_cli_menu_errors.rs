use dtool_cli::harness::run_cli_args;
use dtool_core::workspace::Workspace;
use serde_json::json;

#[test]
fn headless_cli_has_no_menu_provider_and_reports_a_coded_error() {
    let mut workspace = Workspace::default();
    workspace.new_document("Untitled");
    let shared = workspace.into_shared();
    let before = shared.lock().unwrap().session().unwrap().generation;
    let error = run_cli_args(&shared, &["ui.menu.invoke".to_string(), json!({ "id": "layer.lock" }).to_string()]).expect_err("no menu provider in the headless CLI");
    assert_eq!(error.message_key, format!("error.{}", error.code.to_lowercase()));
    assert!(["NOT_AVAILABLE", "CANNOT_EXECUTE"].contains(&error.code.as_str()), "{}", error.code);
    assert_eq!(shared.lock().unwrap().session().unwrap().generation, before);
}