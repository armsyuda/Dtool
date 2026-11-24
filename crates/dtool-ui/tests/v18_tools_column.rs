use dtool_ui::tools_panel::{default_flyouts, ToolId};

#[test]
fn tools_column_lists_basic_and_advanced_shapes() {
    let shapes = default_flyouts()
        .into_iter()
        .find(|flyout| flyout.parent.0 == "tool.shapes")
        .expect("shapes flyout")
        .children
        .into_iter()
        .map(|ToolId(id)| id)
        .collect::<Vec<_>>();
    assert!(shapes.contains(&"tool.shape.rectangle"));
    assert!(shapes.contains(&"tool.shape.triangle"));
    assert!(shapes.contains(&"tool.shape.ellipse"));
    assert!(shapes.contains(&"tool.shape.rounded_rectangle"));
    assert_eq!(shapes.len(), 6 + dtool_core::commands::shape::B19_RECIPE_KEYS.len());
    assert!(shapes.contains(&"tool.shape.polygon"));
}
