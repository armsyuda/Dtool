use dtool_design::icons::{colour_hex, current_icon_colour, icon, icon_colour, icon_file, icon_ids, svg, svg_ids, themed_svg, TOOL_ICON_ALIASES};
use dtool_design::ThemeStyle;
use egui::Color32;

#[test]
fn all_design_icons_are_registered() {
    let ids = svg_ids();
    let registered = icon_ids();
    let registered_svg_count = registered.iter().filter(|id| icon_file(id).is_some_and(|file| file.ends_with(".svg"))).count();
    assert_eq!(ids.len(), registered_svg_count);
    for id in &ids {
        assert!(registered.contains(id), "{id}");
        assert_eq!(icon_file(id), Some(format!("{id}.svg").as_str()));
        let bytes = icon(id).unwrap();
        assert!(bytes.starts_with(b"<svg"), "{id}");
        let text = svg(id).unwrap();
        assert!(text.contains("viewBox=\"0 0 24 24\""), "{id}");
        assert!(text.contains("currentColor"), "{id}");
    }
    for id in ["tool-move", "app-logo", "ui-separator", "ui-spacer", "px-clone"] {
        assert!(ids.contains(&id), "{id}");
    }
    assert!(registered.contains(&"app.main"));
}

#[test]
fn current_colour_is_replaced_with_the_theme_colour() {
    for id in svg_ids() {
        let text = themed_svg(id, Color32::from_rgb(0x12, 0x34, 0x56)).unwrap();
        assert!(!text.contains("currentColor"), "{id}");
        assert!(text.contains("#123456"), "{id}");
    }
    assert_eq!(colour_hex(Color32::from_rgb(255, 0, 10)), "#FF000A");
    assert!(themed_svg("no-such-icon", Color32::WHITE).is_none());
}

#[test]
fn icon_colour_follows_theme_text() {
    assert_eq!(icon_colour(ThemeStyle::Dark), dtool_design::dark().text);
    assert_eq!(icon_colour(ThemeStyle::Light), dtool_design::light().text);
    assert_ne!(icon_colour(ThemeStyle::Dark), icon_colour(ThemeStyle::Light));
    let current = current_icon_colour();
    assert!(current == icon_colour(ThemeStyle::Dark) || current == icon_colour(ThemeStyle::Light));
}

#[test]
fn alias_targets_are_registered_icons() {
    let ids = svg_ids();
    for (tool, id) in TOOL_ICON_ALIASES {
        assert!(tool.starts_with("tool."), "{tool}");
        assert!(ids.contains(&id), "{tool} -> {id}");
    }
}
