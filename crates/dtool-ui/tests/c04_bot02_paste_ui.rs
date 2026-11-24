use dtool_core::clipboard::ClipboardKind;
use dtool_core::{SharedServices, Workspace};
use dtool_ui::os_clipboard::{after_shortcuts, before_shortcuts, OsClipboardEvents};
use serde_json::Value;
use std::sync::Arc;

fn events(raw: egui::RawInput) -> OsClipboardEvents {
    let ctx = egui::Context::default();
    let mut read = OsClipboardEvents::default();
    let mut output = ctx.run_ui(raw, |ui| read = ui.input(OsClipboardEvents::read));
    output.textures_delta.clear();
    read
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn workspace() -> Workspace {
    let mut ws = Workspace::new(Arc::new(SharedServices {}));
    let id = ws.new_document("c04 bot02 paste");
    ws.activate(id).unwrap();
    ws
}

#[test]
fn reads_os_paste_copy_cut_and_their_keys() {
    let ctrl = egui::Modifiers::COMMAND;
    let read = events(egui::RawInput { events: vec![egui::Event::ModifiersChanged(ctrl), egui::Event::Paste("hello".into()), key(egui::Key::V, ctrl)], ..Default::default() });
    assert_eq!(read.paste.as_deref(), Some("hello"));
    assert!(read.key_paste);
    let read = events(egui::RawInput { events: vec![egui::Event::Paste("only".into())], ..Default::default() });
    assert_eq!(read.paste.as_deref(), Some("only"));
    assert!(!read.key_paste);
    let read = events(egui::RawInput { events: vec![egui::Event::ModifiersChanged(ctrl), egui::Event::Copy, egui::Event::Cut], ..Default::default() });
    assert!(read.copy && read.cut && !read.key_copy && !read.key_cut);
    let shift = egui::Modifiers { shift: true, ..ctrl };
    let read = events(egui::RawInput { events: vec![egui::Event::ModifiersChanged(shift), egui::Event::Paste("style".into())], ..Default::default() });
    assert!(read.is_empty());
}

#[test]
fn os_text_is_stored_and_paste_runs_only_without_the_shortcut_key() {
    let mut ws = workspace();
    let session = ws.session_mut().unwrap();
    let mut read = OsClipboardEvents { paste: Some("from OS".into()), ..Default::default() };
    assert_eq!(before_shortcuts(session, &read), Some("edit.paste"));
    let payload = session.clipboard.get_payload();
    assert_eq!(payload.kind, ClipboardKind::Text);
    assert_eq!(payload.text.as_deref(), Some("from OS"));
    read.paste = Some("second\r\nline".into());
    read.key_paste = true;
    assert_eq!(before_shortcuts(session, &read), None);
    assert_eq!(session.clipboard.get_payload().text.as_deref(), Some("second\nline"));
    assert_eq!(before_shortcuts(session, &OsClipboardEvents { paste: Some(String::new()), ..Default::default() }), Some("edit.paste"));
}

#[test]
fn empty_os_text_with_empty_clipboard_does_nothing() {
    let mut ws = workspace();
    let session = ws.session_mut().unwrap();
    assert_eq!(before_shortcuts(session, &OsClipboardEvents { paste: Some(String::new()), ..Default::default() }), None);
    assert_eq!(before_shortcuts(session, &OsClipboardEvents::default()), None);
}

#[test]
fn copy_and_cut_events_run_only_without_keys_and_mirror_app_text() {
    let mut ws = workspace();
    let session = ws.session_mut().unwrap();
    assert_eq!(before_shortcuts(session, &OsClipboardEvents { copy: true, ..Default::default() }), Some("edit.copy"));
    assert_eq!(before_shortcuts(session, &OsClipboardEvents { copy: true, key_copy: true, ..Default::default() }), None);
    assert_eq!(before_shortcuts(session, &OsClipboardEvents { cut: true, ..Default::default() }), Some("edit.cut"));
    assert_eq!(before_shortcuts(session, &OsClipboardEvents { cut: true, key_cut: true, ..Default::default() }), None);
    session.clipboard.set_text("mirror me".into());
    assert_eq!(after_shortcuts(session, &OsClipboardEvents { copy: true, ..Default::default() }).as_deref(), Some("mirror me"));
    assert_eq!(after_shortcuts(session, &OsClipboardEvents::default()), None);
}

#[test]
fn new_from_clipboard_menu_row_is_bound() {
    let tree = dtool_ui::menu_tree::MenuTree::for_persona("pixel").to_json();
    fn find<'a>(tree: &'a Value, id: &str) -> Option<&'a Value> {
        match tree {
            Value::Array(items) => items.iter().find_map(|item| find(item, id)),
            Value::Object(map) => {
                if map.get("id").and_then(Value::as_str) == Some(id) {
                    return Some(tree);
                }
                map.get("children").and_then(|children| find(children, id))
            }
            _ => None,
        }
    }
    let row = find(&tree, "file.new_from_clipboard").expect("row");
    assert_eq!(row["command"], "doc.new_from_clipboard");
}