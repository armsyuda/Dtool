use dtool_core::commands::stroke::{arrowhead, ARROW_NAMES};
use dtool_ui::stroke_panel::StrokePanel;

#[test]
fn c07_panel_offers_every_renderable_arrow() {
    let panel = StrokePanel::new();
    assert_eq!(panel.available_arrows, ARROW_NAMES.iter().map(|name| name.to_string()).collect::<Vec<_>>());
    assert!(panel.available_arrows.iter().all(|name| arrowhead(name, [10.0, 0.0], [0.0, 0.0], 1.0, 1.0).is_some()));
}