use dtool_core::workspace::Workspace;
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, name: &str, params: Value) -> Value {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "params": params } }));
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply["result"].clone()
}

fn query(server: &mut Server, story: u64, start: u64, end: u64) -> Value {
    run(server, "text.style.query", json!({ "story": story, "range": { "start": start, "end": end } }))
}

#[test]
fn mcp_text_bar_style_edits_live_document_and_undoes() {
    let mut workspace = Workspace::default();
    let doc = workspace.new_document("c10-textbar");
    workspace.activate(doc).unwrap();
    let mut server = Server::new(Config::default(), workspace);
    let added = run(&mut server, "text.art.add", json!({ "pos": [10.0, 10.0], "text": "Hello world" }));
    let story = added["story"].as_u64().expect("story");
    run(&mut server, "text.style", json!({ "story": story, "range": { "start": 0, "end": 5 }, "patch": { "family": "Inter", "size_pt": 12.0, "weight": 400 } }));
    run(&mut server, "text.style", json!({ "story": story, "range": { "start": 5, "end": 11 }, "patch": { "family": "Pretendard", "size_pt": 18.0, "weight": 700, "italic": true, "tracking": 50.0 } }));
    let mixed = query(&mut server, story, 0, 11);
    assert_eq!(mixed["family"], json!("Mixed"), "{mixed}");
    assert_eq!(mixed["size_pt"], json!("Mixed"), "{mixed}");
    assert_eq!(mixed["weight"], json!("Mixed"), "{mixed}");
    assert_eq!(mixed["italic"], json!("Mixed"), "{mixed}");
    assert_eq!(mixed["tracking"], json!("Mixed"), "{mixed}");
    let first = query(&mut server, story, 0, 5);
    assert_eq!(first["family"], json!("Inter"));
    assert_eq!(first["italic"], json!(false));
    assert_eq!(first["tracking"], json!(0.0));
    run(&mut server, "text.style", json!({ "story": story, "range": { "start": 0, "end": 11 }, "patch": { "size_pt": 24.0 } }));
    let after = query(&mut server, story, 0, 11);
    assert_eq!(after["size_pt"], json!(24.0));
    assert_eq!(after["family"], json!("Mixed"));
    run(&mut server, "text.para.set", json!({ "story": story, "range": "all", "patch": { "align": "Center", "leading": 20.0 } }));
    let para = query(&mut server, story, 0, 11);
    assert_eq!(para["align"], json!("Center"), "{para}");
    assert_eq!(para["leading"], json!(20.0), "{para}");
    let undo = server.handle_request(json!({ "jsonrpc": "2.0", "id": 2, "method": "dtool.undo", "params": {} }));
    assert!(undo.get("error").is_none(), "{undo}");
    let undone_para = query(&mut server, story, 0, 11);
    assert_ne!(undone_para["leading"], json!(20.0), "{undone_para}");
    let undo_size = server.handle_request(json!({ "jsonrpc": "2.0", "id": 3, "method": "dtool.undo", "params": {} }));
    assert!(undo_size.get("error").is_none(), "{undo_size}");
    assert_eq!(query(&mut server, story, 0, 11)["size_pt"], json!("Mixed"));
    assert_eq!(query(&mut server, story, 0, 5)["family"], json!("Inter"));
    assert_eq!(query(&mut server, story, 5, 11)["family"], json!("Pretendard"));
}
