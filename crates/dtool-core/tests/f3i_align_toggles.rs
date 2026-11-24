#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::document::{Document, Units};
use dtool_core::{ObjectId, Session};
use serde_json::json;

fn session() -> Session {
    Session::new(Document::new([200.0, 100.0], Units::Px, 96.0, 1))
}

fn create(s: &mut Session, pos: [f32; 2], size: [f32; 2]) -> ObjectId {
    let id = s.document.next_object_id;
    s.run_named("object.create", json!({"name": "o", "pos": pos, "size": size})).unwrap();
    ObjectId(id)
}

fn pos(s: &Session, id: ObjectId) -> [f32; 2] {
    s.document.get_object(id).unwrap().pos
}

fn three(s: &mut Session) -> [ObjectId; 3] {
    let a = create(s, [10.0, 10.0], [10.0, 10.0]);
    let b = create(s, [50.0, 30.0], [20.0, 20.0]);
    let c = create(s, [100.0, 60.0], [10.0, 30.0]);
    s.selection.ids = vec![a, b, c];
    [a, b, c]
}

#[test]
fn align_selection_edges_and_undo() {
    let _home = test_home::enter("align_selection_edges_and_undo");
    let mut s = session();
    let [a, b, c] = three(&mut s);
    s.run_named("layer.arrange.align.left", json!({})).unwrap();
    assert_eq!([pos(&s, a)[0], pos(&s, b)[0], pos(&s, c)[0]], [10.0, 10.0, 10.0]);
    assert_eq!(pos(&s, c)[1], 60.0);
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(pos(&s, c), [100.0, 60.0]);
    s.run_named("edit.redo", json!({})).unwrap();
    assert_eq!(pos(&s, c), [10.0, 60.0]);
    s.run_named("layer.arrange.align.bottom", json!({})).unwrap();
    assert_eq!([pos(&s, a)[1], pos(&s, b)[1], pos(&s, c)[1]], [80.0, 70.0, 60.0]);
    s.run_named("layer.arrange.align.centre", json!({})).unwrap();
    assert_eq!(pos(&s, b)[0], 10.0);
    s.run_named("layer.arrange.align.middle", json!({})).unwrap();
    assert_eq!(pos(&s, a)[1], 70.0);
    s.run_named("layer.arrange.align.right", json!({})).unwrap();
    s.run_named("layer.arrange.align.top", json!({})).unwrap();
    assert_eq!(pos(&s, a), [20.0, 60.0]);
}

#[test]
fn align_spread_and_key() {
    let _home = test_home::enter("align_spread_and_key");
    let mut s = session();
    let [a, b, c] = three(&mut s);
    s.run_named("layer.arrange.align.right", json!({"relative_to": "spread", "ids": [a.0]})).unwrap();
    assert_eq!(pos(&s, a), [190.0, 10.0]);
    s.run_named("layer.arrange.align.top", json!({"relative_to": "key", "key_id": b.0})).unwrap();
    assert_eq!([pos(&s, a)[1], pos(&s, b)[1], pos(&s, c)[1]], [30.0, 30.0, 30.0]);
    assert_eq!(pos(&s, b), [50.0, 30.0]);
    s.run_named("layer.arrange.align.left", json!({"relative_to": "key"})).unwrap();
    assert_eq!(pos(&s, b)[0], 190.0);
}

#[test]
fn distribute_selection_and_spread() {
    let _home = test_home::enter("distribute_selection_and_spread");
    let mut s = session();
    let [a, b, c] = three(&mut s);
    s.run_named("layer.arrange.distribute.h", json!({})).unwrap();
    assert_eq!([pos(&s, a)[0], pos(&s, b)[0], pos(&s, c)[0]], [10.0, 50.0, 100.0]);
    s.document.objects.iter_mut().find(|o| o.id == b).unwrap().pos = [20.0, 30.0];
    s.run_named("layer.arrange.distribute.h", json!({})).unwrap();
    assert_eq!(pos(&s, b)[0], 50.0);
    s.run_named("layer.arrange.distribute.v", json!({"relative_to": "spread"})).unwrap();
    assert_eq!([pos(&s, a)[1], pos(&s, b)[1], pos(&s, c)[1]], [0.0, 30.0, 70.0]);
}

#[test]
fn align_rejects_bad_input() {
    let _home = test_home::enter("align_rejects_bad_input");
    let mut s = session();
    let a = create(&mut s, [0.0, 0.0], [5.0, 5.0]);
    s.selection.ids = vec![a];
    assert!(s.run_named("layer.arrange.align.left", json!({})).is_err());
    assert!(s.run_named("layer.arrange.distribute.h", json!({})).is_err());
    assert!(s.run_named("layer.arrange.align.diagonal", json!({})).is_err());
    assert!(s.run_named("layer.arrange.align.left", json!({"relative_to": "page"})).is_err());
    s.selection.ids.clear();
    assert!(s.run_named("layer.arrange.align.left", json!({"relative_to": "spread"})).is_err());
    assert!(s.run_named("layer.arrange.align.left", json!({"relative_to": "spread", "ids": [a.0]})).is_ok());
}

#[test]
fn toggles_flip_state() {
    let _home = test_home::enter("toggles_flip_state");
    let mut s = session();
    let ruler = s.document.rulers.show;
    s.run_named("ruler.toggle", json!({})).unwrap();
    assert_eq!(s.document.rulers.show, !ruler);
    s.run_named("ruler.toggle", json!({})).unwrap();
    assert_eq!(s.document.rulers.show, ruler);
    let guides = s.guides.show;
    s.run_named("guides.toggle", json!({})).unwrap();
    assert_eq!(s.guides.show, !guides);
    let path = std::env::temp_dir().join(format!("dtool_f3i_snap_{}", std::process::id())).join("snap_prefs.json");
    s.snap_prefs = dtool_core::snap_prefs::SnapPrefsService::with_path(path.clone());
    let snap = s.snap_prefs.query().enabled;
    let out = s.run_named("snap.toggle", json!({})).unwrap();
    assert_eq!(s.snap_prefs.query().enabled, !snap);
    assert_eq!(out["changes"][0]["name"], "snap.toggle");
    s.run_named("snap.toggle", json!({})).unwrap();
    assert_eq!(s.snap_prefs.query().enabled, snap);
    assert_eq!(dtool_core::snap_prefs::SnapPrefs::load(path).enabled, snap);
    assert!(s.run_named("ruler.toggle", json!({"show": true})).is_err());
}

#[test]
fn text_align_sets_story_paras_one_undo() {
    let _home = test_home::enter("text_align_sets_story_paras_one_undo");
    let mut s = session();
    s.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "one\ntwo"})).unwrap();
    let text_id = s.document.objects.last().unwrap().id;
    let rect = create(&mut s, [0.0, 0.0], [5.0, 5.0]);
    s.selection.ids = vec![text_id, rect];
    let story = s.document.get_object(text_id).unwrap().story_id().unwrap();
    let before = s.document.stories.iter().find(|st| st.id == story).unwrap().paras.clone();
    s.run_named("text.align", json!({"align": "centre"})).unwrap();
    let paras = &s.document.stories.iter().find(|st| st.id == story).unwrap().paras;
    assert!(!paras.is_empty());
    assert!(paras.iter().all(|p| p.to_string().contains("\"Center\"")));
    s.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(s.document.stories.iter().find(|st| st.id == story).unwrap().paras, before);
    assert!(s.run_named("text.align", json!({"align": "diagonal"})).is_err());
    s.selection.ids = vec![rect];
    assert!(s.run_named("text.align", json!({"align": "left"})).is_err());
}
