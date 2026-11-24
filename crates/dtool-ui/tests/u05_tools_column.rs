use dtool_core::Session;
use dtool_protocol::Command;
use dtool_ui::panel_actions;
use dtool_ui::tools_panel::{self, handle_tool_command};
use serde_json::json;

fn session() -> Session {
    Session::default()
}

fn set_tool(session: &mut Session, id: &str) -> dtool_core::CommandResult {
    handle_tool_command(session, &Command::with_params("ui.tool.set", json!({ "id": id })))
}

#[test]
fn u05_tools_column_click_selects_p0_shape() {
    let mut s = session();
    let ids = tools_panel::tool_column_ids("tool.shapes");
    assert!(ids.contains(&"tool.shape.triangle"));
    assert!(ids.contains(&"tool.shape.rounded_rectangle"));
    set_tool(&mut s, "tool.shape.triangle").expect("triangle");
    assert_eq!(s.active_tool, "tool.shape.triangle");
    set_tool(&mut s, "tool.shape.rounded_rectangle").expect("rounded");
    assert_eq!(s.active_tool, "tool.shape.rounded_rectangle");
}

#[test]
fn u05_advanced_shapes_select_the_matching_canvas_tool() {
    let mut s = session();
    let tools = tools_panel::tool_column_ids("tool.shapes");
    for (key, kind) in dtool_core::commands::shape::B19_RECIPE_KEYS {
        let id = format!("tool.shape.{key}");
        assert!(tools.contains(&id.as_str()), "{id}");
        assert!(!tools_panel::is_held_shape_tool(&id), "{id}");
        set_tool(&mut s, &id).expect(&id);
        assert_eq!(s.active_tool, id);
        assert_eq!(dtool_ui::canvas_input::tool_kind(&id), dtool_ui::canvas_input::ToolKind::Shape(kind));
    }
    assert!(set_tool(&mut s, "tool.shape.unknown").is_err());
}

#[test]
fn u05_panel_actions_with_commands_are_listed() {
    let ids = panel_actions::panel_action_ids();
    assert!(ids.contains(&"panel.menu.close"));
    let unique: std::collections::HashSet<_> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len());
    for action in panel_actions::PANEL_ACTIONS {
        assert!(dtool_strings::has(action.key), "{}", action.key);
        if action.command.is_some() {
            assert!(ids.contains(&action.id.0));
        }
    }
}

