#[allow(dead_code)]
#[path = "../../dtool-core/src/text_styles.rs"]
mod text_styles;

use dtool_core::document::Story as CoreStory;
use dtool_core::StoryId;
use dtool_text::story::StyleLevel;
use serde_json::json;
use text_styles::{AppliedStyle, NamedTextStyle, StyleKind, TextStyleStore};

fn store() -> TextStyleStore {
    let mut store = TextStyleStore::default();
    store.define(NamedTextStyle { name: "Body".into(), kind: StyleKind::Character, based_on: None, props: json!({"size_pt": 10.0, "weight": 400}).as_object().unwrap().clone(), next_style: None }).unwrap();
    store.define(NamedTextStyle { name: "Strong".into(), kind: StyleKind::Character, based_on: Some("Body".into()), props: json!({"weight": 700}).as_object().unwrap().clone(), next_style: None }).unwrap();
    store
}

#[test]
fn applied_style_round_trips_through_core_range_and_story() {
    let store = store();
    let mut applied = store.apply(StyleKind::Character, "Strong", false, &AppliedStyle::default()).unwrap();
    applied.set_override("size_pt", json!(14.0));
    let mut range = json!({"start": 0, "end": 3, "family": "Inter"});
    applied.write_range(StyleKind::Character, &mut range);
    assert_eq!(range[text_styles::CHAR_STYLE_KEY], json!("Strong"));
    assert_eq!(AppliedStyle::read_range(StyleKind::Character, &range), applied);
    let mut para = json!({"start": 0, "end": 5});
    store.apply(StyleKind::Character, "Body", false, &AppliedStyle::default()).unwrap().write_range(StyleKind::Paragraph, &mut para);
    let core = CoreStory { id: StoryId(7), text: "Hello".into(), ranges: vec![range.clone()], paras: vec![para], specials: Vec::new() };
    let story = dtool_text::story_from_core(&core);
    assert_eq!(dtool_text::bridge::CHAR_STYLE_KEY, text_styles::CHAR_STYLE_KEY);
    assert_eq!(dtool_text::bridge::PARA_STYLE_KEY, text_styles::PARA_STYLE_KEY);
    assert_eq!(dtool_text::bridge::STYLE_OVERRIDES_KEY, text_styles::STYLE_OVERRIDES_KEY);
    let char_ref = story.applied_style(StyleLevel::Character, 1).unwrap();
    assert_eq!(char_ref.name.as_deref(), Some("Strong"));
    let back = AppliedStyle::from_pairs(char_ref.name.clone(), &char_ref.overrides);
    assert_eq!(back, applied);
    assert_eq!(AppliedStyle::from_pairs(back.style.clone(), &back.to_pairs()), back);
    let effective = store.effective(StyleKind::Character, &json!({}), &back).unwrap();
    assert_eq!(effective["weight"], json!(700));
    assert_eq!(effective["size_pt"], json!(14.0));
    assert_eq!(story.applied_style(StyleLevel::Paragraph, 4).unwrap().name.as_deref(), Some("Body"));
    assert!(story.applied_style(StyleLevel::Character, 3).is_none());
    assert_eq!(story.chars[0].style.family, "Inter");
}

#[test]
fn clearing_the_style_removes_range_keys() {
    let mut range = json!({"start": 0, "end": 3, "char_style": "Strong", "style_overrides": {"weight": 900}});
    AppliedStyle::default().write_range(StyleKind::Character, &mut range);
    assert!(range.get("char_style").is_none());
    assert!(range.get("style_overrides").is_none());
    let core = CoreStory { id: StoryId(1), text: "abc".into(), ranges: vec![range], paras: Vec::new(), specials: Vec::new() };
    assert!(dtool_text::story_from_core(&core).styles.is_empty());
}