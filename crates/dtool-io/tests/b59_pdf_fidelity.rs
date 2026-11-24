use std::path::PathBuf;

use dtool_core::document::Document;
use dtool_io::import_pdf::{FidelityManifest, fidelity_check};

#[test]
fn test_b59_pdf_fidelity_fixture() {
    let fixture_path = PathBuf::from("tests/fixtures/b59_a4_fixture.pdf");
    
    if !fixture_path.exists() {
        return;
    }

    let opts = dtool_io::import_pdf::ImportOptions::default();
    let (doc, _report) = dtool_io::import_pdf::import_pdf(&fixture_path, &opts).unwrap();

    let expected = FidelityManifest {
        objects: vec![
            ("rect_10_20".to_string(), [10.0, 20.0, 80.0, 40.0]),
        ],
        texts: vec![
            "Korean Text".to_string(),
            "English Text".to_string(),
        ],
        colours: vec![
            "CMYK(0.1, 0.2, 0.3, 0.4)".to_string(),
        ],
    };

    let mismatches = fidelity_check(&doc, &expected, 0.01);
    assert!(mismatches.is_empty(), "Fidelity check failed: {:?}", mismatches);
}

#[test]
fn test_b59_pdf_fidelity_mismatch() {
    let fixture_path = PathBuf::from("tests/fixtures/b59_a4_fixture.pdf");
    
    if !fixture_path.exists() {
        return;
    }

    let opts = dtool_io::import_pdf::ImportOptions::default();
    let (doc, _report) = dtool_io::import_pdf::import_pdf(&fixture_path, &opts).unwrap();

    let expected_wrong = FidelityManifest {
        objects: vec![
            ("rect_10_20".to_string(), [15.0, 20.0, 80.0, 40.0]),
        ],
        texts: vec![
            "Korean Text".to_string(),
            "English Text".to_string(),
        ],
        colours: vec![
            "CMYK(0.1, 0.2, 0.3, 0.4)".to_string(),
        ],
    };

    let mismatches = fidelity_check(&doc, &expected_wrong, 0.01);
    assert!(!mismatches.is_empty());
    assert!(mismatches[0].contains("geometry mismatch"));
}
