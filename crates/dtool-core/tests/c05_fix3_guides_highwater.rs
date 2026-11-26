use dtool_core::document::{Document, Units};
use dtool_core::guides::{GuideId, Guides};
use dtool_core::{Command, Session};
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1))
}

fn add(s: &mut Session, position: f32) -> u64 {
    s.run(&Command::with_params("guides.add", json!({"kind": "Vertical", "position": position}))).unwrap()["id"].as_u64().unwrap()
}

fn undo(s: &mut Session) {
    s.run(&Command::new("edit.undo")).unwrap();
}

fn ids(s: &Session) -> Vec<u64> {
    s.guides.items.iter().map(|g| g.id.0).collect()
}

#[test]
fn canvas_snapshot_undo_keeps_guide_high_water() {
    let mut s = session();
    assert_eq!(add(&mut s, 10.0), 1);
    assert_eq!(add(&mut s, 20.0), 2);
    s.run_named("doc.resize", json!({"w": 400, "h": 200, "dpi": 72})).unwrap();
    assert_eq!(add(&mut s, 30.0), 3);
    undo(&mut s);
    undo(&mut s);
    assert_eq!(ids(&s), vec![1, 2]);
    assert!(s.guides.next_guide_id >= 4);
    let fresh = add(&mut s, 40.0);
    assert_eq!(fresh, 4);
    assert_eq!(ids(&s), vec![1, 2, 4]);
    undo(&mut s);
    assert_eq!(ids(&s), vec![1, 2]);
    assert!(s.guides.get(GuideId(4)).is_none());
}

#[test]
fn canvas_snapshot_redo_does_not_lower_high_water() {
    let mut s = session();
    add(&mut s, 10.0);
    s.run_named("doc.resize", json!({"w": 400, "h": 200, "dpi": 72})).unwrap();
    undo(&mut s);
    s.run(&Command::new("edit.redo")).unwrap();
    assert_eq!(ids(&s), vec![1]);
    let position = s.guides.items[0].position;
    assert!((position - 20.0).abs() < 0.01);
    assert_eq!(add(&mut s, 50.0), 2);
    undo(&mut s);
    undo(&mut s);
    assert!((s.guides.items[0].position - 10.0).abs() < 0.01);
    assert_eq!(add(&mut s, 60.0), 3);
}

#[test]
fn replace_with_takes_the_larger_high_water() {
    let mut current = Guides::default();
    current.next_guide_id = 9;
    let mut restored = Guides::default();
    restored.items.push(dtool_core::guides::Guide { id: GuideId(12), kind: dtool_core::guides::GuideKind::Horizontal, position: 1.0, name: None, locked: false, page_id: None, master_id: None });
    restored.next_guide_id = 3;
    let mut a = current.clone();
    a.replace_with(restored.clone());
    assert_eq!(a.items, restored.items);
    assert_eq!(a.next_guide_id, 13);
    let mut b = Guides::default();
    b.next_guide_id = 20;
    b.replace_with(restored);
    assert_eq!(b.next_guide_id, 20);
    assert_eq!(b.next_id(), GuideId(20));
}
