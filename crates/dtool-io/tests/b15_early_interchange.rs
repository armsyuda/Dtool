use std::path::PathBuf;
use std::sync::Arc;

use dtool_core::workspace::{SharedServices, Workspace};
use dtool_io::{InterchangeFormat, import_interchange, open_interchange};
use lopdf::{Dictionary, Document as Pdf, Object, Stream, dictionary};

const NOT_IMPLEMENTED: &str = "NOT_IMPLEMENTED";
const UNSUPPORTED_PROFILE: &str = "UNSUPPORTED_PROFILE";
const NO_DOCUMENT: &str = "NO_DOCUMENT";

const SVG_BYTES: &[u8] =
    b"<svg width=\"10\" height=\"10\"><rect width=\"10\" height=\"10\"/></svg>";
const AI_HEADER_ONLY_BYTES: &[u8] = b"%PDF-1.5\n%%Creator: Adobe Illustrator\n%%EOF\n";
const AI_LEGACY_BYTES: &[u8] = b"%!PS-Adobe-3.0\n%%Creator: Adobe Illustrator\n";

fn workspace() -> Workspace {
    Workspace::new(Arc::new(SharedServices::default()))
}

fn pdf_bytes() -> Vec<u8> {
    let mut pdf = Pdf::with_version("1.7");
    let pages = pdf.new_object_id();
    let content = pdf.add_object(Stream::new(
        Dictionary::new(),
        b"0 0 1 rg 10 20 30 40 re f".to_vec(),
    ));
    let page = pdf.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 200.into(), 100.into()],
        "Resources" => Dictionary::new(), "Contents" => content,
    });
    pdf.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1,
        }),
    );
    let catalog = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    pdf.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).expect("valid PDF fixture");
    bytes
}

fn sample_path(stem: &str, extension: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("opencode").join(format!(
        "dtool_b15_early_interchange_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("sample dir");
    dir.join(stem).with_extension(extension)
}

fn sample(stem: &str, format: InterchangeFormat, bytes: &[u8]) -> PathBuf {
    let path = sample_path(stem, format.extension());
    std::fs::write(&path, bytes).expect("sample file");
    path
}

fn assert_opened_with_objects(ws: &Workspace, id: dtool_core::ContextId) {
    assert_eq!(ws.active(), Some(id));
    assert_eq!(ws.contexts().len(), 1);
    assert!(!ws.get(id).expect("opened context").session.document.objects.is_empty());
}

#[test]
fn failed_interchange_open_leaves_the_active_document_alone() {
    let mut ws = workspace();
    let target = ws.new_document("Target");
    let before = serde_json::to_value(&ws.get(target).unwrap().session.document).unwrap();
    let generation = ws.get(target).unwrap().session.generation;
    let truncated = sample("truncated_pdf", InterchangeFormat::Pdf, &pdf_bytes()[..40]);
    assert!(open_interchange(&mut ws, &truncated).is_err());
    let missing = sample_path("missing_svg", "svg");
    let _ = std::fs::remove_file(&missing);
    assert!(open_interchange(&mut ws, &missing).is_err());
    let indd = sample("still_x", InterchangeFormat::Indd, b"not indd");
    assert_eq!(open_interchange(&mut ws, &indd).expect_err("indd stays unsupported").code, NOT_IMPLEMENTED);
    assert_eq!(ws.contexts().len(), 1);
    assert_eq!(ws.active(), Some(target));
    assert_eq!(serde_json::to_value(&ws.get(target).unwrap().session.document).unwrap(), before);
    assert_eq!(ws.get(target).unwrap().session.generation, generation);
}

#[test]
fn pdf_open_creates_a_tab_with_mapped_objects() {
    let path = sample("open_pdf", InterchangeFormat::Pdf, &pdf_bytes());
    assert_eq!(
        InterchangeFormat::from_path(&path),
        Some(InterchangeFormat::Pdf)
    );
    let mut ws = workspace();
    let id = open_interchange(&mut ws, &path).expect("pdf open imports");
    assert_opened_with_objects(&ws, id);
}

#[test]
fn svg_open_creates_a_tab_with_mapped_objects() {
    let path = sample("open_svg", InterchangeFormat::Svg, SVG_BYTES);
    assert_eq!(
        InterchangeFormat::from_path(&path),
        Some(InterchangeFormat::Svg)
    );
    let mut ws = workspace();
    let id = open_interchange(&mut ws, &path).expect("svg open imports");
    assert_opened_with_objects(&ws, id);
}

#[test]
fn pdf_compatible_ai_open_creates_a_tab_with_mapped_objects() {
    let path = sample(
        "open_ai_pdf",
        InterchangeFormat::PdfCompatibleAi,
        &pdf_bytes(),
    );
    assert!(dtool_io::is_pdf_compatible(&path));
    let profile = dtool_io::admit_ai(&path).expect("real PDF-compatible AI admitted");
    assert!(profile.supported);
    assert_eq!(profile.pages, 1);
    let mut ws = workspace();
    let id = open_interchange(&mut ws, &path).expect("pdf-compatible ai open imports");
    assert_opened_with_objects(&ws, id);
}

#[test]
fn ai_without_pdf_stream_is_rejected() {
    let path = sample(
        "open_ai_legacy",
        InterchangeFormat::PdfCompatibleAi,
        AI_LEGACY_BYTES,
    );
    assert!(!dtool_io::is_pdf_compatible(&path));
    let mut ws = workspace();
    let err = open_interchange(&mut ws, &path).expect_err("legacy ai must be rejected");
    assert_eq!(err.code, UNSUPPORTED_PROFILE);
    assert_eq!(err.message_key, "error.ai.no_pdf");
    assert!(ws.contexts().is_empty());
    let target = ws.new_document("Target");
    let err = import_interchange(&mut ws, &path).expect_err("legacy AI import must be rejected");
    assert_eq!(err.code, UNSUPPORTED_PROFILE);
    assert_eq!(err.message_key, "error.ai.no_pdf");
    assert_eq!(ws.active(), Some(target));
    assert_eq!(ws.contexts().len(), 1);
}

#[test]
fn header_only_ai_is_not_pdf_compatible() {
    let path = sample(
        "header_only_ai",
        InterchangeFormat::PdfCompatibleAi,
        AI_HEADER_ONLY_BYTES,
    );
    assert!(!dtool_io::is_pdf_compatible(&path));
    let mut ws = workspace();
    let err = open_interchange(&mut ws, &path).expect_err("header alone is not a PDF");
    assert_eq!(err.code, UNSUPPORTED_PROFILE);
    assert_eq!(err.message_key, "error.ai.no_pdf");
    assert!(ws.contexts().is_empty());
}

#[test]
fn import_without_document_returns_no_document() {
    let path = sample("import_none", InterchangeFormat::Pdf, &pdf_bytes());
    let mut ws = workspace();
    let err = import_interchange(&mut ws, &path).expect_err("import needs a document");
    assert_eq!(err.code, NO_DOCUMENT);
}

#[test]
fn import_pdf_svg_and_ai_place_into_current_document() {
    let pdf = pdf_bytes();
    let cases = [
        ("import_pdf", InterchangeFormat::Pdf, pdf.as_slice()),
        ("import_svg", InterchangeFormat::Svg, SVG_BYTES),
        (
            "import_ai_pdf",
            InterchangeFormat::PdfCompatibleAi,
            pdf.as_slice(),
        ),
    ];
    for (stem, format, bytes) in cases {
        let path = sample(stem, format, bytes);
        let mut ws = workspace();
        let target = ws.new_document("Target");
        let before = ws.get(target).unwrap().session.document.objects.len();
        let result = import_interchange(&mut ws, &path);
        assert_eq!(ws.contexts().len(), 1, "{stem}: place must not open a new document");
        assert_eq!(ws.active(), Some(target));
        if format == InterchangeFormat::PdfCompatibleAi && !dtool_io::is_pdf_compatible(&path) {
            assert_eq!(result.expect_err("plain PDF bytes are not an AI file").code, UNSUPPORTED_PROFILE);
            assert_eq!(ws.get(target).unwrap().session.document.objects.len(), before);
            continue;
        }
        result.unwrap_or_else(|error| panic!("{stem}: {error:?}"));
        let session = &mut ws.get_mut(target).unwrap().session;
        assert!(session.document.objects.len() > before, "{stem}: nothing placed");
        session.run(&dtool_core::Command::new("edit.undo")).expect("undo place");
        assert_eq!(session.document.objects.len(), before, "{stem}: one undo removes the place");
    }
    let mut ws = workspace();
    ws.new_document("Target");
    let path = sample("import_indd", InterchangeFormat::Indd, b"not indd");
    assert_eq!(import_interchange(&mut ws, &path).expect_err("indd stays unsupported").code, NOT_IMPLEMENTED);
}

#[test]
fn every_interchange_format_is_named() {
    let names: Vec<&str> = InterchangeFormat::ALL
        .iter()
        .map(|f| f.extension())
        .collect();
    assert_eq!(
        names,
        vec![
            "pdf", "svg", "ai", "eps", "ait", "psd", "idml", "indd", "pub", "dwg", "dxf"
        ]
    );
    for format in InterchangeFormat::ALL {
        let path = PathBuf::from("sample").with_extension(format.extension());
        assert_eq!(InterchangeFormat::from_path(&path), Some(format));
    }
}

#[test]
fn native_path_still_uses_the_real_open_entry() {
    let path = sample_path("native_open", "dtool");
    let mut doc =
        dtool_core::Document::new([123.0, 456.0], dtool_core::document::Units::Pt, 144.0, 1);
    doc.create_object(dtool_core::Object {
        name: "persisted native body".into(),
        pos: [3.0, 4.0],
        size: [5.0, 6.0],
        ..Default::default()
    });
    dtool_io::save(&doc, &path).expect("save real native document");
    let original = std::fs::read(&path).expect("native fixture");
    let mut ws = workspace();
    let id =
        open_interchange(&mut ws, &path).expect("native open goes through Workspace::open_path");
    assert_eq!(ws.active(), Some(id));
    assert_eq!(ws.contexts().len(), 1);
    let context = ws.get(id).expect("opened context");
    assert_eq!(context.path.as_deref(), Some(path.as_path()));
    assert_eq!(context.session.document.pages[0].size, [123.0, 456.0]);
    assert_eq!(
        context.session.document.objects[0].name,
        "persisted native body"
    );
    assert_eq!(context.session.document.objects[0].pos, [3.0, 4.0]);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn empty_native_file_is_rejected_without_a_tab() {
    let path = sample_path("empty_native", "dtool");
    std::fs::write(&path, b"").expect("invalid native fixture");
    let mut ws = workspace();
    let err = open_interchange(&mut ws, &path).expect_err("empty native file is not a document");
    assert_eq!(err.code, "IO.INVALID_FORMAT");
    assert_eq!(err.message_key, "error.io.invalid_format");
    assert!(ws.contexts().is_empty());
    assert!(std::fs::read(&path).unwrap().is_empty());
}
