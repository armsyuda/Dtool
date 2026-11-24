use dtool_ui::persona::{persona_catalog, refuse_planned, PersonaEntry};

#[test]
fn extra_studios_are_refused_and_kept_in_catalog() {
    let extras: Vec<_> = persona_catalog().into_iter().filter(|def| matches!(def.entry, PersonaEntry::Extra(_))).collect();
    assert_eq!(extras.len(), 6);
    for def in &extras {
        let entry = PersonaEntry::parse(&def.entry.key()).unwrap();
        let error = refuse_planned(&entry).unwrap_err();
        assert_eq!(error.code, "NOT_IMPLEMENTED");
    }
    for id in ["vector", "pixel", "layout", "slice"] {
        let entry = PersonaEntry::parse(id).unwrap();
        assert!(refuse_planned(&entry).is_ok());
    }
}
