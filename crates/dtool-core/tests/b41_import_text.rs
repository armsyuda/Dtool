use dtool_core::document::{Document, Units};
use dtool_core::text_import::{import_bytes, ImportFormat};
use dtool_core::Session;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

fn session() -> Session {
    Session::new(Document::new([800.0, 600.0], Units::Px, 96.0, 1))
}

fn temp(name: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("d01_b41_{}_{}", std::process::id(), name));
    std::fs::write(&path, bytes).unwrap();
    path
}

fn import(session: &mut Session, path: &PathBuf) -> Result<Value, dtool_protocol::CommandError> {
    session.run_named("text.import", json!({"path": path.to_string_lossy()}))
}

fn story_of(session: &Session, out: &Value) -> dtool_core::document::Story {
    let id = out["story"].as_u64().unwrap();
    session.document.stories.iter().find(|story| story.id.0 == id).unwrap().clone()
}

fn bold_span(story: &dtool_core::document::Story) -> Option<(u64, u64)> {
    story.ranges.iter().find(|range| range["weight"] == json!(700)).map(|range| (range["start"].as_u64().unwrap(), range["end"].as_u64().unwrap()))
}

const RTF: &str = r"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}{\*\generator test;}\f0\fs24 Hello \b bold\b0  world\par Second para\par}";

fn docx_bytes() -> Vec<u8> {
    let document = r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="w"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/><w:rPr><w:b/></w:rPr></w:pPr><w:r><w:t>Title &amp; one</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve">Plain </w:t></w:r><w:r><w:rPr><w:b/><w:sz w:val="28"/></w:rPr><w:t>bold</w:t></w:r><w:r><w:rPr><w:b w:val="0"/></w:rPr><w:t xml:space="preserve"> end</w:t></w:r></w:p></w:body></w:document>"#;
    let styles = r#"<?xml version="1.0"?><w:styles xmlns:w="w"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("word/document.xml", options).unwrap();
    writer.write_all(document.as_bytes()).unwrap();
    writer.start_file("word/styles.xml", options).unwrap();
    writer.write_all(styles.as_bytes()).unwrap();
    writer.finish().unwrap().into_inner()
}

#[test]
fn txt_utf16_bom_crlf_gives_two_paragraphs() {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in "첫 문단\r\n둘째 문단\r\n".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let mut s = session();
    let path = temp("a.txt", &bytes);
    let out = import(&mut s, &path).unwrap();
    let story = story_of(&s, &out);
    assert_eq!(story.text, "첫 문단\n둘째 문단\n");
    assert_eq!(story.text.trim_end().split('\n').count(), 2);
    let utf8 = import_bytes(ImportFormat::Txt, "a\r\nb".as_bytes()).unwrap();
    assert_eq!(utf8.text, "a\nb");
}

#[test]
fn rtf_two_paragraphs_and_bold_run() {
    let parsed = import_bytes(ImportFormat::Rtf, RTF.as_bytes()).unwrap();
    assert_eq!(parsed.text, "Hello bold world\nSecond para");
    let mut s = session();
    let path = temp("a.rtf", RTF.as_bytes());
    let out = import(&mut s, &path).unwrap();
    let story = story_of(&s, &out);
    assert_eq!(story.text, "Hello bold world\nSecond para");
    assert_eq!(bold_span(&story), Some((6, 10)));
    assert!(story.ranges.iter().all(|range| range["size_pt"] == json!(12.0)));
}

#[test]
fn rtf_unicode_and_escapes() {
    let parsed = import_bytes(ImportFormat::Rtf, br"{\rtf1\uc1 \u54620?\u44544? \{x\} \i it\i0\par}").unwrap();
    assert_eq!(parsed.text, "한글 {x} it");
    assert!(parsed.runs.iter().any(|run| run.italic && run.start == 7 && run.end == 9));
}

#[test]
fn docx_two_paragraphs_bold_run_and_named_style() {
    let bytes = docx_bytes();
    let mut s = session();
    let path = temp("a.docx", &bytes);
    let out = import(&mut s, &path).unwrap();
    let story = story_of(&s, &out);
    assert_eq!(story.text, "Title & one\nPlain bold end");
    assert_eq!(bold_span(&story), Some((18, 22)));
    let bold = story.ranges.iter().find(|range| range["weight"] == json!(700)).unwrap();
    assert_eq!(bold["size_pt"], json!(14.0));
    assert!(story.ranges.iter().any(|range| range["start"] == json!(0) && range["text_style"] == json!("heading 1")));
}

#[test]
fn bad_files_error_without_change() {
    let mut s = session();
    let objects = s.document.objects.len();
    let stories = s.document.stories.len();
    let docx = temp("bad.docx", b"not a zip");
    assert!(import(&mut s, &docx).is_err());
    let rtf = temp("bad.rtf", br"{\rtf1 unclosed");
    assert!(import(&mut s, &rtf).is_err());
    let plain = temp("bad2.rtf", b"plain text");
    assert!(import(&mut s, &plain).is_err());
    assert_eq!(s.document.objects.len(), objects);
    assert_eq!(s.document.stories.len(), stories);
}

#[test]
fn linked_rtf_update_keeps_bold() {
    let mut s = session();
    let path = temp("linked.rtf", RTF.as_bytes());
    let out = s.run_named("text.import", json!({"path": path.to_string_lossy(), "linked": true})).unwrap();
    let id = out["story"].as_u64().unwrap();
    std::fs::write(&path, br"{\rtf1 One \b two\b0\par}").unwrap();
    s.run_named("text.import.update", json!({"story": id})).unwrap();
    let story = story_of(&s, &out);
    assert_eq!(story.text, "One two");
    assert_eq!(bold_span(&story), Some((4, 7)));
}
