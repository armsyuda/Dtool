use dtool_core::colour::ColourModel;
use dtool_core::paint::{Colour, Hatch, HatchLine, HatchUnits, Paint, PaintRole};
use dtool_ui::fill_stroke::{FillStrokeWells, PaintOwners};
use dtool_ui::hatch_editor::edited_colour;

fn hatch(colour: Colour) -> Hatch {
    Hatch {
        lines: vec![HatchLine { angle_rad: 0.0, origin: [0.0, 0.0], shift: 0.0, spacing: 6.0, dash: Vec::new() }],
        colour,
        background: None,
        weight: 1.0,
        units: HatchUnits::Px,
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    }
}

fn cmyk() -> Colour {
    Colour::from_managed(dtool_core::colour::Colour { model: ColourModel::Cmyk, values: [0.0, 100.0, 100.0, 0.0], alpha: 1.0 })
}

#[test]
fn colour_edit_keeps_model() {
    let old = cmyk();
    let alpha_only = edited_colour(&old, [old.r, old.g, old.b, 128]);
    assert_eq!(alpha_only.model, ColourModel::Cmyk);
    assert_eq!(alpha_only.values, old.values);
    assert_eq!(alpha_only.a, 128);
    let moved = edited_colour(&old, [0, 0, 255, 255]);
    assert_eq!(moved.model, ColourModel::Cmyk);
    assert_ne!(moved.values, old.values);
    let rgb = edited_colour(&Colour::new(1, 2, 3, 255), [9, 8, 7, 255]);
    assert_eq!(rgb.model, ColourModel::Rgb);
    assert_eq!([rgb.r, rgb.g, rgb.b], [9, 8, 7]);
}

#[test]
fn popup_hatch_edit_commits_to_owner() {
    let mut paint = Paint::Hatch(hatch(cmyk()));
    let mut o = PaintOwners { source: Some(&mut paint), frame: None, cell: None, edge: None, glyph: None };
    let mut wells = FillStrokeWells::default();
    wells.open_popup_for(&o, PaintRole::SourceObject);
    let mut next = hatch(cmyk());
    next.lines.push(next.lines[0].clone());
    assert!(wells.edit_hatch(next.clone()));
    assert!(!wells.edit_hatch(next));
    assert!(wells.commit_popup(&mut o));
    match &paint {
        Paint::Hatch(h) => {
            assert_eq!(h.lines.len(), 2);
            assert_eq!(h.colour.model, ColourModel::Cmyk);
        }
        _ => panic!("expected hatch"),
    }
}

#[test]
fn edit_hatch_ignores_non_hatch_fill() {
    let mut wells = FillStrokeWells::default();
    assert!(!wells.edit_hatch(hatch(Colour::new(0, 0, 0, 255))));
}
