use dtool_core::document::{Document, Story, StoryId, Units};
use dtool_core::{Command, Session};
use serde_json::{json, Value};

fn session() -> Session {
    let mut s = Session::new(Document::new([400.0, 400.0], Units::Px, 72.0, 1));
    s.document.stories.push(Story { id: StoryId(7), text: "hello world".into(), ranges: vec![json!({"start": 0, "end": 11, "size_pt": 12.0})], specials: Vec::new(), paras: Vec::new() });
    s
}

fn run(s: &mut Session, name: &str, params: Value) -> Value {
    s.run(&Command::with_params(name, params)).unwrap()
}

#[test]
fn style_commands_are_catalogued_and_routed_on_the_session() {
    let catalog = dtool_protocol::CommandCatalog::default();
    for name in ["text.styles.list", "text.style.define", "text.style.define_from_selection", "text.style.remove", "text.style.apply_named", "text.style.clear_override"] {
        assert!(catalog.get(name).is_some_and(|spec| spec.implemented), "{name}");
    }
    let mut s = session();
    assert!(s.can_execute(&Command::with_params("text.style.define", json!({"id": "A"}))));
    assert!(!s.can_execute(&Command::new("text.styles.restore")));
    assert!(s.run(&Command::new("text.styles.restore")).is_err());
}

#[test]
fn define_remove_and_apply_named_each_undo_in_one_step() {
    let mut s = session();
    run(&mut s, "text.style.define", json!({"id": "Big", "props": {"size_pt": 30.0}}));
    assert_eq!(run(&mut s, "text.styles.list", json!({}))["styles"].as_array().unwrap().len(), 1);
    run(&mut s, "text.style.apply_named", json!({"story": 7, "id": "Big", "range": {"start": 0, "end": 5}}));
    assert_eq!(s.document.stories[0].ranges[0]["char_style"], json!("Big"));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.document.stories[0].ranges.iter().all(|r| r.get("char_style").is_none()));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.document.text_styles.styles.is_empty());
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.document.text_styles.styles.len(), 1);
    run(&mut s, "text.style.remove", json!({"id": "Big"}));
    assert!(s.document.text_styles.styles.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.document.text_styles.styles.len(), 1);
    run(&mut s, "text.style.define_from_selection", json!({"story": 7, "id": "Picked", "range": {"start": 6, "end": 8}}));
    assert_eq!(s.document.text_styles.get(dtool_core::text_styles::StyleKind::Character, "Picked").unwrap().props.get("size_pt"), Some(&json!(12.0)));
}
