use dtool_core::context_toolbar::options_for;

#[test]
fn knife_option_labels_have_ko_keys() {
    let specs = options_for("tool.knife");
    for id in ["knife.auto_close", "knife.smoothness", "knife.stabiliser"] {
        let spec = specs.iter().find(|spec| spec.id == id).expect(id);
        assert!(dtool_strings::has(spec.key), "{}", spec.key);
    }
}