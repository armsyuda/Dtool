
mod codex_protocol_compat {
    use dtool_core::Workspace;
    use dtool_mcp::{Config, Server, LEGACY, MODERN, SUPPORTED};
    use dtool_protocol::{ChangeName, Command, CommandCatalog, CommandError, CommandOutcome};
    use serde_json::{json, Value};

    fn server() -> Server {
        let mut workspace = Workspace::default();
        workspace.new_document("protocol-fixture");
        Server::new(Config::default(), workspace)
    }

    #[test]
    fn existing_b02_command_ids_are_preserved() {
        let catalog = CommandCatalog::new();
        for name in ["edit.undo", "edit.redo", "edit.batch", "doc.query", "commands.list"] {
            assert_eq!(catalog.get(name).map(|spec| spec.name), Some(name));
        }
    }

    #[test]
    fn canonical_error_codes_keep_message_key_compatibility() {
        for code in ["NOT_IMPLEMENTED", "UNKNOWN_COMMAND", "CONTEXT_NOT_FOUND", "NO_DOCUMENT", "INVALID_PARAMS", "BATCH_PREFLIGHT_FAILED", "AI_CONNECTION_DISABLED", "EDITOR_BUSY", "NEEDS_CONFIRMATION", "PORT_IN_USE", "PATH_NOT_ALLOWED", "REQUEST_TOO_LARGE", "UNAUTHORIZED", "SHUTDOWN", "UI_TIMEOUT", "NO_WINDOW", "CONTEXT_NOT_ACTIVE"] {
            let error = CommandError::new(code);
            assert_eq!(error.code, code);
            assert_eq!(error.message_key, format!("error.{}", code.to_lowercase()));
            let encoded = serde_json::to_value(&error).unwrap();
            assert_eq!(encoded["code"], code);
            assert_eq!(encoded["message_key"], error.message_key);
        }
    }

    #[test]
    fn command_and_outcome_wire_fields_are_preserved() {
        let command: Command = serde_json::from_value(json!({ "name": "doc.query" })).unwrap();
        assert_eq!(command.name, "doc.query");
        assert!(command.params.is_null());
        let outcome = CommandOutcome { doc_version: 7, changes: vec![ChangeName { name: "object.create".into() }], payload: json!({ "id": 9 }) };
        let encoded = serde_json::to_value(outcome).unwrap();
        assert_eq!(encoded, json!({ "doc_version": 7, "changes": [{ "name": "object.create" }], "payload": { "id": 9 } }));
    }

    #[test]
    fn protocol_versions_and_tool_ids_are_stable() {
        assert_eq!(MODERN, "2026-07-28");
        assert_eq!(LEGACY, "2025-11-25");
        assert_eq!(SUPPORTED, &[MODERN, LEGACY]);
        let capabilities = server().capabilities();
        assert_eq!(capabilities["tools"], json!(["dtool.commands", "dtool.schema", "dtool.run", "dtool.preview", "dtool.query", "dtool.undo"]));
    }

    #[test]
    fn schema_compatibility_uses_the_real_catalog_entry() {
        let server = server();
        let schema = server.schema_for_command("doc.query").unwrap();
        assert_eq!(schema["name"], "doc.query");
        assert_eq!(schema["inputSchema"]["type"], "object");
        assert_eq!(schema["inputSchema"]["properties"]["scope"]["type"], json!(["integer", "string", "object"]));
        let error = server.schema_for_command("unknown.audit.command").unwrap_err();
        assert_eq!(error["code"], "UNKNOWN_COMMAND");
        assert_eq!(error["message_key"], "error.unknown_command");
    }

    #[test]
    fn reusable_scope_source_audit_does_not_change_workspace_behavior() {
        let mut server = server();
        let workspace = server.workspace();
        let before: Value = {
            let workspace = workspace.lock().unwrap();
            let context = workspace.get(workspace.active().unwrap()).unwrap();
            json!({ "id": context.id.0, "document": context.session.document, "generation": context.session.generation, "selection": context.session.selection.ids })
        };
        for (id, method, params) in [(1, "initialize", json!({})), (2, "dtool.schema", json!({ "name": "doc.query" })), (3, "dtool.commands", json!({}))] {
            let response = server.handle_request(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
            assert!(response.get("error").is_none());
        }
        let workspace = workspace.lock().unwrap();
        let context = workspace.get(workspace.active().unwrap()).unwrap();
        let after = json!({ "id": context.id.0, "document": context.session.document, "generation": context.session.generation, "selection": context.session.selection.ids });
        assert_eq!(after, before);
    }
}
