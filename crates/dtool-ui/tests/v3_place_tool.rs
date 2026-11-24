use dtool_core::{ContextId, Workspace};
use dtool_core::commands::image_place::DecodedImage;
use dtool_core::resources::{ImageFormat,ImageResource};
use dtool_ui::place_tool::{PlaceTool,drag_geometry,TOOL};

fn image()->DecodedImage {
    DecodedImage{resource:ImageResource::new_embedded(ImageFormat::Png,[2,1],72.0,vec![1,2,3]),rgba:vec![255;8]}
}

#[test]
fn reverse_drag_preserves_ratio_unless_shift_frees_it() {
    assert_eq!(drag_geometry([10.0,10.0],[4.0,6.0],2.0,false),Some(([2.0,6.0],[8.0,4.0])));
    assert_eq!(drag_geometry([10.0,10.0],[4.0,6.0],2.0,true),Some(([4.0,6.0],[6.0,4.0])));
    assert!(drag_geometry([0.0,0.0],[f32::NAN,1.0],2.0,false).is_none());
}

#[test]
fn cancel_keeps_document_and_previous_tool_unchanged() {
    let mut workspace=Workspace::default();
    let id=workspace.new_document("image");
    workspace.get_mut(id).unwrap().session.active_tool="tool.pen".into();
    let mut tool=PlaceTool::default();
    tool.open(&mut workspace).unwrap();
    tool.cancel(&mut workspace);
    assert_eq!(workspace.get(id).unwrap().session.active_tool,"tool.pen");
    tool.open(&mut workspace).unwrap();
    tool.arm(&mut workspace,image()).unwrap();
    assert_eq!(workspace.get(id).unwrap().session.active_tool,TOOL);
    tool.cancel(&mut workspace);
    let session=&workspace.get(id).unwrap().session;
    assert_eq!(session.active_tool,"tool.pen");
    assert!(session.document.objects.is_empty());
    assert!(!session.undo.can_undo());
}

#[test]
fn placement_is_scoped_one_undo_and_returns_to_the_previous_tool() {
    let mut workspace=Workspace::default();
    let id=workspace.new_document("image");
    workspace.get_mut(id).unwrap().session.active_tool="tool.move".into();
    let mut tool=PlaceTool::default();
    tool.open(&mut workspace).unwrap();
    tool.arm(&mut workspace,image()).unwrap();
    let session=&mut workspace.get_mut(id).unwrap().session;
    assert!(tool.place(ContextId(id.0+1),session,[5.0,6.0],None).is_err());
    assert!(session.document.objects.is_empty());
    tool.place(id,session,[5.0,6.0],Some([20.0,10.0])).unwrap();
    assert_eq!(session.document.objects[0].pos,[5.0,6.0]);
    assert_eq!(session.document.objects[0].size,[20.0,10.0]);
    assert_eq!(session.active_tool,"tool.move");
    session.run_named("edit.undo",serde_json::json!({})).unwrap();
    assert!(session.document.objects.is_empty());
    assert!(!tool.active_for(id));
}
