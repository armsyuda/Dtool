use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_text::fonts::{font_generation, note_font_change};
use dtool_text::FontLibrary;
use dtool_tools::frame_text::FrameTextTool;

#[test]
fn k11_font_generation_invalidates_flow_marks_cache() {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    session.has_document = true;
    let mut tool = FrameTextTool::new();
    tool.flow_marks(&session);
    let first = tool.flow_marks_key().expect("cached");
    tool.flow_marks(&session);
    assert_eq!(tool.flow_marks_key(), Some(first));
    let generation = note_font_change(1, 2);
    tool.flow_marks(&session);
    let second = tool.flow_marks_key().expect("recached");
    assert_ne!(second, first);
    assert!(second.2 >= generation);
}

#[test]
fn k11_no_op_font_change_keeps_generation() {
    let now = font_generation();
    let same = note_font_change(42, 42);
    assert!(same >= now);
    let library = FontLibrary::shared();
    let sig = library.signature();
    assert_eq!(sig, FontLibrary::shared().signature());
    let before = font_generation();
    assert_eq!(note_font_change(sig, library.signature()), font_generation());
    assert!(font_generation() >= before);
}