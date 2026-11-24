
mod canonical_style_contract {
use dtool_text::{CharPatch, CharPatchField, CharStyle, Run, Story, TextRange};
use dtool_text::char_style::Decoration;

fn original_style()->CharStyle {CharStyle {family:"Pretendard".to_owned(),style:"Regular".to_owned(),fill:dtool_core::paint::paint_from_legacy_str("black"),..CharStyle::default()}}
fn fixture()->Story {Story {text:"ABCD".to_owned(),chars:vec![Run {range:TextRange::new(0,4),style:original_style()}],..Story::default()}}
fn at(story:&Story,index:usize)->CharStyle {
    story.chars.iter().find(|run|run.range.start<=index && index<run.range.end).expect("fixture range lost").style.clone()
}
fn patched(patch:CharPatch,expected:CharStyle)->Story {
    let mut story=fixture();story.patch_char(TextRange::new(1,3),&patch);
    assert_eq!(story.text,"ABCD");
    assert_eq!(at(&story,0),original_style(),"left run changed outside selection");
    assert_eq!(at(&story,3),original_style(),"right run changed outside selection");
    assert_eq!(at(&story,1),expected,"patch lost unrelated character attributes");
    assert_eq!(at(&story,2),expected);
    for index in 0..4 {assert_eq!(story.chars.iter().filter(|run|run.range.start<=index && index<run.range.end).count(),1,"runs must cover each scalar exactly once");}
    story
}

#[test]
fn underline_updates_only_selected_scalars_and_mixed_query() {
    let decoration=Decoration {on:true,colour:dtool_core::paint::paint_from_legacy_str("blue"),weight:Some(1.5),offset:Some(0.5)};
    let mut expected=original_style();expected.underline=decoration.clone();
    let story=patched(CharPatch {underline:Some(decoration.clone()),..CharPatch::default()},expected);
    assert!(matches!(story.query_char(TextRange::new(1,3)).underline,CharPatchField::Same(value) if value==decoration));
    assert!(matches!(story.query_char(TextRange::new(0,4)).underline,CharPatchField::Mixed));
}

#[test]
fn baseline_shift_preserves_other_attributes_and_outer_runs() {
    let mut expected=original_style();expected.baseline_pt=3.0;
    let story=patched(CharPatch {baseline_pt:Some(3.0),..CharPatch::default()},expected);
    assert!(matches!(story.query_char(TextRange::new(1,3)).baseline_pt,CharPatchField::Same(3.0)));
    assert!(matches!(story.query_char(TextRange::new(0,4)).baseline_pt,CharPatchField::Mixed));
}

#[test]
fn strikethrough_updates_only_selected_scalars() {
    let decoration=Decoration {on:true,colour:None,weight:Some(1.0),offset:None};
    let mut expected=original_style();expected.strike=decoration.clone();
    let story=patched(CharPatch {strike:Some(decoration.clone()),..CharPatch::default()},expected);
    assert!(matches!(story.query_char(TextRange::new(1,3)).strike,CharPatchField::Same(value) if value==decoration));
    assert!(matches!(story.query_char(TextRange::new(0,4)).strike,CharPatchField::Mixed));
}

#[test]
fn background_highlight_updates_selection_and_can_be_cleared() {
    let mut expected=original_style();expected.background=dtool_core::paint::paint_from_legacy_str("yellow");
    let mut story=patched(CharPatch {background:Some(dtool_core::paint::paint_from_legacy_str("yellow")),..CharPatch::default()},expected);
    assert!(matches!(story.query_char(TextRange::new(0,4)).background,CharPatchField::Mixed));
    story.patch_char(TextRange::new(1,3),&CharPatch {background:Some(None),..CharPatch::default()});
    for index in 0..4 {assert_eq!(at(&story,index),original_style());}
    assert!(matches!(story.query_char(TextRange::new(0,4)).background,CharPatchField::Same(None)));
}

#[test]
fn selected_text_fill_colour_preserves_unselected_text() {
    let mut expected=original_style();expected.fill=dtool_core::paint::paint_from_legacy_str("red");
    let story=patched(CharPatch {fill:Some(dtool_core::paint::paint_from_legacy_str("red")),..CharPatch::default()},expected);
    assert!(matches!(story.query_char(TextRange::new(1,3)).fill,CharPatchField::Same(Some(value)) if dtool_core::paint::paint_to_legacy_str(&value).as_deref()==Some("#ff0000")));
    assert!(matches!(story.query_char(TextRange::new(0,4)).fill,CharPatchField::Mixed));
}

#[test]
fn selected_text_outline_stroke_preserves_fill_and_can_be_cleared() {
    let outline=("blue".to_owned(),2.5);
    let mut expected=original_style();expected.outline=Some(outline.clone());
    let mut story=patched(CharPatch {outline:Some(Some(outline.clone())),..CharPatch::default()},expected);
    assert!(matches!(story.query_char(TextRange::new(1,3)).outline,CharPatchField::Same(Some(value)) if value==outline));
    assert!(matches!(story.query_char(TextRange::new(0,4)).outline,CharPatchField::Mixed));
    story.patch_char(TextRange::new(1,3),&CharPatch {outline:Some(None),..CharPatch::default()});
    for index in 0..4 {assert_eq!(at(&story,index),original_style());}
}

}
