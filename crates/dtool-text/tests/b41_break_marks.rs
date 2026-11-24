use std::sync::Arc;
use dtool_core::document::BreakKind;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::serde_json::{json, Value};
use dtool_core::text_lines::text_break_marks;
use dtool_core::{Command, ContextId, Scope, SharedServices, Workspace};

fn run(ws: &mut Workspace, ctx: ContextId, name: &str, params: Value) -> Value {
    ws.run(Scope::of(ctx), Command::with_params(name, params)).1.unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn story_of(ws: &Workspace, ctx: ContextId, id: ObjectId) -> u64 {
    match ws.get(ctx).unwrap().session.document.get_object(id).unwrap().kind {
        ObjectKind::ArtText { story } | ObjectKind::FrameText { story } => story.0,
        _ => panic!("not text"),
    }
}

fn setup(kind: &str) -> (Workspace, ContextId, ObjectId, u64) {
    assert!(dtool_text::canvas_text::install());
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let ctx = ws.new_document("breaks");
    let text = "aaaa bbbb cccc dddd eeee ffff";
    let reply = match kind {
        "art" => run(&mut ws, ctx, "text.art.add", json!({"pos": [20.0, 30.0], "text": text})),
        _ => run(&mut ws, ctx, "text.frame.add", json!({"rect": [20, 30, 400, 300], "text": text})),
    };
    let id = ObjectId(reply["id"].as_u64().or_else(|| reply["object_id"].as_u64()).unwrap_or_else(|| panic!("{reply}")));
    let story = story_of(&ws, ctx, id);
    (ws, ctx, id, story)
}

const BREAKS: [(&str, usize, BreakKind); 5] = [("column_break", 4, BreakKind::Column), ("frame_break", 9, BreakKind::Frame), ("page_break", 14, BreakKind::Page), ("odd_page_break", 19, BreakKind::OddPage), ("even_page_break", 24, BreakKind::EvenPage)];

#[test]
fn art_text_reports_a_mark_for_each_break_type_live() {
    let (mut ws, ctx, id, story) = setup("art");
    assert!(text_break_marks(&ws.get(ctx).unwrap().session.document, id).is_empty());
    for (what, at, _) in BREAKS {
        run(&mut ws, ctx, "text.special.insert", json!({"story": story, "at": at, "what": what}));
    }
    let doc = &ws.get(ctx).unwrap().session.document;
    let mut marks = text_break_marks(doc, id);
    marks.sort_by_key(|m| m.pos);
    assert_eq!(marks.iter().map(|m| (m.pos, m.kind)).collect::<Vec<_>>(), BREAKS.iter().map(|(_, at, kind)| (*at, *kind)).collect::<Vec<_>>());
    for mark in &marks {
        assert!(mark.at[0].is_finite() && mark.at[1].is_finite() && mark.at[0] > 20.0 && mark.at[1] > 30.0, "{mark:?}");
        assert!(mark.size > 0.0);
    }
    run(&mut ws, ctx, "edit.undo", json!({}));
    let after = text_break_marks(&ws.get(ctx).unwrap().session.document, id);
    assert_eq!(after.len(), 4);
    assert!(after.iter().all(|m| m.kind != BreakKind::EvenPage));
}

#[test]
fn frame_text_column_break_mark_sits_at_line_end_live() {
    let (mut ws, ctx, id, story) = setup("frame");
    run(&mut ws, ctx, "text.special.insert", json!({"story": story, "at": 9, "what": "column_break"}));
    let doc = &ws.get(ctx).unwrap().session.document;
    let marks = text_break_marks(doc, id);
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert_eq!((marks[0].kind, marks[0].pos), (BreakKind::Column, 9));
    assert!(marks[0].at[0] > 20.0 && marks[0].at[0] < 420.0 && marks[0].at[1] > 30.0 && marks[0].at[1] < 330.0, "{:?}", marks[0]);
    let other = doc.objects.iter().find(|o| o.id != id).map(|o| o.id).unwrap_or(ObjectId(9_999));
    assert!(text_break_marks(doc, other).is_empty());
}
