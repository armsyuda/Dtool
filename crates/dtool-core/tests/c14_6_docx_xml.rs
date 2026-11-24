use dtool_core::text_import::{import_bytes, ImportFormat};
use std::io::Write;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn docx(document: &str, styles: Option<&str>) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(document.as_bytes()).unwrap();
        if let Some(styles) = styles {
            zip.start_file("word/styles.xml", options).unwrap();
            zip.write_all(styles.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    out.into_inner()
}

fn body(inner: &str) -> String {
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="{W}"><w:body>{inner}</w:body></w:document>"#)
}

#[test]
fn docx_paragraphs_runs_and_named_styles() {
    let styles = format!(r#"<w:styles xmlns:w="{W}"><w:style w:styleId="Heading1"><w:name w:val="heading 1"/></w:style><w:style w:styleId="Em"><w:name w:val="Emphasis"/></w:style></w:styles>"#);
    let document = body(r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Title</w:t></w:r></w:p><w:p><w:r><w:rPr><w:b/><w:sz w:val="28"/></w:rPr><w:t xml:space="preserve">Bold </w:t></w:r><w:r><w:rPr><w:i w:val="0"/><w:rStyle w:val="Em"/></w:rPr><w:t>A &amp; B</w:t><w:tab/><w:t>x</w:t><w:br/></w:r></w:p>"#);
    let imported = import_bytes(ImportFormat::Docx, &docx(&document, Some(&styles))).expect("docx imports");
    assert_eq!(imported.text, "Title\nBold A & B\tx");
    let title = imported.runs.iter().find(|run| run.start == 0).expect("title run");
    assert_eq!(title.style.as_deref(), Some("heading 1"));
    let bold = imported.runs.iter().find(|run| run.start == 6).expect("bold run");
    assert!(bold.bold);
    assert_eq!(bold.size_pt, Some(14.0));
    let em = imported.runs.iter().find(|run| run.start == 11).expect("styled run");
    assert!(!em.italic);
    assert_eq!(em.style.as_deref(), Some("Emphasis"));
}

#[test]
fn docx_without_body_is_rejected() {
    let document = format!(r#"<w:document xmlns:w="{W}"></w:document>"#);
    assert!(import_bytes(ImportFormat::Docx, &docx(&document, None)).is_err());
}

#[test]
fn docx_xml_edge_cases_parse_like_a_real_xml_parser() {
    let cdata = body("<w:p><w:r><w:t><![CDATA[a<b]]></w:t></w:r></w:p>");
    assert_eq!(import_bytes(ImportFormat::Docx, &docx(&cdata, None)).expect("cdata").text, "a<b");
    let gt = body(r#"<w:p><w:pPr><w:pStyle w:val="a>b"/></w:pPr><w:r><w:t>x</w:t></w:r></w:p>"#);
    let imported = import_bytes(ImportFormat::Docx, &docx(&gt, None)).expect("raw > in attribute");
    assert_eq!(imported.text, "x");
    assert_eq!(imported.runs[0].style.as_deref(), Some("a>b"));
    let prefix = format!(r#"<ns0:document xmlns:ns0="{W}"><ns0:body><ns0:p><ns0:r><ns0:rPr><ns0:b/></ns0:rPr><ns0:t>p</ns0:t></ns0:r></ns0:p></ns0:body></ns0:document>"#);
    let imported = import_bytes(ImportFormat::Docx, &docx(&prefix, None)).expect("other prefix");
    assert_eq!(imported.text, "p");
    assert!(imported.runs[0].bold);
    let broken = body("<w:p><w:r><w:t>open</w:r></w:p>");
    assert!(import_bytes(ImportFormat::Docx, &docx(&broken, None)).is_err());
}