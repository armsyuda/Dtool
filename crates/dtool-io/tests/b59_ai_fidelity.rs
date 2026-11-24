use dtool_io::import_ai::import_ai;
use dtool_io::{
    FidelityManifest, ImportAs, ImportColour, ImportOptions, LossKind, PageSel, fidelity_check,
};
use lopdf::{Dictionary, Document as Pdf, Object, Stream, dictionary};
use std::fs;

fn write_fixture(path: &std::path::Path) {
    let mut pdf = Pdf::with_version("1.7");
    let pages = pdf.new_object_id();
    let font = pdf.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
    let content = pdf.add_object(Stream::new(
        Dictionary::new(),
        b"BT /F1 24 Tf 50 250 Td (Hello AI) Tj ET q 1 0 0 rg 60 60 100 80 re f Q".to_vec(),
    ));
    let page = pdf.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 400.into(), 300.into()], "Contents" => content,
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
    });
    pdf.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    pdf.save(path).expect("valid PDF-compatible AI fixture");
}

#[test]
fn ai_fidelity_preserves_body_and_reports_font_approximation() {
    let dir = std::env::temp_dir()
        .join("opencode")
        .join(format!("dtool_b59_ai_fidelity_test_{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    let fixture_path = dir.join("fixture.ai");
    write_fixture(&fixture_path);
    let original = fs::read(&fixture_path).expect("fixture bytes");

    let opts = ImportOptions {
        pages: PageSel::All,
        import_as: ImportAs::Artboards,
        dpi: 300,
        colour: ImportColour::Rgb,
        favour_editable_text: true,
        group_text_lines: false,
        password: None,
        fonts: vec![],
    };

    let (doc, report) = import_ai(&fixture_path, &opts).expect("import_ai should succeed");

    assert_eq!((report.objects, report.texts, report.images), (1, 1, 0));
    assert_eq!(doc.pages[0].size, [400.0, 300.0]);
    assert_eq!(doc.objects.iter().filter(|obj| !obj.is_group()).count(), 2);
    assert_eq!(doc.stories[0].text, "Hello AI");
    assert_eq!(doc.stories[0].ranges[0]["size_pt"], 24.0);
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.kind == LossKind::Font && loss.required)
    );
    assert!(
        !report.is_acceptable(),
        "font metric and ink-bound approximations must not be hidden"
    );

    let expected = FidelityManifest {
        objects: vec![("rect_60_160".into(), [60.0, 160.0, 100.0, 80.0])],
        texts: vec!["Hello AI".into()],
        colours: vec!["#ff0000".into()],
    };

    let warnings = fidelity_check(&doc, &expected, 0.01);

    assert!(
        warnings.is_empty(),
        "fidelity_check must return empty for AI fixture, got: {:?}",
        warnings
    );
    assert_eq!(fs::read(&fixture_path).unwrap(), original);

    let _ = fs::remove_file(&fixture_path);
}

#[test]
fn ai_fidelity_warning_is_not_pass() {
    let dir = std::env::temp_dir().join("opencode").join(format!(
        "dtool_b59_ai_fidelity_warn_test_{}",
        std::process::id()
    ));
    let _ = fs::create_dir_all(&dir);
    let fixture_path = dir.join("fixture.ai");
    write_fixture(&fixture_path);

    let opts = ImportOptions {
        pages: PageSel::All,
        import_as: ImportAs::Artboards,
        dpi: 300,
        colour: ImportColour::Rgb,
        favour_editable_text: true,
        group_text_lines: false,
        password: None,
        fonts: vec![],
    };

    let (doc, _report) = import_ai(&fixture_path, &opts).expect("import_ai should succeed");

    let expected = FidelityManifest {
        objects: vec![("nonexistent".to_string(), [0.0, 0.0, 100.0, 100.0])],
        texts: vec!["Wrong text".to_string()],
        colours: vec![],
    };

    let warnings = fidelity_check(&doc, &expected, 0.01);

    assert!(
        !warnings.is_empty(),
        "fidelity_check must return non-empty when manifest does not match"
    );

    let _ = fs::remove_file(&fixture_path);
}
