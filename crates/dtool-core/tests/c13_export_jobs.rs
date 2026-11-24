use dtool_core::command::Session;
use dtool_core::commands::ui;
use dtool_core::workspace::ContextId;
use dtool_protocol::{Command, CommandResult};
use serde_json::json;

fn host(_id: Option<ContextId>, session: &Session, command: &Command) -> CommandResult {
    match command.name.as_str() {
        "export.job.list" => Ok(json!({ "items": [] })),
        "export.job.delete" => Ok(json!({ "deleted": command.params["name"] })),
        "export.job.run" => Ok(json!({ "ran": true, "objects": session.document.objects.len() })),
        other => Ok(json!({ "other": other })),
    }
}

#[test]
fn export_job_commands_route_to_the_registered_export_host() {
    for name in ["export.job.run", "export.job.list", "export.job.delete"] {
        assert!(ui::is_export_command(name), "{name}");
    }
    assert!(ui::is_export_command("export.options.schema"));
    let mut none = Session::default();
    none.has_document = false;
    assert_eq!(ui::can_execute(&none, &Command::new("export.job.list")), Some(false), "no host registered yet");
    assert!(ui::register_export_action_for(host));
    assert_eq!(ui::can_execute(&none, &Command::new("export.job.list")), Some(true));
    assert_eq!(ui::can_execute(&none, &Command::new("export.job.delete")), Some(true));
    assert_eq!(ui::can_execute(&none, &Command::new("export.job.run")), Some(false));
    assert_eq!(ui::run(&mut none, &Command::new("export.job.list")).unwrap().unwrap()["items"], json!([]));
    assert_eq!(ui::run(&mut none, &Command::new("export.job.run")).unwrap().unwrap_err().code, "NO_DOCUMENT");
    let mut doc = Session::new(dtool_core::document::Document::new([10.0, 10.0], dtool_core::document::Units::Px, 96.0, 1));
    assert_eq!(ui::run(&mut doc, &Command::new("export.job.run")).unwrap().unwrap()["ran"], true);
    assert_eq!(ui::run(&mut none, &Command::new("export.options.schema")).unwrap().unwrap()["other"], "export.options.schema");
}