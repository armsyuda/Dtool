use dtool_core::document::Units;
use dtool_core::object::{ObjectId, ObjectKind};
use dtool_core::prefs::Prefs;
use dtool_core::workspace::{Scope, Workspace};
use dtool_core::{Command, Document, Session, StoryId};
use dtool_text::autocorrect::{expand_after_insert, expand_in_edit, type_with_autocorrect};

fn enabled() -> Prefs {
    let mut prefs = Prefs::default();
    prefs.autocorrect_enabled = true;
    prefs.ac_replace_while_typing = true;
    prefs
}

fn add_text(session: &mut Session, text: &str) -> StoryId {
    let mut add = Command::new("text.art.add");
    add.params["pos"] = vec![10.0f64, 20.0].into();
    add.params["text"] = text.into();
    let id = ObjectId(session.run(&add).unwrap()["id"].as_u64().unwrap());
    match session.document.get_object(id).unwrap().kind {
        ObjectKind::ArtText { story } => story,
        _ => unreachable!(),
    }
}

fn session_with(text: &str) -> (Session, StoryId) {
    let mut session = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    let story = add_text(&mut session, text);
    (session, story)
}

fn story_text(session: &Session, story: StoryId) -> String {
    session.document.stories.iter().find(|s| s.id == story).unwrap().text.clone()
}

fn undo(session: &mut Session) {
    session.run(&bare("edit.undo")).unwrap();
}

fn bare(name: &str) -> Command {
    let mut c = Command::new(name);
    c.params = Default::default();
    c
}

fn select(story: StoryId, at: usize) -> Command {
    let mut c = Command::new("text.select");
    c.params["story"] = story.0.into();
    c.params["range"]["start"] = at.into();
    c.params["range"]["end"] = at.into();
    c
}

#[test]
fn trigger_and_expansion_are_one_undo_back_to_pre_trigger_text() {
    let (mut s, story) = session_with("Made (c)");
    let depth = s.undo.applied_steps();
    assert!(type_with_autocorrect(&mut s, story, 8, " ", &enabled()).unwrap());
    assert_eq!(story_text(&s, story), "Made \u{a9} ");
    assert_eq!(s.undo.applied_steps(), depth + 1);
    undo(&mut s);
    assert_eq!(story_text(&s, story), "Made (c)");
    s.run(&bare("edit.redo")).unwrap();
    assert_eq!(story_text(&s, story), "Made \u{a9} ");
}

#[test]
fn punctuation_trigger_merges_too() {
    let (mut s, story) = session_with("x(tm)");
    assert!(type_with_autocorrect(&mut s, story, 5, ".", &enabled()).unwrap());
    assert_eq!(story_text(&s, story), "x\u{2122}.");
    undo(&mut s);
    assert_eq!(story_text(&s, story), "x(tm)");
}

#[test]
fn disabled_no_match_and_escape_do_not_expand_or_merge() {
    let mut off = enabled();
    off.autocorrect_enabled = false;
    let mut not_typing = enabled();
    not_typing.ac_replace_while_typing = false;
    for prefs in [off, not_typing] {
        let (mut s, story) = session_with("Made (c)");
        let depth = s.undo.applied_steps();
        assert!(!type_with_autocorrect(&mut s, story, 8, " ", &prefs).unwrap());
        assert_eq!(story_text(&s, story), "Made (c) ");
        assert_eq!(s.undo.applied_steps(), depth + 1);
        undo(&mut s);
        assert_eq!(story_text(&s, story), "Made (c)");
    }
    let (mut s, story) = session_with("hello");
    assert!(!type_with_autocorrect(&mut s, story, 5, " ", &enabled()).unwrap());
    assert_eq!(story_text(&s, story), "hello ");
    let (mut s, story) = session_with("Made (c)");
    let version = s.document.version;
    let depth = s.undo.applied_steps();
    assert!(!type_with_autocorrect(&mut s, story, 8, "\u{1b}", &enabled()).unwrap());
    assert!(!type_with_autocorrect(&mut s, story, 8, "", &enabled()).unwrap());
    assert_eq!(story_text(&s, story), "Made (c)");
    assert_eq!(s.document.version, version);
    assert_eq!(s.undo.applied_steps(), depth);
    assert!(type_with_autocorrect(&mut s, story, 99, " ", &enabled()).is_err());
}

#[test]
fn live_workspace_document_one_undo_and_caret_in_edit_session() {
    let mut ws = Workspace::default();
    let id = ws.new_document("ac2");
    ws.activate(id).unwrap();
    let story = add_text(ws.session_mut().unwrap(), "Made (c)");
    assert!(type_with_autocorrect(ws.session_mut().unwrap(), story, 8, " ", &enabled()).unwrap());
    assert_eq!(story_text(ws.session_mut().unwrap(), story), "Made \u{a9} ");
    ws.run(Scope::active(), bare("edit.undo")).1.unwrap();
    assert_eq!(story_text(ws.session_mut().unwrap(), story), "Made (c)");

    ws.run(Scope::active(), select(story, 8)).1.unwrap();
    assert!(type_with_autocorrect(ws.session_mut().unwrap(), story, 8, " ", &enabled()).unwrap());
    let edit = ws.session_mut().unwrap().text_edit.clone().unwrap();
    assert_eq!((edit.caret, edit.anchor), (7, 7));
    ws.run(Scope::active(), bare("text.edit.end")).1.unwrap();
    assert_eq!(story_text(ws.session_mut().unwrap(), story), "Made \u{a9} ");
    ws.run(Scope::active(), bare("edit.undo")).1.unwrap();
    assert_eq!(story_text(ws.session_mut().unwrap(), story), "Made (c)");
}

#[test]
fn expand_after_insert_keeps_edit_caret_after_expansion() {
    let (mut s, story) = session_with("Made (c)");
    s.run(&select(story, 8)).unwrap();
    let mut ins = Command::new("text.insert");
    ins.params["story"] = story.0.into();
    ins.params["at"] = 8usize.into();
    ins.params["text"] = " ".into();
    s.run(&ins).unwrap();
    assert_eq!(s.text_edit.as_ref().unwrap().caret, 9);
    assert!(expand_after_insert(&mut s, story, 9, " ", &enabled()).unwrap());
    assert_eq!(story_text(&s, story), "Made \u{a9} ");
    let edit = s.text_edit.as_ref().unwrap();
    assert_eq!((edit.caret, edit.anchor), (7, 7));
}
fn edit_type(session: &mut Session, text: &str) {
    let mut c = Command::new("text.edit.type");
    c.params["text"] = text.into();
    session.run(&c).unwrap();
}

#[test]
fn live_edit_session_typing_expands_inside_the_edit_group_one_undo() {
    let mut ws = Workspace::default();
    let id = ws.new_document("ac2_edit");
    ws.activate(id).unwrap();
    let story = add_text(ws.session_mut().unwrap(), "Made");
    let depth = ws.session_mut().unwrap().undo.applied_steps();
    ws.run(Scope::active(), select(story, 4)).1.unwrap();
    let s = ws.session_mut().unwrap();
    for ch in [" ", "(", "c", ")"] {
        edit_type(s, ch);
        assert!(!expand_in_edit(s, ch, &enabled()).unwrap());
    }
    edit_type(s, " ");
    assert!(expand_in_edit(s, " ", &enabled()).unwrap());
    assert_eq!(story_text(s, story), "Made \u{a9} ");
    let edit = s.text_edit.as_ref().unwrap();
    assert_eq!((edit.caret, edit.anchor), (7, 7));
    edit_type(s, "x");
    assert_eq!(story_text(s, story), "Made \u{a9} x");
    ws.run(Scope::active(), bare("text.edit.end")).1.unwrap();
    assert_eq!(ws.session_mut().unwrap().undo.applied_steps(), depth + 1);
    ws.run(Scope::active(), bare("edit.undo")).1.unwrap();
    assert_eq!(story_text(ws.session_mut().unwrap(), story), "Made");
}

#[test]
fn expand_in_edit_ignores_disabled_selection_and_no_edit() {
    let (mut s, story) = session_with("Made (c)");
    let version = s.document.version;
    assert!(!expand_in_edit(&mut s, " ", &enabled()).unwrap());
    s.run(&select(story, 8)).unwrap();
    edit_type(&mut s, " ");
    let mut off = enabled();
    off.autocorrect_enabled = false;
    assert!(!expand_in_edit(&mut s, " ", &off).unwrap());
    assert!(!expand_in_edit(&mut s, "\u{1b}", &enabled()).unwrap());
    assert_eq!(story_text(&s, story), "Made (c) ");
    s.text_edit.as_mut().unwrap().anchor = 0;
    assert!(!expand_in_edit(&mut s, " ", &enabled()).unwrap());
    assert_eq!(story_text(&s, story), "Made (c) ");
    s.run(&bare("text.edit.end")).unwrap();
    assert!(s.document.version >= version);
}