use std::collections::HashSet;
use std::sync::Arc;

use dtool_core::workspace::{SharedServices, Workspace};
use dtool_mcp::{Config, LEGACY, MODERN, SERVER_NAME, SUPPORTED, Server};
use dtool_protocol::CommandCatalog;
use serde_json::{Value, json};

const B02_NAMES: [&str; 5] = ["edit.undo", "edit.redo", "edit.batch", "doc.query", "commands.list"];

fn server() -> Server {
    let workspace = Workspace::new(Arc::new(SharedServices::default()));
    Server::new(Config::default(), workspace)
}

fn request(method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    })
}

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

#[test]
fn catalog_names_stable_with_b02() {
    let catalog = CommandCatalog::new();
    for name in B02_NAMES {
        assert!(catalog.contains(name), "B02 command missing: {name}");
        assert_eq!(catalog.get(name).map(|spec| spec.name), Some(name));
    }
    let server = server();
    assert_eq!(server.catalog().names(), catalog.names());
    assert_eq!(string_list(&server.metadata()["catalog"]), catalog.names());
}

#[test]
fn schema_for_a_known_command() {
    let server = server();
    let schema = server.schema_for_command("doc.query").expect("doc.query is in the catalog");
    assert_eq!(schema["name"], json!("doc.query"));
    assert!(schema["inputSchema"].is_object());
    assert!(schema["inputSchema"]["properties"].get("scope").is_some());

    let mut server = server;
    let reply = server.handle_request(request("dtool.schema", json!({ "name": "doc.query" })));
    assert_eq!(reply["jsonrpc"], json!("2.0"));
    assert_eq!(reply["result"]["name"], json!("doc.query"));
    assert!(reply.get("error").is_none());
}

#[test]
fn unknown_command_returns_unknown_command() {
    let mut server = server();
    let err = server
        .schema_for_command("no.such.command")
        .expect_err("unknown names have no schema");
    assert_eq!(err["code"], json!("UNKNOWN_COMMAND"));
    assert_eq!(err["message_key"], json!("error.unknown_command"));

    let reply = server.handle_request(request("dtool.schema", json!({ "name": "no.such.command" })));
    assert_eq!(reply["error"]["code"], json!("UNKNOWN_COMMAND"));

    let reply = server.handle_request(request("tools/call", json!({ "name": "no.such.command" })));
    assert_eq!(reply["error"]["code"], json!("UNKNOWN_COMMAND"));
    assert!(reply.get("result").is_none());

    let reply = server.handle_request(request("no.such.method", Value::Null));
    assert_eq!(reply["error"]["code"], json!("UNKNOWN_COMMAND"));

    assert!(CommandCatalog::new().assert_named("no.such.command").is_err());
    assert!(CommandCatalog::new().assert_named("").is_err());
}

#[test]
fn protocol_versions_advertised() {
    assert_eq!(MODERN, "2026-07-28");
    assert_eq!(LEGACY, "2025-11-25");
    assert_eq!(SUPPORTED, &[MODERN, LEGACY]);

    let mut server = server();
    let supported: Vec<String> = SUPPORTED.iter().map(|v| v.to_string()).collect();
    assert_eq!(string_list(&server.capabilities()["protocol"]), supported);
    assert_eq!(string_list(&server.metadata()["protocol_versions"]), supported);
    assert_eq!(server.server_info()["name"], json!(SERVER_NAME));

    let reply = server.handle_request(request("initialize", json!({ "protocolVersion": LEGACY })));
    assert_eq!(reply["result"]["protocolVersion"], json!(MODERN));
    assert_eq!(reply["result"]["serverInfo"]["name"], json!(SERVER_NAME));
    assert_eq!(string_list(&reply["result"]["capabilities"]["protocol"]), supported);
}

#[test]
fn no_second_tool_table() {
    let mut server = server();
    let catalog_names = CommandCatalog::new().names();

    let reply = server.handle_request(request("tools/list", Value::Null));
    let listed: Vec<String> = reply["result"]["tools"]
        .as_array()
        .expect("tools/list returns tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect();
    assert_eq!(string_list(&server.capabilities()["tools"]), listed);

    let reply = server.handle_request(request("dtool.commands", Value::Null));
    let commands: Vec<String> = reply["result"]["tools"]
        .as_array()
        .expect("dtool.commands returns tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect();
    assert_eq!(commands, listed);

    assert_eq!(string_list(&server.metadata()["catalog"]), catalog_names);
    assert_eq!(listed.len(), 6);
    let unique: HashSet<&String> = listed.iter().collect();
    assert_eq!(unique.len(), listed.len());
}
