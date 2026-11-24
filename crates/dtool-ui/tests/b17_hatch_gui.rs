use dtool_core::paint::{Colour, Hatch, HatchLine, HatchUnits, Paint, PaintRole};
use dtool_ui::fill_stroke::{FillStrokeWells, PaintOwners, WellState};

fn hatch(colour: Colour) -> Hatch {
    Hatch {
        lines: vec![HatchLine { angle_rad: 0.0, origin: [0.0, 0.0], shift: 0.0, spacing: 10.0, dash: vec![] }],
        colour,
        background: None,
        weight: 1.0,
        units: HatchUnits::Document,
        matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    }
}

fn owners<'a>(source: &'a mut Paint, cell: Option<&'a mut Paint>) -> PaintOwners<'a> {
    PaintOwners { source: Some(source), frame: None, cell, edge: None, glyph: None }
}

fn edit(paint: &mut Paint, change: impl FnOnce(&mut Hatch)) -> Paint {
    let mut wells = FillStrokeWells::default();
    let mut o = owners(paint, None);
    wells.open_popup_for(&o, PaintRole::SourceObject);
    let WellState::Single(Paint::Hatch(mut h)) = wells.role_state(PaintRole::SourceObject).clone() else { panic!("expected hatch well") };
    change(&mut h);
    assert!(wells.preview_popup(&mut o, Paint::Hatch(h)));
    assert!(wells.commit_popup(&mut o));
    paint.clone()
}

fn as_hatch(paint: &Paint) -> &Hatch {
    match paint { Paint::Hatch(h) => h, _ => panic!("expected hatch paint") }
}

#[test]
fn hatch_popup_line_edit_updates_angle_and_spacing() {
    let mut paint = Paint::Hatch(hatch(Colour::new(0, 0, 0, 255)));
    let out = edit(&mut paint, |h| { h.lines[0].angle_rad = std::f64::consts::PI / 4.0; h.lines[0].spacing = 20.0; });
    assert_eq!(as_hatch(&out).lines[0].angle_rad, std::f64::consts::PI / 4.0);
    assert_eq!(as_hatch(&out).lines[0].spacing, 20.0);
}

#[test]
fn hatch_matrix_is_preserved_and_editable() {
    let mut paint = Paint::Hatch(hatch(Colour::new(255, 0, 0, 255)));
    let out = edit(&mut paint, |h| h.matrix = [2.0, 0.0, 0.0, 2.0, 5.0, 5.0]);
    assert_eq!(as_hatch(&out).matrix, [2.0, 0.0, 0.0, 2.0, 5.0, 5.0]);
    let out = edit(&mut paint, |h| h.weight = 3.0);
    assert_eq!(as_hatch(&out).matrix, [2.0, 0.0, 0.0, 2.0, 5.0, 5.0]);
}

#[test]
fn hatch_colour_and_background_are_editable() {
    let mut paint = Paint::Hatch(hatch(Colour::new(0, 0, 0, 255)));
    let out = edit(&mut paint, |h| { h.colour = Colour::new(255, 0, 0, 255); h.background = Some(Colour::new(0, 255, 0, 255)); });
    assert_eq!(as_hatch(&out).colour, Colour::new(255, 0, 0, 255));
    assert_eq!(as_hatch(&out).background, Some(Colour::new(0, 255, 0, 255)));
    let out = edit(&mut paint, |h| h.background = None);
    assert_eq!(as_hatch(&out).background, None);
}

#[test]
fn hatch_units_are_editable() {
    let mut paint = Paint::Hatch(hatch(Colour::new(0, 0, 0, 255)));
    assert_eq!(as_hatch(&edit(&mut paint, |h| h.units = HatchUnits::Mm)).units, HatchUnits::Mm);
    assert_eq!(as_hatch(&edit(&mut paint, |h| h.units = HatchUnits::Pt)).units, HatchUnits::Pt);
}

#[test]
fn hatch_popup_writes_only_the_selected_role_owner() {
    let mut source = Paint::None;
    let mut cell = Paint::Hatch(hatch(Colour::new(0, 0, 0, 255)));
    let mut wells = FillStrokeWells::default();
    {
        let mut o = owners(&mut source, Some(&mut cell));
        wells.open_popup_for(&o, PaintRole::Cell);
        let mut h = hatch(Colour::new(0, 0, 255, 255));
        h.weight = 4.0;
        assert!(wells.preview_popup(&mut o, Paint::Hatch(h)));
        assert!(wells.commit_popup(&mut o));
    }
    assert_eq!(source, Paint::None);
    assert_eq!(as_hatch(&cell).weight, 4.0);
}

#[test]
fn hatch_cancel_restores_original_state() {
    let original = Paint::Hatch(hatch(Colour::new(0, 0, 0, 255)));
    let mut paint = original.clone();
    let mut wells = FillStrokeWells::default();
    {
        let mut o = owners(&mut paint, None);
        wells.open_popup_for(&o, PaintRole::SourceObject);
        let mut h = hatch(Colour::new(255, 255, 0, 255));
        h.lines[0].angle_rad = std::f64::consts::PI / 2.0;
        h.lines[0].spacing = 50.0;
        assert!(wells.preview_popup(&mut o, Paint::Hatch(h)));
        wells.cancel_popup(&mut o);
    }
    assert_eq!(paint, original);
    assert_eq!(wells.active_popup, None);
}

#[test]
fn hatch_paint_accepts_full_edit() {
    let mut paint = Paint::Hatch(hatch(Colour::new(0, 0, 0, 255)));
    let out = edit(&mut paint, |h| {
        h.lines.push(HatchLine { angle_rad: std::f64::consts::PI / 2.0, origin: [0.0, 0.0], shift: 0.0, spacing: 15.0, dash: vec![5.0, 5.0] });
        h.weight = 2.0;
    });
    assert_eq!(as_hatch(&out).lines.len(), 2);
    assert_eq!(as_hatch(&out).weight, 2.0);
}
