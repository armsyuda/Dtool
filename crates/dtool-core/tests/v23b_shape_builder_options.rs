use dtool_core::context_toolbar::options_for;

fn ids_and_defaults() -> Vec<(String, String)> {
    options_for("tool.shape_builder").iter().map(|spec| (spec.id.to_string(), spec.default_value().to_string())).collect()
}

#[test]
fn shape_builder_bar_matches_bundle_order_and_defaults() {
    assert_eq!(ids_and_defaults(), vec![
        ("shape_builder.action".to_string(), "\"add\"".to_string()),
        ("shape_builder.drag_method".to_string(), "\"freehand\"".to_string()),
        ("shape_builder.cleanup".to_string(), "\"none\"".to_string()),
        ("shape_builder.first_style".to_string(), "true".to_string()),
    ]);
}

#[test]
fn shape_builder_choices_and_keys_use_context_prefix() {
    let described: Vec<serde_json::Value> = options_for("tool.shape_builder").iter().map(|spec| spec.describe(&spec.default_value())).collect();
    assert_eq!(described[0]["values"], serde_json::json!(["add", "delete", "create"]));
    assert_eq!(described[1]["values"], serde_json::json!(["freehand", "line", "marquee"]));
    assert_eq!(described[2]["values"], serde_json::json!(["none", "internal", "connected", "unused"]));
    for option in &described {
        assert!(option["key"].as_str().unwrap().starts_with("context.shape_builder."));
    }
}