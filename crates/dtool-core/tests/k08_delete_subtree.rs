use dtool_core::document::{Document, Units};
use dtool_core::object::{Object, ObjectId};

#[test]
fn delete_group_removes_hidden_child_and_grandchild() {
    let mut doc = Document::new([400.0, 300.0], Units::Px, 96.0, 1);
    let group = doc.create_object(Object::default());
    let child = doc.create_object(Object { parent_id: Some(group), pickable: false, ..Object::default() });
    let grandchild = doc.create_object(Object { parent_id: Some(child), ..Object::default() });
    let other = doc.create_object(Object::default());
    doc.delete_object(group).unwrap();
    assert!(doc.get_object(group).is_none());
    assert!(doc.get_object(child).is_none());
    assert!(doc.get_object(grandchild).is_none());
    assert!(doc.get_object(other).is_some());
}

#[test]
fn delete_missing_object_is_not_found() {
    let mut doc = Document::new([400.0, 300.0], Units::Px, 96.0, 1);
    assert_eq!(doc.delete_object(ObjectId(99)).unwrap_err().code, "OBJECT_NOT_FOUND");
}

#[test]
fn delete_with_parent_cycle_terminates() {
    let mut doc = Document::new([400.0, 300.0], Units::Px, 96.0, 1);
    let a = doc.create_object(Object::default());
    let b = doc.create_object(Object { parent_id: Some(a), ..Object::default() });
    doc.objects.iter_mut().find(|o| o.id == a).unwrap().parent_id = Some(b);
    doc.delete_object(a).unwrap();
    assert!(doc.objects.is_empty());
}
