use dtool_core::context_toolbar::{canonical_tool, TOOL_IDS};
use dtool_core::Session;
use serde_json::json;

#[test]
fn u05_fix_p0_shape_ids_are_canonical() {
    for id in ["tool.shape.triangle", "tool.shape.rounded_rectangle"] {
        assert_eq!(canonical_tool(id), Some(id));
        assert_eq!(TOOL_IDS.iter().filter(|known| **known == id).count(), 1);
    }
    assert_eq!(canonical_tool("shape.triangle"), Some("tool.shape.triangle"));
}

#[test]
fn u05_fix_context_query_accepts_p0_shapes() {
    let mut session = Session::default();
    for id in ["tool.shape.triangle", "tool.shape.rounded_rectangle"] {
        let query = session.run_named("ui.context_toolbar.query", json!({ "tool": id })).expect(id);
        assert_eq!(query["tool"], json!(id));
        assert!(query["options"].as_array().is_some_and(|options| options.iter().any(|option| option["id"] == json!("shape.stroke_width"))));
    }
}

#[test]
fn u05_fix_advanced_shapes_are_canonical() {
    for (key, _) in dtool_core::commands::shape::B19_RECIPE_KEYS {
        let id = format!("tool.shape.{key}");
        assert_eq!(canonical_tool(&id), Some(id.as_str()));
        assert_eq!(TOOL_IDS.iter().filter(|known| **known == id).count(), 1);
    }
    assert!(canonical_tool("tool.shape.unknown").is_none());
}
