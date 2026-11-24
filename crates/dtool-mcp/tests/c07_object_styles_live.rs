use dtool_core::object::ObjectId;
use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    let reply = server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }));
    assert!(reply.get("error").is_none(), "{name}: {reply}");
    reply["result"].clone()
}

fn opacity(server: &Server, scope: u64, id: u64) -> f32 {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    ws.get(ContextId(scope)).unwrap().session.document.get_object(ObjectId(id)).unwrap().opacity
}

fn names(server: &mut Server, scope: u64) -> Vec<String> {
    run(server, scope, "style.list", json!({}))["styles"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn c07_object_styles_route_through_workspace_on_live_document() {
    let mut workspace = Workspace::default();
    let d = workspace.new_document("styles").0;
    let mut s = Server::new(Config::default(), workspace);
    run(&mut s, d, "object.create", json!({ "name": "src", "pos": [0.0, 0.0], "size": [20.0, 20.0] }));
    run(&mut s, d, "object.create", json!({ "name": "dst", "pos": [40.0, 0.0], "size": [20.0, 20.0] }));
    let ids: Vec<u64> = {
        let ws = s.workspace();
        let ws = ws.lock().unwrap();
        ws.get(ContextId(d)).unwrap().session.document.objects.iter().map(|o| o.id.0).collect()
    };
    let (src, dst) = (ids[ids.len() - 2], ids[ids.len() - 1]);
    run(&mut s, d, "selection.set", json!({ "ids": [src] }));
    {
        let ws = s.workspace();
        let mut ws = ws.lock().unwrap();
        ws.get_mut(ContextId(d)).unwrap().session.document.find_object_mut(src).unwrap().opacity = 0.25;
    }
    let created = run(&mut s, d, "style.create", json!({ "id": src, "name": "Faint", "category": "Soft" }));
    let style_id = created["style_id"].clone();
    assert_eq!(names(&mut s, d), vec!["Faint"]);
    run(&mut s, d, "style.rename", json!({ "style_id": style_id, "name": "Ghost" }));
    assert_eq!(names(&mut s, d), vec!["Ghost"]);
    let applied = run(&mut s, d, "style.apply", json!({ "style_id": style_id, "ids": [dst] }));
    assert_eq!(applied["applied_count"], 1, "{applied}");
    assert!((opacity(&s, d, dst) - 0.25).abs() < 1e-6);
    run(&mut s, d, "edit.undo", json!({}));
    assert!((opacity(&s, d, dst) - 1.0).abs() < 1e-6, "apply undoes");
    run(&mut s, d, "style.delete", json!({ "style_id": style_id }));
    assert!(names(&mut s, d).is_empty());
    run(&mut s, d, "edit.undo", json!({}));
    assert_eq!(names(&mut s, d), vec!["Ghost"], "delete undoes");
    run(&mut s, d, "edit.undo", json!({}));
    assert_eq!(names(&mut s, d), vec!["Faint"], "rename undoes");
}