use dtool_ui::{Workspace, context_bar::{ContextEdit, close_pen_draft, PEN_CLOSE_CURVE}};
use serde_json::Value;

#[test]
fn preview_close_closes_draft_with_one_undo() {
    let mut workspace = Workspace::default();
    workspace.core.new_document("preview");
    let input = &mut workspace.canvas_input;
    for p in [[10.0, 10.0], [50.0, 10.0], [50.0, 50.0]] {
        input.pen.press(p, 1.0);
        input.pen.release();
    }
    let session = workspace.core.session_mut().unwrap();
    let edit = ContextEdit { id: PEN_CLOSE_CURVE.to_string(), value: Value::Null };
    assert!(close_pen_draft(&edit, input, session));
    assert!(!input.pen.active());
    assert_eq!(session.document.objects.len(), 1);
    assert_eq!(session.undo.applied_steps(), 1);
    assert!(session.document.objects[0].curve().unwrap().contour.closed);
}
