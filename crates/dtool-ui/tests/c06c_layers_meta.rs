use dtool_core::object::ObjectId;
use dtool_core::workspace::{Scope, SharedServices, Workspace};
use dtool_protocol::Command;
use dtool_ui::panels::layers::{display_name, LayersPanel};
use serde_json::json;
use std::sync::Arc;

#[test]
fn layers_panel_reads_and_writes_document_tag_colour_and_collapse() {
    let mut ws = Workspace::new(Arc::new(SharedServices::default()));
    let doc = ws.new_document("layers");
    ws.run(Scope::of(doc), Command::with_params("object.create", json!({"pos": [0.0, 0.0], "size": [10.0, 10.0]}))).1.unwrap();
    let id = ws.get(doc).unwrap().session.document.objects[0].id.0;
    let name = ws.get(doc).unwrap().session.document.get_object(ObjectId(id)).unwrap().name.clone();
    let label = display_name(id, &name);
    assert!(!label.is_empty());
    assert!(label.contains(&id.to_string()));
    assert_eq!(display_name(id, "Box"), "Box");

    let mut panel = LayersPanel::default();
    assert!(panel.set_meta(&mut ws, id, json!({"tag": "hero", "colour": "#00ff00"})));
    assert!(panel.toggle_collapsed(&mut ws, id));
    let meta = ws.get(doc).unwrap().session.document.layer_meta.clone();
    assert_eq!(meta.len(), 1);
    assert!(meta[0].collapsed);
    assert_eq!(meta[0].tag.as_deref(), Some("hero"));

    let mut fresh = LayersPanel::default();
    fresh.sync_from_document(&ws);
    assert!(fresh.collapsed.contains(&id));
    assert_eq!(fresh.tags.get(&id).map(String::as_str), Some("hero"));
    assert_eq!(fresh.tag_colour.get(&id).map(String::as_str), Some("#00ff00"));
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| fresh.show(ui, &mut ws));
    out.textures_delta.clear();
    assert!(fresh.collapsed.contains(&id));

    assert!(fresh.toggle_collapsed(&mut ws, id));
    assert!(!fresh.collapsed.contains(&id));
    assert!(!ws.get(doc).unwrap().session.document.layer_meta[0].collapsed);
    assert!(!fresh.set_meta(&mut ws, id, json!({"colour": "green"})));
    assert!(fresh.last_error.is_some());

    let second = ws.new_document("second");
    let mut other = LayersPanel::default();
    other.sync_from_document(&ws);
    assert!(other.tags.is_empty());
    assert!(ws.get(second).unwrap().session.document.layer_meta.is_empty());
}