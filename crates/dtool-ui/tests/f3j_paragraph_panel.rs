use dtool_core::workspace::Workspace;
use dtool_tools::dtool_text::{story_from_core, Align};
use dtool_ui::dock::PanelId;
use dtool_ui::panels::paragraph::{apply_patch, query_style};
use dtool_ui::panels::{window_panel, workspace_panel, PanelRegistry, WINDOW_PARAGRAPH};
use serde_json::json;

fn workspace_with_text() -> (Workspace, dtool_core::ObjectId) {
    let mut workspace = Workspace::default();
    workspace.new_document("p");
    let session = workspace.session_mut().unwrap();
    session.run_named("text.art.add", json!({"pos": [0.0, 0.0], "text": "one\ntwo"})).unwrap();
    let id = session.document.objects.last().unwrap().id;
    session.selection.ids = vec![id];
    (workspace, id)
}

#[test]
fn panel_edits_selected_story_paragraphs_with_undo() {
    let (mut workspace, id) = workspace_with_text();
    let session = workspace.session_mut().unwrap();
    assert_eq!(query_style(session).unwrap().align, Align::Left);
    apply_patch(session, json!({"align": Align::Center, "space_before": 6.0})).unwrap();
    let style = query_style(session).unwrap();
    assert_eq!(style.align, Align::Center);
    assert_eq!(style.space_before, 6.0);
    let story_id = session.document.get_object(id).unwrap().story_id().unwrap();
    let story = session.document.stories.iter().find(|s| s.id == story_id).unwrap();
    assert!(story_from_core(story).paras.iter().all(|run| run.style.align == Align::Center));
    session.run_named("edit.undo", json!({})).unwrap();
    assert_eq!(query_style(session).unwrap().align, Align::Left);
}

#[test]
fn panel_without_story_is_inert() {
    let (mut workspace, _) = workspace_with_text();
    let session = workspace.session_mut().unwrap();
    session.selection.ids.clear();
    assert!(query_style(session).is_none());
    assert!(apply_patch(session, json!({"align": Align::Right})).is_err());
}

#[test]
fn paragraph_panel_is_registered_and_renders() {
    assert_eq!(window_panel(WINDOW_PARAGRAPH), Some(PanelId::Paragraph));
    let mut registry = PanelRegistry::with_existing_bodies();
    assert!(registry.has_body(&PanelId::Paragraph));
    let (mut workspace, _) = workspace_with_text();
    let ctx = egui::Context::default();
    let mut shown = false;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        shown = workspace_panel(ui, &PanelId::Paragraph, &mut registry, &mut workspace);
    });
    output.textures_delta.clear();
    assert!(shown);
    assert_eq!(registry.paragraph.style.align, Align::Left);
    assert!(registry.paragraph.error.is_none());
}
