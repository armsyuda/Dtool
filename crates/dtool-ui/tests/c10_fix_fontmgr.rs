
use dtool_core::command::Session;
use dtool_core::document::{Document, Units};
use dtool_core::object::ObjectId;
use dtool_protocol::Command;
use dtool_ui::font_manager::{objects_using_font, render_font_manager, FontManagerAction, FontManagerState, ReplaceFontState};
use serde_json::json;

fn session_with_font(family: &str, style: &str, text: &str) -> (Session, u64, u64) {
    let mut session = Session::new(Document::new([400.0, 300.0], Units::Px, 96.0, 1));
    let added = session
        .run_named("text.art.add", json!({ "pos": [10.0, 10.0], "text": text }))
        .unwrap();
    let object = added["id"].as_u64().unwrap();
    let story = added["story"].as_u64().unwrap();
    let end = text.chars().count();
    session
        .run_named(
            "text.style",
            json!({
                "story": story,
                "range": { "start": 0, "end": end },
                "patch": { "family": family, "style": style }
            }),
        )
        .unwrap();
    (session, object, story)
}

#[test]
fn usage_rows_match_font_usage_query() {
    let (mut session, _, _) = session_with_font("SourceSans", "Regular", "hello");
    let query = session.run(&Command::new("font.usage.query")).unwrap();
    let mut manager = FontManagerState::new();
    manager.open_for(&mut session).unwrap();
    assert!(manager.open);
    assert_eq!(manager.usages.len(), 1);
    assert_eq!(manager.usages[0].family, "SourceSans");
    assert_eq!(manager.usages[0].style, "Regular");
    assert_eq!(manager.usages[0].status, dtool_strings::text("fontmgr.status.ok"));
    let items = query["usages"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["family"], "SourceSans");
    assert_eq!(items[0]["style"], "Regular");
}

#[test]
fn replace_is_one_undo_and_updates_the_table() {
    let (mut session, _, _) = session_with_font("SourceSans", "Regular", "hello");
    let mut manager = FontManagerState::new();
    manager.open_for(&mut session).unwrap();
    manager.selected_index = Some(0);
    assert!(manager.has_selection());
    let mut replace = ReplaceFontState::new();
    assert!(manager.begin_replace(&mut replace));
    replace.family = "TargetSerif".into();
    replace.style = "Bold".into();
    let version_before = session.document.version;
    manager.confirm_replace(&mut session, &mut replace).unwrap();
    assert!(!replace.open);
    assert_eq!(manager.usages.len(), 1);
    assert_eq!(manager.usages[0].family, "TargetSerif");
    assert_eq!(manager.usages[0].style, "Bold");
    assert!(session.undo.can_undo());
    assert!(session.document.version > version_before);
    let query = session.run(&Command::new("font.usage.query")).unwrap();
    assert_eq!(query["usages"][0]["family"], "TargetSerif");
    assert_eq!(query["usages"][0]["style"], "Bold");
}

#[test]
fn cancelling_replace_leaves_document_version_unchanged() {
    let (mut session, _, _) = session_with_font("SourceSans", "Regular", "hello");
    let mut manager = FontManagerState::new();
    manager.open_for(&mut session).unwrap();
    manager.selected_index = Some(0);
    let mut replace = ReplaceFontState::new();
    assert!(manager.begin_replace(&mut replace));
    replace.family = "TargetSerif".into();
    let version = session.document.version;
    manager.cancel_replace(&mut replace);
    assert!(!replace.open);
    assert!(replace.classification_filter.is_empty());
    assert_eq!(session.document.version, version);
    manager.hide();
    assert!(!manager.open);
    let query = session.run(&Command::new("font.usage.query")).unwrap();
    assert_eq!(query["usages"][0]["family"], "SourceSans");
}

#[test]
fn locate_selects_objects_that_use_the_chosen_family() {
    let (mut session, first, _) = session_with_font("SourceSans", "Regular", "one");
    let added = session
        .run_named("text.art.add", json!({ "pos": [40.0, 40.0], "text": "two" }))
        .unwrap();
    let second = added["id"].as_u64().unwrap();
    let story = added["story"].as_u64().unwrap();
    session
        .run_named(
            "text.style",
            json!({
                "story": story,
                "range": { "start": 0, "end": 3 },
                "patch": { "family": "OtherFace", "style": "Italic" }
            }),
        )
        .unwrap();
    let mut manager = FontManagerState::new();
    manager.open_for(&mut session).unwrap();
    let index = manager.usages.iter().position(|row| row.family == "SourceSans").unwrap();
    manager.selected_index = Some(index);
    let ids = manager.locate_selected(&mut session).unwrap();
    assert_eq!(ids, vec![ObjectId(first)]);
    assert_eq!(session.selection.ids, vec![ObjectId(first)]);
    assert!(objects_using_font(&session.document, "OtherFace", "Italic").contains(&ObjectId(second)));
}

#[test]
fn dialog_actions_are_exported() {
    let _ = (
        FontManagerAction::Locate,
        FontManagerAction::OpenReplace,
        FontManagerAction::Close,
        FontManagerAction::ConfirmReplace,
        FontManagerAction::CancelReplace,
        render_font_manager as fn(&mut egui::Ui, &mut FontManagerState, &mut ReplaceFontState) -> Option<FontManagerAction>,
    );
}
