use dtool_core::document::Units;
use dtool_core::serde_json::json;
use dtool_core::{Command, Document, ObjectId, Session, StoryId};
use dtool_text::frame::layout_doc_flow;
use dtool_text::spell_live::{pen_edges, story_underlines, underlines, underlines_in_lines, LiveSpell};
use dtool_text::spelling::Speller;
use dtool_text::story::Line;
use dtool_text::FontLibrary;
use std::time::{Duration, Instant};

const PREFERRED: [&str; 6] = ["Segoe UI", "Arial", "Calibri", "Times New Roman", "Noto Sans", "Liberation Serif"];
const TEXT: &str = "the parcel was recieved by the clerk who recieved it and then recieved another box that was recieved late";

fn family(lib: &FontLibrary) -> Option<String> {
    let have: Vec<String> = lib.families().into_iter().map(|s| s.to_string()).collect();
    PREFERRED.iter().find_map(|p| have.iter().find(|h| h.eq_ignore_ascii_case(p)).cloned())
}

fn live(lib: &FontLibrary, width: f32) -> Option<(Session, StoryId, ObjectId)> {
    let family = family(lib)?;
    let mut session = Session::new(Document::new([800.0, 800.0], Units::Px, 72.0, 1));
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [30, 40, 30.0 + width, 700], "text": TEXT}))).unwrap()["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap();
    session.run(&Command::with_params("text.style", json!({"story": story.0, "range": {"start": 0, "end": TEXT.chars().count()}, "patch": {"family": family, "size_pt": 18.0}}))).unwrap();
    Some((session, story, ObjectId(frame)))
}

fn lines(lib: &FontLibrary, session: &Session, story: StoryId) -> Vec<Line> {
    layout_doc_flow(lib, &session.document, story).expect("flow").layout.per_frame.into_iter().flatten().collect()
}

fn marks(session: &Session, story: StoryId) -> Vec<dtool_text::spelling::Misspelling> {
    let speller = Speller::en_us().expect("en_US");
    speller.check_story(session, story).unwrap()
}

fn carets(lines: &[Line], count: usize) -> Vec<[f32; 2]> {
    let mut out = vec![[0.0f32, 0.0]; count + 1];
    for line in lines {
        for (cluster, x0, x1) in pen_edges(line) {
            if cluster < count {
                out[cluster] = [x0, line.baseline];
                out[cluster + 1] = [x1, line.baseline];
            }
        }
    }
    out
}

#[test]
fn pen_edges_follow_the_renderer_origin_and_advances() {
    let lib = FontLibrary::system();
    let Some((session, story, _)) = live(&lib, 2000.0) else { eprintln!("skip: no preferred font"); return };
    let laid = lines(&lib, &session, story);
    let line = &laid[0];
    let edges: Vec<(usize, f32, f32)> = pen_edges(line).collect();
    assert_eq!(edges[0].1, line.glyphs[0].offset[0]);
    for (pair, glyph) in edges.windows(2).zip(line.glyphs.iter()) {
        assert!((pair[0].2 - pair[1].1).abs() < 1e-4);
        assert!((pair[0].2 - pair[0].1 - glyph.advance).abs() < 1e-4);
    }
}

#[test]
fn misspelling_ending_a_wrapped_line_ends_at_its_last_glyph_edge() {
    let lib = FontLibrary::system();
    let mut checked = 0;
    let mut estimates_differ = 0;
    for width in [150.0f32, 170.0, 190.0, 210.0, 230.0, 250.0, 270.0, 290.0] {
        let Some((session, story, _)) = live(&lib, width) else { eprintln!("skip: no preferred font"); return };
        let laid = lines(&lib, &session, story);
        assert!(laid.len() > 2);
        let found = marks(&session, story);
        let lines_out = underlines_in_lines(&found, &laid);
        for line in laid.iter().filter(|l| l.range.end < TEXT.chars().count()) {
            let edges: Vec<(usize, f32, f32)> = pen_edges(line).collect();
            let Some(last_letter) = edges.iter().rev().find(|(c, _, _)| TEXT.chars().nth(*c).is_some_and(|ch| ch.is_alphabetic())).copied() else { continue };
            let Some(item) = found.iter().find(|m| m.end == last_letter.0 + 1) else { continue };
            let seg = lines_out.iter().find(|u| u.start >= line.range.start && u.end == item.end).expect("segment on that line");
            let edge = last_letter.2;
            assert!((seg.x1 - edge).abs() < 1e-3, "x1 {} edge {}", seg.x1, edge);
            let prev = edges.iter().rev().find(|(c, _, _)| *c + 1 == last_letter.0).map(|e| e.2 - e.1).unwrap_or(0.0);
            let estimate = last_letter.1 + prev;
            if (estimate - edge).abs() > 1e-3 {
                estimates_differ += 1;
            }
            assert_eq!(seg.y, line.baseline);
            checked += 1;
        }
    }
    assert!(checked > 0, "no width put a misspelling at a line end");
    assert!(estimates_differ > 0, "previous-advance estimate matched every edge");
}

#[test]
fn mid_line_mark_matches_the_caret_based_underline() {
    let lib = FontLibrary::system();
    let Some((session, story, _)) = live(&lib, 2000.0) else { eprintln!("skip: no preferred font"); return };
    let laid = lines(&lib, &session, story);
    assert_eq!(laid.len(), 1);
    let found = marks(&session, story);
    assert_eq!(found[0].word, "recieved");
    let first = &found[..1];
    let new = underlines_in_lines(first, &laid);
    let old = underlines(first, &carets(&laid, TEXT.chars().count()), 18.0);
    assert_eq!(new.len(), 1);
    assert_eq!(old.len(), 1);
    assert_eq!((new[0].start, new[0].end), (old[0].start, old[0].end));
    assert!((new[0].x0 - old[0].x0).abs() < 0.5 && (new[0].x1 - old[0].x1).abs() < 0.5, "{new:?} {old:?}");
}

#[test]
fn story_underlines_are_in_world_space_and_never_write_the_document() {
    let lib = FontLibrary::system();
    let Some((mut session, story, frame)) = live(&lib, 2000.0) else { eprintln!("skip: no preferred font"); return };
    let version = session.document.version;
    let found = marks(&session, story);
    let world = story_underlines(&lib, &session.document, story, &found);
    let local = underlines_in_lines(&found, &lines(&lib, &session, story));
    assert_eq!(world.len(), local.len());
    let pos = session.document.get_object(frame).unwrap().pos;
    assert_eq!(world[0].0, frame);
    assert!((world[0].1.x0 - (local[0].x0 + pos[0])).abs() < 1e-3);
    assert!((world[0].1.y - (local[0].y + pos[1])).abs() < 1e-3);
    let mut spell = LiveSpell::en_us().with_settle(Duration::ZERO);
    let t0 = Instant::now();
    spell.note_edit(story, t0);
    spell.tick(&session, t0);
    assert!(!spell.marks(story).is_empty());
    spell.ignore(&session, "recieved");
    assert!(spell.marks(story).is_empty());
    assert!(story_underlines(&lib, &session.document, story, spell.marks(story)).is_empty());
    let none = LiveSpell::new(None);
    assert!(story_underlines(&lib, &session.document, story, none.marks(story)).is_empty());
    let _ = &mut session;
    assert_eq!(session.document.version, version);
}
