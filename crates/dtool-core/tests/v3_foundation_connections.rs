use dtool_core::{Command, Document, ObjectId, Session};
use dtool_core::document::Units;
use dtool_core::resources::{ImageFormat, ImageResource};
use serde_json::{json, Value};

fn session() -> Session {
    Session::new(Document::new([400.0,300.0],Units::Px,72.0,1))
}

fn place(s:&mut Session)->u64 {
    s.run_named("image.place",json!({"pixel_size":[2,1],"rgba":[255,0,0,255,0,255,0,200],"pos":[10,20]})).unwrap()["id"].as_u64().unwrap()
}

#[test]
fn placement_connects_resource_object_pixels_and_one_undo() {
    let mut s=session();
    let source=ImageResource::new_embedded(ImageFormat::Png,[2,1],72.0,vec![10,20,30]);
    let result=s.run_named("image.place",json!({"source":source,"pixel_size":[2,1],"rgba":[255,0,0,255,0,255,0,200],"pos":[10,20]})).unwrap();
    let id=ObjectId(result["id"].as_u64().unwrap());
    assert_eq!(s.document.objects.len(),1);
    assert_eq!(s.document.placed_images.len(),1);
    assert_eq!(s.document.resources[0].data,Some(vec![10,20,30]));
    assert!(s.document.pixel_layers.contains(id));
    assert_eq!(s.selection.ids,vec![id]);
    s.run(&Command::new("edit.undo")).unwrap();
    assert!(s.document.objects.is_empty());
    assert!(s.document.resources.is_empty());
    assert!(s.document.pixel_layers.is_empty());
    assert!(s.selection.ids.is_empty());
    s.run(&Command::new("edit.redo")).unwrap();
    assert_eq!(s.document.objects[0].id,id);
    assert_eq!(s.selection.ids,vec![id]);
}

#[test]
fn image_size_uses_document_units_not_a_fixed_pixel_scale() {
    let mut s=session();
    s.document.dpi=144.0;
    let id=place(&mut s);
    assert_eq!(s.document.get_object(ObjectId(id)).unwrap().size,[4.0,2.0]);
    s.document.units=Units::Mm;
    let id=place(&mut s);
    let size=s.document.get_object(ObjectId(id)).unwrap().size;
    assert_eq!(size,[4.0,2.0]);
}

#[test]
fn invalid_image_does_not_leave_a_resource_or_history_change() {
    let mut s=session();
    assert!(s.run_named("image.place",json!({"pixel_size":[2,1],"rgba":[1,2,3],"pos":[0,0]})).is_err());
    assert!(s.document.resources.is_empty());
    assert!(s.document.objects.is_empty());
    assert!(!s.undo.can_undo());
    assert_eq!(s.generation,0);
}

#[test]
fn undo_does_not_reuse_an_allocated_object_id() {
    let mut s=session();
    let first=place(&mut s);
    s.run(&Command::new("edit.undo")).unwrap();
    assert!(place(&mut s)>first);
}

#[test]
fn mask_is_separate_from_pixels_and_survives_native_round_trip() {
    let mut s=session();
    let id=place(&mut s);
    let raw=s.document.pixel_layers.get(ObjectId(id)).unwrap().to_rgba();
    s.run_named("layer.mask.set",json!({"id":id,"mask":{"width":2,"height":1,"coverage":[0,128]}})).unwrap();
    let object=s.document.get_object(ObjectId(id)).unwrap();
    let (_,rgba)=dtool_core::raster_source::rgba(&s.document,object).unwrap();
    assert_eq!(rgba[3],0);
    assert_eq!(rgba[7],100);
    assert_eq!(s.document.pixel_layers.get(ObjectId(id)).unwrap().to_rgba(),raw);
    let reopened:Document=serde_json::from_value(serde_json::to_value(&s.document).unwrap()).unwrap();
    assert_eq!(reopened.layer_masks,s.document.layer_masks);
    s.run(&Command::new("edit.undo")).unwrap();
    assert!(s.document.layer_masks.is_empty());
    s.run(&Command::new("edit.redo")).unwrap();
    assert_eq!(s.document.layer_masks.len(),1);
}

#[test]
fn bad_mask_is_atomic_and_a_pixel_edit_is_read_from_the_ledger() {
    let mut s=session();
    let id=place(&mut s);
    let generation=s.generation;
    assert!(s.run_named("layer.mask.set",json!({"id":id,"mask":{"width":1,"height":1,"coverage":[255]}})).is_err());
    assert_eq!(s.generation,generation);
    assert!(s.document.layer_masks.is_empty());
    s.document.pixel_layers.get_mut(ObjectId(id)).unwrap().set(0,0,[1,2,3,4]);
    let object=s.document.get_object(ObjectId(id)).unwrap();
    let (_,rgba)=dtool_core::raster_source::rgba(&s.document,object).unwrap();
    assert_eq!(&rgba[..4],&[1,2,3,4]);
}

#[test]
fn deleting_an_image_and_undo_keeps_all_sidecars_together() {
    let mut s=session();
    let id=place(&mut s);
    s.run_named("layer.mask.set",json!({"id":id,"mask":{"width":2,"height":1,"coverage":[255,0]}})).unwrap();
    s.run_named("object.delete",json!({"id":id})).unwrap();
    assert!(s.document.layer_masks.is_empty());
    assert!(s.document.placed_images.is_empty());
    assert!(s.document.pixel_layers.is_empty());
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(s.document.layer_masks.len(),1);
    assert_eq!(s.document.placed_images.len(),1);
    assert!(s.document.pixel_layers.contains(ObjectId(id)));
}

#[test]
fn table_selection_does_not_dirty_document_and_invalid_cells_do_not_change_it() {
    let mut s=session();
    let id=s.run_named("table.create",json!({"rows":2,"cols":2,"insert":true})).unwrap()["id"].as_u64().unwrap();
    let version=s.document.version;
    let result=s.run_named("table.selection.set",json!({"id":id,"mode":"rows","rows":[0]})).unwrap();
    assert_eq!(result["cells"],json!(["c1","c2"]));
    assert_eq!(s.document.version,version);
    let before=s.table_selection.clone();
    assert!(s.run_named("table.selection.set",json!({"id":id,"cells":["c999"]})).is_err());
    assert_eq!(s.table_selection,before);
    let query=s.run_named("table.query",json!({"id":id})).unwrap();
    assert_eq!(query["generation"],json!(s.generation));
    assert_eq!(query["document_version"],json!(s.document.version));
}

#[test]
fn deleted_table_cannot_leave_a_selected_cell_in_queries() {
    let mut s=session();
    let id=s.run_named("table.create",json!({"rows":1,"cols":1,"insert":true})).unwrap()["id"].as_u64().unwrap();
    s.run_named("table.selection.set",json!({"id":id,"mode":"all"})).unwrap();
    s.run_named("object.delete",json!({"id":id})).unwrap();
    let result=s.run_named("table.selection.query",Value::Null).unwrap();
    assert_eq!(result["table"],Value::Null);
    assert_eq!(result["cells"],json!([]));
}

#[test]
fn old_native_documents_without_masks_remain_readable() {
    let mut value=serde_json::to_value(Document::default()).unwrap();
    value.as_object_mut().unwrap().remove("layer_masks");
    let document:Document=serde_json::from_value(value).unwrap();
    assert!(document.layer_masks.is_empty());
}

#[test]
fn deleting_a_masked_image_undo_and_redo_preserves_id_and_high_water() {
    let mut s = session();
    let id = place(&mut s);
    s.run_named("layer.mask.set", json!({"id":id,"mask":{"width":2,"height":1,"coverage":[255,0]}})).unwrap();
    let coverage = s.document.layer_masks[0].coverage.coverage.clone();
    let pixels = s.document.pixel_layers.get(ObjectId(id)).unwrap().to_rgba();
    let high = s.document.next_object_id;
    let resource_high = s.document.next_resource_id;
    let generation = s.generation;
    s.run_named("object.delete", json!({"id":id})).unwrap();
    assert!(s.document.get_object(ObjectId(id)).is_none());
    assert!(s.document.layer_masks.is_empty());
    assert!(!s.document.pixel_layers.contains(ObjectId(id)));
    assert!(s.document.resources.is_empty());
    assert!(s.document.next_object_id >= high);
    assert!(s.document.next_resource_id >= resource_high);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(s.document.objects[0].id, ObjectId(id));
    assert_eq!(s.document.layer_masks[0].coverage.coverage, coverage);
    assert_eq!(s.document.pixel_layers.get(ObjectId(id)).unwrap().to_rgba(), pixels);
    assert_eq!(s.document.placed_images.len(), 1);
    assert_eq!(s.document.resources.len(), 1);
    assert!(s.document.next_object_id >= high);
    assert!(s.document.next_resource_id >= resource_high);
    assert!(s.generation > generation);
    s.run(&Command::new("edit.redo")).unwrap();
    assert!(s.document.get_object(ObjectId(id)).is_none());
    assert!(s.document.layer_masks.is_empty());
    assert!(!s.document.pixel_layers.contains(ObjectId(id)));
    assert!(s.document.resources.is_empty());
    assert!(s.document.next_object_id >= high);
    assert!(s.document.next_resource_id >= resource_high);
}
