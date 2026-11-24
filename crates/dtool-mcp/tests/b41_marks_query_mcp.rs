use std::sync::Arc;
use dtool_core::workspace::{SharedServices, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{Value, json};

fn call(server: &mut Server, name: &str, params: Value) -> Value {
    server.handle_request(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": params}}))
}

#[test]
fn text_marks_query_over_mcp() {
    assert!(dtool_text::canvas_text::install());
    let mut workspace = Workspace::new(Arc::new(SharedServices::default()));
    workspace.new_document("marks");
    let mut server = Server::new(Config::default(), workspace);
    let added = call(&mut server, "text.art.add", json!({"pos": [20.0, 30.0], "text": "aaaa bbbb cccc"}));
    let result = |reply: &Value| reply.get("result").and_then(|r| r.get("structuredContent")).or_else(|| reply.get("result")).cloned().unwrap_or_else(|| panic!("{reply}"));
    let id = result(&added)["id"].as_u64().unwrap_or_else(|| panic!("{added}"));
    let object = call(&mut server, "doc.object.query", json!({"id": id}));
    let story = result(&object)["story_id"].as_u64().unwrap_or_else(|| panic!("{object}"));
    call(&mut server, "text.special.insert", json!({"story": story, "at": 4, "what": "column_break"}));
    let reply = call(&mut server, "text.marks.query", json!({"id": id}));
    let marks = result(&reply)["marks"].clone();
    assert_eq!(marks.as_array().map(Vec::len), Some(1), "{reply}");
    assert_eq!((marks[0]["kind"].clone(), marks[0]["offset"].clone()), (json!("Column"), json!(4)));
    let bad = call(&mut server, "text.marks.query", json!({"id": 9_999}));
    assert_eq!(bad["error"]["code"], json!("NOT_FOUND"), "{bad}");
}
