use dtool_core::document::{BreakKind, Document, Special, Story, StoryId, Units};
use dtool_core::{Command, Session};
use dtool_text::{CharStyle, FontLibrary, Run, TextRange, layout_story};

fn session() -> Session {
    let mut document = Document::new([72.0,72.0], Units::Points,72.0,1);
    document.stories.push(Story { id: StoryId(1), text: "AB".to_owned(), ..Story::default() });
    Session::new(document)
}

fn insert(session: &mut Session, what: &str) {
    let mut command = Command::new("text.special.insert");
    command.params["story"] = 1_u64.into();
    command.params["at"] = 1_u64.into();
    command.params["what"] = what.into();
    session.run(&command).unwrap_or_else(|error|panic!("canonical special insertion failed for {what}: {error:?}"));
}

fn undo(session: &mut Session) {
    session.run(&Command::new("edit.undo")).expect("canonical undo failed");
    assert!(!session.undo.can_undo(), "insertion must create exactly one undo step");
}

fn break_contract(what: &str, kind: BreakKind) {
    let mut session = session();
    let generation = session.generation;
    insert(&mut session,what);
    let story = &session.document.stories[0];
    assert_eq!(story.text,"AB", "break marks must not insert U+FFFC or a character");
    assert_eq!(story.specials,vec![(1,Special::Break(kind))]);
    assert_eq!(session.generation,generation+1);
    undo(&mut session);
    assert_eq!(session.document.stories[0].text,"AB");
    assert!(session.document.stories[0].specials.is_empty());
}

fn character_contract(what: &str, character: char) -> Session {
    let mut session = session();
    insert(&mut session,what);
    assert_eq!(session.document.stories[0].text,format!("A{character}B"));
    assert_eq!(session.document.stories[0].text.chars().count(),3);
    assert!(session.document.stories[0].specials.is_empty());
    session
}

#[test]
fn frame_break_is_a_native_mark_and_one_undo() { break_contract("frame_break",BreakKind::Frame); }

#[test]
fn figure_space_u2007_inserts_and_undoes() {
    let mut session = character_contract("figure_space",'\u{2007}');
    undo(&mut session);
    assert_eq!(session.document.stories[0].text,"AB");
}

#[test]
fn page_break_is_a_native_mark_and_one_undo() { break_contract("page_break",BreakKind::Page); }

#[test]
fn punctuation_space_u2008_inserts_and_undoes() {
    let mut session = character_contract("punct_space",'\u{2008}');
    undo(&mut session);
    assert_eq!(session.document.stories[0].text,"AB");
}

#[test]
fn odd_page_break_is_a_native_mark_and_one_undo() { break_contract("odd_page_break",BreakKind::OddPage); }

#[test]
fn nonbreaking_space_u00a0_does_not_create_a_line_break() {
    let mut session = character_contract("nbsp",'\u{00a0}');
    let font = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dtool-ui/assets/fonts/pretendard/Pretendard-Regular.ttf");
    assert!(font.is_file(),"missing fixture font: {}",font.display());
    let library = FontLibrary::from_fixtures(&[font]).expect("fixture-only font loading failed");
    let family = library.families().into_iter().next().expect("fixture has no family").to_owned();
    let face = library.faces(&family).into_iter().next().expect("fixture has no face");
    let story = dtool_text::Story {
        text: session.document.stories[0].text.clone(),
        chars: vec![Run { range:TextRange::new(0,3),style:CharStyle { family,style:face.style.clone(),size_pt:12.0,..CharStyle::default() } }],
        ..dtool_text::Story::default()
    };
    let natural = layout_story(&library,&story,None);
    assert_eq!(natural.len(),1);
    assert!(natural[0].width>0.0,"fixture must have measurable glyphs");
    let constrained = layout_story(&library,&story,Some(natural[0].width*0.6));
    assert_eq!(constrained.len(),1,"NBSP-linked scalars must stay on one line even when they overflow");
    assert_eq!(constrained[0].range,TextRange::new(0,3));
    undo(&mut session);
    assert_eq!(session.document.stories[0].text,"AB");
}
