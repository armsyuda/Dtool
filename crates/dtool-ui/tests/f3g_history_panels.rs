use dtool_core::document::{Document, Units};
use dtool_core::Session;
use dtool_ui::dock::PanelId;
use dtool_ui::{history_panel, snapshots_panel};
use serde_json::json;

fn session() -> Session { Session::new(Document::new([100.0, 100.0], Units::Px, 96.0, 1)) }

fn create(session: &mut Session, name: &str) {
    session.run_named("object.create", json!({"name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
}

#[test]
fn history_rows_follow_commands_and_jump() {
    let mut s = session();
    assert!(history_panel::rows(&mut s).is_empty());
    for n in ["a", "b", "c"] { create(&mut s, n); }
    let rows = history_panel::rows(&mut s);
    assert_eq!(rows.len(), 3);
    assert!(rows[2].current);
    assert!(history_panel::jump(&mut s, 0));
    assert_eq!(s.document.objects.len(), 1);
    let rows = history_panel::rows(&mut s);
    assert!(rows[0].current && !rows[2].current);
    assert!(!history_panel::jump(&mut s, 9));
}

#[test]
fn snapshot_rows_create_and_restore() {
    let mut s = session();
    create(&mut s, "a");
    let id = snapshots_panel::create(&mut s).unwrap();
    create(&mut s, "b");
    assert_eq!(snapshots_panel::rows(&mut s).len(), 1);
    assert!(snapshots_panel::restore(&mut s, id));
    assert_eq!(s.document.objects.len(), 1);
    assert!(!snapshots_panel::restore(&mut s, id + 99));
}

#[test]
fn snapshots_panel_is_a_known_dock_panel() {
    assert!(dtool_ui::panel_ids::all_panel_ids().contains(&PanelId::Other(dtool_ui::panels::SNAPSHOTS_PANEL_KEY.into())));
    assert!(dtool_strings::has("panel.snapshots"));
}
