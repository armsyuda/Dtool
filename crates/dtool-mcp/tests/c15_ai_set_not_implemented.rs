use dtool_core::workspace::Workspace;
use dtool_mcp::{Config, Server};
use serde_json::json;

#[test]
fn ai_connection_set_is_not_implemented_over_mcp_and_query_still_answers() {
    let mut server = Server::new(Config::default(), Workspace::default());
    server.gui_connection_handle().set_from_gui(true);
    let set = server.dispatch(&json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": "app.ai_connection.set", "params": { "enabled": false } } }));
    assert_eq!(set["error"]["code"], "NOT_IMPLEMENTED", "{set}");
    assert!(server.gui_connection_handle().enabled(), "external set must not switch the connection");
    let query = server.dispatch(&json!({ "jsonrpc": "2.0", "id": 2, "method": "dtool.query", "params": { "name": "app.ai_connection.query" } }));
    assert_eq!(query["result"]["enabled"], true, "{query}");
    server.gui_connection_handle().set_from_gui(false);
    let off = server.dispatch(&json!({ "jsonrpc": "2.0", "id": 3, "method": "dtool.run", "params": { "name": "app.ai_connection.set", "params": { "enabled": true } } }));
    assert_eq!(off["error"]["code"], "AI_CONNECTION_DISABLED", "{off}");
}