use std::sync::Mutex;

use dtool_core::command::Session;
use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use dtool_protocol::{Command, CommandError, CommandResult};
use serde_json::{json, Value};

static OWNER: Mutex<Option<ContextId>> = Mutex::new(None);

fn owner_guard(id: Option<ContextId>, _session: &Session, command: &Command) -> CommandResult {
    let mut owner = OWNER.lock().unwrap();
    match command.name.as_str() {
        "ui.export.open" => {
            *owner = id;
            Ok(json!({ "open": true, "owner": id.map(|id| id.0) }))
        }
        "ui.export.close" => {
            *owner = None;
            Ok(json!({ "open": false }))
        }
        _ if owner.is_some() && *owner != id => Err(CommandError::context_not_found()),
        _ => Ok(json!({ "owner": owner.map(|id| id.0), "target": id.map(|id| id.0) })),
    }
}

fn call(server: &mut Server, method: &str, name: &str, scope: ContextId) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": { "name": name, "scope": scope.0, "params": {} } }))
}

#[test]
fn mcp_export_from_another_document_is_context_not_found() {
    assert!(dtool_core::commands::ui::register_export_action_for(owner_guard));
    assert!(dtool_core::commands::ui::export_action_registered());
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a");
    let b = workspace.new_document("b");
    let mut server = Server::new(Config::default(), workspace);

    let open = call(&mut server, "dtool.run", "ui.export.open", a);
    assert_eq!(open["result"]["owner"], json!(a.0), "{open}");

    let foreign = call(&mut server, "dtool.run", "ui.export.set", b);
    assert_eq!(foreign["error"]["code"], "CONTEXT_NOT_FOUND", "{foreign}");
    let foreign_preview = call(&mut server, "dtool.run", "ui.export.preview", b);
    assert_eq!(foreign_preview["error"]["code"], "CONTEXT_NOT_FOUND", "{foreign_preview}");
    let foreign_query = call(&mut server, "dtool.query", "ui.export.query", b);
    assert_eq!(foreign_query["error"]["code"], "CONTEXT_NOT_FOUND", "{foreign_query}");

    let own = call(&mut server, "dtool.run", "ui.export.set", a);
    assert_eq!(own["result"]["target"], json!(a.0), "{own}");

    let close = call(&mut server, "dtool.run", "ui.export.close", b);
    assert!(close.get("error").is_none(), "{close}");
    let after = call(&mut server, "dtool.run", "ui.export.set", b);
    assert_eq!(after["result"]["target"], json!(b.0), "{after}");
}