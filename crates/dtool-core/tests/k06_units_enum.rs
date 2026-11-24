use dtool_core::doc_setup::unit_factor;
use dtool_core::document::Units;

#[test]
fn old_setup_unit_names_still_load() {
    for (name, unit, written) in [("Points", Units::Pt, "Pt"), ("Millimeters", Units::Mm, "Mm"), ("Inches", Units::In, "In"), ("Pixels", Units::Px, "Px")] {
        let parsed: Units = serde_json::from_value(serde_json::json!(name)).unwrap();
        assert_eq!(parsed, unit);
        assert_eq!(serde_json::to_value(parsed).unwrap(), serde_json::json!(written));
    }
}

#[test]
fn unit_factor_matches_units_module() {
    assert!((unit_factor(Units::Mm, 96.0, Units::In, 96.0) - 1.0 / 25.4).abs() < 1e-6);
    assert!((unit_factor(Units::Px, 96.0, Units::Px, 300.0) - 300.0 / 96.0).abs() < 1e-5);
    assert_eq!(unit_factor(Units::Pt, 72.0, Units::Pt, 72.0), 1.0);
}
