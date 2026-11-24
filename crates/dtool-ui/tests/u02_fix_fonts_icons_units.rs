use dtool_core::document::Units;
use dtool_ui::captions::{units_options, ALL_UNITS};
use dtool_ui::dialogs::new_document_unit_options;
use dtool_ui::fonts::{install_ui_fonts, token_font_definitions, ui_families, INTER_ID, MALGUN_ID, PRETENDARD_ID};
use std::path::Path;

#[test]
fn fonts_follow_the_ui_tokens() {
    assert_eq!(ui_families(), vec!["Inter".to_string(), "Pretendard".to_string(), "malgun".to_string()]);
    let (fonts, loaded) = token_font_definitions(&ui_families(), Some(vec![0u8; 4]));
    assert_eq!(loaded, 3);
    assert_eq!(&fonts.families[&egui::FontFamily::Proportional][..3], &[INTER_ID.to_owned(), PRETENDARD_ID.to_owned(), MALGUN_ID.to_owned()]);
    let mono = &fonts.families[&egui::FontFamily::Monospace];
    let defaults = egui::FontDefinitions::default();
    let default_mono = &defaults.families[&egui::FontFamily::Monospace];
    assert_eq!(&mono[..default_mono.len()], default_mono, "code keeps fixed-width fonts first");
    assert_eq!(&mono[default_mono.len()..], &[PRETENDARD_ID.to_owned(), MALGUN_ID.to_owned()], "Korean fallback follows token order");
    assert!(!mono.iter().any(|family| family == INTER_ID));
}

#[test]
fn missing_fallback_file_falls_back_to_bundled_fonts_without_panic() {
    let ctx = egui::Context::default();
    assert!(install_ui_fonts(&ctx, Path::new("Z:\\dtool\\missing\\nope.ttf")));
    let (fonts, loaded) = token_font_definitions(&ui_families(), None);
    assert_eq!(loaded, 2);
    assert!(!fonts.font_data.contains_key(MALGUN_ID));
}

#[test]
fn nothing_loads_when_no_family_is_known_and_no_fallback() {
    let (_, loaded) = token_font_definitions(&["NoSuchFont".to_string(), "malgun".to_string()], None);
    assert_eq!(loaded, 0);
    let (fonts, loaded) = token_font_definitions(&["NoSuchFont".to_string()], Some(vec![1, 2, 3]));
    assert_eq!(loaded, 1);
    assert!(fonts.font_data.contains_key(MALGUN_ID));
}

#[test]
fn tool_ids_map_to_design_icon_ids() {
    assert_eq!(dtool_design::icons::tool_icon_id("tool.move"), "tool-move");
    assert_eq!(dtool_design::icons::tool_icon_id("tool.shape_builder"), "tool-shape-builder");
    assert!(dtool_design::icons::tool_icon("tool.move").is_some());
    assert!(dtool_design::icons::icon_ids().contains(&"tool-move"));
    assert!(dtool_design::icons::icon_ids().contains(&"app.main"));
}

#[test]
fn new_document_dialog_uses_unit_captions() {
    let options = new_document_unit_options();
    assert_eq!(options, units_options());
    assert_eq!(options.iter().map(|(u, _)| *u).collect::<Vec<Units>>(), ALL_UNITS.to_vec());
    assert_eq!(options[0].1, dtool_strings::text("caption.units.px"));
    assert!(options.iter().all(|(_, caption)| !caption.starts_with("caption.")));
}
