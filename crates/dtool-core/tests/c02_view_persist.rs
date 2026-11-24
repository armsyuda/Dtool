#[path = "support/test_home.rs"]
mod test_home;
use dtool_core::workspace::{CloseAnswer, Scope, Workspace};
use dtool_core::Command;
use serde_json::json;

#[test]
fn view_state_survives_native_save_and_open() {
    let home = test_home::enter("c02_view_persist");
    let path = home.root.join("view.dtool");
    let mut ws = Workspace::default();
    let id = ws.new_document("View");
    ws.run(Scope::of(id), Command::with_params("view.flags.set", json!({ "clip_to_canvas": true, "preview_mode": true }))).1.unwrap();
    {
        let ctx = ws.get_mut(id).unwrap();
        ctx.view_state.zoom = 2.5;
        ctx.view_state.scroll = [12.0, -34.0];
        ctx.session.view = ctx.view_state.clone();
    }
    let saved = ws.get(id).unwrap().view_state.clone();
    ws.run(Scope::of(id), Command::with_params("doc.save_as", json!({ "path": path }))).1.unwrap();
    ws.close_with(id, CloseAnswer::No).unwrap();
    let opened = ws.open_path(&path).unwrap();
    let ctx = ws.get(opened).unwrap();
    assert_eq!(ctx.view_state, saved);
    assert_eq!(ctx.session.view, saved);
    assert!(ws.native_extras(opened).map_or(true, |extras| extras.get("view").is_none()));
    let q = ws.run(Scope::of(opened), Command::new("view.flags.query")).1.unwrap();
    assert_eq!(q["flags"]["clip_to_canvas"], true);
}