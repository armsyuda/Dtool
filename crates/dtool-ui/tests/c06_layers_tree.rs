use dtool_core::workspace::Workspace;
use dtool_ui::panel_ids;
use dtool_ui::dock::PanelId;
use dtool_ui::panels::layer_states::{LayerStatesPanel, LAYER_STATES_PANEL_KEY};
use dtool_ui::panels::layers::{row_label, LayersPanel};
use serde_json::json;

fn create(workspace: &mut Workspace, name: &str) -> u64 {
    workspace.session_mut().unwrap().run_named("object.create", json!({"name": name, "pos": [0.0, 0.0], "size": [10.0, 10.0]})).unwrap();
    workspace.session().unwrap().document.objects.last().unwrap().id.0
}

#[test]
fn layers_panel_keeps_hidden_and_locked_rows_from_tree_query() {
    let mut workspace = Workspace::default();
    workspace.new_document("c06");
    let a = create(&mut workspace, "a");
    let b = create(&mut workspace, "b");
    {
        let objects = &mut workspace.session_mut().unwrap().document.objects;
        objects[0].pickable = false;
        objects[1].locked = true;
    }
    let panel = LayersPanel::default();
    let rows = panel.rows(&mut workspace);
    let ids: Vec<u64> = rows.iter().map(|row| row.id).collect();
    assert_eq!(ids, vec![b, a]);
    assert!(rows[0].locked && rows[0].visible);
    assert!(!rows[1].visible && !rows[1].locked);
    assert_ne!(row_label(&rows[1]), rows[1].name);
}

#[test]
fn layer_states_panel_round_trip() {
    assert!(panel_ids::all_panel_ids().contains(&PanelId::Other(LAYER_STATES_PANEL_KEY.into())));
    assert!(dtool_strings::has("panel.layer_states"));
    let mut workspace = Workspace::default();
    workspace.new_document("c06");
    create(&mut workspace, "a");
    let mut panel = LayerStatesPanel::default();
    panel.name = "one".into();
    panel.save(&mut workspace);
    assert_eq!(panel.last_error, None);
    assert!(panel.names(&mut workspace).contains(&"one".to_string()));
    panel.rename_to(&mut workspace, "one", "two");
    assert!(panel.names(&mut workspace).contains(&"two".to_string()));
    workspace.session_mut().unwrap().document.objects[0].pickable = false;
    panel.restore(&mut workspace, "two");
    assert_eq!(panel.last_error, None);
    assert!(workspace.session().unwrap().document.objects[0].pickable);
    panel.delete(&mut workspace, "two");
    assert!(!panel.names(&mut workspace).contains(&"two".to_string()));
}
