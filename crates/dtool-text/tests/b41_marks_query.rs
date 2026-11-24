use std::sync::Arc;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::serde_json::{json, Value};
use dtool_core::text_lines::marks_query;
use dtool_core::{Command, ContextId, Scope, SharedServices, Workspace};

fn run(ws: &mut Workspace, ctx: ContextId, name: &str, params: Value) -> Result<Value, String> {
    ws.run(Scope::of(ctx), Command::with_params(name, params)).1.map_err(|e| e.code)
}

fn setup() -> (Workspace, ContextId, ObjectId) {
    assert!(dtool_text::canvas_text::install());
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let ctx = ws.new_document("marks");
    let reply = run(&mut ws, ctx, "text.art.add", json!({"pos": [20.0, 30.0], "text": "aaaa bbbb cccc dddd eeee ffff"})).unwrap();
    let id = ObjectId(reply["id"].as_u64().unwrap());
    let ObjectKind::ArtText { story, .. } = ws.get(ctx).unwrap().session.document.get_object(id).unwrap().kind else { panic!() };
    for (what, at) in [("column_break", 4), ("frame_break", 9), ("page_break", 14), ("odd_page_break", 19), ("even_page_break", 24)] {
        run(&mut ws, ctx, "text.special.insert", json!({"story": story.0, "at": at, "what": what})).unwrap();
    }
    (ws, ctx, id)
}

fn check(reply: &Value, id: ObjectId) {
    assert_eq!(reply["id"], json!(id.0));
    assert_eq!(reply["text"], json!(true));
    let marks = reply["marks"].as_array().unwrap();
    let kinds: Vec<(&str, u64)> = marks.iter().map(|m| (m["kind"].as_str().unwrap(), m["offset"].as_u64().unwrap())).collect();
    assert_eq!(kinds, vec![("Column", 4), ("Frame", 9), ("Page", 14), ("OddPage", 19), ("EvenPage", 24)]);
    for mark in marks {
        let pos = mark["page_pos"].as_array().unwrap();
        let (x, y) = (pos[0].as_f64().unwrap(), pos[1].as_f64().unwrap());
        assert!(x.is_finite() && y.is_finite() && x > 20.0 && y > 30.0, "{mark}");
        assert!(mark["size"].as_f64().unwrap() > 0.0);
    }
}

#[test]
fn marks_query_lists_every_break_with_offset_and_page_position() {
    let (ws, ctx, id) = setup();
    let doc = &ws.get(ctx).unwrap().session.document;
    check(&marks_query(doc, &json!({"id": id.0})).unwrap(), id);
    assert_eq!(marks_query(doc, &json!({})).unwrap_err().code, "INVALID_PARAMS");
    assert_eq!(marks_query(doc, &json!({"id": 0})).unwrap_err().code, "INVALID_PARAMS");
    assert_eq!(marks_query(doc, &json!({"id": 9_999})).unwrap_err().code, "NOT_FOUND");
}

#[test]
fn text_marks_query_through_workspace_run() {
    let (mut ws, ctx, id) = setup();
    let version = ws.get(ctx).unwrap().session.document.version;
    check(&run(&mut ws, ctx, "text.marks.query", json!({"id": id.0})).unwrap(), id);
    assert_eq!(ws.get(ctx).unwrap().session.document.version, version);
    assert_eq!(run(&mut ws, ctx, "text.marks.query", json!({"id": 9_999})).unwrap_err(), "NOT_FOUND");
    run(&mut ws, ctx, "edit.undo", json!({})).unwrap();
    assert_eq!(run(&mut ws, ctx, "text.marks.query", json!({"id": id.0})).unwrap()["marks"].as_array().unwrap().len(), 4);
}
