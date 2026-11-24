#[allow(dead_code)]
#[path = "../src/text_styles.rs"]
mod text_styles;

use serde_json::{json, Map, Value};
use text_styles::{AppliedStyle, NamedTextStyle, StyleKind, TextStyleStore};

fn props(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn style(name: &str, based_on: Option<&str>, value: Value) -> NamedTextStyle {
    NamedTextStyle { name: name.into(), kind: StyleKind::Paragraph, based_on: based_on.map(str::to_string), props: props(value), next_style: None }
}

fn store() -> TextStyleStore {
    let mut store = TextStyleStore::default();
    store.define(style("Body", None, json!({"indent_left": 0.0, "space_after": 6.0, "align": "Left"}))).unwrap();
    store.define(style("Quote", Some("Body"), json!({"indent_left": 12.0}))).unwrap();
    store.define(style("Pull", Some("Quote"), json!({"align": "Center"}))).unwrap();
    store
}

#[test]
fn define_inherit_and_resolve() {
    let store = store();
    let pull = store.resolve(StyleKind::Paragraph, "Pull").unwrap();
    assert_eq!(pull["indent_left"], json!(12.0));
    assert_eq!(pull["space_after"], json!(6.0));
    assert_eq!(pull["align"], json!("Center"));
    assert_eq!(store.names(StyleKind::Paragraph), vec!["Body", "Quote", "Pull"]);
    assert!(store.names(StyleKind::Character).is_empty());
}

#[test]
fn parent_change_flows_to_children() {
    let mut store = store();
    store.define(style("Body", None, json!({"indent_left": 0.0, "space_after": 10.0, "align": "Left"}))).unwrap();
    assert_eq!(store.resolve(StyleKind::Paragraph, "Pull").unwrap()["space_after"], json!(10.0));
}

#[test]
fn based_on_cycle_is_rejected() {
    let mut store = store();
    let before = store.clone();
    let err = store.define(style("Body", Some("Pull"), json!({}))).unwrap_err();
    assert_eq!(err.code, "STYLE_CYCLE");
    let err = store.define(style("Quote", Some("Quote"), json!({}))).unwrap_err();
    assert_eq!(err.code, "STYLE_CYCLE");
    assert_eq!(store, before);
}

#[test]
fn unknown_parent_or_kind_mismatch_is_rejected() {
    let mut store = store();
    assert_eq!(store.define(style("X", Some("Missing"), json!({}))).unwrap_err().code, "NOT_FOUND");
    let character = NamedTextStyle { name: "Emph".into(), kind: StyleKind::Character, based_on: Some("Body".into()), props: Map::new(), next_style: None };
    assert_eq!(store.define(character).unwrap_err().code, "NOT_FOUND");
    assert_eq!(store.define(style(" ", None, json!({}))).unwrap_err().code, "INVALID_PARAMS");
}

#[test]
fn apply_override_clear_cycle() {
    let store = store();
    let base = json!({"indent_left": 0.0, "space_after": 0.0, "align": "Left", "leading": 14.0});
    let mut applied = store.apply(StyleKind::Paragraph, "Quote", false, &AppliedStyle::default()).unwrap();
    let effective = store.effective(StyleKind::Paragraph, &base, &applied).unwrap();
    assert_eq!(effective["indent_left"], json!(12.0));
    assert_eq!(effective["leading"], json!(14.0));
    applied.set_override("indent_left", json!(30.0));
    assert!(applied.has_override());
    assert_eq!(store.effective(StyleKind::Paragraph, &base, &applied).unwrap()["indent_left"], json!(30.0));
    let kept = store.apply(StyleKind::Paragraph, "Pull", true, &applied).unwrap();
    assert_eq!(kept.overrides["indent_left"], json!(30.0));
    let reset = store.apply(StyleKind::Paragraph, "Pull", false, &applied).unwrap();
    assert!(!reset.has_override());
    assert!(applied.clear_override(Some("indent_left")));
    assert!(!applied.clear_override(None));
    assert_eq!(store.effective(StyleKind::Paragraph, &base, &applied).unwrap(), store.effective(StyleKind::Paragraph, &base, &store.apply(StyleKind::Paragraph, "Quote", false, &AppliedStyle::default()).unwrap()).unwrap());
    assert_eq!(store.apply(StyleKind::Paragraph, "Missing", false, &applied).unwrap_err().code, "NOT_FOUND");
    let unstyled = store.effective(StyleKind::Paragraph, &base, &AppliedStyle::default()).unwrap();
    assert_eq!(unstyled, base);
}

#[test]
fn create_from_selection_stores_only_differences() {
    let mut store = store();
    let current = props(json!({"indent_left": 12.0, "space_after": 6.0, "align": "Right"}));
    store.define_from_selection("QuoteRight", StyleKind::Paragraph, Some("Quote"), &current).unwrap();
    let created = store.get(StyleKind::Paragraph, "QuoteRight").unwrap();
    assert_eq!(created.props, props(json!({"align": "Right"})));
    assert_eq!(store.resolve(StyleKind::Paragraph, "QuoteRight").unwrap(), current);
}

#[test]
fn remove_reparents_children_without_changing_their_look() {
    let mut store = store();
    let before = store.resolve(StyleKind::Paragraph, "Pull").unwrap();
    store.remove(StyleKind::Paragraph, "Quote").unwrap();
    assert_eq!(store.get(StyleKind::Paragraph, "Pull").unwrap().based_on.as_deref(), Some("Body"));
    assert_eq!(store.resolve(StyleKind::Paragraph, "Pull").unwrap(), before);
    assert_eq!(store.remove(StyleKind::Paragraph, "Quote").unwrap_err().code, "NOT_FOUND");
}

#[test]
fn store_survives_native_round_trip_and_snapshot_restore() {
    let mut store = store();
    store.define(NamedTextStyle { name: "Emph".into(), kind: StyleKind::Character, based_on: None, props: props(json!({"italic": true})), next_style: None }).unwrap();
    let saved = serde_json::to_value(&store).unwrap();
    let reopened: TextStyleStore = serde_json::from_value(saved.clone()).unwrap();
    assert_eq!(reopened, store);
    let snapshot = store.clone();
    store.define(style("Body", None, json!({"indent_left": 99.0}))).unwrap();
    store = snapshot;
    assert_eq!(serde_json::to_value(&store).unwrap(), saved);
    let empty: TextStyleStore = serde_json::from_value(json!({})).unwrap();
    assert!(empty.styles.is_empty());
}