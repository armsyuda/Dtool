use dtool_core::paint::{Colour, Paint};
use dtool_design::WellValue;
use dtool_ui::fill_stroke::{well_value, WellState};

#[test]
fn well_value_maps_mixed_none_and_colour() {
    assert_eq!(well_value(&WellState::Mixed), WellValue::Mixed);
    assert_eq!(well_value(&WellState::None), WellValue::None);
    assert_eq!(well_value(&WellState::Single(Paint::None)), WellValue::None);
    let red = Paint::Solid { colour: Colour::new(255, 0, 0, 255) };
    assert_eq!(well_value(&WellState::Single(red)), WellValue::Colour(egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255)));
}

#[test]
fn migrated_sites_use_shared_helpers() {
    let dialogs = include_str!("../src/dialogs.rs");
    assert!(!dialogs.contains("key_pressed(egui::Key::Escape)"));
    assert!(!dialogs.contains("key_pressed(egui::Key::Enter)"));
    assert!(include_str!("../src/dialogs/locked_delete.rs").contains("dialog_keys"));
    assert!(include_str!("../src/dialogs/document_setup.rs").contains("dialog_keys"));
    assert!(!include_str!("../src/context_bar.rs").contains("egui::ComboBox"));
    assert!(include_str!("../src/fill_stroke.rs").contains("WellValue::Mixed"));
}
