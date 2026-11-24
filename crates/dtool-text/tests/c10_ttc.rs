use dtool_core::command::Session;
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::Command;
use dtool_text::fonts::{FontId, FontLibrary};
use dtool_text::glyphs::{get_subsets, glyph_insert_command, query_glyphs, resolve_glyph_char, GlyphPage, GlyphQuery, RecentGlyphs, Subset};
use skrifa::raw::{FontRef, TableProvider};

fn ttc_face(lib: &FontLibrary, family: &str) -> Option<(FontId, u32, Vec<u8>)> {
    let Some(id) = lib.resolve(family, "Regular") else {
        eprintln!("skipped: {family} is not installed, the TTC tests need batang.ttc");
        return None;
    };
    let face = lib.face(id).unwrap();
    if !face.path.to_string_lossy().to_ascii_lowercase().ends_with(".ttc") {
        eprintln!("skipped: {family} is not loaded from a .ttc collection");
        return None;
    }
    Some((id, face.index, std::fs::read(&face.path).unwrap()))
}

fn page(lib: &FontLibrary, font: FontId, search: Option<&str>, limit: usize) -> GlyphPage {
    query_glyphs(lib, &GlyphQuery { font, search: search.map(str::to_owned), subset: Subset::All, offset: 0, limit }, &RecentGlyphs::new()).expect("query")
}

fn face_glyph_count(data: &[u8], index: u32) -> usize {
    usize::from(FontRef::from_index(data, index).unwrap().maxp().unwrap().num_glyphs())
}

#[test]
fn c10_ttc_index_above_zero_queries_its_own_face() {
    let lib = FontLibrary::system();
    let Some((font, index, data)) = ttc_face(&lib, "BatangChe") else { return };
    assert!(index > 0);
    let all = page(&lib, font, None, 32);
    assert!(!all.items.is_empty());
    assert_eq!(all.total, face_glyph_count(&data, index));
    let ga = page(&lib, font, Some("U+AC00"), 8);
    assert_eq!(ga.items[0].unicode, Some('\u{AC00}'));
    assert_eq!(resolve_glyph_char(&lib, font, ga.items[0].gid), Some('\u{AC00}'));
    assert!(!get_subsets(&lib, font).is_empty());
}

#[test]
fn c10_ttc_index_zero_of_the_same_file_still_works() {
    let lib = FontLibrary::system();
    let Some((font, index, data)) = ttc_face(&lib, "Batang") else { return };
    assert_eq!(index, 0);
    let all = page(&lib, font, None, 32);
    assert!(!all.items.is_empty());
    assert_eq!(all.total, face_glyph_count(&data, 0));
    assert_eq!(page(&lib, font, Some("U+AC00"), 8).items[0].unicode, Some('\u{AC00}'));
}

#[test]
fn c10_ttc_insert_from_index_above_zero_is_one_undo() {
    let lib = FontLibrary::system();
    let Some((font, _, _)) = ttc_face(&lib, "BatangChe") else { return };
    let ga = page(&lib, font, Some("U+AC00"), 1).items[0].clone();
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    session.has_document = true;
    session.document.stories.push(Story { id: StoryId(41), text: "AB".into(), ..Story::default() });
    let before = session.document.clone();
    page(&lib, font, None, 64);
    assert_eq!(session.document.stories, before.stories);
    session.run(&glyph_insert_command(41, 1, font, &ga)).unwrap();
    let text = |s: &Session| s.document.stories.iter().find(|story| story.id == StoryId(41)).unwrap().text.clone();
    assert_eq!(text(&session), "A\u{AC00}B");
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(text(&session), "AB");
}