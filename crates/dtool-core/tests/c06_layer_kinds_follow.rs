use dtool_core::commands::layer_kinds::{follow_page_resize, is_page_layer};
use dtool_core::document::Units;
use dtool_core::object::{Object, ObjectId};
use dtool_core::{Command, Document, Session};
use serde_json::json;

#[test]
fn fill_layer_follows_page_resize() {
    let mut s = Session::new(Document::new([200.0, 100.0], Units::Px, 72.0, 1));
    let id = s.run(&Command::with_params("layer.fill.add", json!({}))).unwrap()["id"].as_u64().unwrap();
    let other = s.document.create_object(Object { name: "box".into(), pos: [0.0, 0.0], size: [200.0, 100.0], ..Object::default() });
    assert!(is_page_layer(s.document.get_object(ObjectId(id)).unwrap()));
    assert!(!is_page_layer(s.document.get_object(other).unwrap()));
    let old = dtool_core::scene::page_rects(&s.document);
    s.document.pages[0].size = [300.0, 150.0];
    assert_eq!(follow_page_resize(&mut s.document, &old), vec![ObjectId(id)]);
    assert_eq!(s.document.get_object(ObjectId(id)).unwrap().size, [300.0, 150.0]);
    assert_eq!(s.document.get_object(other).unwrap().size, [200.0, 100.0]);
}
