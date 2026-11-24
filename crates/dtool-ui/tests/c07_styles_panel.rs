use dtool_core::object::{Object, ObjectId};
use dtool_core::paint::{Colour, Paint};
use dtool_core::workspace::Workspace;
use dtool_protocol::Command;
use dtool_ui::dock::PanelId;
use dtool_ui::panels::styles::{categories, parse_styles, thumbnail_colour, StylesPanel, STYLES_PANEL_KEY};
use dtool_ui::panels::{window_panel, workspace_panel, PanelRegistry, WINDOW_PANELS, WINDOW_STYLES};
use serde_json::json;

fn red() -> Paint {
    Paint::Solid { colour: Colour::new(200, 10, 20, 255) }
}

fn workspace() -> (Workspace, ObjectId, ObjectId) {
    let mut workspace = Workspace::default();
    workspace.new_document("c07_styles_panel");
    let session = workspace.session_mut().unwrap();
    let source = session.document.create_object(Object { name: "a".into(), size: [10.0, 10.0], fill: red(), opacity: 0.5, ..Object::default() });
    let target = session.document.create_object(Object { name: "b".into(), pos: [20.0, 0.0], size: [10.0, 10.0], ..Object::default() });
    session.selection.ids = vec![source];
    (workspace, source, target)
}

fn object(workspace: &mut Workspace, id: ObjectId) -> Object {
    workspace.session_mut().unwrap().document.get_object(id).unwrap().clone()
}

#[test]
fn window_vector_styles_opens_the_styles_panel_body() {
    let id = PanelId::Other(STYLES_PANEL_KEY.to_string());
    assert_eq!(window_panel(WINDOW_STYLES), Some(id.clone()));
    assert!(WINDOW_PANELS.contains(&WINDOW_STYLES));
    assert!(PanelRegistry::new().has_body(&id));
    assert_eq!(id.title_key(), "panel.styles");
    let (mut workspace, _, _) = workspace();
    let mut registry = PanelRegistry::new();
    registry.styles.new_name = "Red".into();
    registry.styles.create_from_selection(&mut workspace).expect("created");
    let ctx = egui::Context::default();
    let mut shown = false;
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        shown = workspace_panel(ui, &id, &mut registry, &mut workspace);
    });
    output.textures_delta.clear();
    assert!(shown);
}

#[test]
fn panel_creates_lists_by_category_and_applies_on_click_with_one_undo() {
    let (mut workspace, source, target) = workspace();
    let mut panel = StylesPanel::default();
    panel.new_name = "Red".into();
    panel.new_category = "Brand".into();
    let red_id = panel.create_from_selection(&mut workspace).expect("created");
    assert!(panel.new_name.is_empty());
    panel.new_category.clear();
    workspace.session_mut().unwrap().selection.ids = vec![target];
    let plain_id = panel.create_from_selection(&mut workspace).expect("created");
    let rows = panel.list(&mut workspace);
    assert_eq!(rows.iter().map(|row| (row.id, row.name.as_str(), row.category.as_str())).collect::<Vec<_>>(), vec![(red_id, "Red", "Brand"), (plain_id, "Style 2", "")]);
    let groups = categories(&rows);
    assert_eq!(groups.iter().map(|group| (group.name.as_str(), group.styles.len())).collect::<Vec<_>>(), vec![("Brand", 1), ("", 1)]);
    assert_eq!(thumbnail_colour(&rows[0]), egui::Color32::from_rgba_unmultiplied(200, 10, 20, 255));
    let before = object(&mut workspace, target);
    assert!(panel.apply(&mut workspace, red_id));
    let applied = object(&mut workspace, target);
    assert_eq!((applied.fill.clone(), applied.opacity), (red(), 0.5));
    workspace.session_mut().unwrap().run(&Command::new("edit.undo")).expect("undo");
    let undone = object(&mut workspace, target);
    assert_eq!((undone.fill, undone.opacity), (before.fill, before.opacity));
    assert_eq!(object(&mut workspace, source).fill, red());
}

#[test]
fn delete_and_errors_are_reported_without_changing_the_list() {
    let (mut workspace, _, _) = workspace();
    let mut panel = StylesPanel::default();
    let id = panel.create_from_selection(&mut workspace).expect("created");
    assert!(!panel.apply(&mut workspace, id + 7));
    assert!(panel.last_error.is_some());
    assert_eq!(panel.list(&mut workspace).len(), 1);
    assert!(panel.delete(&mut workspace, id));
    assert!(panel.last_error.is_none());
    assert!(panel.list(&mut workspace).is_empty());
    workspace.session_mut().unwrap().run(&Command::new("edit.undo")).expect("undo");
    assert_eq!(panel.list(&mut workspace).len(), 1);
}

#[test]
fn parse_skips_entries_without_id_and_tolerates_missing_fill() {
    let rows = parse_styles(&json!({ "styles": [{ "name": "x" }, { "id": 4, "name": "n", "style": {} }] }));
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].id, rows[0].fill.is_none()), (4, true));
    assert_eq!(thumbnail_colour(&rows[0]), dtool_design::color("panel.bg"));
}