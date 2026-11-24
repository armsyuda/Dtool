#[path = "../src/text_find.rs"]
mod text_find;

use dtool_core::document::{Document, Units};
use dtool_core::{Command, Session, Story, StoryId};
use text_find::{TEXT_FIND, TEXT_REPLACE, TEXT_REPLACE_ALL, find_all, find_all_with, find_next, replace_all, replace_all_with, replace_one, selection_scope, FindFlags, FindMatch, FindQuery, FindScope};

fn session(text: &str) -> Session {
    let mut doc = Document::new([400.0, 300.0], Units::Px, 72.0, 1);
    let mut story = Story { id: StoryId(3), text: text.into(), ..Default::default() };
    story.ranges.push(Default::default());
    story.ranges[0]["start"] = 0u64.into();
    story.ranges[0]["end"] = (text.chars().count() as u64).into();
    story.ranges[0]["size_pt"] = 12.0f64.into();
    doc.stories.push(story);
    Session::new(doc)
}

fn text(session: &Session) -> &str {
    &session.document.stories[0].text
}

#[test]
fn c11_literal_find_next_on_two_paragraphs() {
    let s = session("cat and dog\nthe Cat sat");
    let doc = &s.document;
    let query = FindQuery::literal("cat");
    let all = find_all(doc, &query, &FindScope::Document).unwrap();
    assert_eq!(all, vec![FindMatch { story: 3, start: 0, end: 3 }]);
    let loose = FindQuery { case_sensitive: false, ..FindQuery::literal("cat") };
    let all = find_all(doc, &loose, &FindScope::Story(3)).unwrap();
    assert_eq!(all.iter().map(|m| m.start).collect::<Vec<_>>(), vec![0, 16]);
    let first = find_next(doc, &loose, &FindScope::Document, None, false).unwrap().unwrap();
    assert_eq!(first.start, 0);
    let second = find_next(doc, &loose, &FindScope::Document, Some((3, first.end)), false).unwrap().unwrap();
    assert_eq!((second.start, second.end), (16, 19));
    let wrapped = find_next(doc, &loose, &FindScope::Document, Some((3, second.end)), false).unwrap().unwrap();
    assert_eq!(wrapped.start, 0);
    let back = find_next(doc, &loose, &FindScope::Document, Some((3, 5)), true).unwrap().unwrap();
    assert_eq!(back.start, 0);
    let back_wrap = find_next(doc, &loose, &FindScope::Document, Some((3, 0)), true).unwrap().unwrap();
    assert_eq!(back_wrap.start, 16);
    assert!(find_all(doc, &FindQuery::literal("a.d"), &FindScope::Document).unwrap().is_empty());
    assert!(find_all(doc, &FindQuery::literal("x"), &FindScope::Story(99)).is_err());
    assert_eq!([TEXT_FIND, TEXT_REPLACE, TEXT_REPLACE_ALL], ["text.find", "text.replace", "text.replace_all"]);
}

#[test]
fn c11_regex_replace_all_is_one_undo() {
    let mut s = session("a1 b22 c333\n한글 9");
    let before = s.document.stories[0].clone();
    let version = s.document.version;
    let steps = s.undo.applied_steps();
    let n = replace_all(&mut s, &FindQuery::regex(r"(\d+)"), &FindScope::Document, "<$1>").unwrap();
    assert_eq!(n, 4);
    assert_eq!(text(&s), "a<1> b<22> c<333>\n한글 <9>");
    assert_eq!(s.undo.applied_steps(), steps + 1);
    assert!(s.document.version > version);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(s.document.stories[0], before);
    assert_eq!(replace_all(&mut s, &FindQuery::regex("zzz"), &FindScope::Document, "y").unwrap(), 0);
}

#[test]
fn c11_invalid_regex_leaves_story_unchanged() {
    let mut s = session("abc abc");
    let before = s.document.stories[0].clone();
    let (version, steps) = (s.document.version, s.undo.applied_steps());
    let err = replace_all(&mut s, &FindQuery::regex("(ab"), &FindScope::Document, "x").unwrap_err();
    assert_eq!(err.code, "INVALID_PATTERN");
    assert!(find_all(&s.document, &FindQuery::regex("[a-"), &FindScope::Document).is_err());
    assert!(replace_all(&mut s, &FindQuery::literal(""), &FindScope::Document, "x").is_err());
    assert!(replace_all(&mut s, &FindQuery::literal("abc"), &FindScope::Selection { story: 3, start: 2, end: 99 }, "x").is_err());
    assert_eq!(s.document.stories[0], before);
    assert_eq!((s.document.version, s.undo.applied_steps()), (version, steps));
}

#[test]
fn c11_selection_scope_replace_keeps_outside_text() {
    let mut s = session("abc abc abc");
    s.text_edit = Some(dtool_core::command::TextEditState { story: StoryId(3), object: None, caret: 4, anchor: 11, before: None });
    let scope = selection_scope(&s).unwrap();
    assert_eq!(scope, FindScope::Selection { story: 3, start: 4, end: 11 });
    s.text_edit = None;
    let n = replace_all(&mut s, &FindQuery::literal("abc"), &scope, "X").unwrap();
    assert_eq!(n, 2);
    assert_eq!(text(&s), "abc X X");

    let mut s = session("one two one");
    let target = find_next(&s.document, &FindQuery::literal("one"), &FindScope::Document, Some((3, 1)), false).unwrap().unwrap();
    let steps = s.undo.applied_steps();
    let placed = replace_one(&mut s, &FindQuery::literal("one"), &FindScope::Document, target, "three").unwrap();
    assert_eq!(text(&s), "one two three");
    assert_eq!((placed.start, placed.end), (8, 13));
    assert_eq!(s.undo.applied_steps(), steps + 1);
    assert_eq!(replace_one(&mut s, &FindQuery::literal("one"), &FindScope::Document, target, "x").unwrap_err().code, "STALE");
}

#[test]
fn c11_whole_word_find_and_replace() {
    let mut s = session("cat catalog cat cats\nThe CAT.");
    let whole = FindFlags { whole_word: true };
    let query = FindQuery::literal("cat");
    assert_eq!(find_all_with(&s.document, &query, &FindScope::Document, whole).unwrap().iter().map(|m| m.start).collect::<Vec<_>>(), vec![0, 12]);
    let loose = FindQuery { case_sensitive: false, ..FindQuery::literal("cat") };
    assert_eq!(find_all_with(&s.document, &loose, &FindScope::Document, whole).unwrap().iter().map(|m| m.start).collect::<Vec<_>>(), vec![0, 12, 25]);
    let steps = s.undo.applied_steps();
    let version = s.document.version;
    let n = replace_all_with(&mut s, &query, &FindScope::Document, "dog", whole).unwrap();
    assert_eq!(n, 2);
    assert_eq!(text(&s), "dog catalog dog cats\nThe CAT.");
    assert_eq!(s.undo.applied_steps(), steps + 1);
    assert!(s.document.version > version);
    s.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(text(&s), "cat catalog cat cats\nThe CAT.");

    let hangul = session("한글 한글이다 한글");
    assert_eq!(find_all_with(&hangul.document, &FindQuery::literal("한글"), &FindScope::Document, whole).unwrap().iter().map(|m| (m.start, m.end)).collect::<Vec<_>>(), vec![(0, 2), (8, 10)]);

    let digits = session("a1 22 b333");
    assert_eq!(find_all_with(&digits.document, &FindQuery::regex(r"\d+"), &FindScope::Document, whole).unwrap(), vec![FindMatch { story: 3, start: 3, end: 5 }]);
}

#[test]
fn replacement_preserves_matched_format_without_inheriting_neighbour_properties() {
    use dtool_core::serde_json::json;
    let mut s = session("aa cat");
    s.run(&Command::with_params("text.style", json!({"story": 3, "range": {"start": 0, "end": 2}, "patch": {"weight": 700}}))).unwrap();
    s.run(&Command::with_params("text.style", json!({"story": 3, "range": {"start": 3, "end": 6}, "patch": {"italic": true}}))).unwrap();
    let before = s.document.stories[0].clone(); let depth = s.undo.applied_steps();
    assert_eq!(replace_all(&mut s, &FindQuery::literal("cat"), &FindScope::Story(3), "kitten").unwrap(), 1);
    assert_eq!(text(&s), "aa kitten");
    let range = s.document.stories[0].ranges.iter().find(|r| r["start"].as_u64().unwrap() <= 3 && r["end"].as_u64().unwrap() > 3).unwrap();
    assert_eq!(range["italic"], true); assert!(range["weight"].is_null()); assert_eq!(range["size_pt"], 12.0);
    assert_eq!(s.undo.applied_steps(), depth + 1);
    let after = s.document.stories[0].clone();
    s.run(&Command::new("edit.undo")).unwrap(); assert_eq!(s.document.stories[0], before);
    s.run(&Command::new("edit.redo")).unwrap(); assert_eq!(s.document.stories[0], after);
}
