use dtool_core::document::{Document, Units};
use dtool_core::guides::GuideKind;
use dtool_core::Session;
use dtool_ui::guides_dialog::{guide_rows, GuidesDialog};
use serde_json::json;

fn session_with_guide() -> Session {
    let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1));
    session.has_document = true;
    session.run_named("guides.add", json!({ "kind": GuideKind::Horizontal, "position": 100.0 })).unwrap();
    session
}

fn positions(session: &Session) -> Vec<f32> {
    session.guides.list().iter().map(|guide| guide.position).collect()
}

#[test]
fn drag_previews_without_touching_the_document() {
    let session = session_with_guide();
    let mut dialog = GuidesDialog::default();
    dialog.open();
    let mut row = guide_rows(&session)[0].clone();
    row.position = 140.0;
    dialog.begin_preview(row.clone());
    row.position = 160.0;
    dialog.begin_preview(row);
    assert_eq!(positions(&session), vec![100.0]);
    assert_eq!(dialog.preview_line(), Some((GuideKind::Horizontal, 160.0)));
}

#[test]
fn release_commits_one_undo_step() {
    let mut session = session_with_guide();
    let mut dialog = GuidesDialog::default();
    dialog.open();
    let mut row = guide_rows(&session)[0].clone();
    for step in 1..=5 {
        row.position = 100.0 + f64::from(step) * 10.0;
        dialog.begin_preview(row.clone());
    }
    dialog.finish_preview(&mut session, &row).unwrap();
    assert!(dialog.preview.is_none());
    assert_eq!(positions(&session), vec![150.0]);
    session.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(positions(&session), vec![100.0]);
}

#[test]
fn escape_mid_drag_leaves_document_and_undo_unchanged() {
    let mut session = session_with_guide();
    let mut dialog = GuidesDialog::default();
    dialog.open();
    let mut row = guide_rows(&session)[0].clone();
    row.position = 180.0;
    dialog.begin_preview(row.clone());
    dialog.cancel_preview();
    dialog.begin_preview(row.clone());
    assert!(dialog.preview.is_none());
    dialog.finish_preview(&mut session, &row).unwrap();
    assert_eq!(positions(&session), vec![100.0]);
    session.run_named("edit.undo", json!({})).unwrap();
    assert!(positions(&session).is_empty());
}
