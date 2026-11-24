use dtool_core::document::{Document, Units};
use dtool_core::{Command, Session};
use serde_json::json;

#[test]
fn text_host_commands_are_not_implemented_until_a_host_installs() {
    assert!(!dtool_core::commands::text_host::installed());
    let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 72.0, 1));
    for name in dtool_core::commands::text_host::NAMES {
        let command = Command::with_params(name, json!({ "pattern": "x", "replacement": "y" }));
        assert!(!session.can_execute(&command), "{name}");
        assert_eq!(session.run(&command).unwrap_err().code, "NOT_IMPLEMENTED", "{name}");
    }
}