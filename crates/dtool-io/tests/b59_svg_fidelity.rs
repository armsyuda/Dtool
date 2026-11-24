use dtool_io::{FidelityManifest, LossKind, fidelity_check, import_svg};
use std::fs;
use std::path::PathBuf;

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300" viewBox="0 0 400 300">
<defs><linearGradient id="g"><stop offset="0" stop-color="#00ff00"/><stop offset="1" stop-color="#ffff00"/></linearGradient></defs>
<rect id="box" x="60" y="40" width="100" height="80" fill="#ff0000"/>
<circle id="dot" cx="300" cy="200" r="20" fill="#0000ff"/>
<rect id="band" x="10" y="250" width="380" height="30" fill="url(#g)"/>
</svg>"##;

fn fixture(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dtool_b59_svg_fidelity_{}_{}", std::process::id(), test));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fixture.svg");
    fs::write(&path, SVG).unwrap();
    path
}

#[test]
fn svg_fidelity_matches_geometry_and_colour_and_reports_gradient_loss() {
    let path = fixture("match");
    let original = fs::read(&path).unwrap();
    let (doc, report) = import_svg(&path).expect("svg imports");
    assert_eq!(report.objects, 3);
    assert!(report.losses.iter().any(|loss| loss.kind == LossKind::Gradient), "{:?}", report.losses);
    let expected = FidelityManifest {
        objects: vec![
            ("box".into(), [60.0, 40.0, 100.0, 80.0]),
            ("dot".into(), [280.0, 180.0, 40.0, 40.0]),
            ("band".into(), [10.0, 250.0, 380.0, 30.0]),
        ],
        texts: vec![],
        colours: vec!["#ff0000".into(), "#0000ff".into()],
    };
    let warnings = fidelity_check(&doc, &expected, 0.5);
    assert!(warnings.is_empty(), "svg fidelity mismatches: {warnings:?}");
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn svg_fidelity_mismatch_is_reported() {
    let path = fixture("mismatch");
    let (doc, _) = import_svg(&path).expect("svg imports");
    let expected = FidelityManifest {
        objects: vec![("box".into(), [0.0, 0.0, 10.0, 10.0])],
        texts: vec!["missing".into()],
        colours: vec!["#123456".into()],
    };
    assert_eq!(fidelity_check(&doc, &expected, 0.5).len(), 3);
}