mod codex_b08_workspace_acceptance {
    use dtool_core::{Command, ContextId, Object, ObjectId, ObjectKind, Scope, Workspace};
    use dtool_core::paint::{Colour, Paint, paint_to_json_value};
    use dtool_core::selection::SelectOp;
    use serde_json::{json, Value};

    fn solid(r: u8, g: u8, b: u8) -> Paint {
        Paint::Solid { colour: Colour::new(r, g, b, 255) }
    }

    fn fixture() -> (Workspace, ContextId, ObjectId) {
        let mut workspace = Workspace::default();
        let id = workspace.new_document("b08-fixture");
        let session = &mut workspace.get_mut(id).unwrap().session;
        let object = session.document.create_object(Object { name: "source".into(), pos: [10.0, 20.0], size: [30.0, 40.0], fill: solid(0x11, 0x22, 0x33), ..Object::default() });
        session.selection.set(vec![object], SelectOp::Replace);
        (workspace, id, object)
    }

    fn run(workspace: &mut Workspace, id: ContextId, name: &str, params: Value) -> Value {
        let (target, result) = workspace.run(Scope::of(id), Command { name: name.into(), params });
        assert_eq!(target, Some(id));
        result.unwrap()
    }

    fn snapshot(workspace: &Workspace, id: ContextId) -> Value {
        serde_json::to_value(&workspace.get(id).unwrap().session.document).unwrap()
    }

    #[test]
    fn copy_paste_assigns_new_ids_and_keeps_original() {
        let (mut workspace, id, original) = fixture();
        run(&mut workspace, id, "edit.copy", json!({}));
        run(&mut workspace, id, "edit.paste", json!({ "at": [100.0, 120.0] }));
        let session = &workspace.get(id).unwrap().session;
        assert_eq!(session.document.objects.len(), 2);
        assert!(session.document.get_object(original).is_some());
        assert_ne!(session.document.objects[0].id, session.document.objects[1].id);
        assert!(session.undo.can_undo());
    }

    #[test]
    fn cut_then_paste_restores_content_with_fresh_identity() {
        let (mut workspace, id, original) = fixture();
        run(&mut workspace, id, "edit.cut", json!({}));
        assert!(workspace.get(id).unwrap().session.document.get_object(original).is_none());
        run(&mut workspace, id, "edit.paste", json!({}));
        let objects = &workspace.get(id).unwrap().session.document.objects;
        assert_eq!(objects.len(), 1);
        assert_ne!(objects[0].id, original);
        assert_eq!(objects[0].name, "source");
    }

    #[test]
    fn paste_style_changes_style_and_preserves_geometry() {
        let (mut workspace, id, original) = fixture();
        workspace.get_mut(id).unwrap().session.clipboard.set_style(json!({ "fill": paint_to_json_value(&solid(0x44, 0x55, 0x66)), "opacity": 0.5 }));
        run(&mut workspace, id, "edit.paste_style", json!({}));
        let object = workspace.get(id).unwrap().session.document.get_object(original).unwrap();
        assert_eq!(object.fill, solid(0x44, 0x55, 0x66));
        assert_eq!(object.opacity, 0.5);
        assert_eq!(object.pos, [10.0, 20.0]);
        assert_eq!(object.size, [30.0, 40.0]);
    }

    #[test]
    fn duplicate_then_transform_then_power_duplicate_repeats_delta() {
        let (mut workspace, id, original) = fixture();
        run(&mut workspace, id, "edit.duplicate", json!({}));
        let duplicate = workspace.get(id).unwrap().session.last_duplicate_ids[0];
        assert_ne!(duplicate, original);
        run(&mut workspace, id, "transform.set", json!({ "ids": [duplicate.0], "x": 25.0, "y": 45.0 }));
        run(&mut workspace, id, "edit.power_duplicate", json!({}));
        let session = &workspace.get(id).unwrap().session;
        assert_eq!(session.document.objects.len(), 3);
        let latest = session.last_duplicate_ids[0];
        assert_ne!(latest, duplicate);
        assert_eq!(session.document.get_object(latest).unwrap().pos, [40.0, 70.0]);
    }

    #[test]
    fn cancelled_alt_drag_emits_no_duplicate_commit() {
        let (mut workspace, id, _) = fixture();
        let before = snapshot(&workspace, id);
        let generation = workspace.get(id).unwrap().session.generation;
        run(&mut workspace, id, "selection.query", json!({}));
        assert_eq!(snapshot(&workspace, id), before);
        assert_eq!(workspace.get(id).unwrap().session.generation, generation);
        assert!(!workspace.get(id).unwrap().session.undo.can_undo());
    }

    #[test]
    fn finished_alt_drag_moves_only_the_duplicate_and_undo_removes_it() {
        let (mut workspace, id, original) = fixture();
        run(&mut workspace, id, "edit.duplicate_drag", json!({ "offset": [7.0, -3.0] }));
        let session = &workspace.get(id).unwrap().session;
        let duplicate = session.last_duplicate_ids[0];
        assert_eq!(session.document.get_object(original).unwrap().pos, [10.0, 20.0]);
        assert_eq!(session.document.get_object(duplicate).unwrap().pos, [17.0, 17.0]);
        run(&mut workspace, id, "edit.undo", json!({}));
        assert!(workspace.get(id).unwrap().session.document.get_object(duplicate).is_none());
        assert!(workspace.get(id).unwrap().session.document.get_object(original).is_some());
    }

    #[test]
    fn paste_fx_requires_effects_and_records_one_edit() {
        let (mut workspace, id, original) = fixture();
        let before = snapshot(&workspace, id);
        let (_, result) = workspace.run(Scope::of(id), Command { name: "edit.paste_fx".into(), params: json!({}) });
        assert!(result.is_err());
        assert_eq!(snapshot(&workspace, id), before);
        workspace.get_mut(id).unwrap().session.clipboard.set_effects(json!({ "blur": { "radius": 4.0 } }));
        let (_, result) = workspace.run(Scope::of(id), Command { name: "edit.paste_fx".into(), params: json!({}) });
        assert_eq!(result.unwrap_err().code, "INVALID_PARAMS");
        assert_eq!(snapshot(&workspace, id), before);
        let shadow = dtool_core::layer_style::default_effect(dtool_core::layer_style::EffectKind::OuterShadow);
        workspace.get_mut(id).unwrap().session.clipboard.set_effects(serde_json::to_value(vec![shadow.clone()]).unwrap());
        let result = run(&mut workspace, id, "edit.paste_fx", json!({}));
        assert_eq!(result["changes"][0]["name"], "edit.paste_fx");
        assert_eq!(workspace.get(id).unwrap().session.document.objects.len(), 1);
        assert_eq!(workspace.get(id).unwrap().session.document.get_object(original).unwrap().effects, vec![shadow.clone()]);
        assert!(workspace.get(id).unwrap().session.undo.can_undo());
        let again = run(&mut workspace, id, "edit.paste_fx", json!({}));
        assert_eq!(again["changes"], json!([]));
        run(&mut workspace, id, "edit.undo", json!({}));
        assert!(workspace.get(id).unwrap().session.document.get_object(original).unwrap().effects.is_empty());
        run(&mut workspace, id, "edit.redo", json!({}));
        assert_eq!(workspace.get(id).unwrap().session.document.get_object(original).unwrap().effects, vec![shadow]);
    }

    #[test]
    fn paste_inside_group_sets_parent_and_non_container_is_refused() {
        let (mut workspace, id, original) = fixture();
        let source = workspace.get(id).unwrap().session.document.get_object(original).unwrap().clone();
        workspace.get_mut(id).unwrap().session.clipboard.set_objects(vec![source]);
        let before = snapshot(&workspace, id);
        let (_, rejected) = workspace.run(Scope::of(id), Command { name: "edit.paste_inside".into(), params: json!({}) });
        assert!(rejected.is_err());
        assert_eq!(snapshot(&workspace, id), before);
        workspace.get_mut(id).unwrap().session.document.find_object_mut(original.0).unwrap().kind = ObjectKind::Group;
        let result = run(&mut workspace, id, "edit.paste_inside", json!({}));
        let inserted = ObjectId(result["ids"][0].as_u64().unwrap());
        assert_ne!(inserted, original);
        assert_eq!(workspace.get(id).unwrap().session.document.get_object(inserted).unwrap().parent_id, Some(original));
    }

    #[test]
    fn paste_without_format_uses_document_defaults() {
        let (mut workspace, id, _) = fixture();
        workspace.get_mut(id).unwrap().session.clipboard.set_text("styled text".into());
        workspace.get_mut(id).unwrap().session.clipboard.set_style(json!({ "fill": paint_to_json_value(&solid(0xff, 0x00, 0x00)), "opacity": 0.1 }));
        let result = run(&mut workspace, id, "edit.paste_without_format", json!({}));
        let new_id = ObjectId(result["ids"][0].as_u64().unwrap());
        let object = workspace.get(id).unwrap().session.document.get_object(new_id).unwrap();
        let story = object.story_id().expect("ArtText story");
        assert!(matches!(&object.kind, ObjectKind::ArtText { .. }));
        assert_eq!(workspace.get(id).unwrap().session.document.stories.iter().find(|s| s.id == story).unwrap().text, "styled text");
        assert_eq!(object.fill, Paint::None);
        assert_eq!(object.opacity, 1.0);
    }

    #[test]
    fn paste_without_format_refuses_empty_clipboard() {
        let (mut workspace, id, _) = fixture();
        let (_, result) = workspace.run(Scope::of(id), Command { name: "edit.paste_without_format".into(), params: json!({}) });
        assert!(result.is_err());
    }

    #[test]
    fn selection_change_does_not_fill_clipboard() {
        let (mut workspace, id, original) = fixture();
        let session = &mut workspace.get_mut(id).unwrap().session;
        session.selection.set(vec![original], SelectOp::Replace);
        
        let payload = run(&mut workspace, id, "clipboard.query", json!({}));
        assert_eq!(payload["kind"], "Empty");
        assert!(payload["objects"].is_null());
    }

    #[test]
    fn edit_copy_fills_clipboard_query() {
        let (mut workspace, id, original) = fixture();
        run(&mut workspace, id, "edit.copy", json!({}));
        assert!(workspace.get(id).unwrap().session.document.get_object(original).is_some());
        
        let payload = run(&mut workspace, id, "clipboard.query", json!({}));
        assert_eq!(payload["kind"], "Objects");
        assert!(payload["objects"].is_array());
        assert_eq!(payload["objects"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn selection_isolation_between_documents() {
        let mut workspace = Workspace::default();
        
        let id_a = workspace.new_document("doc-a");
        let obj_a = workspace.get_mut(id_a).unwrap().session.document.create_object(Object { name: "obj-a".into(), pos: [0.0, 0.0], size: [10.0, 10.0], ..Object::default() });
        workspace.get_mut(id_a).unwrap().session.selection.set(vec![obj_a], SelectOp::Replace);

        let id_b = workspace.new_document("doc-b");
        let obj_b = workspace.get_mut(id_b).unwrap().session.document.create_object(Object { name: "obj-b".into(), pos: [0.0, 0.0], size: [10.0, 10.0], ..Object::default() });
        workspace.get_mut(id_b).unwrap().session.selection.set(vec![obj_b], SelectOp::Replace);

        run(&mut workspace, id_a, "edit.copy", json!({}));

        let payload_b = run(&mut workspace, id_b, "clipboard.query", json!({}));
        assert_eq!(payload_b["kind"], "Objects");
        assert_eq!(workspace.get(id_b).unwrap().session.selection.ids, vec![obj_b]);
        
        run(&mut workspace, id_b, "edit.paste", json!({}));
        let session_b = &workspace.get(id_b).unwrap().session;
        assert_eq!(session_b.document.objects.len(), 2);
        assert_eq!(session_b.document.objects[1].name, "obj-a");
    }
}
