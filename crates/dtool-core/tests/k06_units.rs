use dtool_core::document::Units;
use dtool_core::units::{from_px, parse_length, parse_number, pt_to_px, px_to_pt, to_px};

fn close(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
}

#[test]
fn parse_length_suffixes() {
    assert!(close(parse_length("10mm", Units::Px, 96.0).unwrap(), 37.795, 0.001));
    assert!(close(parse_length("1in", Units::Px, 300.0).unwrap(), 300.0, 1e-4));
    assert!(close(parse_length("72pt", Units::Px, 96.0).unwrap(), 96.0, 1e-4));
    assert!(close(parse_length("1pc", Units::Px, 72.0).unwrap(), 12.0, 1e-4));
    assert!(close(parse_length("5", Units::Mm, 96.0).unwrap(), 18.898, 0.001));
}

#[test]
fn parse_length_expressions() {
    assert!(close(parse_length("10mm+2mm", Units::Px, 96.0).unwrap(), 45.354, 0.001));
    assert!(close(parse_length("(1in-0.5in)*2", Units::Px, 100.0).unwrap(), 100.0, 1e-4));
    assert!(close(parse_length("10mm/2", Units::Px, 96.0).unwrap(), 18.898, 0.001));
    assert_eq!(parse_length("-5px", Units::Px, 96.0).unwrap(), -5.0);
    assert_eq!(parse_length("2*3", Units::Px, 96.0).unwrap(), 6.0);
}

#[test]
fn parse_length_errors() {
    for text in ["", "abc", "10qq", "(1", "1/0", "1e40", "1mm*1mm"] {
        assert_eq!(parse_length(text, Units::Px, 96.0).unwrap_err().code, "INVALID_PARAMS", "{text}");
    }
}

#[test]
fn round_trip_each_unit_at_72_96_300() {
    for unit in [Units::Px, Units::Pt, Units::Mm, Units::Cm, Units::In] {
        for dpi in [72.0, 96.0, 300.0] {
            let value = 12.5;
            let back = from_px(to_px(value, unit, dpi).unwrap(), unit, dpi).unwrap();
            assert!(close(back, value, 1e-4), "{unit:?} {dpi}");
        }
    }
}

#[test]
fn px_pt_boundary() {
    assert_eq!(px_to_pt(96.0, 96.0), 72.0);
    assert_eq!(pt_to_px(72.0, 300.0), 300.0);
    assert_eq!(px_to_pt(10.0, 0.0), 10.0);
}

#[test]
fn parse_number_percent_degree() {
    assert_eq!(parse_number("50%").unwrap(), 50.0);
    assert_eq!(parse_number("45°").unwrap(), 45.0);
    assert_eq!(parse_number("90/2").unwrap(), 45.0);
    assert!(parse_number("5mm").is_err());
}
