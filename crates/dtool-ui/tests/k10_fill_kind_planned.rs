use dtool_ui::fill_stroke::{fill_kind_label, fill_kind_planned, PLANNED_FILL_KINDS};

#[test]
fn mesh_and_diffusion_are_planned() {
    assert!(fill_kind_planned("fill.kind.mesh"));
    assert!(fill_kind_planned("fill.kind.diffusion"));
    assert_eq!(PLANNED_FILL_KINDS.len(), 2);
}

#[test]
fn approved_kinds_stay_selectable() {
    for key in ["fill.kind.solid", "fill.kind.gradient", "fill.kind.hatch"] {
        assert!(!fill_kind_planned(key), "{key}");
        assert_eq!(fill_kind_label(key), dtool_strings::text(key).to_string());
    }
}

#[test]
fn planned_label_carries_suffix() {
    let label = fill_kind_label("fill.kind.mesh");
    assert!(label.contains(&dtool_strings::text("menu.planned.suffix").to_string()));
    assert!(label.starts_with(&dtool_strings::text("fill.kind.mesh").to_string()));
}
