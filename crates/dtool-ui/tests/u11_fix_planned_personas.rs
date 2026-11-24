use dtool_core::{Command, Session};
use dtool_ui::persona::{install, persona_catalog, persona_status, refuse_excluded, PersonaEntry, PersonaId, PersonaStore};
use serde_json::json;

const EXTRA_IDS: [&str; 6] = ["typography", "compositing", "retouching", "colour_grading", "astrophotography", "scripting"];

fn command(name: &str, params: serde_json::Value) -> Command {
    Command { name: name.into(), params }
}

#[test]
fn u11_planned_entries_keep_ids_and_are_refused() {
    let catalog = persona_catalog();
    assert_eq!(catalog.iter().filter(|def| matches!(def.entry, PersonaEntry::Extra(_))).count(), 6);
    assert!(catalog.iter().any(|def| def.entry == PersonaEntry::Builtin(PersonaId::Liquify)));
    for id in EXTRA_IDS.iter().copied().chain(["liquify"]) {
        let entry = PersonaEntry::parse(id).unwrap();
        assert_eq!(entry.key(), id);
        assert_eq!(refuse_excluded(&entry).unwrap_err().code, "NOT_IMPLEMENTED");
        assert_eq!(persona_status(&entry), "planned");
    }
    for id in ["vector", "pixel", "layout", "slice"] {
        let entry = PersonaEntry::parse(id).unwrap();
        assert!(refuse_excluded(&entry).is_ok());
        assert_eq!(persona_status(&entry), "supported");
    }
}

#[test]
fn u11_set_refuses_planned_without_state_change_and_list_reports_status() {
    let dir = std::env::temp_dir().join(format!("dtool_u11_fix_{}", std::process::id()));
    install(PersonaStore { dir: dir.clone() }).unwrap();
    let mut session = Session::default();
    let run = |session: &mut Session, name: &str, params: serde_json::Value| dtool_core::commands::persona::run(session, &command(name, params)).unwrap();
    let before = run(&mut session, "ui.persona.query", json!({})).unwrap();
    for id in EXTRA_IDS.iter().copied().chain(["liquify"]) {
        let error = run(&mut session, "ui.persona.set", json!({"id": id})).unwrap_err();
        assert_eq!(error.code, "NOT_IMPLEMENTED");
        let after = run(&mut session, "ui.persona.query", json!({})).unwrap();
        assert_eq!(after["id"], before["id"]);
        assert_eq!(after["revision"], before["revision"]);
    }
    let switched = run(&mut session, "ui.persona.set", json!({"id": "pixel"})).unwrap();
    assert_eq!(switched["id"], "pixel");
    let list = run(&mut session, "ui.persona.list", json!({})).unwrap();
    let status = |id: &str| list.as_array().unwrap().iter().find(|item| item["id"] == id).unwrap()["status"].clone();
    assert_eq!(status("typography"), "planned");
    assert_eq!(status("liquify"), "planned");
    assert_eq!(status("vector"), "supported");
    let _ = std::fs::remove_dir_all(&dir);
}