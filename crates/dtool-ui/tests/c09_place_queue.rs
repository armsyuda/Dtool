use dtool_core::Workspace;
use dtool_core::commands::image_place::DecodedImage;
use dtool_core::resources::{ImageFormat, ImageResource};
use dtool_ui::place_tool::{PlaceTool, TOOL};

fn image() -> DecodedImage {
    DecodedImage { resource: ImageResource::new_embedded(ImageFormat::Png, [2, 1], 72.0, vec![1, 2, 3]), rgba: vec![255; 8] }
}

#[test]
fn multi_file_queue_cancel_after_first_leaves_remaining_and_click_uses_native_size() {
    let mut workspace = Workspace::default();
    let id = workspace.new_document("image");
    workspace.get_mut(id).unwrap().session.active_tool = "tool.move".into();
    let mut tool = PlaceTool::default();
    tool.open(&mut workspace).unwrap();
    tool.queue_set_decoded(&mut workspace, vec![
        ("a.png".into(), image()),
        ("b.png".into(), image()),
        ("c.png".into(), image()),
    ]).unwrap();
    assert_eq!(tool.queue_next(), Some("a.png"));
    assert_eq!(workspace.get(id).unwrap().session.active_tool, TOOL);
    let session = &mut workspace.get_mut(id).unwrap().session;
    tool.place(id, session, [5.0, 6.0], None).unwrap();
    assert_eq!(session.document.objects.len(), 1);
    assert_eq!(session.document.objects[0].pos, [5.0, 6.0]);
    let dpi = session.document.dpi;
    let unit = dpi / 72.0;
    let expected = [2.0 * 72.0 / 72.0 * unit, 1.0 * 72.0 / 72.0 * unit];
    assert_eq!(session.document.objects[0].size, expected);
    assert_eq!(tool.queue_next(), Some("b.png"));
    assert_eq!(tool.queue_remaining().len(), 1);
    assert!(tool.active_for(id));
    assert_eq!(session.active_tool, TOOL);
    let objects = session.document.objects.len();
    let version = session.document.version;
    drop(session);
    tool.cancel(&mut workspace);
    let session = &workspace.get(id).unwrap().session;
    assert_eq!(session.document.objects.len(), objects);
    assert_eq!(session.document.version, version);
    assert_eq!(session.active_tool, "tool.move");
    assert!(!tool.active_for(id));
    assert_eq!(tool.queue_remaining().len(), 2);
    assert_eq!(tool.queue_next(), Some("b.png"));
}
