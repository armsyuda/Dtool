use dtool_text::story::{StyleLevel, StyleRef};
use dtool_text::{Story, TextRange};

fn named(level: StyleLevel, name: &str) -> StyleRef {
    StyleRef { level, name: Some(name.into()), overrides: Vec::new() }
}

fn story(text: &str) -> Story {
    Story { text: text.into(), ..Story::default() }
}

#[test]
fn set_query_and_clear_split_by_level() {
    let mut s = story("Hello world");
    assert!(s.styles.is_empty());
    assert!(s.set_applied_style(TextRange::new(0, 11), named(StyleLevel::Paragraph, "Body")));
    assert!(s.set_applied_style(TextRange::new(2, 8), named(StyleLevel::Character, "Emphasis")));
    assert!(!s.set_applied_style(TextRange::new(2, 8), named(StyleLevel::Character, "Emphasis")));
    assert!(!s.set_applied_style(TextRange::new(3, 30), named(StyleLevel::Character, "Bad")));
    assert_eq!(s.applied_style(StyleLevel::Character, 4).unwrap().name.as_deref(), Some("Emphasis"));
    assert_eq!(s.applied_style(StyleLevel::Paragraph, 4).unwrap().name.as_deref(), Some("Body"));
    assert!(s.applied_style(StyleLevel::Character, 9).is_none());
    assert!(s.set_applied_style(TextRange::new(4, 6), named(StyleLevel::Character, "Strong")));
    let names: Vec<(usize, usize, String)> = s.applied_styles(StyleLevel::Character).iter().map(|r| (r.range.start, r.range.end, r.style.name.clone().unwrap())).collect();
    assert_eq!(names, vec![(2, 4, "Emphasis".into()), (4, 6, "Strong".into()), (6, 8, "Emphasis".into())]);
    assert!(s.clear_applied_style(StyleLevel::Character, TextRange::new(0, 11)));
    assert!(s.applied_styles(StyleLevel::Character).is_empty());
    assert_eq!(s.applied_styles(StyleLevel::Paragraph).len(), 1);
    assert!(!s.clear_applied_style(StyleLevel::Character, TextRange::new(0, 11)));
}

#[test]
fn edits_shift_and_drop_applied_styles() {
    let mut s = story("abcdef");
    let mut keep = named(StyleLevel::Character, "Emphasis");
    keep.overrides.push(("weight".into(), "700".into()));
    s.set_applied_style(TextRange::new(2, 4), keep.clone());
    s.insert(0, "XY");
    assert_eq!(s.styles[0].range, TextRange::new(4, 6));
    s.insert(5, "Z");
    assert_eq!(s.styles[0].range, TextRange::new(4, 7));
    assert_eq!(s.styles[0].style, keep);
    s.delete(TextRange::new(3, 8));
    assert!(s.styles.is_empty());
}

#[test]
fn clone_and_prefix_delete_keep_styles() {
    let mut s = story("abcdef");
    s.set_applied_style(TextRange::new(1, 5), named(StyleLevel::Paragraph, "Body"));
    let copy = s.clone();
    assert_eq!(copy, s);
    s.delete(TextRange::new(0, 2));
    assert_eq!(s.styles[0].range, TextRange::new(0, 3));
}