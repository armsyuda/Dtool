use dtool_core::workspace::{ContextId, Workspace};
use dtool_mcp::{Config, Server};
use serde_json::{json, Value};

fn run(server: &mut Server, scope: u64, name: &str, params: Value) -> Value {
    server.handle_request(json!({ "jsonrpc": "2.0", "id": 1, "method": "dtool.run", "params": { "name": name, "scope": scope, "params": params } }))
}

fn ok(reply: Value) -> Value {
    assert!(reply.get("error").is_none(), "{reply}");
    reply
}

fn objects(server: &Server, scope: u64) -> Vec<(String, f64)> {
    let ws = server.workspace();
    let ws = ws.lock().unwrap();
    ws.get(ContextId(scope)).unwrap().session.document.objects.iter().map(|o| (o.name.clone(), o.pos[0] as f64)).collect()
}

fn setup() -> (Server, u64, u64) {
    let mut workspace = Workspace::default();
    let a = workspace.new_document("a").0;
    let b = workspace.new_document("b").0;
    workspace.activate(ContextId(b)).unwrap();
    (Server::new(Config::default(), workspace), a, b)
}

fn create(name: &str, x: f64) -> Value {
    json!({ "name": name, "pos": [x, 0.0], "size": [10.0, 10.0] })
}

#[test]
fn mcp_record_on_a_then_play_recorded_steps_acts_on_live_document_with_one_undo() {
    let (mut s, a, b) = setup();
    ok(run(&mut s, a, "macro.record.start", json!({})));
    ok(run(&mut s, a, "object.create", create("one", 10.0)));
    ok(run(&mut s, a, "view.flags.query", json!({})));
    ok(run(&mut s, a, "object.create", create("two", 40.0)));
    ok(run(&mut s, b, "object.create", create("b-only", 90.0)));
    let stopped = ok(run(&mut s, a, "macro.record.stop", json!({})));
    let text = stopped.to_string();
    assert!(text.contains("\"one\"") && text.contains("\"two\"") && !text.contains("view.flags.query") && !text.contains("b-only"), "{text}");
    ok(run(&mut s, a, "macro.play", json!({})));
    assert_eq!(objects(&s, a), vec![("one".into(), 10.0), ("two".into(), 40.0), ("one".into(), 10.0), ("two".into(), 40.0)]);
    ok(run(&mut s, a, "edit.undo", json!({})));
    assert_eq!(objects(&s, a), vec![("one".into(), 10.0), ("two".into(), 40.0)]);
    assert_eq!(objects(&s, b), vec![("b-only".into(), 90.0)]);
}

#[test]
fn mcp_play_explicit_steps_on_other_document_and_refuse_nested_macro() {
    let (mut s, a, b) = setup();
    let steps = json!([
        { "command": "object.create", "params": create("x", 1.0), "enabled": true, "show_name": false },
        { "command": "object.create", "params": create("off", 2.0), "enabled": false, "show_name": false }
    ]);
    ok(run(&mut s, b, "macro.play", json!({ "steps": steps })));
    assert_eq!(objects(&s, b), vec![("x".into(), 1.0)]);
    assert!(objects(&s, a).is_empty());
    let nested = json!([{ "command": "macro.play", "params": {}, "enabled": true, "show_name": false }]);
    let reply = run(&mut s, b, "macro.play", json!({ "steps": nested }));
    assert!(reply.get("error").is_some(), "{reply}");
    assert!(reply.to_string().contains("MACRO_STEP_NOT_ALLOWED"), "{reply}");
    assert_eq!(objects(&s, b), vec![("x".into(), 1.0)]);
    let empty = run(&mut s, a, "macro.play", json!({}));
    assert!(empty.to_string().contains("MACRO_EMPTY"), "{empty}");
}

#[test]
fn recorded_refs_replay_on_another_document_atomically() {
    let (mut s, a, b) = setup();
    ok(run(&mut s, a, "object.create", create("source", 10.0)));
    ok(run(&mut s, b, "object.create", create("untouched", 30.0)));
    ok(run(&mut s, b, "object.create", create("destination", 50.0)));
    let (source, destination) = {
        let ws = s.workspace();
        let mut ws = ws.lock().unwrap();
        let src = ws.get_mut(ContextId(a)).unwrap();
        let source = src.session.document.objects.last().unwrap().id;
        src.session.selection.ids = vec![source];
        let dst = ws.get_mut(ContextId(b)).unwrap();
        let destination = dst.session.document.objects.last().unwrap().id;
        dst.session.selection.ids = vec![destination];
        (source.0, destination.0)
    };
    assert_ne!(source, destination);
    ok(run(&mut s, a, "macro.record.start", json!({})));
    ok(run(&mut s, a, "object.update", json!({"id": source, "name": "selected changed", "pos": [70.0, 0.0]})));
    ok(run(&mut s, a, "object.create", create("created", 80.0)));
    let created = {
        let ws = s.workspace();
        let ws = ws.lock().unwrap();
        ws.get(ContextId(a)).unwrap().session.document.objects.last().unwrap().id.0
    };
    ok(run(&mut s, a, "object.update", json!({"id": created, "name": "created changed", "pos": [90.0, 0.0]})));
    ok(run(&mut s, a, "macro.record.stop", json!({})));
    let steps = {
        let ws = s.workspace();
        let ws = ws.lock().unwrap();
        serde_json::to_value(dtool_export::macros::recorded_steps(&ws.get(ContextId(a)).unwrap().session)).unwrap()
    };
    assert_eq!(steps[0]["params"]["id"], "$sel:0");
    assert_eq!(steps[2]["params"]["id"], "$new:0");
    let before_a = objects(&s, a);
    let before_b = objects(&s, b);
    ok(run(&mut s, b, "macro.play", json!({"steps": steps})));
    assert_eq!(objects(&s, b), vec![("untouched".into(), 30.0), ("selected changed".into(), 70.0), ("created changed".into(), 90.0)]);
    assert_eq!(objects(&s, a), before_a);
    let after_b = objects(&s, b);
    ok(run(&mut s, b, "edit.undo", json!({})));
    assert_eq!(objects(&s, b), before_b, "one undo restores all replayed steps");
    ok(run(&mut s, b, "edit.redo", json!({})));
    assert_eq!(objects(&s, b), after_b);
}

#[test]
fn unresolved_macro_ref_rolls_back_prior_steps_and_preserves_redo() {
    let (mut s, a, _) = setup();
    ok(run(&mut s, a, "object.create", create("existing", 10.0)));
    ok(run(&mut s, a, "object.create", create("redo", 20.0)));
    ok(run(&mut s, a, "edit.undo", json!({})));
    let before = objects(&s, a);
    let reply = run(&mut s, a, "macro.play", json!({"steps": [
        {"command": "object.create", "params": create("rolled back", 40.0)},
        {"command": "object.update", "params": {"id": "$new:99", "name": "bad"}}
    ]}));
    assert!(reply.get("error").is_some(), "{reply}");
    assert!(reply.to_string().contains("MACRO_REF_UNRESOLVED"), "{reply}");
    assert_eq!(objects(&s, a), before);
    ok(run(&mut s, a, "edit.redo", json!({})));
    assert_eq!(objects(&s, a), vec![("existing".into(), 10.0), ("redo".into(), 20.0)]);
}
