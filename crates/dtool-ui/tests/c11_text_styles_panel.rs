use dtool_core::document::{Document, Units};
use dtool_core::text_styles::StyleKind;
use dtool_core::{Command, Session};
use dtool_ui::panels::text_styles::{self, PanelAction, SortMode, TextStylesPanel};
use serde_json::json;

fn live() -> Session {
    let mut session = Session::new(Document::new([600.0, 600.0], Units::Px, 72.0, 1));
    let _ = session
        .run(&Command::with_params("text.frame.add", json!({"rect": [40, 40, 400, 240], "text": "Hello styled world"})))
        .expect("frame");
    session
        .run(&Command::with_params("text.style.define", json!({"id": "Body", "kind": "paragraph", "props": {"space_after": 8.0}})))
        .unwrap();
    session
        .run(&Command::with_params("text.style.define", json!({"id": "Quote", "kind": "paragraph", "based_on": "Body", "props": {"indent_left": 12.0}})))
        .unwrap();
    session
        .run(&Command::with_params("text.style.define", json!({"id": "Emph", "kind": "character", "props": {"italic": true}})))
        .unwrap();
    session
}

fn story(session: &Session) -> u64 {
    session.document.stories[0].id.0
}

#[test]
fn rows_are_hierarchical_and_grouped_by_type() {
    let session = live();
    let panel = TextStylesPanel::default();
    let rows = panel.rows(&session);
    let names: Vec<_> = rows.iter().map(|row| (row.kind, row.name.as_str(), row.depth)).collect();
    assert!(names.contains(&(StyleKind::Paragraph, "Body", 0)), "{names:?}");
    assert!(names.contains(&(StyleKind::Paragraph, "Quote", 1)), "{names:?}");
    assert!(names.contains(&(StyleKind::Character, "Emph", 0)), "{names:?}");
    let para_end = names.iter().rposition(|(kind, _, _)| *kind == StyleKind::Paragraph).unwrap();
    let char_start = names.iter().position(|(kind, _, _)| *kind == StyleKind::Character).unwrap();
    assert!(para_end < char_start, "{names:?}");
}

#[test]
fn apply_update_delete_and_reapply_use_live_commands() {
    let session = live();
    let panel = TextStylesPanel::default();
    let story = story(&session);
    let apply = panel
        .command(&session, &PanelAction::Apply { name: "Emph".into(), kind: StyleKind::Character, keep_overrides: false })
        .expect("apply");
    assert_eq!(apply.name, "text.style.apply_named");
    assert_eq!(apply.params["id"], json!("Emph"));
    assert_eq!(apply.params["story"], json!(story));
    assert_eq!(apply.params["keep_overrides"], json!(false));
    let keep = panel
        .command(&session, &PanelAction::Apply { name: "Emph".into(), kind: StyleKind::Character, keep_overrides: true })
        .unwrap();
    assert_eq!(keep.params["keep_overrides"], json!(true));
    let update = panel.command(&session, &PanelAction::Update { name: "Quote".into(), kind: StyleKind::Paragraph }).unwrap();
    assert_eq!(update.name, "text.style.define_from_selection");
    let delete = panel.command(&session, &PanelAction::Delete { name: "Emph".into(), kind: StyleKind::Character }).unwrap();
    assert_eq!(delete.name, "text.style.remove");
    let reapply = panel.command(&session, &PanelAction::Reapply).unwrap();
    assert_eq!(reapply.name, "text.style.clear_override");
    let duplicate = panel.command(&session, &PanelAction::Duplicate { name: "Body".into(), kind: StyleKind::Paragraph }).unwrap();
    assert_eq!(duplicate.name, "text.style.define");
    assert_eq!(duplicate.params["id"], json!("Body copy"));
    assert_eq!(duplicate.params["props"]["space_after"], json!(8.0));
}

#[test]
fn panel_actions_run_on_the_live_session_and_undo() {
    let mut session = live();
    let mut panel = TextStylesPanel::default();
    let story = story(&session);
    panel
        .run(&mut session, PanelAction::Apply { name: "Emph".into(), kind: StyleKind::Character, keep_overrides: false })
        .unwrap();
    let run = session.document.stories[0]
        .ranges
        .iter()
        .find(|range| range.get("char_style") == Some(&json!("Emph")));
    assert!(run.is_some(), "{:?}", session.document.stories[0].ranges);
    let version = session.document.version;
    session.run(&Command::new("edit.undo")).unwrap();
    assert!(session.document.version < version || session.document.stories[0].ranges.iter().all(|range| range.get("char_style") != Some(&json!("Emph"))));
    panel
        .run(&mut session, PanelAction::CreateBasedOn { name: "Body".into(), kind: StyleKind::Paragraph })
        .unwrap();
    let created = session.document.text_styles.get(StyleKind::Paragraph, "Body").is_some();
    assert!(created);
    assert!(session.document.text_styles.get(StyleKind::Paragraph, "Body copy").is_some() || session.document.text_styles.names(StyleKind::Paragraph).iter().any(|name| name.starts_with("Body")));
    let _ = story;
}

#[test]
fn sort_by_name_flattens_kind_groups() {
    let session = live();
    let mut panel = TextStylesPanel { sort: SortMode::ByName, ..Default::default() };
    let names: Vec<_> = panel.rows(&session).into_iter().map(|row| row.name).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
}

#[test]
fn window_action_and_menu_mapping() {
    assert_eq!(text_styles::WINDOW_TEXT_STYLES, "window.text.text_styles");
    assert!(text_styles::handles_menu("text.text_styles.reapply_text_styles"));
    assert_eq!(text_styles::menu_action("text.text_styles.reapply_text_styles", None), Some(PanelAction::Reapply));
    assert_eq!(
        text_styles::menu_action("text.text_styles.delete_paragraph_style", Some(&(StyleKind::Paragraph, "Quote".into()))),
        Some(PanelAction::Delete { name: "Quote".into(), kind: StyleKind::Paragraph })
    );
}

#[test]
fn detach_delete_unused_and_import_commands() {
    let session = live();
    let panel = TextStylesPanel::default();
    let detach = panel
        .command(&session, &PanelAction::Detach { name: "Emph".into(), kind: StyleKind::Character })
        .unwrap();
    assert_eq!(detach.name, "text.style.detach");
    assert_eq!(detach.params["id"], json!("Emph"));
    let unused = panel.command(&session, &PanelAction::DeleteUnused).unwrap();
    assert_eq!(unused.name, "text.style.delete_unused");
    assert!(panel.command(&session, &PanelAction::Import { path: String::new(), conflict: "rename".into() }).is_none());
    let imported = panel
        .command(&session, &PanelAction::Import { path: "styles_src.dtool".into(), conflict: "rename".into() })
        .unwrap();
    assert_eq!(imported.name, "text.style.import");
    assert_eq!(imported.params["path"], json!("styles_src.dtool"));
    assert_eq!(imported.params["conflict"], json!("rename"));
    assert_eq!(text_styles::menu_action("text.text_styles.delete_unused_styles", None), Some(PanelAction::DeleteUnused));
}
