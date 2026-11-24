use dtool_core::Session;
use dtool_protocol::Command;
use dtool_ui::context_bar::{ContextBarModel, ContextControl, ContextEdit};
use dtool_ui::{menu_tree, panel_ids, tools_panel, tooltips};
use serde_json::{json, Value};
use std::collections::HashSet;

fn session() -> Session {
    Session::default()
}

#[test]
fn b11_ui_keys_are_present() {
    assert!(menu_tree::missing_b11_keys().is_empty(), "{:?}", menu_tree::missing_b11_keys());
}

#[test]
fn menu_tree_top_level_follows_order() {
    for persona in menu_tree::PERSONAS {
        let ids = menu_tree::MenuTree::for_persona(persona).top_level_ids();
        let known: Vec<&str> = ids.iter().copied().filter(|id| menu_tree::TOP_LEVEL_ORDER.contains(id)).collect();
        let expected: Vec<&str> = menu_tree::TOP_LEVEL_ORDER.iter().copied().filter(|id| known.contains(id)).collect();
        assert_eq!(known, expected, "persona {persona}");
        assert!(known.contains(&"file"), "persona {persona}");
    }
}

#[test]
fn menu_query_matches_tree_order() {
    assert!(menu_tree::install());
    let mut s = session();
    let result = s.run_named("ui.menu.query", json!({ "persona": "pixel" })).expect("menu query");
    let ids: Vec<&str> = result
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .collect();
    assert_eq!(ids, menu_tree::MenuTree::for_persona("pixel").top_level_ids());
}

#[test]
fn flyout_children_set_active_tool() {
    let mut s = session();
    for flyout in tools_panel::default_flyouts() {
        for child in flyout.children.iter().chain(std::iter::once(&flyout.parent)) {
            if dtool_core::context_toolbar::canonical_tool(child.0).is_none() {
                continue;
            }
            let cmd = Command::with_params("ui.tool.set", json!({ "id": child.0 }));
            tools_panel::handle_tool_command(&mut s, &cmd).expect(child.0);
            assert_eq!(Some(s.active_tool.as_str()), dtool_core::context_toolbar::canonical_tool(child.0));
        }
    }
}

#[test]
fn flyout_rejects_unknown_tool() {
    let mut s = session();
    let cmd = Command::with_params("ui.tool.set", json!({ "id": "tool.not_a_tool" }));
    assert!(tools_panel::handle_tool_command(&mut s, &cmd).is_err());
    assert_eq!(s.active_tool, "tool.move");
}

#[test]
fn tooltip_keys_resolve() {
    for id in ["move", "zoom"] {
        let key = format!("tooltip.tool.{id}");
        assert!(dtool_strings::has(&key), "{key}");
        assert_ne!(tooltips::tool_tooltip(id), key);
        assert_eq!(tooltips::tool_tooltip(&format!("tool.{id}")), tooltips::tool_tooltip(id));
    }
}

#[test]
fn context_bar_query_per_tool() {
    let mut s = session();
    s.run_named("ui.tool.set", json!({ "id": "tool.move" })).expect("move");
    let query = s.run_named("ui.context_toolbar.query", json!({})).expect("query");
    let model = ContextBarModel::from_query(&query);
    assert_eq!(model.tool.as_deref(), Some("tool.move"));
    assert!(model.ids().contains(&"move.align"));
    assert_eq!(model, ContextBarModel::current(&s));
    s.run_named("ui.tool.set", json!({ "id": "tool.zoom" })).expect("zoom");
    let zoom = ContextBarModel::current(&s);
    assert_eq!(zoom.tool.as_deref(), Some("tool.zoom"));
    assert_ne!(zoom.ids(), model.ids());
    assert!(zoom.items.iter().all(|item| dtool_strings::has(&item.key)), "{:?}", zoom.ids());
}

#[test]
fn context_bar_edit_updates_store() {
    let mut s = session();
    s.run_named("ui.tool.set", json!({ "id": "tool.shape.polygon" })).expect("polygon");
    let model = ContextBarModel::current(&s);
    let item = model.items.iter().find(|item| item.id == "shape.sides").expect("sides");
    assert!(matches!(item.control, ContextControl::Number { .. }));
    let edit = ContextEdit { id: "shape.sides".into(), value: json!(7) };
    dtool_ui::context_bar::apply_edit(&mut s, "tool.shape.polygon", &edit).expect("set");
    let after = ContextBarModel::current(&s);
    let item = after.items.iter().find(|item| item.id == "shape.sides").expect("sides");
    assert!(matches!(item.control, ContextControl::Number { value, .. } if value == 7.0));
}

#[test]
fn panel_ids_are_unique_and_titled() {
    let ids = panel_ids::all_panel_ids();
    let keys: HashSet<String> = ids.iter().map(|id| id.entry().key).collect();
    assert_eq!(keys.len(), ids.len());
    for id in panel_ids::named_panel_ids() {
        assert!(dtool_strings::has(&id.title_key()), "{}", id.title_key());
    }
}

#[test]
fn context_menu_routes_by_target() {
    assert!(menu_tree::install());
    assert!(dtool_ui::context_menu::install());
    let mut s = session();
    let ids = |rows: &Value| -> Vec<String> { rows.as_array().map(|rows| rows.iter().filter_map(|row| row.get("id").and_then(Value::as_str).map(String::from)).collect()).unwrap_or_default() };
    let object = s.run_named("ui.context_menu.query", json!({ "target": "Object" })).expect("object");
    let editing = s.run_named("ui.context_menu.query", json!({ "target": "TextEditing" })).expect("editing");
    assert!(ids(&object).iter().any(|id| id == "edit.cut"));
    assert!(!ids(&object).iter().any(|id| id == "context.convert_text_to_field"));
    assert!(ids(&editing).iter().any(|id| id == "context.convert_text_to_field"));
    assert_ne!(object, editing);
    assert!(s.run_named("ui.context_menu.query", json!({ "target": "Nowhere" })).is_err());
}

#[test]
fn menu_invoke_rejects_unknown_id() {
    assert!(menu_tree::install());
    let mut s = session();
    assert!(s.run_named("ui.menu.invoke", json!({ "id": "menu.not_an_item" })).is_err());
}

#[test]
fn shared_panel_action_ids() {
    let ids = dtool_ui::panel_actions::panel_action_ids();
    assert!(ids.contains(&"panel.menu.close"));
    let unique: HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len());
    for action in dtool_ui::panel_actions::PANEL_ACTIONS {
        assert!(dtool_strings::has(action.key), "{}", action.key);
    }
    let mut s = session();
    assert!(dtool_ui::panel_actions::invoke_panel_action(&mut s, "panel.menu.close").is_err());
}
