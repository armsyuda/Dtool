use dtool_core::paint::Paint;
use dtool_ui::fill_stroke::{default_fill_for_kind, FILL_MENU_KINDS, PLANNED_FILL_KINDS};

#[test]
fn c07_fill_menu_hides_mesh_and_diffusion() {
    for key in PLANNED_FILL_KINDS {
        assert!(!FILL_MENU_KINDS.contains(&key), "{key}");
        assert!(default_fill_for_kind(key).is_none(), "{key}");
    }
    assert!(!FILL_MENU_KINDS.iter().any(|key| key.contains("mesh") || key.contains("diffusion")));
}

#[test]
fn c07_every_menu_kind_creates_its_paint() {
    for key in FILL_MENU_KINDS {
        let paint = default_fill_for_kind(key).unwrap_or_else(|| panic!("{key}"));
        assert!(!matches!(paint, Paint::Mesh(_) | Paint::Diffusion(_) | Paint::None), "{key}");
    }
    assert!(matches!(default_fill_for_kind("fill.kind.solid"), Some(Paint::Solid { .. })));
    assert!(matches!(default_fill_for_kind("fill.kind.gradient"), Some(Paint::Gradient { .. })));
    assert!(matches!(default_fill_for_kind("fill.kind.hatch"), Some(Paint::Hatch(_))));
}