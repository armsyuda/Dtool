pub use dtool_text::fonts;

#[allow(dead_code)]
#[path = "../src/glyphs.rs"]
mod glyphs;

use dtool_core::command::Session;
use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::Command;
use fonts::{FontId, FontLibrary};
use glyphs::{glyph_insert_command, query_glyphs, GlyphQuery, RecentGlyphs, Subset};

fn korean_font(lib: &FontLibrary) -> Option<FontId> {
    let found = lib.resolve("Malgun Gothic", "Regular");
    if found.is_none() {
        eprintln!("skipped: Malgun Gothic is not installed, the glyph tests need a Hangul font");
    }
    found
}

fn query(lib: &FontLibrary, font: FontId, search: Option<&str>, offset: usize, limit: usize) -> glyphs::GlyphPage {
    query_glyphs(lib, &GlyphQuery { font, search: search.map(str::to_owned), subset: Subset::All, offset, limit }, &RecentGlyphs::new()).expect("query")
}

fn session_with_story(text: &str) -> Session {
    let mut session = Session::new(Document::new([200.0, 200.0], Units::Px, 72.0, 1));
    session.has_document = true;
    session.document.stories.push(Story { id: StoryId(41), text: text.into(), ..Story::default() });
    session
}

fn story_text(session: &Session) -> String {
    session.document.stories.iter().find(|story| story.id == StoryId(41)).unwrap().text.clone()
}

#[test]
fn c10_fix10_search_ac00_has_no_duplicates_and_ga_first() {
    let lib = FontLibrary::system();
    let Some(font) = korean_font(&lib) else { return };
    let page = query(&lib, font, Some("U+AC00"), 0, 64);
    assert!(!page.items.is_empty());
    assert_eq!(page.items[0].unicode, Some('가'));
    let mut gids: Vec<u16> = page.items.iter().map(|g| g.gid).collect();
    gids.sort_unstable();
    gids.dedup();
    assert_eq!(gids.len(), page.items.len());
    let by_char = query(&lib, font, Some("가"), 0, 64);
    assert_eq!(by_char.items[0].unicode, Some('가'));
}

#[test]
fn c10_fix10_unencoded_glyph_has_no_unicode() {
    let lib = FontLibrary::system();
    let Some(font) = korean_font(&lib) else { return };
    let page = query(&lib, font, None, 0, 4);
    assert_eq!(page.items[0].gid, 0);
    assert_eq!(page.items[0].unicode, None);
    let all = query(&lib, font, None, 0, usize::MAX);
    let mut gids: Vec<u16> = all.items.iter().map(|g| g.gid).collect();
    gids.dedup();
    assert_eq!(gids.len(), all.items.len());
}

#[test]
fn c10_fix10_recent_lists_last_inserted_first() {
    let mut recent = RecentGlyphs::new();
    recent.push(FontId(1), 10);
    recent.push(FontId(1), 11);
    recent.push(FontId(1), 10);
    assert_eq!(recent.items, vec![(FontId(1), 10), (FontId(1), 11)]);
    let mut session = session_with_story("AB");
    for gid in [300u16, 301] {
        let mut command = Command::new("text.glyph.insert");
        command.params["story"] = 41u64.into();
        command.params["at"] = 0u64.into();
        command.params["font"] = 9_001u64.into();
        command.params["gid"] = gid.into();
        command.params["char"] = "x".into();
        session.run(&command).unwrap();
    }
    let core_recent = dtool_core::text_cmd::recent_glyphs();
    let mine: Vec<_> = core_recent.iter().filter(|(font, _)| *font == 9_001).collect();
    assert_eq!(mine[0], &(9_001, 301));
    assert_eq!(mine[1], &(9_001, 300));
}

#[test]
fn c10_fix10_insert_ga_resolves_char_and_is_one_undo() {
    let lib = FontLibrary::system();
    let Some(font) = korean_font(&lib) else { return };
    let page = query(&lib, font, Some("U+AC00"), 0, 1);
    let ga = page.items[0].clone();
    let mut session = session_with_story("AB");
    let mut bare = Command::new("text.glyph.insert");
    bare.params["story"] = 41u64.into();
    bare.params["at"] = 1u64.into();
    bare.params["font"] = u64::from(font.0).into();
    bare.params["gid"] = ga.gid.into();
    let reply = session.run(&bare).unwrap();
    assert_eq!(reply["encoded"], true);
    assert_eq!(story_text(&session), "A가B");
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&session), "AB");
    session.run(&glyph_insert_command(41, 1, font, &ga)).unwrap();
    assert_eq!(story_text(&session), "A가B");
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(story_text(&session), "AB");
    let notdef = query(&lib, font, None, 0, 1).items[0].clone();
    let span = glyph_insert_command(41, 1, font, &notdef);
    assert!(span.params.get("char").is_none());
    let reply = session.run(&span).unwrap();
    assert_eq!(reply["encoded"], false);
}