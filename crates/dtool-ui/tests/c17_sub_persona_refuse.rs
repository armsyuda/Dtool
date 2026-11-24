use dtool_ui::persona::{enter_sub_state, leave_sub_state, persona_catalog, refuse_planned, sub_target, PersonaEntry, PersonaId, PersonaState, PersonaStore};

fn store(name: &str) -> PersonaStore {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c17_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    PersonaStore { dir }
}

fn snapshot(state: &PersonaState) -> String {
    serde_json::to_string(&state.active).unwrap()
}

#[test]
fn liquify_and_extras_refused_by_enter_sub_without_state_change() {
    let store = store("refuse");
    let mut state = PersonaState::default();
    let mut sub = None;
    let before = snapshot(&state);
    let mut ids = vec!["liquify".to_string()];
    ids.extend(persona_catalog().iter().filter(|d| matches!(d.entry, PersonaEntry::Extra(_))).map(|d| d.entry.key()));
    assert_eq!(ids.len(), 7);
    for id in &ids {
        let err = enter_sub_state(&mut state, &mut sub, &store, id, 3).unwrap_err();
        assert_eq!(err.code, "NOT_IMPLEMENTED", "{id}");
        assert!(sub.is_none());
        assert_eq!(snapshot(&state), before);
    }
    assert!(!store.dir.join("state.json").exists());
    let reply = leave_sub_state(&mut state, &mut sub, &store, false).unwrap();
    assert_eq!(reply["noop"], serde_json::json!(true));
    assert_eq!(snapshot(&state), before);
    assert!(sub.is_none());
    assert_eq!(leave_sub_state(&mut state, &mut sub, &store, true).unwrap_err().code, "NOT_IN_SUB_PERSONA");
}

#[test]
fn liquify_refused_for_persona_set_and_kept_in_catalog() {
    assert!(persona_catalog().iter().any(|d| d.entry == PersonaEntry::Builtin(PersonaId::Liquify)));
    assert_eq!(refuse_planned(&PersonaEntry::Builtin(PersonaId::Liquify)).unwrap_err().code, "NOT_IMPLEMENTED");
    assert_eq!(sub_target(&PersonaEntry::Builtin(PersonaId::Vector)).unwrap_err().code, "INVALID_PARAMS");
    assert_eq!(sub_target(&PersonaEntry::Builtin(PersonaId::AiImage)).unwrap(), PersonaId::AiImage);
}

#[test]
fn allowed_sub_enters_and_cancel_restores_previous() {
    let store = store("enter");
    let mut state = PersonaState::default();
    let mut sub = None;
    let before = state.active.clone();
    enter_sub_state(&mut state, &mut sub, &store, "panorama", 9).unwrap();
    assert_eq!(state.active, PersonaEntry::Builtin(PersonaId::Panorama));
    assert_eq!(sub.as_ref().map(|s| s.id), Some(PersonaId::Panorama));
    assert_eq!(enter_sub_state(&mut state, &mut sub, &store, "ai_image", 9).unwrap_err().code, "ALREADY_IN_SUB_PERSONA");
    let reply = leave_sub_state(&mut state, &mut sub, &store, false).unwrap();
    assert_eq!(reply["applied"], serde_json::json!(false));
    assert_eq!(reply["doc_version_at_enter"], serde_json::json!(9));
    assert_eq!(state.active, before);
    assert!(sub.is_none());
}

#[test]
fn failed_leave_keeps_sub_session() {
    let mut state = PersonaState::default();
    let mut sub = None;
    let good = store("fail");
    enter_sub_state(&mut state, &mut sub, &good, "astro_stack", 1).unwrap();
    let file = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c17_blocker_{}", std::process::id()));
    std::fs::write(&file, b"x").unwrap();
    let bad = PersonaStore { dir: file.join("nested") };
    let active = state.active.clone();
    assert_eq!(leave_sub_state(&mut state, &mut sub, &bad, true).unwrap_err().code, "IO_ERROR");
    assert!(sub.is_some());
    assert_eq!(state.active, active);
    leave_sub_state(&mut state, &mut sub, &good, true).unwrap();
    assert!(sub.is_none());
}
