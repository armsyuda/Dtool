use dtool_core::document::Units;
use dtool_core::transform::parse_field;

fn close(a: f32, b: f32) -> bool { (a - b).abs() < 1e-3 }

#[test]
fn brief_cases() {
    assert_eq!(parse_field("10+5", Units::Px, 72.0).unwrap(), 15.0);
    assert!(close(parse_field("1in", Units::Px, 72.0).unwrap(), 72.0));
    assert!(parse_field("invalid", Units::Px, 72.0).is_err());
}

#[test]
fn operators_parentheses_and_precedence() {
    assert_eq!(parse_field("2+3*4", Units::Px, 72.0).unwrap(), 14.0);
    assert_eq!(parse_field("(2+3)*4", Units::Px, 72.0).unwrap(), 20.0);
    assert_eq!(parse_field("100/4-5", Units::Px, 72.0).unwrap(), 20.0);
    assert_eq!(parse_field(" -(3-1) ", Units::Px, 72.0).unwrap(), -2.0);
}

#[test]
fn suffix_applies_to_whole_expression_and_default_units_use_dpi() {
    assert!(close(parse_field("(1+1)in", Units::Px, 72.0).unwrap(), 144.0));
    assert!(close(parse_field("10+15 mm", Units::Px, 72.0).unwrap(), parse_field("25mm", Units::Px, 72.0).unwrap()));
    assert!(close(parse_field("10+15mm", Units::Px, 72.0).unwrap(), 10.0 + 15.0 / 25.4 * 72.0));
    assert!(close(parse_field("(10+15)mm", Units::Px, 300.0).unwrap(), 25.0 / 25.4 * 300.0));
    assert!(close(parse_field("1+1", Units::In, 300.0).unwrap(), 600.0));
    assert!(close(parse_field("1in+36pt", Units::Px, 72.0).unwrap(), 108.0));
}

#[test]
fn bad_input_is_rejected() {
    for text in ["", "1+", "(1+2", "1/0", "2mm*3mm", "10xyz", "(1+1)%", "1in in"] {
        assert!(parse_field(text, Units::Px, 72.0).is_err(), "{text}");
    }
}
