use dtool_design::{color, missing, number};

#[test]
fn v107_flat_tokens_and_gaps() {
    let value: serde_json::Value = serde_json::from_str(include_str!("../tokens.json")).unwrap();
    assert!(value.as_object().unwrap().values().all(|v| !v.is_object() && !v.is_array()));
    let keys = ["navigator.thumb.bg", "navigator.thumb.item", "navigator.thumb.page", "navigator.viewbox.fill", "navigator.viewbox.stroke", "nav.thumb.radius", "nav.thumb.width", "nav.thumb.height", "smart_guide", "selection.ants", "error.red", "success.green", "panel.fg", "invisibles.mark", "text.primary", "text.secondary", "font.size.md"];
    assert!(missing(&keys).is_empty());
    assert_eq!(color("accent"), egui::Color32::from_gray(184));
    assert_eq!(value["color.light.accent"], "#585858");
    assert_eq!(value["color.dark.accent"], "#B8B8B8");
    assert_eq!(number("bar.menu.height"), 29.6);
    assert_eq!(number("bar.toolbar.height"), 52.8);
}
