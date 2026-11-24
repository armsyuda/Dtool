use dtool_core::document::Units;
use dtool_core::serde_json::json;
use dtool_core::{Command, Document, ObjectId, Session};

#[test]
fn spell_commands_are_catalogued_and_routed_through_the_session() {
    assert!(dtool_text::text_host::install());
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    for name in ["text.spell.check", "text.spell.suggest", "text.spell.replace", "text.spell.ignore"] {
        assert!(session.catalog.resolve(name).is_some_and(|spec| spec.implemented), "{name}");
    }
    let frame = session.run(&Command::with_params("text.frame.add", json!({"rect": [20, 20, 400, 300], "text": "I recieved it"}))).unwrap()["id"].as_u64().unwrap();
    let story = dtool_core::frame::frame_story(&session.document, ObjectId(frame)).unwrap();
    let version = session.document.version;
    let marks = session.run(&Command::with_params("text.spell.check", json!({"story": story.0}))).unwrap();
    assert_eq!(marks["marks"][0]["word"], json!("recieved"));
    assert_eq!(session.document.version, version);
    assert!(session.can_execute(&Command::with_params("text.spell.suggest", json!({"word": "recieved"}))));
    session.run(&Command::with_params("text.spell.replace", json!({"story": story.0, "start": 2, "end": 10, "word": "recieved", "with": "received"}))).unwrap();
    assert_eq!(session.document.stories.iter().find(|s| s.id == story).unwrap().text, "I received it");
    session.run(&Command::new("edit.undo")).unwrap();
    assert_eq!(session.document.stories.iter().find(|s| s.id == story).unwrap().text, "I recieved it");
}
