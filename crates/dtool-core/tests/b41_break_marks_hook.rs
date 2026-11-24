use dtool_core::document::{BreakKind, Units};
use dtool_core::object::ObjectId;
use dtool_core::text_lines::{register_text_break_marks, text_break_marks, BreakMark};
use dtool_core::Document;

fn stub(doc: &Document, id: ObjectId) -> Option<Vec<BreakMark>> {
    let object = doc.get_object(id)?;
    Some(vec![BreakMark { kind: BreakKind::Page, pos: 3, at: object.pos, size: 12.0 }])
}

#[test]
fn break_marks_hook_returns_provider_marks_and_nothing_for_unknown_objects() {
    let mut doc = Document::new([100.0, 100.0], Units::Px, 72.0, 1);
    let id = doc.create_object(dtool_core::Object { pos: [5.0, 6.0], size: [10.0, 10.0], ..Default::default() });
    assert!(text_break_marks(&doc, id).is_empty());
    assert!(register_text_break_marks(stub));
    assert!(register_text_break_marks(stub));
    assert_eq!(text_break_marks(&doc, id), vec![BreakMark { kind: BreakKind::Page, pos: 3, at: [5.0, 6.0], size: 12.0 }]);
    assert!(text_break_marks(&doc, ObjectId(9_999)).is_empty());
}
