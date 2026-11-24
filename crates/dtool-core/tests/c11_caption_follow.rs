use dtool_core::commands::caption::follow;
use dtool_core::object::{Object, ObjectId, ObjectKind};
use dtool_core::{Command, Scope, Workspace};
use serde_json::{json, Value};

fn live() -> (Workspace, u64) {
    let mut ws = Workspace::default();
    let ctx = ws.new_document("c11 caption follow");
    let owner = ws.get_mut(ctx).unwrap().session.document.create_object(Object { pos: [10.0, 10.0], size: [40.0, 30.0], ..Object::default() }).0;
    (ws, owner)
}

fn run(ws: &mut Workspace, hooked: bool, name: &str, params: Value) -> Value {
    let value = ws.run(Scope::active(), Command::with_params(name, params)).1.unwrap_or_else(|e| panic!("{name}: {e:?}"));
    if !hooked {
        let ctx = ws.active().unwrap();
        follow(&mut ws.get_mut(ctx).unwrap().session);
    }
    value
}

fn caption_of(ws: &Workspace, owner: u64) -> Object {
    let ctx = ws.active().unwrap();
    ws.get(ctx).unwrap().session.document.objects.iter().find(|o| o.parent_id == Some(ObjectId(owner)) && matches!(o.kind, ObjectKind::Text(_))).cloned().expect("caption")
}

fn steps(ws: &Workspace) -> usize {
    let ctx = ws.active().unwrap();
    ws.get(ctx).unwrap().session.undo.applied_steps()
}

fn scenario(hooked: bool) {
    let (mut ws, owner) = live();
    run(&mut ws, hooked, "caption.add", json!({ "owner": owner }));
    assert_eq!(caption_of(&ws, owner).pos, [10.0, 44.0]);
    assert_eq!(caption_of(&ws, owner).name, "Caption below 4");
    let before = steps(&ws);
    run(&mut ws, hooked, "transform.set", json!({ "ids": [owner], "x": 100.0, "y": 50.0 }));
    assert_eq!(caption_of(&ws, owner).pos, [100.0, 84.0]);
    assert_eq!(steps(&ws), before + 1, "caption follow adds no undo step");
    run(&mut ws, hooked, "edit.undo", json!({}));
    assert_eq!(caption_of(&ws, owner).pos, [10.0, 44.0], "undo of the owner move restores the caption");
    run(&mut ws, hooked, "edit.redo", json!({}));
    assert_eq!(caption_of(&ws, owner).pos, [100.0, 84.0]);
    run(&mut ws, hooked, "selection.set", json!({ "ids": [owner] }));
    run(&mut ws, hooked, "transform.selection_box", json!({ "origin": [100.0, 50.0], "scale": [2.0, 2.0] }));
    let cap = caption_of(&ws, owner);
    assert_eq!((cap.pos, cap.size), ([100.0, 114.0], [80.0, 20.0]), "scale: below the 80x60 owner, full width");
    run(&mut ws, hooked, "transform.flip", json!({ "ids": [owner], "axis": "Vertical", "anchor": "Centre" }));
    assert_eq!(caption_of(&ws, owner).pos, [100.0, 114.0], "flip keeps the caption below the flipped bounds");
    run(&mut ws, hooked, "transform.rotate_90", json!({ "ids": [owner], "direction": "Left" }));
    let ctx = ws.active().unwrap();
    let owner_obj = ws.get(ctx).unwrap().session.document.get_object(ObjectId(owner)).cloned().unwrap();
    let [x, y, w, h] = dtool_core::geometry::object_world_bounds(&owner_obj);
    let cap = caption_of(&ws, owner);
    assert!((cap.pos[0] - x).abs() < 0.01 && (cap.pos[1] - (y + h + 4.0)).abs() < 0.01 && (cap.size[0] - w).abs() < 0.01, "rotate: {cap:?} vs {:?}", [x, y, w, h]);
    let dup = run(&mut ws, hooked, "edit.duplicate", json!({}));
    let copy = dup["ids"][0].as_u64().unwrap();
    run(&mut ws, hooked, "transform.set", json!({ "ids": [copy], "x": 300.0, "y": 300.0 }));
    let copy_cap = caption_of(&ws, copy);
    let copy_obj = ws.get(ctx).unwrap().session.document.get_object(ObjectId(copy)).cloned().unwrap();
    let [cx, cy, _, ch] = dtool_core::geometry::object_world_bounds(&copy_obj);
    assert!((copy_cap.pos[0] - cx).abs() < 0.01 && (copy_cap.pos[1] - (cy + ch + 4.0)).abs() < 0.01, "duplicate caption follows its own owner");
    assert_eq!(caption_of(&ws, owner).pos, cap.pos, "original caption unaffected");
    run(&mut ws, hooked, "node.delete", json!({ "nodes": [owner] }));
    assert!(ws.get(ctx).unwrap().session.document.objects.iter().all(|o| o.parent_id != Some(ObjectId(owner))), "deleting the owner removes its caption");
    run(&mut ws, hooked, "edit.undo", json!({}));
    assert_eq!(caption_of(&ws, owner).pos, cap.pos, "undo delete restores owner and caption in place");
}

#[test]
fn caption_follows_owner_transforms_with_follow_after_each_command() {
    scenario(false);
}

#[test]
fn side_and_gap_survive_remove_undo_and_left_right_follow() {
    let (mut ws, owner) = live();
    run(&mut ws, false, "caption.add", json!({ "owner": owner, "settings": { "side": "left", "gap": 6.0, "height": 10.0 } }));
    let cap = caption_of(&ws, owner);
    assert_eq!((cap.pos, cap.name.as_str()), ([10.0 - 40.0 - 6.0, 10.0], "Caption left 6"));
    run(&mut ws, false, "transform.set", json!({ "ids": [owner], "x": 200.0, "y": 0.0 }));
    assert_eq!(caption_of(&ws, owner).pos, [200.0 - 40.0 - 6.0, 0.0]);
    run(&mut ws, false, "caption.remove", json!({ "owner": owner }));
    run(&mut ws, false, "edit.undo", json!({}));
    let back = caption_of(&ws, owner);
    assert_eq!((back.pos, back.name.as_str()), ([200.0 - 40.0 - 6.0, 0.0], "Caption left 6"));
}

#[test]
fn caption_follows_owner_transforms_through_workspace_run_alone() {
    scenario(true);
}
