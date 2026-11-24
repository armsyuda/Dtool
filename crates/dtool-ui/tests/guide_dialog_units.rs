use dtool_core::document::{Document, Units};
use dtool_core::guides::GuideKind;
use dtool_core::Session;
use dtool_ui::guides_dialog::{add_guide, guide_rows, rename_guide, update_guide};
use serde_json::json;

fn session(units: Units) -> Session {
    let mut session = Session::new(Document::new([400.0, 300.0], units, 96.0, 1));
    session.has_document = true;
    session
}

fn positions(session: &Session) -> Vec<f32> {
    session.guides.list().iter().map(|guide| guide.position).collect()
}

#[test]
fn dialog_add_and_update_accept_unit_positions_in_px() {
    let mut s = session(Units::Px);
    let id = add_guide(&mut s, GuideKind::Vertical, 120.0, "a").unwrap();
    assert_eq!(positions(&s), vec![120.0]);
    let mut row = guide_rows(&s)[0].clone();
    row.position = 80.0;
    update_guide(&mut s, &row).unwrap();
    assert_eq!(positions(&s), vec![80.0]);
    rename_guide(&mut s, id, "b").unwrap();
    assert_eq!(s.guides.list()[0].name.as_deref(), Some("b"));
    s.run_named("edit.undo", json!({})).unwrap();
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(positions(&s), vec![120.0]);
}

#[test]
fn dialog_add_and_update_keep_document_units_in_mm() {
    let mut s = session(Units::Mm);
    add_guide(&mut s, GuideKind::Horizontal, 10.0, "").unwrap();
    assert!((positions(&s)[0] - 10.0).abs() < 1e-3, "{:?}", positions(&s));
    let mut row = guide_rows(&s)[0].clone();
    row.position = 25.0;
    update_guide(&mut s, &row).unwrap();
    assert!((positions(&s)[0] - 25.0).abs() < 1e-3, "{:?}", positions(&s));
}