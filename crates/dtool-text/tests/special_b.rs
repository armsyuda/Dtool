use dtool_core::document::{Document, Story, StoryId, Units, Special, BreakKind};
use dtool_core::{Command, Session};
use dtool_text::{CharStyle, FontLibrary, Run, TextRange};

fn session() -> Session {
    let mut doc=Document::new([240.0,120.0],Units::Pt,72.0,1);
    doc.stories.push(Story {id:StoryId(1),text:"AB".into(),..Story::default()});
    Session::new(doc)
}
fn insert(session:&mut Session,what:&str) {
    let mut command=Command::new("text.special.insert");
    command.params["story"]=1_u64.into();command.params["at"]=1_u64.into();command.params["what"]=what.into();
    session.run(&command).expect("canonical special command must be routed");
    assert_eq!(session.undo.applied_steps(),1);
}
fn undo_redo(session:&mut Session,text:&str,specials:&[(usize,Special)]) {
    session.run(&Command::new("edit.undo")).expect("undo");
    assert_eq!(session.document.stories[0].text,"AB");
    assert!(session.document.stories[0].specials.is_empty());
    assert!(!session.undo.can_undo());
    session.run(&Command::new("edit.redo")).expect("redo");
    assert_eq!(session.document.stories[0].text,text);
    assert_eq!(session.document.stories[0].specials,specials);
}
fn font() -> (FontLibrary,CharStyle) {
    let path=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dtool-ui/assets/fonts/pretendard/Pretendard-Regular.ttf");
    assert!(path.is_file(),"required fixture font missing: {}",path.display());
    let lib=FontLibrary::from_fixtures(&[path]).expect("fixture font");
    let family=lib.families().into_iter().next().expect("font family").to_owned();
    let style=lib.faces(&family).into_iter().next().expect("font face").style.clone();
    (lib,CharStyle {family,style,size_pt:12.0,..CharStyle::default()})
}
fn text_story(text:String,style:CharStyle) -> dtool_text::Story {
    let count=text.chars().count();
    dtool_text::Story {text,chars:vec![Run {range:TextRange::new(0,count),style}],..dtool_text::Story::default()}
}
fn character(what:&str,ch:char) {
    let mut session=session();insert(&mut session,what);
    let expected=format!("A{ch}B");
    assert_eq!(session.document.stories[0].text,expected);
    assert!(session.document.stories[0].specials.is_empty());
    let (lib,style)=font();let story=text_story(expected.clone(),style);
    let lines=dtool_text::layout_story(&lib,&story,None);
    assert_eq!(lines.len(),1);assert_eq!(lines[0].range,TextRange::new(0,3));
    assert!(lines[0].width>0.0);
    undo_redo(&mut session,&expected,&[]);
}
#[test]
fn right_indent_tab_preserves_text_and_moves_suffix_to_right_edge() {
    let mut session=session();insert(&mut session,"right_indent_tab");
    assert_eq!(session.document.stories[0].text,"AB");
    assert_eq!(session.document.stories[0].specials,vec![(1,Special::RightIndentTab)]);
    let (lib,style)=font();let mut story=text_story("AB".into(),style);
    dtool_text::layout::insert_special(&mut story,1,dtool_text::para_style::Special::RightIndentTab);
    let lines=dtool_text::layout_story(&lib,&story,Some(200.0));
    assert_eq!(lines.len(),1);assert!(lines[0].width>=199.0,"suffix must reach the right edge");
    undo_redo(&mut session,"AB",&[(1,Special::RightIndentTab)]);
}
#[test]
fn en_space_u2002_inserts_layouts_and_undoes() {character("en_space",'\u{2002}');}
#[test]
fn paragraph_break_u2029_starts_a_second_paragraph_line() {
    let mut session=session();insert(&mut session,"para_break");
    assert_eq!(session.document.stories[0].text,"A\u{2029}B");
    let (lib,style)=font();let story=text_story(session.document.stories[0].text.clone(),style);
    let lines=dtool_text::layout_story(&lib,&story,None);
    assert_eq!(lines.len(),2);assert!(lines[1].baseline>lines[0].baseline);
    assert_eq!(lines[1].range.end,3);undo_redo(&mut session,"A\u{2029}B",&[]);
}
#[test]
fn thin_space_u2009_inserts_layouts_and_undoes() {character("thin_space",'\u{2009}');}
#[test]
fn column_break_moves_suffix_to_next_column() {
    let mut session=session();insert(&mut session,"column_break");
    assert_eq!(session.document.stories[0].text,"AB");
    assert_eq!(session.document.stories[0].specials,vec![(1,Special::Break(BreakKind::Column))]);
    let (lib,style)=font();let mut story=text_story("AB".into(),style);
    dtool_text::layout::insert_special(&mut story,1,dtool_text::para_style::Special::Break(dtool_text::para_style::BreakKind::Column));
    let frame=dtool_text::frame::FrameProps {size:[220.0,100.0],columns:2,gutter:20.0,..Default::default()};
    let layout=dtool_text::frame::layout_chain(&lib,&story,&[frame]);
    assert!(layout.overset.is_none());assert_eq!(layout.per_frame.len(),1);
    let lines=&layout.per_frame[0];assert_eq!(lines.len(),2);
    assert_eq!(lines[0].range,TextRange::new(0,1));assert_eq!(lines[1].range,TextRange::new(1,2));
    let left=lines[0].glyphs.first().expect("first column glyph").offset[0];
    let right=lines[1].glyphs.first().expect("second column glyph").offset[0];
    assert!((right-left-120.0).abs()<0.1,"suffix must begin in the second column");
    undo_redo(&mut session,"AB",&[(1,Special::Break(BreakKind::Column))]);
}
#[test]
fn hair_space_u200a_inserts_layouts_and_undoes() {character("hair_space",'\u{200A}');}
